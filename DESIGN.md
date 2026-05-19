# Continuum — Design Document (v2)

A high-performance, persistent **Model Context Protocol (MCP) server** that provides
code context and cross-agent memory. It lets different AI agents (Gemini CLI, Claude
Code, Codex, Cursor) collaborate sequentially on the same codebase without losing
context, architectural intent, or state.

This document is the authoritative spec. Decisions here are **locked** unless this
file is updated.

---

## 1. Architecture

To solve the stdio-isolation problem of standard MCP clients, Continuum uses a
**Daemon + Thin Client** architecture.

```
  AI IDE / CLI                  AI IDE / CLI
       │ stdio (MCP JSON-RPC)         │ stdio
  ┌────▼─────────┐              ┌─────▼────────┐
  │   Adapter    │              │   Adapter    │   thin clients
  │ (thin proxy) │              │ (thin proxy) │
  └────┬─────────┘              └─────┬────────┘
       │ TCP loopback (JSON-RPC + token)     │
       └──────────────┬──────────────────────┘
              ┌────────▼─────────┐
              │  Continuum Daemon │  one per workspace
              │  ┌─────────────┐  │
              │  │ Code Graph  │  │  in-memory
              │  ├─────────────┤  │
              │  │ Indexer     │  │  tree-sitter + FS watcher
              │  ├─────────────┤  │
              │  │ Memory      │──┼──► SQLite (.continuum/continuum.db)
              │  └─────────────┘  │
              └───────────────────┘
```

- **Daemon** — long-lived, stateful background process, one per workspace. Holds the
  AST knowledge graph in memory; persists inter-agent memory to local SQLite.
- **Adapter** — lightweight binary spawned by the AI IDE/CLI. Exposes a standard MCP
  stdio interface to the agent and proxies all JSON-RPC requests to the daemon over
  TCP loopback. Holds no domain state.

---

## 2. IPC & Daemon Lifecycle

### Transport
- **TCP loopback** on `127.0.0.1`, ephemeral port. One code path on every OS, zero
  `#[cfg]` branching.
- The transport sits behind a `Transport` trait so a named-pipe / UDS backend can be
  added in v2 without touching callers.

### Security
- Any local process can connect to a loopback port, so every daemon generates a random
  32-byte **token** (hex-encoded) at startup.
- The token is written to the lockfile and sent by the adapter in the handshake. Wrong
  or missing token → connection refused.

### Lockfile
Per-workspace key = `hash(canonical absolute workspace path)`.
File: `<workspace>/.continuum/daemon.lock`

```json
{
  "pid": 12345,
  "endpoint": "127.0.0.1:49213",
  "token": "a1b2c3...",
  "protocol_version": 1
}
```

### Startup sequence (adapter)
1. Read `daemon.lock`.
2. If absent, OR `pid` is dead, OR `protocol_version` mismatches → spawn a daemon.
3. To spawn safely: take an **advisory file lock** on `daemon.lock`. The winner spawns
   the daemon and waits for it to write a ready lockfile; losers wait, release, and
   re-read.
4. Connect to `endpoint`.
5. **First RPC = handshake** — adapter sends `{protocol_version, token}`. On mismatch
   the daemon refuses; the adapter respawns the correct daemon.

### Idle shutdown
- Daemon tracks active adapter connections (`AtomicUsize`) and a `last_activity`
  timestamp.
- A background task checks every minute. If `connections == 0` **and** idle for
  **30 minutes** (configurable), the daemon exits.
- Cold restart triggers a full re-index. (Graph snapshotting → see §10 Future.)

---

## 3. Crate Layout (Cargo Workspace)

Multi-crate workspace. Crate boundaries enforce encapsulation: a crate cannot reach
into another's internals.

```
continuum/
├── Cargo.toml                  # [workspace]
└── crates/
    ├── continuum-core/         # domain models, DTOs, error types, protocol consts
    ├── continuum-transport/    # MCP JSON-RPC, TCP IPC server, stdio proxy, Transport trait
    ├── continuum-graph/        # CodeGraph + heuristic resolver
    ├── continuum-indexer/      # FS watcher + tree-sitter parsing + .scm queries
    ├── continuum-memory/       # SQLite writer-actor
    ├── continuum-daemon/       # [[bin]] daemon — wires everything
    └── continuum-adapter/      # [[bin]] thin client — transport + core only
```

