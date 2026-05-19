# Releasing Continuum

A release ships prebuilt binaries on a GitHub Release and (optionally) the
`continuum-mcp` npm wrapper that downloads them.

## Versions are kept in lockstep

Three places carry the version and must match:

- `Cargo.toml` → `[workspace.package].version`
- `npm/package.json` → `version`
- the git tag → `vX.Y.Z`

The npm wrapper downloads release assets from the tag `v<npm package version>`,
so a mismatch means `npx continuum-mcp` cannot find its binaries.

## Cutting a release

1. Bump the version in `Cargo.toml` and `npm/package.json`.
2. Move the `CHANGELOG.md` `[Unreleased]` entries under a new `[X.Y.Z]` heading.
3. Commit, then tag and push:

   ```sh
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

4. The **Release** workflow builds `continuum-daemon` and `continuum-adapter`
   for Linux x64, macOS x64/arm64, and Windows x64, and attaches them to the
   GitHub Release for the tag.

## Publishing the npm wrapper

```sh
cd npm
npm publish        # add --access public on the first publish
```

`npx continuum-mcp` then works for everyone — its postinstall downloads the
release binaries.

> **Note.** The release assets must be publicly downloadable for `npx` to fetch
> them, so npm publishing is only meaningful once the repository (and therefore
> its releases) is public.
