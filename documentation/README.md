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
| `OpenWord` | `7664fcbca34da7882ee7eecd69d185ee694110fc` |
| `OpenExcel` | `7664fcbca34da7882ee7eecd69d185ee694110fc` |
| `OpenPPT` | `9fd602b93a9a456da4ba824e22c90b600f12fdf0` |
| `OpenPDF` | `7664fcbca34da7882ee7eecd69d185ee694110fc` |

Treat the pin as one shared decision. When bumping it, update the revision in every consumer that is being moved in the same change, then regenerate the lockfile. Leaving one repository on an older revision puts two `rust-boost` source trees in the graph: build time grows and shared types can exist twice, which is exactly the duplication this host is meant to avoid. That state was closed on 2026-11-25 when `OpenPDF` moved onto the shared revision. The same day `OpenPPT` moved its own pin forward to `9fd602b9` (a descendant of `7664fcbc`) at dev tip `c3748ee`, so the graph again carries two revisions. The duplication is recorded here as fact and not masked by `cargo update`; closing it is a coordinated five-repo bump outside this host's control.

`Cargo.lock` is ignored by git in this repository, so use the lockfile locally to confirm which revisions the build actually selected.

## Source ledger (clean-room, 2026-11-25)

The pinned combination was re-verified on 2026-11-25 15:57 +08:00 with `cargo metadata --locked --format-version 1` and `cargo test` against the five sibling working trees checked out at the SHAs below. The four engine SHAs equal the live `origin/dev` tips at that moment (parallel work may move them later; CI consumes the pinned SHAs, not tips). The graph resolves from the committed lockfile and carries two `rust-boost` revisions: this host, `OpenWord`, `OpenExcel` and `OpenPDF` on `7664fcbc`, `OpenPPT` on its own descendant pin `9fd602b9`. `cargo metadata --locked` is clean and `cargo test` passes 61 tests with 0 failures (60 in `tests/office.rs`, 1 in `sub_projects`). This ledger is the provenance record the plan requires; `sibling.lock` is the machine-readable build input CI consumes.

Host commit: `OpenOffice` @ `972f8297b07d8d7a1db768cf5e2f6161aea81cf5`.

Engine crate path dependencies (resolved sibling SHA = checkout pinned in `sibling.lock`):

| Engine crate | Path (relative to this repository's parent) | Resolved sibling SHA |
|--------------|---------------------------------------------|----------------------|
| `open-word-document` | `OpenWord/projects/crates/model` | `ad76e2f080b8f35bb53160f673a88b83a42cb6a3` |
| `open-excel-workbook` | `OpenExcel/projects/crates/model` | `01639b5c04b80468dfea05154c42f2e33c674898` |
| `open-ppt-deck` | `OpenPPT/projects/crates/deck` | `c3748ee06d07a5625ec519549c7ee7f13abd341a` |
| `open-pdf-core` | `OpenPDF/projects/crates/pdf` | `d4f5dce395b833b616adf3174fa9e4744842d5e4` |

The `OpenPDF` replacement gate is cleared. Its dev tip `afb4b1a` carries the typed error-contract change: the product surface owns a local `ServiceError` with `code`, `message` and `params` instead of re-exporting `document_pdf::ServiceError`. The host absorbed that contract — the `EngineFailure` adapter targets the product-surface type and forwards its typed parameters, and the shared `document-pdf` entry points that still report the upstream string error are lifted through the product surface's lossless `From` conversion, so codes and parameters survive routing unchanged. Its current dev tip `d4f5dce` adds the switch of page ordering and link extraction to the upstream `document-pdf` compose and links entry points, deleting the product-side split-and-merge workarounds, and threads named destinations through `list_destinations`, the napi binding, the SDK and the CLI. The re-pointed combination was verified on the host with `cargo test --offline` against the sibling working trees at the SHAs above; the CI rebuild from `sibling.lock` is the clean-room proof.

Boost git dependencies, resolved by full source URL and 40-character revision. Two revisions appear in the graph: `7664fcbc` is the pin of this host manifest (and of `OpenWord`, `OpenExcel` and `OpenPDF`), `9fd602b9` is `OpenPPT`'s own pin at `c3748ee`. 23 crate names resolve twice across the two trees (`boost-fs`, `document-formats`, `document-ir` and friends); the build and tests pass with the duplication and it is tracked as the open coordinated-bump item above.

| Full source URL | Crates resolved from it |
|-----------------|-------------------------|
| `git+https://github.com/oovm/rust-boost?rev=7664fcbca34da7882ee7eecd69d185ee694110fc#7664fcbca34da7882ee7eecd69d185ee694110fc` | `boost-collection`, `boost-collection-macro`, `boost-console`, `boost-fs`, `boost-harness`, `boost-harness-macro`, `boost-math`, `boost-random`, `boost-source`, `data-binary-core`, `data-binary-docx`, `data-binary-pdf`, `data-binary-zip`, `data-checksum`, `data-compression`, `data-text-core`, `data-text-css-selector`, `data-text-csv`, `data-text-dsv`, `data-text-html`, `data-text-markdown`, `data-text-tex`, `data-text-xml`, `data-text-xpath`, `document-formats`, `document-ir`, `document-pdf`, `document-session`, `document-spreadsheet-formula`, `geometry-types`, `media-codec`, `media-image`, `media-types` |
| `git+https://github.com/oovm/rust-boost?rev=9fd602b93a9a456da4ba824e22c90b600f12fdf0#9fd602b93a9a456da4ba824e22c90b600f12fdf0` | `boost-collection`, `boost-collection-macro`, `boost-console`, `boost-fs`, `boost-harness`, `boost-harness-macro`, `boost-math`, `boost-random`, `boost-source`, `data-binary-core`, `data-binary-docx`, `data-binary-ttf`, `data-binary-zip`, `data-checksum`, `data-compression`, `data-encoding`, `data-text-core`, `data-text-css-selector`, `data-text-html`, `data-text-markdown`, `data-text-tex`, `data-text-xml`, `data-text-xpath`, `document-compose`, `document-fonts`, `document-formats`, `document-ir`, `document-layout`, `document-presentation`, `document-presentation-pdf`, `document-presentation-pptx`, `document-renderer`, `document-renderer-pdf`, `document-text`, `plotter-scene`, `plotter-types` |

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
