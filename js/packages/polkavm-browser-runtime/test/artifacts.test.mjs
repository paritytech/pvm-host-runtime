import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { resolve } from "node:path";
import test from "node:test";

const root = resolve(import.meta.dirname, "..");
const dist = resolve(root, "dist");

test("browser runtime artifacts match their published checksums", async () => {
  const expected = new Map(
    (await readFile(resolve(dist, "SHA256SUMS"), "utf8"))
      .trim()
      .split("\n")
      .map((line) => {
        const [digest, file] = line.split(/\s+/, 2);
        return [file, digest];
      }),
  );
  for (const file of [
    "polkavm-browser-runtime.wasm",
    "polkavm-worker.js",
    "polkavm-gpu-worker.js",
    "polkavm-wasm-translated.js",
    "polkavm-runtime-core.js",
    "polkavm-wasm-worker-entry.js",
  ]) {
    const bytes = await readFile(resolve(dist, file));
    assert.equal(
      createHash("sha256").update(bytes).digest("hex"),
      expected.get(file),
    );
  }
});

test("Wasm runtime omits machine-specific Rust source paths", async () => {
  const bytes = await readFile(resolve(dist, "polkavm-browser-runtime.wasm"));
  for (const root of [
    resolve(homedir(), ".cargo"),
    resolve(homedir(), ".rustup"),
  ]) {
    assert.equal(bytes.includes(Buffer.from(root)), false, `embedded ${root}`);
  }
});

test("Wasm runtime exports the neutral graphics and motion ABI", async () => {
  const bytes = await readFile(resolve(dist, "polkavm-browser-runtime.wasm"));
  const module = await WebAssembly.compile(bytes);
  const exports = new Set(
    WebAssembly.Module.exports(module).map(({ name }) => name),
  );
  for (const name of [
    "polkavm_browser_launch_begin_v2",
    "polkavm_browser_take_tri2d",
    "polkavm_browser_set_gpu_capabilities",
    "polkavm_browser_send_gpu_event",
    "polkavm_browser_take_gpu_batch",
    "polkavm_browser_set_motion_availability",
    "polkavm_browser_send_motion_sample",
    "polkavm_browser_uses_motion",
    "polkavm_browser_uses_pointer_capture",
    "polkavm_browser_set_pointer_capture_supported",
    "polkavm_browser_set_pointer_capture_active",
    "polkavm_browser_take_pointer_capture_request",
  ]) {
    assert.ok(exports.has(name), `missing ${name}`);
  }
});
