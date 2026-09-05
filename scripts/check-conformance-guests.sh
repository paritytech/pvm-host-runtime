#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURES="$ROOT/rust/crates/polkavm-host-runtime/tests/fixtures"
TOOLCHAIN="${POLKAVM_GUEST_TOOLCHAIN:-nightly-2025-10-09}"
GUESTS=(
  "polkavm-app-v2/host-frame-roundtrip polkavm_host_frame_roundtrip host-frame-roundtrip.polkavm"
  "polkavm-app-v2/ui-output polkavm_ui_output ui-output.polkavm"
  "polkavm-app-v2/pointer-capture polkavm_pointer_capture pointer-capture.polkavm"
  "polkadot-host-computer-0.1/core-context polkavm_computer_core_context computer-core-context.polkavm"
  "polkadot-host-computer-0.1/core-services polkavm_computer_core_services computer-core-services.polkavm"
  "polkadot-host-computer-0.1/tty-fs-roundtrip polkavm_computer_tty_fs_roundtrip computer-tty-fs-roundtrip.polkavm"
  "polkadot-host-computer-0.1/pipe-filter polkavm_computer_pipe_filter computer-pipe-filter.polkavm"
  "polkadot-host-computer-0.1/pipe-driver polkavm_computer_pipe_driver computer-pipe-driver.polkavm"
  "polkadot-host-computer-0.1/tcp-roundtrip polkavm_computer_tcp_roundtrip computer-tcp-roundtrip.polkavm"
  "polkadot-host-computer-0.1/workspace-pane polkavm_computer_workspace_pane computer-workspace-pane.polkavm"
  "polkadot-host-computer-0.1/workspace-driver polkavm_computer_workspace_driver computer-workspace-driver.polkavm"
  "polkadot-host-computer-0.1/filesystem polkavm_computer_filesystem computer-filesystem.polkavm"
)

for tool in cargo polkatool rustup; do
  command -v "$tool" >/dev/null 2>&1 || {
    printf 'missing required tool: %s\n' "$tool" >&2
    exit 1
  }
done

rustup component add rust-src --toolchain "$TOOLCHAIN" >/dev/null
RUSTC="$(rustup which --toolchain "$TOOLCHAIN" rustc)"
TARGET_JSON="$(RUSTC="$RUSTC" polkatool get-target-json-path --bitness 32)"
TARGET_NAME="$(basename "$TARGET_JSON" .json)"
TARGET_DIR="$(mktemp -d)"
trap 'rm -rf "$TARGET_DIR"' EXIT

for guest in "${GUESTS[@]}"; do
  read -r directory artifact fixture <<<"$guest"
  cargo +"$TOOLCHAIN" build \
    -Z build-std=core \
    --locked \
    --manifest-path "$ROOT/conformance/runtime/$directory/Cargo.toml" \
    --target-dir "$TARGET_DIR" \
    --target "$TARGET_JSON" \
    --release

  polkatool link \
    "$TARGET_DIR/$TARGET_NAME/release/$artifact.elf" \
    -o "$TARGET_DIR/$fixture"

  cmp "$FIXTURES/$fixture" "$TARGET_DIR/$fixture"
  printf 'Verified %s\n' "$FIXTURES/$fixture"
done
