# PolkaVM Host Runtime

Host-neutral PolkaVM application runtime for native and browser Hosts.

The repository owns one implementation of the App Manifest v2 PolkaVM execution contract across native Rust, browser WebAssembly, framebuffer, Tri2D, WebGPU Raster, and expanded WebGPU presentation. Runtime limits, GPU records, browser workers, and distributable assets are built and reviewed together.

## Layout

- `rust/crates/polkavm-host-runtime`: execution, hostcalls, lifecycle, bounds, and native/wasm backends.
- `rust/crates/polkavm-gpu-wire`: bounded Tri2D and WebGPU wire protocol.
- `rust/crates/polkavm-motion-wire`: bounded motion-sample wire protocol.
- `rust/crates/polkavm-ui-wire`: bounded cursor, clipboard, navigation, and IME output protocol.
- `rust/crates/polkavm-host-runtime-assets`: source-identified browser assets exposed as static Rust data.
- `rust/crates/polkavm-assets-export`: exports those assets for Android, iOS, and browser packaging.
- `js/packages/polkavm-browser-runtime`: source-built `@parity/polkavm-browser-runtime` package.
- `docs/runtime/polkavm-app-abi-v2.md`: application ABI contract.
- `docs/runtime/tri2d-v1.md`: Tri2D frame, command, retained-resource, and limit contract.

## Host boundary

Hosts integrate through the `truapi-polkavm-host` bridge in [`paritytech/host-rust-core`](https://github.com/paritytech/host-rust-core). The bridge pins one immutable release of this repository and exposes the supported Rust API plus browser asset identity. Host applications do not pin this repository independently.

## Build and test

```bash
cargo +nightly fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
npm ci
npm test
```

Build and export browser assets:

```bash
npm run build
cargo run -p polkavm-assets-export -- --output ./out/polkavm-host-runtime
```

## Releases

A release is identified by one source commit and records:

- Rust workspace version.
- `@parity/polkavm-browser-runtime` version.
- native and wasm PolkaVM revisions.
- SHA-256 digest of every browser artifact.

Release tags use `v<version>`. Moving branch references are not release inputs.

## License

MPL-2.0. See `LICENSE`.