### Dependency direction (no cycles)
```
continuum-core      ← everyone
continuum-transport ← core
continuum-graph     ← core
continuum-indexer   ← core, graph
continuum-memory    ← core
continuum-daemon    ← core, transport, graph, indexer, memory
continuum-adapter   ← core, transport          (stays thin — no graph/memory/indexer)
```

`protocol_version` lives in `continuum-core` as a single constant; bump it on any
breaking IPC change.

---

## 4. Modules

### 4.1 `continuum-transport`
- MCP JSON-RPC serialization/deserialization.
- TCP IPC server (daemon side): tokio TCP listener, one task per connection.
- stdio proxy (adapter side): reads MCP from stdin, forwards over TCP, writes back.
- `Transport` trait abstracts the wire (TCP now, named pipe later).
- **Must not** leak tree-sitter or graph types — DTOs only.

### 4.2 `continuum-indexer`
- FS watcher via `notify`, **~300 ms debouncer** → coalesces bursts into one batch.
- Reads changed files into memory, passes to the tree-sitter engine.
- Applies `.scm` capture queries → extracts functions, classes/structs, methods,
  imports, calls.
- Hands extracted metadata to `continuum-graph`. Does not own the graph.

### 4.3 `continuum-graph`
- `CodeGraph` = `petgraph::StableGraph` + `HashMap<SymbolId, NodeIndex>` — **one
  source of truth** (`StableGraph` keeps `NodeIndex` stable across removals).
- **Nodes:** Files, Symbols (Classes/Structs, Functions, Variables).
- **Edges:** `Contains`, `Calls`, `Imports`, `Inherits`.
- **Heuristic Resolver** — background task. Best-effort cross-file reference
  resolution: import path → file → exported symbol. Edges are typed
  `Resolved | Unresolved`. Re-exports, aliases, dynamic imports stay `Unresolved`;
  the unresolved count is exposed. **No accuracy promise.** Per-language rules are
  pluggable later.

### 4.4 `continuum-memory`
- `rusqlite` in **WAL mode**.
- A dedicated **writer actor** owns the `Connection`, runs on its own OS thread (not a
  tokio worker — SQLite calls block). Commands arrive over an `mpsc` channel; replies
  go back over `oneshot`. Writes serialize naturally.
- Tables:
  - **`lore`** — architectural guidelines and ADRs (Architecture Decision Records).
  - **`action_history`** — which agent changed what, with stated intent and files
    touched. Server-stamped timestamp + monotonic sequence.
  - **`scratchpad`** — **append log** of `{agent, timestamp, message}`. Never
    overwritten — avoids one agent clobbering another's message before it is read.

---

## 5. MCP Tools API

The daemon exposes **12 tools** to the connected agent.

### Code Navigation & Search
| Tool | Signature | Returns |
|------|-----------|---------|
| `search_code` | `(query: string, limit?: int, kind?: string)` | BM25-ranked symbols. One compact row per hit — the token-efficient replacement for grep. |
| `get_file_outline` | `(path: string)` | File structure (classes, function signatures), bodies folded as `/* body omitted */`. |
| `get_symbol_definition` | `(symbol_name: string, file_hint?: string)` | Full source + docstring of a symbol. |
| `find_callers` | `(symbol_name: string)` | Files and line numbers where the symbol is invoked. |
| `get_local_graph` | `(symbol_name: string, depth: int)` | Dependency tree — what the symbol calls internally. |

### Handoff & Memory (Continuity)
| Tool | Signature | Effect |
|------|-----------|--------|
| `store_architectural_decision` | `(topic: string, description: string)` | Saves a design decision to `lore`. |
| `read_project_guidelines` | `()` | Returns all active architectural rules / project lore. |
| `commit_intent` | `(agent_id: string, intent: string, files_touched: string[])` | Logs what the agent did and what is expected next. |
| `get_recent_changes` | `(limit: int)` | Most recent intents logged by previous agents. |
| `write_scratchpad` | `(agent_id: string, message: string)` | **Appends** an entry to the scratchpad log. |
| `read_scratchpad` | `(limit: int)` | Returns the last N scratchpad entries. |

