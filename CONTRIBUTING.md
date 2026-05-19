# Contributing to Continuum

Thanks for your interest in Continuum. This guide covers the local workflow and
the bar a change has to clear.

## Prerequisites

- A recent stable Rust toolchain (`rustup` recommended).
- A C compiler — the build compiles bundled C for SQLite and the tree-sitter
  grammars. On Linux/macOS the system compiler is enough; on Windows use either
  the MSVC build tools or an llvm-mingw toolchain.

> **Windows without Visual Studio:** build against the llvm-mingw target —
> `rustup target add x86_64-pc-windows-gnullvm` and add a `.cargo/config.toml`
> selecting it. That file is git-ignored so it never affects other contributors.

## Build, test, lint

```sh
cargo build --workspace
cargo test  --workspace
cargo fmt   --all
cargo clippy --workspace --all-targets -- -D warnings
```

CI runs exactly these (`fmt --check`, `clippy -D warnings`, build, and test on
Linux and Windows). A change must pass all of them.

## Architecture

Read [DESIGN.md](DESIGN.md) first. The workspace is split into focused crates
with a strict dependency direction — keep transport, graph, indexer, memory, and
search concerns isolated, and never leak tree-sitter or graph types across the
transport boundary. Use the DTOs in `continuum-core`.

## Conventions

- **Commits** follow [Conventional Commits](https://www.conventionalcommits.org)
  (`feat:`, `fix:`, `test:`, `docs:`, `refactor:`, …).
- **Errors** are propagated, never `panic!`. Library crates use `thiserror`;
  binaries may use `anyhow`. Transport-layer errors become JSON-RPC errors.
- **Files stay small** — one type and its immediate methods per file.
- New behaviour comes with tests.

## Pull requests

Keep PRs focused. Describe the change and how you verified it. Green CI is
required before review.
