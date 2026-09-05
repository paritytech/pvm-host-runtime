# Contributing

## Pull requests

Runtime changes must preserve the reviewed host boundary:

- Keep native and browser bounds and failure behavior aligned.
- Treat `polkavm-gpu-wire` changes as compatibility changes.
- Generate browser artifacts from source; do not edit generated assets.
- Add behavioral coverage for observable runtime changes.
- Pin every git dependency to an immutable full commit.

Run the affected checks locally. Shared Rust, GPU wire, lockfile, artifact, and release changes require the complete matrix.

## Commit messages

Use concise conventional subjects such as `fix(runtime): reject stale GPU sequences` or `build(browser): reproduce release assets`.

## Releases

Release manifests and artifacts must be produced from a clean tagged source commit. Artifact digests are part of the release contract.
