# Security Policy

## Reporting a vulnerability

Please report security issues privately — do **not** open a public issue.
Use GitHub's [private security advisory](https://github.com/redstone-md/Continuum/security/advisories/new)
form. We aim to acknowledge a report within a few days and will keep you
informed as we work on a fix.

## Supported versions

Continuum is pre-1.0. Security fixes land on `main` and in the latest release;
older versions are not maintained.

## Trust model

Continuum is a local developer tool. Its security boundary is the local machine.

- **Transport.** The daemon listens only on `127.0.0.1` (TCP loopback). It is
  never exposed to a network interface.
- **Authentication.** Each daemon generates a fresh random 32-character token at
  startup. Every adapter connection must present it during the handshake.
- **The token** is stored in `<workspace>/.continuum/daemon.lock`. Any process
  that can read that file can connect to the daemon — so the daemon trusts every
  local user who can read the workspace. This is the same trust level as a
  process that can read the source tree itself.
- **No code execution.** The daemon parses source files with tree-sitter and
  serves the resulting metadata. It does not execute project code.
- **Filesystem scope.** Indexing is confined to the workspace root; build,
  dependency, and VCS directories are skipped, and oversized files are ignored.
- **Persistence.** Agent memory is a local SQLite database under `.continuum/`.

## Supply chain

On first run the daemon downloads an embedding model (~30 MB) from the
HuggingFace Hub. Operators in restricted environments can run fully offline:
search degrades to lexical-only ranking when the model is unavailable.

Dependencies are pinned in `Cargo.lock`, which is committed.
