# OpenOffice maintainer guide

This repository builds `open-office`, a thin host over four sibling engine crates. The host owns routing, argument parsing, the conversion guardrails and the shared report envelope; the engines own parsing, inspection, statistics, outlines, search and export. Nothing in this repository re-implements a document model.

## Repository layout

```text
OpenOffice/
├── Cargo.toml                       workspace; default members are sub_projects and office
├── sibling.lock                     pinned sibling SHAs and rust-boost revisions (CI input)
├── projects/
│   ├── crates/office/               open-office library and binary
│   │   ├── src/lib.rs               routing, reports, conversion, verification
│   │   └── src/main.rs              CLI parser, help text, exit codes
│   └── sub_projects/                Rust template scaffold, not part of the product
├── documentation/
│   ├── README.md                    this guide
│   └── agents.md                    agent command and report reference
├── .github/workflows/rust.yml       CI
├── readme.md                        user-facing landing page
└── License.md                       MPL-2.0
```

`projects/sub_projects` is a leftover scaffold crate. It compiles as a default workspace member but the binary does not depend on it. Its `readme.md` is included as crate documentation by `src/lib.rs`, so it stays in place.

## Sibling repositories

The host links four engine crates through relative path dependencies:

| Expected checkout (same parent as this repository) | Crate | Used for |
|----------------------------------------------------|-------|----------|
| `../OpenWord/projects/crates/model` | `open-word-document` | Word open, summary, stats, inspect, find, outline, export |
| `../OpenExcel/projects/crates/model` | `open-excel-workbook` | Workbook open, summary, inspect, find, export |
| `../OpenPPT/projects/crates/deck` | `open-ppt-deck` | Deck open/import, summary, outline, export |
| `../OpenPDF/projects/crates/pdf` | `open-pdf-core` | PDF inspect, extract, arrange, bookmarks |

The manifest paths are relative (`../../../../OpenWord/...` from `projects/crates/office`), so all four repositories must be checked out beside `OpenOffice`. Clone the engines first, then work on the host.

The single binary has no vendored engine code. `cargo` compiles the four sibling sources into the host at build time, which is why the host is built after the engines: any engine API change has to land in that engine repository first, and the host is rebuilt and tested against that checkout. A host-only change must not be merged while a sibling is missing or stale.

## Build, test, format

Run from the repository root:

```bash
cargo build --release -p open-office
cargo test -p open-office
cargo fmt -p open-office -- --check
```

- The toolchain is pinned to `nightly` in `rust-toolchain.toml`; the office crate uses edition 2024.
- `rustfmt.toml` applies to the workspace (`max_width = 128`, `control_brace_style = "ClosingNextLine"`).
- `cargo build` resolves the four sibling checkouts and the pinned `rust-boost` dependencies; it does not fetch the sibling repositories.
- The host tests cover routing, probe evidence, container mismatch, page ranges, outline, search, batch error codes and conversion guardrails. Detailed format conformance and fixtures belong to each engine repository.

## Dependency pin discipline

`open-office` depends on `document-formats` from `rust-boost` with the `package-detect` feature. That feature is what `probe` and the byte-level route check use; do not drop it.

Current pins:

| Repository | `rust-boost` revision |
|------------|-----------------------|
| `OpenOffice` (host) | `7664fcbca34da7882ee7eecd69d185ee694110fc` |
| `OpenWord` | `1c2dbba30d576794bbbe51fdd2e9f5f4f8fe59d1` |
| `OpenExcel` | `0ded81d40572c38177040e0bfaff7a0f8c78769c` |
| `OpenPPT` | `0270cfe33e30f4a56bd2001679d8ec21dab7dcd0` |
| `OpenPDF` | `2246d76b9ec083f738e784e8af6ec0182f90b33f` |

Treat the pin as one shared decision. When bumping it, update the revision in every consumer that is being moved in the same change, then regenerate the lockfile. Leaving one repository on an older revision puts two `rust-boost` source trees in the graph: build time grows and shared types can exist twice, which is exactly the duplication this host is meant to avoid. As of the 2026-11-25 re-verification below each engine has moved its own pin forward past the host's `7664fcbc`, so the graph now carries five `rust-boost` revisions instead of the previous two. The duplication is recorded here as fact and not masked by `cargo update`; closing it is a coordinated five-repo bump outside this host's control.

`Cargo.lock` is ignored by git in this repository, so use the lockfile locally to confirm which revisions the build actually selected.

## Source ledger (clean-room, 2026-11-25)

