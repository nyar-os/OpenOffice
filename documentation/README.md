# OpenOffice maintainer guide

This repository builds `open-office`, a thin host over four sibling engine crates. The host owns routing, argument parsing, the conversion guardrails and the shared report envelope; the engines own parsing, inspection, statistics, outlines, search and export. Nothing in this repository re-implements a document model.

## Repository layout

```text
OpenOffice/
├── Cargo.toml                       workspace; default members are sub_projects and office
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
| `OpenOffice` (host) | `ad76d3060dc8c0e219ba1016f88bf5ab905d48d9` |
| `OpenWord` | `ad76d3060dc8c0e219ba1016f88bf5ab905d48d9` |
| `OpenExcel` | `ad76d3060dc8c0e219ba1016f88bf5ab905d48d9` |
| `OpenPPT` | `ad76d3060dc8c0e219ba1016f88bf5ab905d48d9` |
| `OpenPDF` | `d84299a951f16961ea24f51be711d0eef97ce491` |

Treat the pin as one shared decision. When bumping it, update the revision in every consumer that is being moved in the same change, then regenerate the lockfile. Leaving one repository on an older revision puts two `rust-boost` source trees in the graph: build time grows and shared types can exist twice, which is exactly the duplication this host is meant to avoid. The current `Cargo.lock` resolves both revisions because `OpenPDF` trails the others; that is a state to close, not a pattern to copy.

`Cargo.lock` is ignored by git in this repository, so use the lockfile locally to confirm which revisions the build actually selected.

## CI

`.github/workflows/rust.yml` runs on pushes and pull requests to `master` and `dev`, across `ubuntu-latest`, `macos-latest` and `windows-latest`. It installs a nightly toolchain with the `rustfmt` and `clippy` components, then runs:

```bash
cargo build --release
cargo test --release
```

The workflow runs from the repository root and checks out only this repository. It has no steps that clone or place `OpenWord`, `OpenExcel`, `OpenPPT` and `OpenPDF` at the expected relative paths, so it assumes those checkouts already exist in the runner workspace. Add sibling checkout steps before relying on the workflow in a fresh CI environment. The workflow installs formatting and lint components but runs neither `cargo fmt --check` nor `cargo clippy`. Check formatting locally with `cargo fmt -p open-office -- --check`; clippy is not part of the checks described here.

## Release and publishing

There is no release automation and no release tag in this repository. The `open-office` package sets `publish = false`, and the root `package.json` is private and only provides local convenience scripts (`cargo doc`, `cargo test`, `cargo fmt`, `git push`, `cargo upgrade`). Nothing is published to crates.io or npm; the distributable artifact is the compiled `open-office` binary. `nifty.config.ts` declares a Trusted Publisher target for `nyar-os/OpenOffice`, but no `publish-npm.yml` exists because there is no npm artifact to publish.

## Documentation map

- `readme.md` - user-facing landing page: what the product is, agent usage, install and supported targets.
- `documentation/agents.md` - agent reference: commands, flags, report shapes, error codes, routing and guardrails.
- `documentation/README.md` - this maintainer runbook.
