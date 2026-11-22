# OpenOffice agent reference

`open-office` is one binary that routes a document to the engine that owns it: Word, Excel, PowerPoint or PDF. Reports are JSON on stdout. This page lists the command surface, the report envelopes, the stable error codes and the rules an agent must not break.

## Discover first

```bash
open-office capabilities
open-office formats
```

`capabilities` reports the host engine, the routed commands, each bundled engine's own capability object and the conversion targets per family. `formats` reports only the import and export matrix. Check these before assuming a format or command is available in the installed build.

## Commands

| Command | Flags | Notes |
|---------|-------|-------|
| `open <file>` | `--print` | Routes by extension and opens the file with the platform viewer: `cmd /C start` on Windows, `open` on macOS, `xdg-open` elsewhere. `--print` reports `{ kind, path, opener, opened: false }` without launching anything; use it on headless hosts and in tests. The suite ships no viewer of its own. |
| `probe <file>...` | none | Reads the bytes; does not route by extension and does not open the document. One path returns the single envelope, more paths return a batch report. Aliases: `inspect-file`, `inspect_file`. |
| `inspect <file>...` | none | Dispatches to the owning engine; one path returns the single envelope, more paths return a batch report. |
| `stats <file>...` | none | Lightweight per-family statistics; one path returns the single envelope, more paths return a batch report. |
| `outline <file>...` | `--points` (presentations), `--depth <n>` (word and PDF) | `--points` is accepted only for presentations. `--depth` keeps word headings and PDF bookmarks at that level or shallower and is rejected for other families. One path returns the single envelope, more paths return a batch report. |
| `search <file>...` | `--text <query>` / `-t` (required), `--limit <n>` (optional) | Case-sensitive in every family. Default limit is 100, clamped to 1-500. One path returns the single envelope, more paths return a batch report. |
| `render <file>` | `--sheet <name>` / `-s` (workbooks), `--page <n>` / `-p` (presentations), `--out <file>` / `-o`, `--out-dir <dir>`, `--overwrite` | SVG only. `--out` needs exactly one page; `--out-dir` writes `page-<n>.svg` per page. |
| `formats` | none | Import and export matrix. |
| `capabilities` | none | Capability matrix. |
| `convert <file>` | `--format <f>` / `-f` (required), `--out <file>` / `-o` (required), `--overwrite`, `--pages <ranges>` | Single-input form. `--pages` is PDF-only. |
| `convert <file>...` | `--format <f>` / `-f` (required), `--out-dir <dir>` (required), `--overwrite` | Multiple inputs. `--out`, `--out-dir` with one input, and `--pages` with multiple inputs are rejected. |
| `version` / `--version` / `-V` | none | Prints `open-office <version>`. |
| `help` / `--help` / `-h` | none | Prints usage. |

Unknown options are usage errors. `convert` lowercases `--format`, and batch targets are written as `<out-dir>/<source-stem>.<format>`.

`open` is a routing convenience, not an editor. It checks the extension and that the file exists, then hands the file to the platform. A missing opener or a non-zero exit reports `open-failed`; use the individual product CLI (`open-word open`, `open-excel open`, `open-ppt open`, `open-pdf open`) when the product GUI or reader should handle the file.

Exit codes: `0` success, `2` usage error, `1` runtime error. A batch conversion, a multi-file `probe`, `stats`, `outline`, `search` or `inspect` prints its JSON report and then exits `1` when `failed` is greater than zero.

## Report envelopes

Every command returns an object. The shared keys are `kind` (one of `word`, `excel`, `presentation`, `pdf`) and `path` where a document was read.

### `probe`

```json
{
  "path": "download.bin",
  "bytes": 12345,
  "extension": "bin",
  "format": null,
  "container": "zip",
  "confidence": "ambiguous",
  "candidates": ["docx", "xlsx", "pptx", "epub"],
  "evidence": "..."
}
```