### Diagnostics
| Tool | Signature | Returns |
|------|-----------|---------|
| `get_stats` | `()` | Index health: graph size, semantic-search state, server uptime. |

> **Note on `agent_id`:** v1 trusts the self-reported string. The daemon stamps a
> server-side timestamp + monotonic sequence so ordering cannot be forged. v2:
> handshake-assigned session id.

---

## 6. Concurrency Model

The graph and memory are accessed concurrently by the FS watcher and multiple MCP
requests.

- **Graph** — `RwLock<CodeGraph>`. MCP queries take a read lock (cheap, parallel); the
  watcher takes a write lock once per debounced batch. No `dashmap` — a second store
  means two sources of truth and sync pain, with no win at this scale.
- **Memory** — single writer actor; all writes serialize through one channel.
- **IPC** — one tokio task per connection; requests handled concurrently.
- **Watcher** — tokio task: `notify` events → 300 ms debounce → batch → indexer →
  graph write lock.

---

## 7. Error Handling

- **Never panic.** All fallible paths return `Result`.
- Library crates use `thiserror` domain error enums; binaries may use `anyhow`.
- At the transport boundary, every error maps to a properly formatted **JSON-RPC error
  response** with an appropriate code. Errors never cross modules as panics.

---

## 8. Implementation Directives

1. **Strict encapsulation** — never leak tree-sitter or graph types into the transport
   layer. Use DTOs / domain models. Crate boundaries enforce this.
2. **Small files** — one struct/interface and its immediate methods per file.
3. **Concurrency safety** — use the patterns in §6. No ad-hoc shared mutable state.
4. **No panics** — see §7.
5. **Step-by-step** — do not write the whole system at once. Follow the order in §9.

---

## 9. Implementation Order

1. **Transport + IPC** — TCP loopback, lockfile, daemon lifecycle, handshake, stdio
   proxy. A daemon and adapter that can round-trip a ping.
2. **Graph** — `CodeGraph`, node/edge types, `RwLock` access.
3. **Indexer** — FS watcher, debouncer, tree-sitter AST traversal, feeding the graph.
4. **Memory** — SQLite writer actor, the three tables.
5. **MCP Tools** — wire the tools to graph + memory.
6. **Resolver** — heuristic cross-file resolution as a background task.
7. **Semantic search** — `continuum-search`: embeddings + vector index, fused
   into `search_code`.

---

## 10. Semantic Search (`continuum-search`)

`search_code` is **hybrid**: it fuses the graph's lexical BM25 ranking with
embedding-based semantic search, so an agent finds code by meaning as well as by
name. Implemented in the `continuum-search` crate.

- **Embeddings** — model2vec static embeddings (`minishlab/potion-base-8M`, ~30 MB):
  a distilled token→vector table with mean pooling. Pure Rust, no ONNX runtime, so
  it builds on any toolchain and adds no native-library dependency. Downloaded once
  from HuggingFace; if loading fails the daemon degrades to lexical-only search.
- **Vector index** — in memory, brute-force cosine. Symbol counts are modest
  (thousands), so a flat scan is sub-millisecond and needs no ANN structure.
- **Fusion** — reciprocal rank fusion (RRF) merges the lexical and semantic lists.
  RRF needs no score calibration between the two rankers, only their ranks.
- **Sync** — the indexer embeds each file's symbols alongside every graph update.

## 11. Future

- **Named pipe / UDS transport** — second `Transport` impl, OS-ACL security.
- **SQLite read pool** — concurrent readers separate from the writer actor.
- **Handshake-assigned session id** — replaces self-reported `agent_id`.
- **Graph snapshotting** — persist the graph on idle shutdown for fast warm restart
  without a full re-index.
- **Background model load** — load the embedding model off the startup path so the
  first-ever run does not block on the model download.
- **Semantic memory search** — extend embeddings to the `lore` table so agents can
  find architectural decisions by meaning.
