//! Golden tests:样例公式 → canvas 指令帧断言(存在性 / 尺寸 / 路径化 / 确定性)
//! + 解析错误诚实失败契约。

use ratex_canvas::{DrawCmd, Session, Theme};

/// 样例公式覆盖:上下标 / 分数 / 嵌套根号 / 求和上下限 / 定积分 / 希腊字母 / 矩阵
const FORMULAS: &[&str] = &[
    "x^2",
    r"\frac{a}{b}",
    r"\sqrt{\frac{a}{b}}",
    r"\sum_{i=1}^{n} i",
    r"\int_0^1 f(x)\,dx",
    r"\alpha\beta\gamma\Omega",
    r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
];

/// 解析必须诚实失败的病态源
const PARSE_ERRORS: &[&str] = &[
    r"\frac{a}{",      // 未闭合组
    r"\unknownmacroxyz", // 未定义控制序列
    r"{",              // 裸开括号
];

fn path_cmd_count(frame: &ratex_canvas::Frame) -> usize {
    frame
        .layers
        .iter()
        .flat_map(|l| l.commands.iter())
        .filter(|c| c.cmd_type == "path")
        .count()
}

fn all_paint_solid_foreground(frame: &ratex_canvas::Frame, foreground: &str) -> bool {
    frame.layers.iter().all(|l| {
        l.commands.iter().all(|c: &DrawCmd| {
            c.fill
                .as_ref()
                .map(|p| matches!(p, ratex_canvas::Paint::Solid(s) if s == foreground))
                .unwrap_or(true)
                && c.stroke
                    .as_ref()
                    .map(|p| matches!(p, ratex_canvas::Paint::Solid(s) if s == foreground))
                    .unwrap_or(true)
        })
    })
}

#[test]
fn golden_formulas_render_with_path_commands() {
    for source in FORMULAS {
        let session = Session::construct(*source, None)
            .unwrap_or_else(|e| panic!("{source}: construct failed: {e}"));
        let frame = session
            .render(1.0)
            .unwrap_or_else(|e| panic!("{source}: render failed: {e}"));

        assert!(
            !frame.layers.is_empty(),
            "{source}: expected non-empty layers"
        );
        assert!(
            frame.width > 0.0 && frame.height > 0.0,
            "{source}: expected positive intrinsic size, got {}x{}",
            frame.width,
            frame.height
        );
        let paths = path_cmd_count(&frame);
        assert!(
            paths > 0,
            "{source}: expected path commands (baked glyphs), got none"
        );
    }
}

#[test]
fn golden_formulas_command_types_are_vocabulary_members() {
    for source in FORMULAS {
        let frame = Session::construct(*source, None)
            .unwrap()
            .render(1.0)
            .unwrap();
        for layer in &frame.layers {
            for cmd in &layer.commands {
                assert!(
                    cmd.cmd_type == "rect" || cmd.cmd_type == "path",
                    "{source}: unexpected cmd_type {:?} (vocabulary: rect|path)",
                    cmd.cmd_type
                );
                assert!(
                    !cmd.params.is_empty(),
                    "{source}: empty params on {:?}",
                    cmd.cmd_type
                );
                // rect 固定四参数;path 段前缀编码非空
                if cmd.cmd_type == "rect" {
                    assert_eq!(cmd.params.len(), 4, "{source}: rect params arity");
                }
            }
        }
    }
}

#[test]
fn golden_render_is_deterministic() {
    for source in FORMULAS {
        let session = Session::construct(*source, None).unwrap();
        let a = session.render(0.0).unwrap();
        let b = session.render(1.0);
        assert_eq!(Ok(a.clone()), b, "{source}: render(t) must be deterministic");
        // 重新构造同源 → 逐字节一致(跨会话确定性)
        let fresh = Session::construct(*source, None).unwrap().render(0.0).unwrap();
        assert_eq!(a, fresh, "{source}: cross-session determinism");
    }
}

#[test]
fn golden_theme_flows_into_paints() {
    let theme = Theme {
        foreground: "#ff8800".to_string(),
        font_size: 24.0,
    };
    let session = Session::construct(r"\frac{a}{b}", Some(theme)).unwrap();
    let frame = session.render(0.0).unwrap();
    assert!(
        all_paint_solid_foreground(&frame, "#ff8800"),
        "all paints must carry the injected foreground"
    );
    // 字号影响 intrinsic 尺寸:24px 应为 20px 缺省的 1.2 倍(同源对比)
    let baseline = Session::construct(r"\frac{a}{b}", None).unwrap().render(0.0).unwrap();
    let ratio = frame.width / baseline.width;
    assert!(
        (ratio - 1.2).abs() < 1e-9,
        "font-size scaling: expected 1.2, got {ratio}"
    );
}

