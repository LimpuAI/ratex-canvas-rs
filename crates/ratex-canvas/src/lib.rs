//! ratex-canvas — LaTeX 数学公式 → echodawn canvas 绘制指令。
//!
//! 管线:`ratex-parser::parse` → `ratex_layout::layout` → `to_display_list`
//! → 字形轮廓烘焙为矢量路径(不经文本通道,保持数学布局)。
//!
//! 输出词汇表与宿主共享画布协议 `echodawn:canvas/draw@2.0.0` 的
//! draw-cmd 段前缀编码逐字段对齐(0=MoveTo 1=LineTo 2=Cubic 3=Quad 5=Close)。
//!
//! 契约:会话 API 永不 panic(内部 `catch_unwind` 兜底),解析/布局/字体
//! 失败一律返回诚实 `Err`,由宿主降级源码显示。

pub mod bake;

use std::any::Any;
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};

use ratex_layout::to_display_list;
use ratex_types::display_item::DisplayList;

/// 缺省基准字号(逻辑 px;宿主经 set_theme 注入 token 派生值后覆盖)
pub const DEFAULT_FONT_SIZE: f64 = 20.0;
/// 缺省前景色(宿主注入前的一致兜底)
pub const DEFAULT_FOREGROUND: &str = "#000000";

#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// 主题注入(唯一色彩通道;对齐 WIT math-theme)
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub foreground: String,
    pub font_size: f64,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            foreground: DEFAULT_FOREGROUND.to_string(),
            font_size: DEFAULT_FONT_SIZE,
        }
    }
}

/// 填充/描边颜料(对齐 canvas paint::solid;渐变不在数学输出词汇内)
#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    Solid(String),
}

/// 绘制指令(canvas draw-cmd 的发射子集;未列字段由 wasm 边界填缺省)
#[derive(Debug, Clone, PartialEq)]
pub struct DrawCmd {
    /// "rect" | "path"
    pub cmd_type: String,
    /// rect: [x, y, w, h];path: 段前缀编码(见 crate 文档)
    pub params: Vec<f64>,
    pub fill: Option<Paint>,
    pub stroke: Option<Paint>,
    pub stroke_width: Option<f64>,
    /// 虚线节律(线段/间隙交替,px);仅 dashed rule 线使用
    pub dash: Option<Vec<f64>>,
}

/// 输出层(对齐 canvas layer)
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub kind: String,
    pub dirty: bool,
    pub z_index: u32,
    pub commands: Vec<DrawCmd>,
}

/// 渲染帧(layers + intrinsic 尺寸,逻辑 px)
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub layers: Vec<Layer>,
    pub width: f64,
    pub height: f64,
}

/// 有状态渲染会话:LaTeX 源 → DisplayList(em 单位缓存)→ 按主题烘焙 px 帧
pub struct Session {
    source: String,
    theme: Theme,
    constraint_width: Option<f64>,
    display_list: DisplayList,
}

impl Session {
    /// 构造会话:解析失败返回 Err(诚实失败,宿主降级)
    pub fn construct(source: impl Into<String>, theme: Option<Theme>) -> Result<Self> {
        let source = source.into();
        let display_list = layout_source(&source)?;
        Ok(Self {
            source,
            theme: theme.unwrap_or_default(),
            constraint_width: None,
            display_list,
        })
    }

    /// 更新公式源:新源解析失败时保留旧源并返回 Err
    pub fn update_source(&mut self, source: impl Into<String>) -> Result<()> {
        let source = source.into();
        let display_list = layout_source(&source)?;
        self.source = source;
        self.display_list = display_list;
        Ok(())
    }

    /// fit-to-width 约束(scale-down only,mermaid guest 同款语义):
    /// intrinsic 宽超出约束时等比缩小;非正/非有限值清除约束
    pub fn resize(&mut self, width: f64) {
        self.constraint_width = if width.is_finite() && width > 0.0 {
            Some(width)
        } else {
            None
        };
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// 渲染当前帧。`t` 为 Tier 1 相位参数,静态数学内容忽略之。
    pub fn render(&self, _t: f64) -> Result<Frame> {
        let theme = self.theme.clone();
        let constraint = self.constraint_width;
        guard(|| bake::bake_frame(&self.display_list, &theme, constraint))
    }
}

/// parse → layout → DisplayList(em 单位;y 向下,基线在 y = height)
fn layout_source(source: &str) -> Result<DisplayList> {
    guard(|| {
        let nodes =
            ratex_parser::parser::parse(source).map_err(|e| Error(e.to_string()))?;
        let options = ratex_layout::LayoutOptions::default();
        let lbox = ratex_layout::layout(&nodes, &options);
        Ok(to_display_list(&lbox))
    })
}

/// panic 兜底:病态输入触发的 panic 一律折叠为诚实 Err,绝不越界
fn guard<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => Err(Error(format!(
            "internal panic: {}",
            panic_message(&payload)
        ))),
    }
}

fn panic_message(payload: &Box<dyn Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string())
}
