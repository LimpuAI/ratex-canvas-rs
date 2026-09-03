//! DisplayList(em 单位)→ canvas 绘制指令(逻辑 px)烘焙。
//!
//! 数学内容一律矢量路径发射:字形经 KaTeX TTF 轮廓(ab_glyph)转为 path
//! fill 指令 —— 不走 text 通道,宿主 parley shaping 无法摧毁数学布局。
//! 轮廓变换镜像 ratex-render tiny_skia 渲染核:font y-up → canvas y-down,
//! `X = base_x + gx * s`、`Y = base_y - gy * s`。
//!
//! KaTeX SVG 路径(stretchy 箭头/大定界符)的分量绕向可相反,逐 MoveTo
//! 段独立成 fill 指令,避免合并填充时绕向抵消(ratex-render 同款策略)。

use ab_glyph::{Font, FontRef, OutlineCurve};
use ratex_font::FontId;
use ratex_types::display_item::{DisplayItem, DisplayList};
use ratex_types::path_command::PathCommand;

use crate::{DrawCmd, Error, Frame, Layer, Paint, Result, Theme};

/// path 段前缀(echodawn:canvas/draw@2.0.0 decode_path 契约)
mod seg {
    pub const MOVE_TO: f64 = 0.0;
    pub const LINE_TO: f64 = 1.0;
    pub const CUBIC_TO: f64 = 2.0;
    pub const QUAD_TO: f64 = 3.0;
    pub const CLOSE: f64 = 5.0;
}

/// 非填充路径(如 \angle)的描边宽:0.04em ≈ ratex-render 在 40px em 下的 1.5px 视觉档
const STROKE_WIDTH_EM: f64 = 0.04;

/// 烘焙整帧:单层输出(kind "math"),fit-to-width 仅缩小不放大
pub fn bake_frame(
    display_list: &DisplayList,
    theme: &Theme,
    constraint_width: Option<f64>,
) -> Result<Frame> {
    let em = theme.font_size;
    if !(em.is_finite() && em > 0.0) {
        return Err(Error(format!("invalid font-size {em}")));
    }

    let intrinsic_w = display_list.width * em;
    let intrinsic_h = display_list.total_height() * em;
    let fit = match constraint_width {
        Some(cw) if cw > 0.0 && intrinsic_w > cw => cw / intrinsic_w,
        _ => 1.0,
    };
    // em → px 总系数(含 fit 因子)
    let k = em * fit;

    let baker = GlyphBaker::load(&display_list.items)?;
    let foreground = theme.foreground.clone();

    let mut commands = Vec::new();
    for item in &display_list.items {
        match item {
            DisplayItem::GlyphPath {
                x,
                y,
                scale,
                font,
                char_code,
                ..
            } => {
                if let Some(cmd) =
                    baker.glyph_cmd(*x * k, *y * k, *scale * k, font, *char_code, &foreground)
                {
                    commands.push(cmd);
                }
            }
            DisplayItem::Line {
                x,
                y,
                width,
                thickness,
                dashed,
                ..
            } => {
                commands.push(line_cmd(
                    *x * k,
                    *y * k,
                    *width * k,
                    *thickness * k,
                    *dashed,
                    &foreground,
                ));
            }
            DisplayItem::Rect {
                x,
                y,
                width,
                height,
                ..
            } => {
                commands.push(DrawCmd {
                    cmd_type: "rect".to_string(),
                    params: vec![x * k, y * k, width * k, height * k],
                    fill: Some(Paint::Solid(foreground.clone())),
                    stroke: None,
                    stroke_width: None,
                    dash: None,
                });
            }
            DisplayItem::Path {
                x,
                y,
                commands: path_cmds,
                fill,
                ..
            } => {
                emit_svg_path(
                    &mut commands,
                    *x * k,
                    *y * k,
                    path_cmds,
                    *fill,
                    k,
                    &foreground,
                );
            }
        }
    }

    Ok(Frame {
        layers: vec![Layer {
            kind: "math".to_string(),
            dirty: true,
            z_index: 0,
            commands,
        }],
        width: intrinsic_w * fit,
        height: intrinsic_h * fit,
    })
}

/// 字形轮廓烘焙器:内嵌 KaTeX FontSet 持有,FontRef 按需构造(零拷贝切片,
/// 轮廓解析经 ratex-font-loader 全局缓存)
struct GlyphBaker {
    fonts: ratex_font_loader::FontSet,
}

