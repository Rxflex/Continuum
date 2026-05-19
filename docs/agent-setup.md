# Connecting Agents to Continuum

`continuum-adapter` is a standard MCP server over stdio. Every MCP-capable agent
registers it the same way — as a stdio server command. The adapter auto-spawns
the per-workspace daemon and proxies all traffic to it, so no separate daemon
setup is needed.

## Build

```
cargo build --release
```

The adapter binary is then at `target/release/continuum-adapter`
(`continuum-adapter.exe` on Windows). Use its **absolute path** in the configs
below — shown here as `/abs/path/continuum-adapter`.

The adapter treats its working directory as the workspace root. To pin a
workspace explicitly, add `"--workspace", "/path/to/project"` to `args`.

---

## Claude Code

Project-scoped `.mcp.json` at the repository root:

```json
{
  "mcpServers": {
    "continuum": {
      "command": "/abs/path/continuum-adapter",
      "args": []
    }
  }
}
```

Or via the CLI:

```
claude mcp add continuum -- /abs/path/continuum-adapter
```

## Codex CLI

In `~/.codex/config.toml`:

```toml
[mcp_servers.continuum]
command = "/abs/path/continuum-adapter"
args = []
```

Or via the CLI:

```
codex mcp add continuum --transport stdio --command "/abs/path/continuum-adapter"
```

## Gemini CLI

In `~/.gemini/settings.json` (global) or `.gemini/settings.json` (project):

```json
{
  "mcpServers": {
    "continuum": {
      "command": "/abs/path/continuum-adapter",
      "args": []
    }
  }
}
```

## OpenCode

In `opencode.json` (project) or `~/.config/opencode/opencode.json` (global):

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "continuum": {
      "type": "local",
      "command": ["/abs/path/continuum-adapter"],
      "enabled": true
    }
  }
}
```

---

## Notes

- **One daemon per workspace.** The first adapter to start spawns it; the rest
  attach to it. It shuts down 30 minutes after the last adapter disconnects.
- **Multiple agents, shared state.** Point several agents at the same workspace
  and they share one code graph and one memory store — that is the point.
- **First run** downloads a ~30 MB embedding model. If it fails, search falls
  back to lexical-only ranking; everything else is unaffected.
- **Logs** for a workspace are in `<workspace>/.continuum/daemon.log`.
