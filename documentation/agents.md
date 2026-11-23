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
| `inspect <file>...` | `--checks <list>` (optional) | Dispatches to the owning engine; one path returns the single envelope, more paths return a batch report. `--checks` is a comma-separated list of rule-check names and applies to Word, Excel and presentation inputs. |
| `stats <file>...` | none | Lightweight per-family statistics; one path returns the single envelope, more paths return a batch report. |
| `totals <file>...` | none | Aggregates the statistics of every input into one suite-level report with per-family sums; a failing document is counted in `failed` and does not stop the batch. |
| `metadata <file>...` | one or more paths | Reports the descriptive metadata the owning engine stores: `{ title }` for word, workbook and presentation files, and the whole PDF information dictionary (`title`, `author`, `subject`, `keywords`, `creator`, `producer`) for PDFs. Nothing is derived, so a missing field is `null`. With several paths it reports a best-effort batch (`counted`, `failed`, `results`) instead of stopping at the first failure. |
| `outline <file>...` | `--points` (presentations), `--depth <n>` (word and PDF) | `--points` is accepted only for presentations. `--depth` keeps word headings and PDF bookmarks at that level or shallower and is rejected for other families. One path returns the single envelope, more paths return a batch report. |
| `compare <left> <right>` | exactly two paths, optional `--ignore-case` / `-i`, `--depth <n>`, `--points`, `--notes` | Outline-level diff of two documents of the same family: word heading titles, workbook sheet names, slide titles (or `Title · point` entries with `--points`, or `Title · note line` entries with `--notes`) or PDF bookmark titles, aligned in document order into `added`, `removed`, `renamed` and `moved` sections. `--ignore-case` folds case, so a pure case change is not a rename. `--depth` keeps only word headings and PDF bookmarks at that level or shallower and is rejected for other families. `--points` expands presentation body points and `--notes` expands presentation speaker-note lines; both are rejected for other families and cannot be combined. Different families fail with `kind-mismatch`. |
| `search <file>...` | `--text <query>` / `-t` (required), `--limit <n>` (optional) | Case-sensitive in every family. Default limit is 100, clamped to 1-500. One path returns the single envelope, more paths return a batch report. |
| `replace <file>...` | `--find <query>` / `-f` (required), `--with <replacement>` / `-w` (required), `--ignore-case` / `-i`, `--limit <n>`, `--dry-run` (optional) | Find-and-replace saved in place for word documents, workbooks and presentations; PDFs fail with `unsupported-target`. Matching is case-sensitive unless `--ignore-case`. `--dry-run` counts matches but writes nothing. One path returns the single envelope, more paths return a batch report. |
| `render <file>...` | `--sheet <name>` / `-s` (workbooks), `--page <n>` / `-p` or `--pages <list>` (presentations), `--out <file>` / `-o`, `--out-dir <dir>`, `--overwrite` | SVG only. `--out` needs exactly one page; `--out-dir` writes `page-<n>.svg` per page. Several paths require `--out-dir` and get one subdirectory per source stem. |
| `formats` | none | Import and export matrix. |
| `capabilities` | none | Capability matrix. |
| `convert <file>` | `--format <f>` / `-f` (required), `--out <file>` / `-o` (required), `--overwrite`, `--pages <ranges>` | Single-input form. `--pages` is PDF-only. |
| `convert <file>...` | `--format <f>` / `-f` (required), `--out-dir <dir>` (required), `--overwrite` | Multiple inputs. `--out`, `--out-dir` with one input, and `--pages` with multiple inputs are rejected. |
| `version` / `--version` / `-V` | none | Prints `open-office <version>`. |
| `help` / `--help` / `-h` | none | Prints usage. |

Unknown options are usage errors. `convert` lowercases `--format`, and batch targets are written as `<out-dir>/<source-stem>.<format>`.

`open` is a routing convenience, not an editor. It checks the extension and that the file exists, then hands the file to the platform. A missing opener or a non-zero exit reports `open-failed`; use the individual product CLI (`open-word open`, `open-excel open`, `open-ppt open`, `open-pdf open`) when the product GUI or reader should handle the file.

Exit codes: `0` success, `2` usage error, `1` runtime error. A batch conversion, a multi-file `probe`, `stats`, `totals`, `outline`, `search`, `replace` or `inspect` prints its JSON report and then exits `1` when `failed` is greater than zero.

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
  "commands": ["probe", "inspect", "convert", "open", "formats", "stats", "totals", "metadata", "outline", "compare", "search", "replace", "render", "capabilities", "version"],
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
| `word` | `kind`, `path`, `checks`, `summary`, `stats`, `inspect` |
| `excel` | `kind`, `path`, `checks`, `summary`, `inspect` |
| `presentation` | `kind`, `path`, `checks`, `summary`, `losses`, `inspect` |
| `pdf` | `kind`, `path`, `checks`, `inspect` |

