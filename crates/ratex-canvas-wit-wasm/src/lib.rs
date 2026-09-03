//! ratex-canvas-wit-wasm — WASI Component(wasm32-wasip2)导出层。
//!
//! wit-bindgen 0.57 从 wit/ratex-viz.wit 生成 guest 绑定,导出 world
//! `ratex-canvas-viz`(resource math 会话),包装 ratex-canvas 的 Session。
//!
//! 错误边界:constructor 无错误通道 —— 解析失败落 Failed 态,后续 render
//! 返回诚实 Err(宿主降级源码显示);任何路径不 panic 越界(panic 由
//! ratex-canvas 内部 guard 折叠为 Err)。

wit_bindgen::generate!({
    world: "ratex:viz/ratex-canvas-viz@1.0.0",
    path: "wit",
    generate_all,
});

use std::cell::RefCell;

use exports::ratex::viz::math_renderer::{Guest, GuestMath, MathTheme, RenderResult};
use ratex_canvas::Session;

struct RatexVizComponent;

impl Guest for RatexVizComponent {
    type Math = MathResource;
}

enum State {
    Ready(Session),
    Failed(String),
}

/// resource math 实现体 — trait 方法为 &self(bindgen 约定),会话可变性经 RefCell
pub struct MathResource {
    /// 最近一次注入的主题(Failed 态重试 construct 时复用,主题零丢失)
    theme: RefCell<ratex_canvas::Theme>,
    state: RefCell<State>,
}

fn theme_to_lib(theme: MathTheme) -> ratex_canvas::Theme {
    ratex_canvas::Theme {
        foreground: theme.foreground,
        font_size: theme.font_size,
    }
}

impl GuestMath for MathResource {
    fn new(source: String, opts: Option<MathTheme>) -> Self {
        let theme = opts.map(theme_to_lib).unwrap_or_default();
        let state = match Session::construct(&source, Some(theme.clone())) {
            Ok(session) => State::Ready(session),
            Err(e) => State::Failed(e.0),
        };
        Self {
            theme: RefCell::new(theme),
            state: RefCell::new(state),
        }
    }

    fn update_source(&self, source: String) -> Result<(), String> {
        let mut state = self.state.borrow_mut();
        match &mut *state {
            State::Ready(session) => session.update_source(&source).map_err(|e| e.0),
            State::Failed(_) => match Session::construct(&source, Some(self.theme.borrow().clone()))
            {
                Ok(session) => {
                    *state = State::Ready(session);
                    Ok(())
                }
                Err(e) => Err(e.0),
            },
        }
    }

    fn resize(&self, width: f64) {
        if let State::Ready(session) = &mut *self.state.borrow_mut() {
            session.resize(width);
        }
    }

    fn set_theme(&self, theme: MathTheme) {
        let theme = theme_to_lib(theme);
        *self.theme.borrow_mut() = theme.clone();
        if let State::Ready(session) = &mut *self.state.borrow_mut() {
            session.set_theme(theme);
        }
    }

    fn render(&self, t: f64) -> Result<RenderResult, String> {
        let state = self.state.borrow();
        match &*state {
            State::Ready(session) => session.render(t).map(frame_to_bindgen).map_err(|e| e.0),
            State::Failed(msg) => Err(msg.clone()),
        }
    }
}

fn frame_to_bindgen(frame: ratex_canvas::Frame) -> RenderResult {
    RenderResult {
        layers: frame
            .layers
            .into_iter()
            .map(|layer| echodawn::canvas::draw::Layer {
                kind: layer.kind,
                dirty: layer.dirty,
                z_index: layer.z_index,
                commands: layer.commands.into_iter().map(cmd_to_bindgen).collect(),
            })
            .collect(),
        width: frame.width,
        height: frame.height,
    }
}

fn cmd_to_bindgen(cmd: ratex_canvas::DrawCmd) -> echodawn::canvas::draw::DrawCmd {
    echodawn::canvas::draw::DrawCmd {
        cmd_type: cmd.cmd_type,
        params: cmd.params,
        fill: cmd.fill.map(paint_to_bindgen),
        stroke: cmd.stroke.map(paint_to_bindgen),
        stroke_width: cmd.stroke_width,
        corner_radius: None,
        corner_radii: None,
        dash: cmd.dash,
        line_cap: None,
        shadow: None,
        text_content: None,
        font: None,
        group_depth: 0,
        id: None,
        anims: Vec::new(),
    }
}

fn paint_to_bindgen(paint: ratex_canvas::Paint) -> echodawn::canvas::draw::Paint {
    match paint {
        ratex_canvas::Paint::Solid(color) => echodawn::canvas::draw::Paint::Solid(color),
    }
}

export!(RatexVizComponent);
