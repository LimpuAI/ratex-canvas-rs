# ratex-canvas-rs

LaTeX 数学公式 → Canvas 2D 绘制指令序列的后端无关 Rust 库,面向 WASI Component Model(echodawn:canvas 共享词汇表提供方)。

- **矢量路径输出** — 字形经 KaTeX TTF 轮廓烘焙为 path fill 指令,不经文本通道,数学布局零损
- **RaTeX 引擎** — `ratex-parser` / `ratex-layout` 0.1.14(KaTeX 兼容布局)
- **WASM-first** — wasm32-wasip2 组件导出 `ratex:viz/ratex-canvas-viz@1.0.0` world

## Crate 结构

| Crate | 说明 |
|-------|------|
| [ratex-canvas](crates/ratex-canvas) | 会话库:parse → layout → DisplayList → canvas 指令烘焙(字形路径化) |
| [ratex-canvas-wit-wasm](crates/ratex-canvas-wit-wasm) | WASI 组件导出层(wit-bindgen 0.57) |

## 构建

```bash
# 测试(会话库 golden tests)
cargo test -p ratex-canvas

# WASI 组件(release)
cargo build -p ratex-canvas-wit-wasm --target wasm32-wasip2 --release
```
