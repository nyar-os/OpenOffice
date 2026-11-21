# OpenOffice Agent 参考

## 定位

OpenOffice 是 Office 全家桶薄宿主 — 集成 OpenWord / OpenExcel / OpenPPT / OpenPDF 四模块的单二进制。

## CLI 入口（规划中）

```bash
open-office capabilities --json   # 看本机构建了哪些模块
open-office version
```

## 各子产品独立 CLI

```bash
open-word ...
open-excel ...
open-ppt ...
open-pdf ...
```

## 能力交付状态

本仓库当前是骨架，所有真实能力、fixture 和验收脚本在各子产品下游 harness。agent 调用前先查 `capabilities --json`。