impl GlyphBaker {
    /// 内嵌 KaTeX 字体(ratex-font-loader embed-fonts)按 DisplayList 需求加载
    fn load(items: &[DisplayItem]) -> Result<Self> {
        let fonts = ratex_font_loader::load_fonts_for_items("", items)
            .map_err(|e| Error(format!("font load failed: {e}")))?;
        if !fonts.contains_key(&FontId::MainRegular) {
            return Err(Error("Main-Regular font not found".to_string()));
        }
        Ok(Self { fonts })
    }

    fn font_for(&self, font_id: FontId) -> Option<FontRef<'_>> {
        let bytes = self
            .fonts
            .get(&font_id)
            .or_else(|| self.fonts.get(&FontId::MainRegular))?;
        FontRef::try_from_slice_and_index(bytes, 0).ok()
    }

    /// 单字形 → path fill 指令(基线点 (px, py),字形缩放 s = k * script_scale)
    fn glyph_cmd(
        &self,
        px: f64,
        py: f64,
        s: f64,
        font: &str,
        char_code: u32,
        foreground: &str,
    ) -> Option<DrawCmd> {
        let font_id = FontId::parse(font).unwrap_or(FontId::MainRegular);
        let mut face = self.font_for(font_id)?;
        let ch = ratex_font::katex_ttf_glyph_char(font_id, char_code);
        let mut gid = face.glyph_id(ch);
        let mut font_id = font_id;
        if gid.0 == 0 {
            // 请求字体缺字形 → Main-Regular 兜底(ratex-render 首选 fallback)
            font_id = FontId::MainRegular;
            face = self.font_for(font_id)?;
            gid = face.glyph_id(ch);
            if gid.0 == 0 {
                return None;
            }
        }
        let curves = ratex_font_loader::outline_cache::get_or_compute_outline(
            font_id, &face, gid,
        )?;
        if curves.is_empty() {
            return None; // 空白字形等无轮廓项
        }

        let units_per_em = face.units_per_em().unwrap_or(1000.0) as f64;
        let glyph_scale = s / units_per_em;

        let mut params = Vec::with_capacity(curves.len() * 7);
        let mut last: Option<(f64, f64)> = None;
        for curve in curves.iter() {
            let (start, end) = transform_curve(curve, px, py, glyph_scale);
            let need_move = match last {
                None => true,
                // 换轮廓:闭合上一段后显式 MoveTo
                Some((lx, ly)) => (lx - start.0).abs() > 1e-4 || (ly - start.1).abs() > 1e-4,
            };
            if need_move {
                if last.is_some() {
                    params.push(seg::CLOSE);
                }
                params.push(seg::MOVE_TO);
                params.push(start.0);
                params.push(start.1);
            }
            match curve {
                OutlineCurve::Line(_, p1) => {
                    params.push(seg::LINE_TO);
                    params.push(px + p1.x as f64 * glyph_scale);
                    params.push(py - p1.y as f64 * glyph_scale);
                }
                OutlineCurve::Quad(_, p1, p2) => {
                    params.push(seg::QUAD_TO);
                    params.push(px + p1.x as f64 * glyph_scale);
                    params.push(py - p1.y as f64 * glyph_scale);
                    params.push(px + p2.x as f64 * glyph_scale);
                    params.push(py - p2.y as f64 * glyph_scale);
                }
                OutlineCurve::Cubic(_, p1, p2, p3) => {
                    params.push(seg::CUBIC_TO);
                    params.push(px + p1.x as f64 * glyph_scale);
                    params.push(py - p1.y as f64 * glyph_scale);
                    params.push(px + p2.x as f64 * glyph_scale);
                    params.push(py - p2.y as f64 * glyph_scale);
                    params.push(px + p3.x as f64 * glyph_scale);
                    params.push(py - p3.y as f64 * glyph_scale);
                }
            }
            last = Some(end);
        }
        if params.is_empty() {
            return None;
        }
        params.push(seg::CLOSE);

        Some(DrawCmd {
            cmd_type: "path".to_string(),
            params,
            fill: Some(Paint::Solid(foreground.to_string())),
            stroke: None,
            stroke_width: None,
            dash: None,
        })
    }
}