- `checks` echoes the requested check names, or `null` when every check ran.
- Word `summary` and `stats` are the document summary and statistics. `inspect` is the rule-check report.
- Excel `summary` is the workbook summary; `inspect` is the rule-check report.
- Presentation `summary` is the deck summary; `losses` lists what the PPTX import could not preserve; `inspect` is the deck rule-check report.
- PDF `inspect` is the PDF engine report: `label`, `version`, `pages`, `encrypted`, `metadata`, `pageSizes`, `bookmarks`, `formFields`, `warnings`. Encrypted documents are rejected before inspection, so `encrypted` is `false` on a successful report. PDF has no selectable checks, so `--checks` on a PDF is a usage error.

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

### `totals`

```json
{
  "counted": 2,
  "failed": 0,
  "families": {
    "word": { "documents": 1, "blocks": 4, "paragraphs": 2, "headings": 1, "tables": 0, "words": 9, "characters": 42 },
    "excel": { "documents": 0, "sheets": 0, "cells": 0 },
    "presentation": { "documents": 1, "slides": 3 },
    "pdf": { "documents": 0, "pages": 0 }
  },
  "results": [
    { "source": "report.docx", "ok": true, "kind": "word", "totals": { "blocks": 4, "paragraphs": 2, "headings": 1, "tables": 0, "words": 9, "characters": 42 } },
    { "source": "deck.pptx", "ok": true, "kind": "presentation", "totals": { "slides": 3 } }
  ]
}
```

Every number is the owning engine's own `stats` value, summed per family across
the inputs; word totals add up blocks, paragraphs, headings (all levels),
tables, words and characters, workbooks add up sheets and cells, presentations
add up slides and PDFs add up pages. `families` always lists all four families,
even the empty ones. A failing document appears only in `results` with its
stable `code` and `error`, and the command exits `1` after printing the report.

### `metadata`

`metadata` reads one file and keeps the fields its family actually stores:

```json
{ "kind": "pdf", "path": "paper.pdf", "metadata": { "title": "Paper", "author": null, "subject": null, "keywords": null, "creator": null, "producer": "OpenPDF" } }
```

Word, workbook and presentation engines keep a title only, so their `metadata`
holds `{ "title": ... }` and nothing else. PDFs answer with the information
dictionary. Missing values stay `null`; nothing is synthesized from the file
name or the first heading.

With several paths, `metadata` switches to a best-effort batch report:

```json
{ "counted": 2, "failed": 1, "results": [ { "source": "brief.oword", "ok": true, "kind": "word", "metadata": { "title": "Brief" } }, { "source": "missing.oword", "ok": false, "code": "io/not-found", "error": "..." } ] }
```

Each input is attempted in order; a failing document contributes only its
stable `code` and `error`, and the command exits `1` after printing the report.

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

### `compare`

```json
{
  "kind": "word",
  "left": { "path": "draft.docx", "sections": 3 },
  "right": { "path": "final.docx", "sections": 4 },
  "ignoreCase": false,
  "depth": null,
  "points": false,
  "notes": false,
  "identical": false,
  "added": ["Appendix"],
  "removed": [],
  "renamed": [{ "from": "Details", "to": "Summary" }],
  "moved": [{ "title": "Intro", "from": 0, "to": 2 }]
}
```

Sections come from the owning engine's own outline projection, flattened to titles in document order: word heading titles (nested children flattened), workbook sheet names, slide titles and PDF bookmark titles. The two lists are aligned with a longest-common-subsequence walk. A title that exists on both sides outside that subsequence only changed position, so it is reported under `moved` with its zero-based `from` and `to` indexes instead of counting as an add plus a remove. Inside each remaining change block a removed title paired with an added title is reported as `renamed`, and unpaired entries stay in `added` or `removed`. `identical` is true only when all four lists are empty; with `--ignore-case` titles compare after lowercasing, and the report echoes `ignoreCase`. With `--depth` both outlines are trimmed to that level first, so a deeper change is invisible to a shallow comparison; the report echoes `depth`, which is `null` when unbounded, and workbooks and presentations reject the option like `outline` does. With `--points` each presentation slide contributes one `Title · point` entry per body point, so added, removed or renamed bullets are reported; the report echoes `points`, and other families reject the option. With `--notes` each presentation slide contributes one `Title · line` entry per non-empty speaker-note line (lines are trimmed, blank lines dropped, and a slide without notes falls back to its title), so note edits show up as added, removed or renamed entries; the report echoes `notes`, other families reject the option, and `--points` and `--notes` are mutually exclusive (`unsupported-option`). The two paths must route to the same family, otherwise the command fails with `kind-mismatch`; every other guardrail of `outline` (missing file, wrong container, unsupported extension) applies to each side.

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

### `replace`

```json
{
  "kind": "word",
  "path": "notes.md",
  "find": "Acme",
  "replace": "Globex",
  "matchCase": true,
  "replaced": 2,
  "revision": 2,
  "saved": { "format": "md", "report": {} }
}
```

