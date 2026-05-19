// postinstall: fetch the prebuilt Continuum binaries for this platform from the
// matching GitHub release and place them side by side in vendor/.

"use strict";

const fs = require("fs");
const path = require("path");
const pkg = require("../package.json");

// Node platform/arch -> Rust target triple used in the release asset names.
const TARGETS = {
  "linux-x64": "x86_64-unknown-linux-gnu",
  "darwin-x64": "x86_64-apple-darwin",
  "darwin-arm64": "aarch64-apple-darwin",
  "win32-x64": "x86_64-pc-windows-msvc",
};

const REPO = "Rxflex/Continuum";

async function main() {
  const key = `${process.platform}-${process.arch}`;
  const target = TARGETS[key];
  if (!target) {
    // Not a hard failure: the user can still build from source.
    process.stderr.write(
      `continuum-mcp: no prebuilt binary for ${key}; build from source (see README).\n`,
    );
    return;
  }

  const ext = process.platform === "win32" ? ".exe" : "";
  const base = `https://github.com/${REPO}/releases/download/v${pkg.version}`;
  const vendor = path.join(__dirname, "..", "vendor");
  fs.mkdirSync(vendor, { recursive: true });

  for (const name of ["continuum-adapter", "continuum-daemon"]) {
    const asset = `${name}-${target}${ext}`;
    const dest = path.join(vendor, name + ext);
    process.stderr.write(`continuum-mcp: downloading ${asset}\n`);

    const res = await fetch(`${base}/${asset}`, { redirect: "follow" });
    if (!res.ok) {
      throw new Error(`download failed (${res.status}) for ${asset}`);
    }
    fs.writeFileSync(dest, Buffer.from(await res.arrayBuffer()));
    if (process.platform !== "win32") {
      fs.chmodSync(dest, 0o755);
    }
  }
  process.stderr.write("continuum-mcp: ready\n");
}

main().catch((err) => {
  process.stderr.write(`continuum-mcp: install failed: ${err.message}\n`);
  process.exit(1);
});