- `container`: `plain`, `zip`, `ole-compound` or `pdf`.
- `confidence`: `verified`, `hinted`, `ambiguous` or `unknown`.
- `format` is `null` when the bytes do not prove a format; `candidates` keeps every package still possible and `evidence` quotes what was found.
- The extension is a hint only. `probe` accepts a path with no extension and a path whose extension is unknown.

With more than one path, `probe` returns
`{ probed, failed, results: [{ source, ok, format, container, confidence, candidates, evidence? } | { source, ok: false, code, error }] }`
in input order and exits non-zero when any file failed. `evidence` appears only
when the detector quoted some.

### `capabilities`

```json
{
  "engine": { "package": "open-office", "version": "0.1.0" },
  "commands": ["probe", "inspect", "convert", "formats", "stats", "outline", "search", "render", "capabilities", "version"],
  "modules": {
    "word": {},
    "excel": {},
    "presentation": {},
    "pdf": {}
  },
  "convert": {
    "word": ["docx", "pdf", "html", "md", "txt", "rtf"],
    "excel": ["xlsx", "csv", "tsv", "json", "html", "pdf"],
    "presentation": ["pptx", "ppsx", "potx", "pdf"],
    "pdf": ["md", "txt", "text"]
  }
}
```

Each `modules` value is the owning engine's own capability object, passed through unchanged. Do not infer support from the key alone; read that object or run `formats`.

### `formats`

```json
{
  "word": { "import": ["..."], "export": ["..."] },
  "excel": { "import": ["..."], "export": ["..."] },
  "presentation": { "import": ["..."], "export": ["..."] },
  "pdf": { "import": ["pdf"], "export": ["md", "txt", "text"] }
}
```

### `inspect`

The envelope differs per family because each engine reports different facts:

| Family | Top-level keys |
|--------|----------------|
| `word` | `kind`, `path`, `summary`, `stats`, `inspect` |
| `excel` | `kind`, `path`, `summary`, `inspect` |
| `presentation` | `kind`, `path`, `summary`, `losses` |
| `pdf` | `kind`, `path`, `inspect` |

- Word `summary` and `stats` are the document summary and statistics. `inspect` is the rule-check report.
- Excel `summary` is the workbook summary; `inspect` is the rule-check report.
- Presentation `summary` is the deck summary; `losses` lists what the PPTX import could not preserve.
- PDF `inspect` is the PDF engine report: `label`, `version`, `pages`, `encrypted`, `metadata`, `pageSizes`, `bookmarks`, `formFields`, `warnings`. Encrypted documents are rejected before inspection, so `encrypted` is `false` on a successful report.

### `stats`

```json
{ "kind": "pdf", "path": "paper.pdf", "stats": { "pages": 12, "version": "1.7", "encrypted": false } }
```

- `word`: the document statistics object (block, paragraph, heading, table, word and character counts).
- `excel`: the workbook summary object.
- `presentation`: the deck summary object plus `losses`.
- `pdf`: only `pages`, `version` and `encrypted`.

With more than one path, `stats` reports every input in order and does not stop
at the first failure:

```json
{
  "inspected": 1,
  "failed": 1,
  "results": [
    { "source": "report.docx", "ok": true, "kind": "word", "stats": {} },
    { "source": "missing.pdf", "ok": false, "code": "not-found", "error": "..." }
  ]
}
```

The batch form prints its JSON report and then exits `1` when `failed` is
greater than zero.

With more than one path, `inspect` reports every input in order and does not
stop at the first failure. Successful entries keep the full per-family
`inspect` envelope under `inspect`:

```json
{
  "inspected": 1,
  "failed": 1,
  "results": [
    { "source": "report.docx", "ok": true, "kind": "word", "inspect": { "kind": "word", "path": "report.docx", "summary": {}, "stats": {}, "inspect": {} } },
    { "source": "missing.pdf", "ok": false, "code": "not-found", "error": "..." }
  ]
}
```