The request reaches the owning engine's own find-and-replace: Word covers section titles, paragraphs, list items and table cells; workbooks cover the text cells of every sheet that holds content (formulas are left alone); presentations cover slide text, table cells and speaker notes. `--limit` caps replacements per document for Word and per sheet for workbooks, and is ignored for presentations. Only a document that changed is written back, in its own format (`oword`/`oexcel` projects save natively, `project`/`oppt`/`opresent` decks save natively, every other extension re-exports through the engine's export route). A deck imports from its PresentationML package and re-exports it, so its import and export losses are the same as `convert`. A document with no match reports `replaced: 0` and `saved: null` and its file is untouched. `--dry-run` reports the matches it would rewrite, sets `dryRun: true` and keeps `saved: null`, so the file bytes never change. PDFs fail with `unsupported-target`, and macro-enabled presentation packages have no write-back route and fail the same way.

With more than one path, `replace` returns

`{ find, replace, dryRun, updated, failed, results: [{ source, ok, kind, replaced, revision } | { source, ok: false, code, error }] }`

in input order and exits non-zero when any document failed.

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

A workbook yields exactly one page for the selected sheet (`sheet`, `sheetId`, `svg`); a presentation yields one page per slide. `--page` is one-based, selects a single presentation page and is refused for workbooks; `--pages` selects several presentation pages in the requested order (`2,1` renders slide two then slide one) and cannot be combined with `--page`. `--sheet` is refused for presentations. `--out` writes the single selected SVG and fails with `multiple-pages` when the selection has more than one page; `--out-dir` writes every selected page as `page-<n>.svg`, numbered from the one-based page index. Both refuse existing targets unless `--overwrite` is passed and cannot be combined. Word and PDF are refused with `unsupported-target` because neither bundled engine exposes an SVG renderer in this build.

With more than one path, `render` writes into `<out-dir>/<source stem>/` and returns

```json
{
  "outDir": "preview",
  "rendered": 2,
  "failed": 1,
  "results": [
    { "source": "budget.xlsx", "outDir": "preview/budget", "ok": true, "totalPages": 1, "written": ["preview/budget/page-1.svg"] },
    { "source": "notes.docx", "outDir": "preview/notes", "ok": false, "code": "unsupported-target", "error": "…" }
  ]
}
```

`--out` is refused for several documents (`unsupported-option`) because one file cannot hold several selections; pass `--out-dir`. A failing document does not stop the batch, and the command exits non-zero when `failed` is above zero. `--sheet` and `--page` apply to every document, so a mixed batch that pairs `--page` with a workbook still reports that workbook as failed instead of aborting.

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
| `unsupported-option` | `--points` or `--notes` for a non-presentation, `--points` with `--notes`, `--pages` for a non-PDF, or `--checks` on a PDF. |
| `invalid-query` | `--text` or `--find` was empty. |
| `kind-mismatch` | `compare` was given two documents from different families. |
| `invalid-pages` | A page range is empty, malformed, zero, reversed, duplicated or beyond the page count. |
| `conversion-failed` | The owning engine failed while exporting or extracting. |
| `replace-failed` | The owning engine failed while replacing text or saving a replaced document back in place. |
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

After routing, `inspect`, `stats`, `totals`, `metadata`, `outline`, `search`, `render` and `convert` read the whole file and cross-check the detected container against the route. A mismatch is refused with the probe evidence. When the bytes do not resolve a family, the extension route stands. `probe` performs no routing and never opens the document.

## Guardrails

- Do not guess a format. Run `probe` when the extension is missing or doubtful, then use a command that routes that family.
- Do not rename a file to force a route. The byte check will reject a mismatched container.
- `--overwrite` is required to replace an existing output. There is no implicit replacement.
- `--points` and `--notes` apply to presentations only and cannot be combined.
- `inspect --checks` names come from the owning engine's `checks` map in `capabilities`; an unknown name fails that input. PDF has no selectable checks.
- `render` writes SVG only; `--sheet` is workbook-only and `--page` / `--pages` are presentation-only and mutually exclusive. `--out` needs a single selected page, while `--out-dir` writes every selected page. A batch needs `--out-dir` and writes one subdirectory per source stem.
- `--pages` in `convert` applies to single-input PDF text conversion only; in `render` it selects presentation pages. Neither can be combined with multiple inputs in `convert`, nor with `--page` in `render`.
- Search is case-sensitive everywhere. A query that works in one family will not silently become case-insensitive in another.
- `replace` writes changed documents back to their own path; PDFs have no replacement route. A `--find` that matches nothing leaves the file untouched.
- PDF search reports lines in extracted Markdown, not PDF pages.
- For batches, check `failed` and each `results[].code`; a zero exit code is not guaranteed even when the JSON report is printed.
- Capability claims come from the engines. If `capabilities` or `formats` does not list a route, do not plan around it.