The pinned combination was re-verified on 2026-11-25 17:39 +08:00 (sibling `origin/dev` fetched at 17:34 +08:00) with `cargo metadata --locked --format-version 1` and `cargo test` against the five repositories checked out at the SHAs below. The four engine SHAs equal the live `origin/dev` tips at that moment (parallel work may move them later; CI consumes the pinned SHAs, not tips). The engines were consumed as detached worktrees at these exact tips because the main `OpenExcel` working tree carried unrelated uncommitted edits mid-task, so the verified build reflects committed code only. The graph resolves from the committed lockfile and now carries five `rust-boost` revisions: this host on `7664fcbc` and each engine on its own newer pin (`OpenWord` `1c2dbba3`, `OpenExcel` `0ded81d4`, `OpenPPT` `0270cfe3`, `OpenPDF` `2246d76b`). `cargo metadata --locked` is clean and `cargo test` passes 62 tests with 0 failures (61 in `tests/office.rs`, 1 in `sub_projects`), including the new single-binary acceptance test that opens, edits, saves and re-verifies a real DOCX, XLSX, PPTX and PDF through one `Office` instance in the same process. This ledger is the provenance record the plan requires; `sibling.lock` is the machine-readable build input CI consumes.

Host commit: `OpenOffice` @ `daea9ab073f0f35aa4bdca87d6c18a60dfb19e8f`.

Engine crate path dependencies (resolved sibling SHA = live `origin/dev` tip at the fetch moment; `sibling.lock` still carries the previous combination `ad76e2f` / `01639b5` / `c3748ee` / `d4f5dce`, and re-pointing it is a separate accounting change):