The same batch rules apply: the JSON report is printed first, then the process
exits `1` when `failed` is greater than zero.

### `outline`

```json
{ "kind": "word", "path": "report.docx", "outline": { "revision": 3, "items": [] } }
```

| Family | `outline` payload |
|--------|-------------------|
| `word` | `revision`, `items`; each item has `id`, `level`, `title`, `blockIndex`, `children` |
| `excel` | `revision`, `sheets`; each sheet has `id`, `index`, `name`, `hidden`, `cells` |
| `presentation` | `revision`, `slides`; each slide has `id`, `index`, `title`, `hidden`, `elements`, `notes`, `points` |
| `pdf` | `pages`, `bookmarks`; each bookmark has `title`, `page`, `depth` |

Presentation outlines always include a `points` array, derived from the slide body text and capped by the deck engine; a slide without body text reports an empty array. `--points` is accepted for presentations and is a usage error for every other family.

`--depth <n>` keeps word headings and PDF bookmarks whose level or depth is `n` or shallower; deeper entries and their descendants are dropped. Workbooks and presentations reject it as a usage error.

### `search`

```json
{
  "kind": "excel",
  "path": "budget.xlsx",
  "query": "total",
  "limit": 20,
  "hits": [],
  "truncated": false,
  "notes": []
}
```

`limit` is the effective clamped value. `truncated` is true when the hit cap was reached. Matching is case-sensitive for all four families.

Hit shapes:

| Family | Hit fields |
|--------|------------|
| `word` | `block_id`, `block_index`, `path`, `start`, `end`, `text` |
| `excel` | `sheet`, `address`, `field` (`value` or `formula`), `start`, `end`, `text` |
| `presentation` | `slideId`, `slideIndex`, `elementId`, `source` (`notes`, `text` or `table`), `start`, `end`, `text`, `context` |
| `pdf` | `line`, `start`, `end`, `text`, `context` |

For Word hits, `path` is the enclosing section address or `null`. Offsets are character offsets, not byte offsets. PDF hits are line-scoped occurrences in the extracted Markdown, not PDF pages: `line` is 1-based and page numbers are not tracked per hit. PDF `notes` may also report how many pages lost extractable text.

With more than one path, `search` applies the same query and limit to every document and returns

`{ query, searched, failed, results: [{ source, ok, kind, search } | { source, ok: false, code, error }] }`

where `search` is the single-document envelope above. `limit` applies per document, not to the batch.

### `render`

```json
{
  "kind": "presentation",
  "path": "deck.pptx",
  "format": "svg",
  "revision": 3,
  "out": null,
  "outDir": null,
  "written": [],
  "totalPages": 2,
  "pages": [
    { "slideId": "slide-1", "index": 0, "widthPx": 960, "heightPx": 540, "svg": "<svg …" }
  ],
  "losses": []
}
```

A workbook yields exactly one page for the selected sheet (`sheet`, `sheetId`, `svg`); a presentation yields one page per slide. `--page` is one-based, selects a single presentation page and is refused for workbooks; `--sheet` is refused for presentations. `--out` writes the single selected SVG and fails with `multiple-pages` when the selection has more than one page; `--out-dir` writes every selected page as `page-<n>.svg`, numbered from the one-based page index. Both refuse existing targets unless `--overwrite` is passed and cannot be combined. Word and PDF are refused with `unsupported-target` because neither bundled engine exposes an SVG renderer in this build.

### `convert` (single input)

```json
{
  "source": "paper.pdf",
  "kind": "pdf",
  "format": "md",
  "target": "out/paper.md",
  "report": { "bytes": 1234, "losses": [] }
}
```

- Word, Excel and presentation `report` is the owning engine's export report.
- PDF `report` contains `bytes` and `losses`; with `--pages` it also contains `pages: { requested, extracted, numbers }`.

