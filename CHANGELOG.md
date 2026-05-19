# Changelog

All notable changes to Continuum are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Multi-agent MCP server with a daemon + thin-adapter architecture over TCP
  loopback, with a token handshake and one daemon per workspace.
- Code knowledge graph built from tree-sitter parsing of Rust, Python,
  JavaScript, TypeScript, and Go, kept current by a debounced filesystem
  watcher.
- 13 MCP tools: `search_code`, `find_text`, `get_file_outline`,
  `get_symbol_definition`, `find_callers`, `get_local_graph`, six cross-agent
  memory tools, and `get_stats` for index diagnostics.
- Hybrid `search_code` — BM25 lexical ranking fused with model2vec semantic
  embeddings via reciprocal rank fusion.
- SQLite-backed cross-agent memory: architectural decisions, an action-history
  log, and an append-only scratchpad.
- Background embedding-model load so the daemon never blocks startup.
- Graceful shutdown on Ctrl-C / SIGTERM.
- Reliability limits: AST-depth cap, file-size cap, clamped tool arguments,
  a bounded framed-message size, and a concurrent-connection cap.
- Environment-variable configuration: `CONTINUUM_MODEL`,
  `CONTINUUM_IDLE_MINUTES`, `CONTINUUM_MAX_FILE_KIB`, `CONTINUUM_DEBOUNCE_MS`.
- Unit and end-to-end test suites, and a GitHub Actions CI pipeline (fmt,
  clippy, build, test on Linux and Windows).

[Unreleased]: https://github.com/Rxflex/Continuum/commits/main
