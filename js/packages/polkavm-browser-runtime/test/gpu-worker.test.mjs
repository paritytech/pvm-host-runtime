import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";
import vm from "node:vm";

const source = await readFile(
  resolve(import.meta.dirname, "../src/polkavm-gpu-worker.js"),
  "utf8",
);
const context = vm.createContext({
  ArrayBuffer,
  DataView,
  Map,
  Set,
  TextDecoder,
  TextEncoder,
  Uint8Array,
  onmessage: null,
  postMessage() {},
});
vm.runInContext(
  `${source}\nglobalThis.gpuWorkerTest = { GpuEngine, parseCommand, parseCommands };`,
  context,
);
const { GpuEngine, parseCommand, parseCommands } = context.gpuWorkerTest;

function parse(opcode, payload) {
  return parseCommand({ opcode, payload, index: 0 });
}

function shaderCommands(count) {
  const wgsl = new TextEncoder().encode("@vertex fn main() {}");
  return Array.from({ length: count }, (_, index) => {
    const payload = new Uint8Array(8 + wgsl.byteLength);
    const view = new DataView(payload.buffer);
    view.setUint32(0, (1 << 20) | (index + 1), true);
    view.setUint32(4, wgsl.byteLength, true);
    payload.set(wgsl, 8);
    return { opcode: 6, payload, index };
  });
}

test("parses the R8Unorm texture format", () => {
  const payload = new Uint8Array(24);
  const view = new DataView(payload.buffer);
  view.setUint32(0, 1, true);
  view.setUint32(4, 64, true);
  view.setUint32(8, 64, true);
  view.setUint16(12, 1, true);
  view.setUint16(14, 1, true);
  view.setUint16(16, 7, true);
  view.setUint8(18, 1);
  view.setUint32(20, 4, true);

  assert.equal(parse(3, payload).format, "r8unorm");
});

test("parses read-only storage buffer layouts", () => {
  const payload = new Uint8Array(40);
  const view = new DataView(payload.buffer);
  view.setUint32(0, 1, true);
  view.setUint32(4, 1, true);
  view.setUint32(8, 3, true);
  view.setUint32(12, 3, true);
  view.setUint16(16, 4, true);
  view.setBigUint64(24, 16n, true);

  const [entry] = parse(7, payload).entries;
  assert.equal(entry.binding, 3);
  assert.equal(entry.buffer.type, "read-only-storage");
  assert.equal(entry.buffer.minBindingSize, 16);

  view.setUint16(18, 2, true);
  assert.throws(() => parse(7, payload), /invalid buffer binding layout/);
});

test("parses writable storage buffer layouts", () => {
  const payload = new Uint8Array(40);
  const view = new DataView(payload.buffer);
  view.setUint32(0, 1, true);
  view.setUint32(4, 1, true);
  view.setUint32(8, 3, true);
  view.setUint32(12, 4, true);
  view.setUint16(16, 5, true);
  view.setBigUint64(24, 16n, true);

  const [entry] = parse(7, payload).entries;
  assert.equal(entry.binding, 3);
  assert.equal(entry.buffer.type, "storage");
  assert.equal(entry.buffer.minBindingSize, 16);
});

test("rejects writable storage buffer layouts in the vertex stage", () => {
  const payload = new Uint8Array(40);
  const view = new DataView(payload.buffer);
  view.setUint32(0, 1, true);
  view.setUint32(4, 1, true);
  view.setUint32(8, 3, true);
  view.setUint32(12, 1, true);
  view.setUint16(16, 5, true);
  view.setBigUint64(24, 16n, true);

  assert.throws(() => parse(7, payload), /invalid buffer binding layout/);
});