| Engine crate | Path (relative to this repository's parent) | Resolved sibling SHA |
|--------------|---------------------------------------------|----------------------|
| `open-word-document` | `OpenWord/projects/crates/model` | `c13cf0669430059756726eeded137b9d224c6287` |
| `open-excel-workbook` | `OpenExcel/projects/crates/model` | `0f559fd537ea689a14b906b90c4dd1482545f676` |
| `open-ppt-deck` | `OpenPPT/projects/crates/deck` | `45bc20c3222f0aa3cc087c774edbf3327544abd7` |
| `open-pdf-core` | `OpenPDF/projects/crates/pdf` | `25bb3edd5edc2298641c67c6d56384a3dc467687` |

The `OpenPDF` replacement gate is cleared. Its dev tip `afb4b1a` carries the typed error-contract change: the product surface owns a local `ServiceError` with `code`, `message` and `params` instead of re-exporting `document_pdf::ServiceError`. The host absorbed that contract — the `EngineFailure` adapter targets the product-surface type and forwards its typed parameters, and the shared `document-pdf` entry points that still report the upstream string error are lifted through the product surface's lossless `From` conversion, so codes and parameters survive routing unchanged. Its current dev tip `d4f5dce` adds the switch of page ordering and link extraction to the upstream `document-pdf` compose and links entry points, deleting the product-side split-and-merge workarounds, and threads named destinations through `list_destinations`, the napi binding, the SDK and the CLI. The re-pointed combination was verified on the host with `cargo test --offline` against the sibling working trees at the SHAs recorded for that moment; the CI rebuild from `sibling.lock` is the clean-room proof.

Boost git dependencies, resolved by full source URL and 40-character revision. Five revisions appear in the graph at the 17:39 re-verification: `7664fcbc` is the pin of this host manifest, while the engine manifests each pin their own newer descendant (`OpenWord` `1c2dbba3`, `OpenExcel` `0ded81d4`, `OpenPPT` `0270cfe3`, `OpenPDF` `2246d76b`). 116 packages resolve from `rust-boost` and 24 crate names resolve more than once across the five trees (`boost-fs`, `document-formats`, `document-ir`, `document-session` and friends); the build and tests pass with the duplication and it is tracked as the open coordinated-bump item above.

| Full source URL | Crates resolved from it |
|-----------------|-------------------------|
| `git+https://github.com/oovm/rust-boost?rev=7664fcbca34da7882ee7eecd69d185ee694110fc#7664fcbca34da7882ee7eecd69d185ee694110fc` (host) | `boost-collection`, `boost-collection-macro`, `boost-console`, `boost-fs`, `boost-harness`, `boost-harness-macro`, `boost-math`, `boost-random`, `boost-source`, `data-binary-core`, `data-binary-zip`, `data-checksum`, `data-compression`, `document-formats`, `document-ir` |
| `git+https://github.com/oovm/rust-boost?rev=1c2dbba30d576794bbbe51fdd2e9f5f4f8fe59d1#1c2dbba30d576794bbbe51fdd2e9f5f4f8fe59d1` (OpenWord) | `boost-collection`, `boost-collection-macro`, `boost-console`, `boost-fs`, `boost-harness`, `boost-harness-macro`, `boost-math`, `boost-random`, `boost-source`, `data-binary-core`, `data-binary-docx`, `data-binary-zip`, `data-checksum`, `data-compression`, `data-text-core`, `data-text-css-selector`, `data-text-html`, `data-text-markdown`, `data-text-tex`, `data-text-xml`, `data-text-xpath`, `document-formats`, `document-ir`, `document-session` |
| `git+https://github.com/oovm/rust-boost?rev=0ded81d40572c38177040e0bfaff7a0f8c78769c#0ded81d40572c38177040e0bfaff7a0f8c78769c` (OpenExcel) | `boost-collection`, `boost-collection-macro`, `boost-console`, `boost-fs`, `boost-harness`, `boost-harness-macro`, `boost-math`, `boost-random`, `boost-source`, `data-binary-core`, `data-binary-docx`, `data-binary-zip`, `data-checksum`, `data-compression`, `data-text-core`, `data-text-csv`, `data-text-dsv`, `data-text-xml`, `data-text-xpath`, `document-formats`, `document-ir`, `document-session`, `document-spreadsheet-formula` |
| `git+https://github.com/oovm/rust-boost?rev=0270cfe33e30f4a56bd2001679d8ec21dab7dcd0#0270cfe33e30f4a56bd2001679d8ec21dab7dcd0` (OpenPPT) | `boost-collection`, `boost-collection-macro`, `boost-console`, `boost-fs`, `boost-harness`, `boost-harness-macro`, `boost-math`, `boost-random`, `boost-source`, `data-binary-core`, `data-binary-docx`, `data-binary-ttf`, `data-binary-zip`, `data-checksum`, `data-compression`, `data-encoding`, `data-text-core`, `data-text-css-selector`, `data-text-html`, `data-text-markdown`, `data-text-tex`, `data-text-xml`, `data-text-xpath`, `document-compose`, `document-fonts`, `document-formats`, `document-ir`, `document-layout`, `document-presentation`, `document-presentation-pdf`, `document-presentation-pptx`, `document-renderer`, `document-renderer-pdf`, `document-text`, `plotter-scene`, `plotter-types` |
| `git+https://github.com/oovm/rust-boost?rev=2246d76b9ec083f738e784e8af6ec0182f90b33f#2246d76b9ec083f738e784e8af6ec0182f90b33f` (OpenPDF) | `boost-collection`, `boost-collection-macro`, `boost-console`, `boost-fs`, `boost-harness`, `boost-harness-macro`, `boost-math`, `boost-random`, `boost-source`, `data-binary-core`, `data-binary-pdf`, `data-checksum`, `data-compression`, `document-pdf`, `geometry-types`, `media-codec`, `media-image`, `media-types` |

## CI

`.github/workflows/rust.yml` runs on pushes and pull requests to `master` and `dev`, across `ubuntu-latest`, `macos-latest` and `windows-latest`. It installs a nightly toolchain with the `rustfmt` and `clippy` components, then runs:

```bash
cargo build --release
cargo test --release
```

The workflow reads `sibling.lock` and checks out the four engines at those exact SHAs (not `dev` tips), then verifies the host manifest still pins the locked `rust-boost` revision, so every CI run builds the recorded combination from scratch. A pinned SHA must be reachable on the sibling's `dev` branch before this workflow can fetch it. The workflow installs formatting and lint components but runs neither `cargo fmt --check` nor `cargo clippy`. Check formatting locally with `cargo fmt -p open-office -- --check`; clippy is not part of the checks described here.

## Release and publishing

There is no release automation and no release tag in this repository. The `open-office` package sets `publish = false`, and the root `package.json` is private and only provides local convenience scripts (`cargo doc`, `cargo test`, `cargo fmt`, `git push`, `cargo upgrade`). Nothing is published to crates.io or npm; the distributable artifact is the compiled `open-office` binary. `nifty.config.ts` declares a Trusted Publisher target for `nyar-os/OpenOffice`, but no `publish-npm.yml` exists because there is no npm artifact to publish.

## Documentation map

- `readme.md` - user-facing landing page: what the product is, agent usage, install and supported targets.
- `documentation/agents.md` - agent reference: commands, flags, report shapes, error codes, routing and guardrails.
- `documentation/README.md` - this maintainer runbook.