/// 轮廓曲线端点变换(font y-up → canvas y-down)
fn transform_curve(
    curve: &OutlineCurve,
    px: f64,
    py: f64,
    s: f64,
) -> ((f64, f64), (f64, f64)) {
    let pt = |p: &ab_glyph::Point| (px + p.x as f64 * s, py - p.y as f64 * s);
    match curve {
        OutlineCurve::Line(p0, p1) => (pt(p0), pt(p1)),
        OutlineCurve::Quad(p0, _, p2) => (pt(p0), pt(p2)),
        OutlineCurve::Cubic(p0, _, _, p3) => (pt(p0), pt(p3)),
    }
}

/// 规则线(分数线/上划线等):实线走 rect fill,虚线走 stroke path + dash
fn line_cmd(x: f64, y: f64, width: f64, thickness: f64, dashed: bool, foreground: &str) -> DrawCmd {
    if dashed {
        let t = thickness.max(0.5);
        let dash = (4.0 * t).max(2.0);
        DrawCmd {
            cmd_type: "path".to_string(),
            params: vec![
                seg::MOVE_TO,
                x,
                y,
                seg::LINE_TO,
                x + width,
                y,
                seg::CLOSE,
            ],
            fill: None,
            stroke: Some(Paint::Solid(foreground.to_string())),
            stroke_width: Some(t),
            dash: Some(vec![dash, dash]),
        }
    } else {
        DrawCmd {
            cmd_type: "rect".to_string(),
            params: vec![x, y - thickness / 2.0, width, thickness],
            fill: Some(Paint::Solid(foreground.to_string())),
            stroke: None,
            stroke_width: None,
            dash: None,
        }
    }
}

/// KaTeX SVG 路径:fill 逐 MoveTo 段独立指令(绕向抵消规避);非 fill 单条描边
#[allow(clippy::too_many_arguments)]
fn emit_svg_path(
    out: &mut Vec<DrawCmd>,
    x0: f64,
    y0: f64,
    commands: &[PathCommand],
    fill: bool,
    k: f64,
    foreground: &str,
) {
    if fill {
        for segment in split_subpaths(commands) {
            out.push(DrawCmd {
                cmd_type: "path".to_string(),
                params: encode_subpath(segment, x0, y0, k),
                fill: Some(Paint::Solid(foreground.to_string())),
                stroke: None,
                stroke_width: None,
                dash: None,
            });
        }
    } else {
        out.push(DrawCmd {
            cmd_type: "path".to_string(),
            params: encode_subpath(commands, x0, y0, k),
            fill: None,
            stroke: Some(Paint::Solid(foreground.to_string())),
            stroke_width: Some((STROKE_WIDTH_EM * k).max(0.75)),
            dash: None,
        });
    }
}

/// 按 MoveTo 边界拆子路径(跳过前导无 MoveTo 的孤立段)
fn split_subpaths(commands: &[PathCommand]) -> Vec<&[PathCommand]> {
    let mut segments = Vec::new();
    let mut start: Option<usize> = None;
    for (i, cmd) in commands.iter().enumerate() {
        if matches!(cmd, PathCommand::MoveTo { .. }) {
            if let Some(s) = start {
                segments.push(&commands[s..i]);
            }
            start = Some(i);
        }
    }
    if let Some(s) = start {
        segments.push(&commands[s..]);
    }
    segments
}

/// 子路径 → 段前缀编码(坐标偏移 + em→px 缩放)
fn encode_subpath(commands: &[PathCommand], x0: f64, y0: f64, k: f64) -> Vec<f64> {
    let mut params = Vec::with_capacity(commands.len() * 7);
    let at = |cx: f64, cy: f64| (x0 + cx * k, y0 + cy * k);
    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo { x, y } => {
                let (ax, ay) = at(x, y);
                params.extend_from_slice(&[seg::MOVE_TO, ax, ay]);
            }
            PathCommand::LineTo { x, y } => {
                let (ax, ay) = at(x, y);
                params.extend_from_slice(&[seg::LINE_TO, ax, ay]);
            }
            PathCommand::CubicTo { x1, y1, x2, y2, x, y } => {
                let (a1x, a1y) = at(x1, y1);
                let (a2x, a2y) = at(x2, y2);
                let (ax, ay) = at(x, y);
                params.extend_from_slice(&[seg::CUBIC_TO, a1x, a1y, a2x, a2y, ax, ay]);
            }
            PathCommand::QuadTo { x1, y1, x, y } => {
                let (a1x, a1y) = at(x1, y1);
                let (ax, ay) = at(x, y);
                params.extend_from_slice(&[seg::QUAD_TO, a1x, a1y, ax, ay]);
            }
            PathCommand::Close => params.push(seg::CLOSE),
        }
    }
    params
}