test("validates compute pipeline dispatch batches", () => {
  const shader = new TextEncoder().encode("@compute @workgroup_size(1) fn cs_main() {}");
  const shaderPayload = new Uint8Array(8 + Math.ceil(shader.byteLength / 4) * 4);
  const shaderView = new DataView(shaderPayload.buffer);
  shaderView.setUint32(0, handle(1), true);
  shaderView.setUint32(4, shader.byteLength, true);
  shaderPayload.set(shader, 8);
  const layoutPayload = u32s([handle(3), 0]);
  const computePipelinePayload = u32s([handle(4), handle(3), handle(1), 0]);
  const batch = commands([
    [6, shaderPayload],
    [8, layoutPayload],
    [24, computePipelinePayload],
    [25, new Uint8Array()],
    [26, u32s([handle(4)])],
    [28, u32s([1, 1, 1])],
    [29, new Uint8Array()],
  ]);
  const engine = validationEngine();

  const validated = engine.validate(parseCommands(batch));

  assert.equal(validated.commands.at(-2).opcode, 28);
});

test("rejects nested render pass inside compute pass", () => {
  const batch = commands([
    [25, new Uint8Array()],
    [
      12,
      u32s([
        0,
        0,
        1,
        0,
        0,
        0,
        0,
        0x3f800000,
        0x3f800000,
      ]),
    ],
  ]);
  const engine = validationEngine();

  assert.throws(() => engine.validate(parseCommands(batch)), /nested GPU pass/);
});

test("completes a validated batch without render commands", async () => {
  let completions = 0;
  const engine = Object.create(GpuEngine.prototype);
  Object.assign(engine, {
    stopped: false,
    resources: new Map(),
    handleSlots: new Map(),
    lastSequence: 0n,
    testReadbacksRemaining: 0,
    testDeviceLossPending: false,
    device: {
      pushErrorScope() {},
      popErrorScope: async () => null,
      queue: {
        onSubmittedWorkDone: async () => {
          completions++;
        },
      },
    },
  });
  engine.validate = () => ({
    commands: [],
    slots: new Map(),
  });
  const batch = new Uint8Array(24);
  const view = new DataView(batch.buffer);
  batch.set(new TextEncoder().encode("EPG1"));
  view.setUint16(4, 1, true);
  view.setUint32(8, batch.byteLength, true);
  view.setBigUint64(16, 1n, true);

  await engine.execute(batch);

  assert.equal(completions, 1);
  assert.equal(engine.lastSequence, 1);
});

function handle(slot) {
  return (1 << 20) | slot;
}

function u32s(values) {
  const bytes = new Uint8Array(values.length * 4);
  const view = new DataView(bytes.buffer);
  values.forEach((value, index) => view.setUint32(index * 4, value, true));
  return bytes;
}

function validationEngine() {
  return Object.assign(Object.create(GpuEngine.prototype), {
    resources: new Map(),
    handleSlots: new Map(),
    lastSequence: 0n,
    limits: [
      4096,
      16 * 1024 * 1024,
      16,
      4,
      8,
      16,
      4,
      256 * 1024 * 1024,
      64 * 1024 * 1024,
      8192,
      4 * 1024 * 1024,
      16 * 1024 * 1024,
      16 * 1024 * 1024,
      8,
      16 * 1024,
      256,
      256,
      256,
      64,
      65_535,
      8192,
    ],
    surfaceGeneration: 1,
  });
}

function commands(items) {
  const commandBytes = items.reduce(
    (total, [, payload]) => total + 8 + payload.byteLength,
    0,
  );
  const bytes = new Uint8Array(24 + commandBytes);
  const view = new DataView(bytes.buffer);
  bytes.set(new TextEncoder().encode("EPG1"));
  view.setUint16(4, 1, true);
  view.setUint32(8, bytes.byteLength, true);
  view.setUint32(12, items.length, true);
  view.setBigUint64(16, 1n, true);
  let offset = 24;
  for (const [opcode, payload] of items) {
    view.setUint16(offset, opcode, true);
    view.setUint32(offset + 4, 8 + payload.byteLength, true);
    bytes.set(payload, offset + 8);
    offset += 8 + payload.byteLength;
  }
  return bytes;
}

test("validates the GPUI editor's nineteen-compilation init batch", () => {
  const engine = validationEngine();
  engine.validate({ sequence: 1, commands: shaderCommands(19) });
});

test("rejects more compilations than the per-batch bound", () => {
  const engine = validationEngine();
  assert.throws(
    () => engine.validate({ sequence: 1, commands: shaderCommands(33) }),
    /too many GPU compilations/,
  );
});