`--pages` accepts one-based ranges such as `1-3,5`. Zero, reversed ranges, duplicate pages, empty entries and pages beyond the document are refused. The option is rejected for non-PDF input and for batch conversion.

### `convert` (multiple inputs)

```json
{
  "converted": 1,
  "failed": 1,
  "results": [
    { "source": "a.docx", "target": "out/a.pdf", "ok": true, "report": {} },
    { "source": "b.bin", "target": "out/b.pdf", "ok": false, "code": "unsupported-extension", "error": "..." }
  ]
}
```

`results` follows input order. A failing document does not stop the batch. On success the entry's `report` is the same engine report the single-input form returns. On failure the entry carries `code` and `error`.

## Stable error codes

Codes are exposed to library callers and in batch `results[].code`. The CLI itself prints `open-office: <message>` and does not print the code, so read the JSON report when a code is needed.

| Code | Cause |
|------|-------|
| `not-found` | The input path does not exist. |
| `unreadable` | The file exists but cannot be read. |
| `missing-extension` | A routed command needs an extension and the path has none. |
| `unsupported-extension` | The extension is not a Word, workbook, presentation or PDF format in this build. |
| `container-mismatch` | The bytes contradict the extension route, for example a PDF renamed to `.xlsx` or plain text renamed to `.pdf`. |
| `unsupported-target` | `--format` is not one of the export targets the source family wires. |
| `target-exists` | The output exists and `--overwrite` was not passed. |
| `unsupported-option` | `--points` for a non-presentation, or `--pages` for a non-PDF. |
| `invalid-query` | `--text` was empty. |
| `invalid-pages` | A page range is empty, malformed, zero, reversed, duplicated or beyond the page count. |
| `conversion-failed` | The owning engine failed while exporting or extracting. |
| `output-unwritable` | The output directory cannot be created, or the PDF text target cannot be written. |
| `usage` | Generic usage failure with no more specific code. |
| `runtime` | Generic runtime failure with no more specific code. |

## Family routing

Routing is by lowercased extension:

| Family | Extensions |
|--------|------------|
| `word` | `docx`, `docm`, `rtf`, `txt`, `md`, `markdown`, `html`, `htm`, `oword` |
| `excel` | `xlsx`, `xlsm`, `csv`, `tsv`, `oexcel` |
| `presentation` | `pptx`, `pptm`, `ppsx`, `ppsm`, `potx`, `potm`, `project`, `oppt`, `opresent` |
| `pdf` | `pdf` |

`.project`, `.oppt` and `.opresent` open as native presentations; every other presentation extension is imported as PresentationML and reports import losses. Files without an extension fail with `missing-extension`; any other extension fails with `unsupported-extension`.

After routing, `inspect`, `stats`, `outline`, `search`, `render` and `convert` read the whole file and cross-check the detected container against the route. A mismatch is refused with the probe evidence. When the bytes do not resolve a family, the extension route stands. `probe` performs no routing and never opens the document.

## Guardrails

- Do not guess a format. Run `probe` when the extension is missing or doubtful, then use a command that routes that family.
- Do not rename a file to force a route. The byte check will reject a mismatched container.
- `--overwrite` is required to replace an existing output. There is no implicit replacement.
- `--points` applies to presentations only.
- `render` writes SVG only; `--sheet` is workbook-only and `--page` is presentation-only. `--out` needs a single selected page, while `--out-dir` writes every selected page.
- `--pages` applies to single-input PDF text conversion only; it cannot be combined with multiple inputs.
- Search is case-sensitive everywhere. A query that works in one family will not silently become case-insensitive in another.
- PDF search reports lines in extracted Markdown, not PDF pages.
- For batches, check `failed` and each `results[].code`; a zero exit code is not guaranteed even when the JSON report is printed.
- Capability claims come from the engines. If `capabilities` or `formats` does not list a route, do not plan around it.
