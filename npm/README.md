# continuum-mcp

The [Continuum](https://github.com/redstone-md/Continuum) MCP server, packaged for
`npx`. Continuum is a persistent, multi-agent Model Context Protocol server that
gives AI coding agents a shared, live code graph and a memory that survives
across agents and sessions.

## Use

Point any MCP-capable agent (Claude Code, Codex CLI, Gemini CLI, OpenCode) at:

```json
{
  "command": "npx",
  "args": ["-y", "continuum-mcp"]
}
```

On install, the package downloads the prebuilt Continuum binaries for your
platform. The launcher then runs the MCP adapter, which auto-spawns one daemon
per workspace.

Supported platforms: Linux x64, macOS x64/arm64, Windows x64. On any other
platform, build from source — see the
[main repository](https://github.com/redstone-md/Continuum).

## License

MIT.
