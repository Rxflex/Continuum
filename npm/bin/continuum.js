#!/usr/bin/env node
// Thin launcher: exec the prebuilt continuum-adapter, which speaks MCP over
// stdio and auto-spawns the per-workspace daemon. The daemon binary sits next
// to the adapter in vendor/, where the adapter expects to find it.

"use strict";

const { spawnSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const ext = process.platform === "win32" ? ".exe" : "";
const adapter = path.join(__dirname, "..", "vendor", "continuum-adapter" + ext);

if (!fs.existsSync(adapter)) {
  process.stderr.write(
    "continuum-mcp: adapter binary missing — reinstall the package " +
      "(its postinstall step downloads the prebuilt binaries).\n",
  );
  process.exit(1);
}

const result = spawnSync(adapter, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  process.stderr.write(`continuum-mcp: ${result.error.message}\n`);
  process.exit(1);
}
process.exit(result.status === null ? 1 : result.status);