#[test]
fn golden_resize_fits_down_only() {
    let session = Session::construct(r"\sum_{i=1}^{n} i", None).unwrap();
    let intrinsic = session.render(0.0).unwrap();
    assert!(intrinsic.width > 10.0, "sanity: intrinsic width");

    let mut session = session;
    // 放大约束:scale-down only → 保持 intrinsic
    session.resize(intrinsic.width * 10.0);
    let big = session.render(0.0).unwrap();
    assert!((big.width - intrinsic.width).abs() < 1e-9);
    assert!((big.height - intrinsic.height).abs() < 1e-9);

    // 缩小约束:等比缩小,纵横比锁定
    let target = intrinsic.width / 2.0;
    session.resize(target);
    let small = session.render(0.0).unwrap();
    assert!(
        (small.width - target).abs() < 1e-9,
        "fit-to-width: expected {target}, got {}",
        small.width
    );
    let expected_h = intrinsic.height / 2.0;
    assert!(
        (small.height - expected_h).abs() < 1e-9,
        "aspect-locked height: expected {expected_h}, got {}",
        small.height
    );

    // 非正/非有限宽清除约束
    session.resize(0.0);
    let cleared = session.render(0.0).unwrap();
    assert!((cleared.width - intrinsic.width).abs() < 1e-9);
    session.resize(f64::NAN);
    let cleared = session.render(0.0).unwrap();
    assert!((cleared.width - intrinsic.width).abs() < 1e-9);
}

#[test]
fn golden_update_source_keeps_old_on_error() {
    let mut session = Session::construct("x^2", None).unwrap();
    let before = session.render(0.0).unwrap();
    assert!(session.update_source(r"\frac{a}{").is_err());
    let after = session.render(0.0).unwrap();
    assert_eq!(before, after, "failed update must keep old content");
    assert_eq!(session.source(), "x^2");

    assert!(session.update_source(r"\sqrt{2}").is_ok());
    let updated = session.render(0.0).unwrap();
    assert!(updated.width > 0.0);
    assert!(path_cmd_count(&updated) > 0);
}

#[test]
fn golden_parse_errors_are_honest_errs() {
    for source in PARSE_ERRORS {
        let result = Session::construct(*source, None);
        assert!(result.is_err(), "{source}: expected parse error");
        // 错误消息非空(可观测性)
        let msg = result.err().unwrap().to_string();
        assert!(!msg.is_empty(), "{source}: error message must be non-empty");
    }
}

#[test]
fn golden_layer_shape_matches_canvas_protocol() {
    let frame = Session::construct(r"x^2", None).unwrap().render(0.0).unwrap();
    assert_eq!(frame.layers.len(), 1);
    let layer = &frame.layers[0];
    assert_eq!(layer.kind, "math");
    assert!(layer.dirty);
    assert_eq!(layer.z_index, 0);
}

#[test]
fn golden_glyph_paths_have_valid_segment_encoding() {
    // path params 段前缀必须 ∈ {0,1,2,3,5} 且各段消费正确数量的坐标
    for source in FORMULAS {
        let frame = Session::construct(*source, None).unwrap().render(0.0).unwrap();
        for layer in &frame.layers {
            for cmd in &layer.commands {
                if cmd.cmd_type != "path" {
                    continue;
                }
                let mut i = 0usize;
                let mut saw_move = false;
                while i < cmd.params.len() {
                    let prefix = cmd.params[i] as u32 as f64;
                    assert_eq!(
                        prefix, cmd.params[i],
                        "{source}: segment prefix must be an integer, got {}",
                        cmd.params[i]
                    );
                    let consumed = match prefix {
                        0.0 => {
                            saw_move = true;
                            3
                        }
                        1.0 => 3,
                        2.0 => 7,
                        3.0 => 5,
                        5.0 => 1,
                        other => panic!(
                            "{source}: unknown segment prefix {other} at {i}"
                        ),
                    };
                    assert!(
                        i + consumed <= cmd.params.len(),
                        "{source}: truncated segment at {i}"
                    );
                    i += consumed;
                }
                assert!(saw_move, "{source}: path must start with MoveTo");
            }
        }
    }
}
