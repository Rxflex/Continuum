# Continuum

A persistent, multi-agent **Model Context Protocol (MCP)** server. Continuum gives
AI coding agents — Claude Code, OpenCode, Codex, Gemini CLI — a shared, live view
of a codebase and a memory that survives across agents and sessions.

## Why

Standard MCP servers are spawned per-agent over stdio and forget everything when
the agent exits. Continuum runs a single long-lived daemon per workspace: agents
come and go, but the code graph and the inter-agent memory persist. One agent can
hand off architectural intent to the next without re-deriving context.

## Features

- **Live code graph** — tree-sitter parsing of Rust, Python, JavaScript,
  TypeScript and Go, kept in sync by a filesystem watcher.
- **Hybrid code search** — `search_code` fuses lexical BM25 ranking with semantic
  embeddings (a local, pure-Rust model2vec model) via reciprocal rank fusion. One
  compact row per hit — a token-efficient replacement for grep.
- **Code navigation** — file outlines, symbol definitions, caller lookup, and
  local dependency graphs, served from the in-memory code graph.
- **Cross-agent memory** — architectural decisions, an action-history log of
  agent intents, and an append-only scratchpad for handoffs.
- **Daemon + thin adapter** — one stateful daemon per workspace; each agent runs
  a lightweight stdio↔TCP proxy that auto-spawns the daemon on demand.

## Architecture

```
  AI agent ──stdio(MCP)──> continuum-adapter ──TCP loopback──> continuum-daemon
                           (thin proxy)                       (graph + memory)
```

The daemon holds the AST knowledge graph in memory and persists agent memory to a
local SQLite database. Adapters are stateless. See [DESIGN.md](DESIGN.md) for the
full design.

## Build

Requires a recent stable Rust toolchain.

```
cargo build --release
```

This produces two binaries: `continuum-daemon` and `continuum-adapter`.

On first run the daemon downloads a ~30 MB embedding model from HuggingFace for
semantic search; if that fails (e.g. offline), search falls back to lexical-only
ranking and everything else works unchanged.

> **Windows without Visual Studio:** with no MSVC linker available, build against
> the llvm-mingw toolchain — `rustup target add x86_64-pc-windows-gnullvm`, then
> add a `.cargo/config.toml` selecting that target.

## Connecting an agent

Continuum speaks MCP over stdio through the adapter. Point any MCP-capable agent
at the `continuum-adapter` binary; it auto-spawns the per-workspace daemon on
first use and proxies all traffic to it:

```json
{
  "command": "/absolute/path/to/continuum-adapter",
  "args": []
}
```

The adapter uses the current working directory as the workspace root, or pass
`--workspace <path>` explicitly.

Per-agent configuration — Claude Code, Codex CLI, Gemini CLI, and OpenCode — is
in [docs/agent-setup.md](docs/agent-setup.md).

## Configuration

Continuum reads its settings from the environment, so they propagate through the
agent → adapter → daemon spawn chain. All are optional.

| Variable | Default | Purpose |
|----------|---------|---------|
| `CONTINUUM_MODEL` | `minishlab/potion-base-8M` | Embedding-model repo. Set to `off` to disable semantic search (lexical-only). |
| `CONTINUUM_IDLE_MINUTES` | `30` | Idle minutes before the daemon exits (`0` = never). |
| `CONTINUUM_MAX_FILE_KIB` | `2048` | Largest file size indexed, in KiB. |
| `CONTINUUM_DEBOUNCE_MS` | `300` | Filesystem-watch debounce window. |

## MCP tools

| Tool | Purpose |
|------|---------|
| `search_code` | Ranked symbol search — the token-efficient replacement for grep |
| `get_file_outline` | File structure — definitions with bodies folded |
| `get_symbol_definition` | Full source + docstring of a symbol |
| `find_callers` | Every call site of a symbol |
| `get_local_graph` | Recursive tree of what a symbol calls |
| `store_architectural_decision` | Persist a design decision / ADR |
| `read_project_guidelines` | Read all stored decisions and lore |
| `commit_intent` | Log what an agent did and expects next |
| `get_recent_changes` | Recent agent intents |
| `write_scratchpad` | Append to the shared scratchpad |
| `read_scratchpad` | Read recent scratchpad entries |

## Project layout

```
crates/
  continuum-core       shared domain types, DTOs, protocol
  continuum-transport  JSON-RPC + MCP wire types, IPC framing, stdio proxy
  continuum-graph      in-memory code knowledge graph
  continuum-indexer    tree-sitter parsing + filesystem watcher
  continuum-memory     SQLite-backed agent memory
  continuum-search     semantic search — embeddings + in-memory vector index
  continuum-daemon     the workspace daemon (binary)
  continuum-adapter    the thin MCP adapter (binary)
```

## License

[MIT](LICENSE).
