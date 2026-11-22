# OpenOffice

![Formats](https://img.shields.io/badge/formats-word%20%7C%20excel%20%7C%20pptx%20%7C%20pdf-blue)
![One binary](https://img.shields.io/badge/shipping-one%20binary-brightgreen)
![License](https://img.shields.io/badge/license-MPL--2.0-blue)

[Getting started](#-getting-started) · [Agent reference](documentation/agents.md) · [License](License.md)

## 💡 What is OpenOffice?

OpenOffice is one binary for four document families: Word, Excel, PowerPoint and PDF. It probes what a file really is, inspects its structure, reports statistics, projects its outline, searches its text, and converts it to another format.

The binary does not grow a fifth document model. Every request is routed to the engine that owns the format, so summaries, checks, statistics, outlines and search hits are the owning engine's own reports. There is no copied parser and no second source of truth. Conversion targets are the ones the owning engine actually writes: a target that cannot be produced is refused with a clear message instead of being faked.

## 🚀 Getting started

### 1. Agent / prompt (recommended)

Give an agent the `open-office` binary and a goal. It can discover capabilities, inspect before it edits, and convert only to a target the source engine supports.

> Inspect `budget.xlsx`, list its sheets and any problem cells, then convert it to `out/budget.pdf`. Do not overwrite an existing file.

The agent resolves that request with three commands:

```bash
open-office capabilities
open-office inspect budget.xlsx
open-office convert budget.xlsx --format pdf --out out/budget.pdf
```

Every command prints JSON on stdout, so the agent can branch on `kind`, `stats`, `hits`, `outline` or `report` without scraping prose. The full command surface, report shapes, error codes and guardrails are in the [agent reference](documentation/agents.md).

### 2. Manual install

Install the binary from a source checkout with one command:

```bash
cargo install --path projects/crates/office
```

You can also place a prebuilt `open-office` binary on your `PATH`. Source builds read the four engine repositories from adjacent checkouts.

## ✨ Highlights

- **One binary, four families.** Word, Excel, PowerPoint and PDF share a single executable and a single command vocabulary.
- **Reports from the engine that did the work.** Word documents return Word summaries and checks, workbooks return workbook summaries and checks, decks return slide summaries and import losses, PDFs return version, geometry, metadata, bookmarks and form fields.
- **Bytes before extensions.** `probe` reads the file, names the container, keeps every still-possible candidate and quotes the evidence. The extension is a hint, not proof.
- **Preview without the app.** `render` asks the owning engine for SVG: one page for a workbook sheet, one page per presentation slide, with `--out` writing a single selected page and `--out-dir` writing every page.
- **Real conversion targets.** A workbook exports to CSV, JSON, HTML or PDF, a deck becomes PPTX or PDF, a PDF extracts to Markdown or text. Unsupported targets are refused with the list of targets that work.
- **Guardrails by default.** Existing outputs require `--overwrite`, `--points` is accepted only for presentations, `--pages` only for PDF text, and bytes that contradict the extension are rejected.
- **Batch without surprises.** Multiple inputs convert into an output directory named after each source stem, and `stats` and `inspect` accept several files at once; one failure does not stop the rest.
- **Stable locations.** Search hits carry the owning engine's own locators, and batch failures carry stable error codes.

Supported formats:

| Source family | Import extensions | Conversion targets |
|---------------|-------------------|--------------------|
| Word | `docx`, `docm`, `rtf`, `txt`, `md`, `markdown`, `html`, `htm`, `oword` | `docx`, `pdf`, `html`, `md`, `txt`, `rtf` |
| Excel | `xlsx`, `xlsm`, `csv`, `tsv`, `oexcel` | `xlsx`, `csv`, `tsv`, `json`, `html`, `pdf` |
| Presentation | `pptx`, `pptm`, `ppsx`, `ppsm`, `potx`, `potm`, `project`, `oppt`, `opresent` | `pptx`, `ppsx`, `potx`, `pdf` |
| PDF | `pdf` | `md`, `txt`, `text` |

## 📊 How OpenOffice compares

| | A toolbox of four separate converters | OpenOffice |
|---|---|---|
| Install | Four tools with four command sets | One binary and one command set |
| Format detection | Each tool trusts its own input rules | `probe` inspects the bytes once, for every family |
| Reports | Four unrelated output shapes | One envelope with `kind`, `path` and a family payload |
| Capability truth | Four places to check | `capabilities` aggregates each engine's own matrix |
| Conversion safety | Varies per tool | One rule set: verified container, explicit target, no silent overwrite |

## 🧪 Examples

- [Agent command reference](documentation/agents.md) - command list, required and optional flags, JSON report shapes, error codes, family routing and guardrails.
- `open-office formats` - the import and export matrix of the binary you installed.
- `open-office capabilities` - the capability matrix reported by each bundled engine.

## 📜 License

OpenOffice is released under the Mozilla Public License 2.0. See [License.md](License.md).
