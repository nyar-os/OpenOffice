# OpenOffice / OpenWord / OpenExcel / OpenPPT / OpenPDF 开发者手册

## 架构原则

- **OpenX 薄宿主** — Rust 核心归 `rust-boost/projects` 中立能力树，各产品只留 Rust 薄绑定 + N-API + TS 适配层 + CLI/UI
- **Office 一个二进制集成四模块**（OpenWord/OpenExcel/OpenPPT/OpenPDF），四独立版仍可用
- **上游只保留 smoke** — 详细行为、fixture、验收脚本全部下移到各产品下游 harness

## Layout（每产品通用）

```text
crates/core          # Rust 领域核心（从 rust-boost/projects 引用）
crates/napi          # N-API cdylib 出口
packages/<product>/  # TS meta package（library + CLI + locales）
packages/<product>-ui/     # 可选 VMZ WebView UI
packages/<product>-skills/ # 可选 agent 技能
packages/<product>-win32-x64/  # 平台包
```

## Build（每产品通用）

```bash
pnpm install
pnpm run build:napi   # Rust + stage .node
pnpm run build:ts     # CLI + library
pnpm run build:ui     # 可选 VMZ UI
pnpm run check:rust   # fmt + clippy -D warnings + test
```

## 注意

各 Office 产品仓库的能力以各自 `capabilities` 输出为准。OpenOffice 已提供真实的
`open-office inspect / convert` 二进制，通过路径依赖直接调用四个引擎，不复制模型、
不伪造格式转换；其余产品仍按各自仓库的骨架/成熟度逐步推进。
