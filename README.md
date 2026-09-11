# ratex-canvas-rs

LaTeX 数学公式 → Canvas 2D 绘制指令序列的后端无关 Rust 库,面向 WASI Component Model（canvas 共享绘制词汇表 `echodawn:canvas` 契约实现方）。

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

## 打包（aixpack 标准插件包）

宿主应用装载的标准插件包由 [aixpack](https://github.com/LimpuAI/aixpack)（LimpuAI 生态工具链，`cargo install --git https://github.com/LimpuAI/aixpack`）从本仓 `pack.toml` 产出：

```bash
aixpack --root <本仓> build       # wasip2 release 组件 → dist/staging/
aixpack --root <本仓> pkg --zip   # dist/<author>--<name>/ + 标准包 zip（唯一顶层）
aixpack --root <本仓> verify --wit-root <WIT路径>   --contracts-root <契约仓> --registry-root <登记处仓>
```

verify 三查：WIT 双侧一致（本仓 wit 副本 ↔ 契约权威副本）/ world ⊆ 扩展点登记处 / 包内哈希三方一致。

