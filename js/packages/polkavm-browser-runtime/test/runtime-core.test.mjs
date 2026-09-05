import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

await import("../src/polkavm-wasm-translated.js");
await import("../src/polkavm-runtime-core.js");

const packageRoot = resolve(import.meta.dirname, "..");
const repositoryRoot = resolve(packageRoot, "../../..");

function bytesBuffer(bytes) {
  return bytes.buffer.slice(
    bytes.byteOffset,
    bytes.byteOffset + bytes.byteLength,
  );
}

function endpoint() {
  const messages = [];
  const receiver = {
    onmessage: null,
    postMessage(message) {
      messages.push(message);
    },
  };
  globalThis.createPolkaVmRuntime(receiver);
  return { messages, receiver };
}

async function settle() {
  await new Promise((resolve) => setImmediate(resolve));
}

async function waitForMessage(messages, type, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const message = messages.find((candidate) => candidate.type === type);
    if (message) {
      return message;
    }
    const error = messages.find((candidate) => candidate.type === "error");
    if (error) {
      throw new Error(error.message);
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(`timed out waiting for browser runtime message ${type}`);
}
async function waitForStartupStage(messages, stage, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (
      messages.some(
        (candidate) =>
          candidate.type === "startup" && candidate.stage === stage,
      )
    ) {
      return;
    }
    const error = messages.find((candidate) => candidate.type === "error");
    if (error) {
      throw new Error(error.message);
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  throw new Error(
    `timed out waiting for browser runtime startup stage ${stage}`,
  );
}

function invalidStart(overrides = {}) {
  return {
    type: "start",
    runtime: new Uint8Array([0]),
    program: new Uint8Array([1]),
    assets: [],
    graphicsProfile: "framebuffer",
    audioEnabled: false,
    cacheKey: "invalid",
    ...overrides,
  };
}

function pointerDelta(x, y) {
  const bytes = new Uint8Array(8);
  const view = new DataView(bytes.buffer);
  bytes[0] = 6;
  view.setInt16(2, x, true);
  view.setInt16(4, y, true);
  return bytes;
}

function motionSample() {
  const bytes = new Uint8Array(48);
  const view = new DataView(bytes.buffer);
  bytes.set([0x50, 0x4d, 0x4f, 0x31]);
  view.setUint16(4, 1, true);
  view.setUint16(6, 6, true);
  view.setUint32(8, 48, true);
  view.setUint32(12, 1, true);
  view.setFloat64(16, 10, true);
  view.setFloat32(40, -2, true);
  view.setFloat32(44, 4, true);
  return bytes;
}

function motionResult(bytes) {
  const value = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  return {
    status: new DataView(
      value.buffer,
      value.byteOffset,
      value.byteLength,
    ).getInt32(0, true),
    sample: value.subarray(4),
  };
}

function gpuCapabilities(surfaceGeneration) {
  const bytes = new Uint8Array(56);
  const view = new DataView(bytes.buffer);
  bytes.set([0x45, 0x47, 0x43, 0x31]);
  view.setUint16(4, 1, true);
  view.setUint32(8, bytes.byteLength, true);
  view.setUint16(12, 1, true);
  view.setUint32(16, 640, true);
  view.setUint32(20, 480, true);
  view.setUint32(24, 640, true);
  view.setUint32(28, 480, true);
  view.setFloat32(32, 1, true);
  view.setUint32(36, surfaceGeneration, true);
  view.setUint32(40, 1, true);
  return bytes;
}

test("browser runtime rejects unbounded launch inputs before compilation", async () => {
  for (const [message, expected] of [
    [invalidStart({ program: new Uint8Array() }), /program must contain/],
    [
      invalidStart({
        assets: [
          { path: "same.bin", bytes: new Uint8Array() },
          { path: "same.bin", bytes: new Uint8Array() },
        ],
      }),
      /duplicated/,
    ],
    [
      invalidStart({
        assets: [{ path: "../escape", bytes: new Uint8Array() }],
      }),
      /invalid PolkaVM browser asset path/,
    ],
    [
      invalidStart({ graphicsProfile: "webgpu-raster" }),
      /WebGPU capabilities are required/,
    ],
    [
      invalidStart({ motionAvailability: 3 }),
      /invalid PolkaVM browser motion availability/,
    ],
  ]) {
    const { messages, receiver } = endpoint();
    receiver.onmessage({ data: message });
    await settle();
    assert.match(
      messages.find((candidate) => candidate.type === "error")?.message ?? "",
      expected,
    );
  }
});

test("compiler backend enforces the declared graphics profile", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "tri2d",
      audioEnabled: false,
      cacheKey: "profile-enforcement",
    },
  });
  const ready = await waitForMessage(messages, "ready");
  assert.equal(ready.backend, "compiler");
  assert.equal(ready.usesMotion, false);
  await new Promise((resolve) => setTimeout(resolve, 100));
  assert.equal(
    messages.some((message) => message.type === "frame"),
    false,
    "a framebuffer submission must not escape a tri2d declaration",
  );
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");
});

test("compiler backend returns complete u64 clock values to 32-bit guests", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/clock-u64.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "clock-u64",
    },
  });
  const compiled = await waitForMessage(messages, "compiled");
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");

  const outputs = [];
  const translated = new globalThis.TranslatedPolkaVmRuntime(
    compiled.module,
    [],
    (output) => outputs.push(output),
    1_000_000,
    false,
    "framebuffer",
  );
  translated.initialize();

  translated.update(17);
  assert.deepEqual(
    outputs.findLast((output) => output.type === "save").bytes,
    new Uint8Array([17, 0, 0, 0, 0, 0, 0, 0]),
  );

  translated.update(0x2_0000_0011);
  assert.deepEqual(
    outputs.findLast((output) => output.type === "save").bytes,
    new Uint8Array([17, 0, 0, 0, 2, 0, 0, 0]),
  );
  translated.stop();
});

test("compiler startup keeps the newest GPU capabilities", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
    ),
  );
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  let observedCapabilities;
  globalThis.TranslatedPolkaVmRuntime = class extends Runtime {
    constructor(...args) {
      observedCapabilities = new Uint8Array(args[6]);
      super(...args);
    }
  };
  try {
    const { messages, receiver } = endpoint();
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "webgpu-raster",
        gpuCapabilities: gpuCapabilities(1).buffer,
        audioEnabled: false,
        cacheKey: "gpu-capabilities-startup",
      },
    });
    receiver.onmessage({
      data: {
        type: "gpu-capabilities",
        bytes: gpuCapabilities(2).buffer,
      },
    });
    const ready = await waitForMessage(messages, "ready");
    assert.equal(ready.backend, "compiler");
    assert.equal(
      new DataView(
        observedCapabilities.buffer,
        observedCapabilities.byteOffset,
        observedCapabilities.byteLength,
      ).getUint32(36, true),
      2,
    );
    receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(messages, "terminated");
  } finally {
    globalThis.TranslatedPolkaVmRuntime = Runtime;
  }
});

test("native-Wasm and translated backends round-trip opaque host frames", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/host-frame-roundtrip.polkavm",
    ),
  );
  const requestBytes = new TextEncoder().encode(
    "host-frame-conformance-request-v1",
  );
  const responseBytes = new TextEncoder().encode(
    "host-frame-conformance-response-v1",
  );
  const successBytes = new TextEncoder().encode("host-frame-roundtrip-ok");

  for (const forceInterpreter of [false, true]) {
    const { messages, receiver } = endpoint();
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "framebuffer",
        audioEnabled: false,
        cacheKey: `host-frame-roundtrip-${forceInterpreter}`,
        forceInterpreter,
      },
    });

    const request = await waitForMessage(messages, "host-frame-request");
    assert.deepEqual(new Uint8Array(request.bytes), requestBytes);

    receiver.onmessage({
      data: {
        type: "host-frame-response",
        bytes: bytesBuffer(responseBytes),
      },
    });
    const save = await waitForMessage(messages, "save");
    assert.deepEqual(new Uint8Array(save.bytes), successBytes);

    receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(messages, "terminated");
  }
});

test("native-Wasm and translated backends validate and emit UI output v1", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/ui-output.polkavm",
    ),
  );
  const expected = {
    cursorIcon: "text",
    mutableTextUnderCursor: true,
    ime: {
      rect: [10, 20, 210, 60],
      cursorRect: [24, 22, 25, 58],
    },
    commands: [
      { type: "copy-text", text: "hello" },
      {
        type: "open-url",
        url: "https://example.test",
        newSurface: true,
      },
    ],
  };

  for (const forceInterpreter of [false, true]) {
    const { messages, receiver } = endpoint();
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "tri2d",
        audioEnabled: false,
        cacheKey: `ui-output-${forceInterpreter}`,
        forceInterpreter,
      },
    });

    await waitForMessage(messages, "ready");
    const output = await waitForMessage(messages, "ui-output");
    assert.deepEqual(output.output, expected);

    receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(messages, "terminated");
  }
});

test("compiler backend implements MotionSample v1 status and reads", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/motion-test.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "motion-sample-v1",
    },
  });
  const compiled = await waitForMessage(messages, "compiled");
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");

  const outputs = [];
  const translated = new globalThis.TranslatedPolkaVmRuntime(
    compiled.module,
    [],
    (output) => outputs.push(output),
    1_000_000,
    false,
    "framebuffer",
    null,
    1,
  );
  const sample = motionSample();
  translated.sendMotionSample(sample);
  translated.initialize();
  const written = motionResult(
    outputs.find((output) => output.type === "save").bytes,
  );
  assert.equal(written.status, 48);
  assert.deepEqual(written.sample, sample);
  translated.stop();

  const deniedOutputs = [];
  const denied = new globalThis.TranslatedPolkaVmRuntime(
    compiled.module,
    [],
    (output) => deniedOutputs.push(output),
    1_000_000,
    false,
    "framebuffer",
    null,
    2,
  );
  denied.initialize();
  assert.equal(
    motionResult(deniedOutputs.find((output) => output.type === "save").bytes)
      .status,
    -2,
  );
  denied.stop();
});

test("browser endpoint routes motion samples to the interpreter", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/motion-test.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "motion-sample-interpreter",
      forceInterpreter: true,
      motionAvailability: 1,
    },
  });
  receiver.onmessage({
    data: { type: "motion", bytes: motionSample().buffer },
  });
  const ready = await waitForMessage(messages, "ready");
  assert.equal(ready.usesMotion, true);
  const result = motionResult((await waitForMessage(messages, "save")).bytes);
  assert.equal(result.status, 48);
  assert.deepEqual(result.sample, motionSample());
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");
});

test("JIT fallback preserves a motion sample queued during startup", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/motion-test.polkavm",
    ),
  );
  const warn = console.warn;
  console.warn = () => {};
  const Runtime = globalThis.TranslatedPolkaVmRuntime;
  globalThis.TranslatedPolkaVmRuntime = class extends Runtime {
    initialize() {
      throw new Error("forced translated initialization failure");
    }
  };
  try {
    const { messages, receiver } = endpoint();
    receiver.onmessage({
      data: {
        type: "start",
        runtime: bytesBuffer(runtime),
        program: bytesBuffer(program),
        assets: [],
        graphicsProfile: "framebuffer",
        audioEnabled: false,
        cacheKey: "motion-fallback",
        motionAvailability: 1,
      },
    });
    receiver.onmessage({
      data: { type: "motion", bytes: motionSample().buffer },
    });
    const ready = await waitForMessage(messages, "ready");
    assert.equal(ready.backend, "interpreter");
    assert.equal(ready.usesMotion, true);
    const result = motionResult((await waitForMessage(messages, "save")).bytes);
    assert.equal(result.status, 48);
    assert.deepEqual(result.sample, motionSample());
    receiver.onmessage({ data: { type: "stop" } });
    await waitForMessage(messages, "terminated");
  } finally {
    console.warn = warn;
    globalThis.TranslatedPolkaVmRuntime = Runtime;
  }
});

test("compiler backend discards stale CoreVM mouse movement", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "mouse-backlog",
    },
  });
  const compiled = await waitForMessage(messages, "compiled");
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");

  const translated = new globalThis.TranslatedPolkaVmRuntime(
    compiled.module,
    [],
    () => {},
    1_000_000,
    false,
    "framebuffer",
  );
  translated.coreVm = true;
  translated.imports = ["pvm_fetch_epoca_inputs"];
  translated.sendInput(pointerDelta(100, -60));
  translated.sendInput(pointerDelta(12, -7));
  translated.sendInput(pointerDelta(430, 314));
  assert.equal(translated.epocaInput.length, 1);
  assert.deepEqual(translated.epocaInput[0], pointerDelta(12, -7));

  translated.imports = [];
  translated.sendInput(pointerDelta(100, 0));
  translated.sendInput(pointerDelta(80, 0));
  assert.deepEqual(translated.coreInput, [[0xa3, 80]]);

  translated.setMotionAvailability(2);
  assert.equal(translated.motionAvailability, 2);
  assert.throws(
    () => translated.sendMotionSample(new Uint8Array(48)),
    /invalid motion sample/,
  );
  const motion = motionSample();
  translated.sendMotionSample(motion);
  assert.equal(translated.motionAvailability, 1);
  assert.deepEqual(translated.motionSample, motion);
});

test("browser runtime can select the interpreter without attempting translation", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/framebuffer-test.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "forced-interpreter",
      forceInterpreter: true,
    },
  });

  const ready = await waitForMessage(messages, "ready");
  assert.equal(ready.backend, "interpreter");
  assert.equal(ready.usesMotion, false);
  assert.equal(ready.cacheHit, false);
  assert.equal(ready.translationMs, 0);
  assert.equal(ready.compilationMs, 0);
  assert.equal(ready.translatedWasmBytes, 0);
  assert.equal(
    messages.some(
      (message) => message.type === "translated" || message.type === "compiled",
    ),
    false,
  );

  await waitForStartupStage(messages, "first-update-completed");
  assert.deepEqual(
    messages
      .filter((message) => message.type === "startup")
      .map((message) => message.stage),
    [
      "runtime-instantiating",
      "runtime-instantiated",
      "interpreter-staging-program",
      "interpreter-program-staged",
      "interpreter-launch-begin",
      "interpreter-launch-begun",
      "interpreter-mounting-assets",
      "interpreter-assets-mounted",
      "interpreter-launch-starting",
      "interpreter-launch-started",
      "interpreter-initializing",
      "interpreter-initialized",
      "first-update-started",
      "first-update-completed",
    ],
  );

  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");
});

test("translated backend keeps pointer capture under Host policy", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/motion-test.polkavm",
    ),
  );
  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "pointer-capture-policy",
    },
  });
  const compiled = await waitForMessage(messages, "compiled");
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");

  const translated = new globalThis.TranslatedPolkaVmRuntime(
    compiled.module,
    [],
    () => {},
    1_000_000,
    false,
    "framebuffer",
  );
  assert.equal(translated.usesPointerCapture(), false);
  assert.equal(translated.takePointerCaptureRequest(), null);

  translated.setPointerCaptureActive(true);
  translated.setPointerCaptureActive(true);
  translated.setPointerCaptureActive(false);
  const records = translated.input.map((record) => [record[0], record[1]]);
  assert.deepEqual(
    records,
    [
      [15, 1],
      [15, 0],
    ],
    "each capture transition reaches the guest exactly once",
  );
  translated.stop();
});

test("both browser backends answer the pointer capture hostcall", async () => {
  const runtime = await readFile(
    resolve(packageRoot, "dist/polkavm-browser-runtime.wasm"),
  );
  const program = await readFile(
    resolve(
      repositoryRoot,
      "rust/crates/polkavm-host-runtime/tests/fixtures/pointer-capture.polkavm",
    ),
  );
  const status = (bytes) => {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    return [0, 4, 8, 12].map((offset) => view.getInt32(offset, true));
  };

  const { messages, receiver } = endpoint();
  receiver.onmessage({
    data: {
      type: "start",
      runtime: bytesBuffer(runtime),
      program: bytesBuffer(program),
      assets: [],
      graphicsProfile: "framebuffer",
      audioEnabled: false,
      cacheKey: "pointer-capture-hostcall",
      pointerCaptureSupported: true,
    },
  });
  const ready = await waitForMessage(messages, "ready");
  assert.equal(
    ready.usesPointerCapture,
    true,
    "the Host learns that this guest arms capture itself",
  );
  const saved = await waitForMessage(messages, "save");
  assert.deepEqual(
    status(saved.bytes),
    [1, -2, 0, 1],
    "arm, undefined request, release, arm",
  );
  const request = await waitForMessage(messages, "pointer-capture");
  assert.equal(request.capture, true, "the newest guest request reaches the Host");
  const compiled = await waitForMessage(messages, "compiled");
  receiver.onmessage({ data: { type: "stop" } });
  await waitForMessage(messages, "terminated");

  const outputs = [];
  const unsupported = new globalThis.TranslatedPolkaVmRuntime(
    compiled.module,
    [],
    (output) => outputs.push(output),
    1_000_000,
    false,
    "framebuffer",
  );
  assert.equal(unsupported.usesPointerCapture(), true);
  unsupported.initialize();
  assert.deepEqual(
    status(outputs.find((output) => output.type === "save").bytes),
    [-1, -1, -1, -1],
    "a backend without capture support answers every request alike",
  );
  assert.equal(unsupported.takePointerCaptureRequest(), null);
  unsupported.stop();

  const supportedOutputs = [];
  const supported = new globalThis.TranslatedPolkaVmRuntime(
    compiled.module,
    [],
    (output) => supportedOutputs.push(output),
    1_000_000,
    false,
    "framebuffer",
  );
  supported.setPointerCaptureSupported(true);
  supported.initialize();
  assert.deepEqual(
    status(supportedOutputs.find((output) => output.type === "save").bytes),
    [1, -2, 0, 1],
    "the translated backend matches the native status codes",
  );
  assert.equal(supported.takePointerCaptureRequest(), true);
  supported.setPointerCaptureSupported(false);
  supported.initialize();
  assert.equal(
    supported.takePointerCaptureRequest(),
    null,
    "revoking support drops the request the Host has not served",
  );
  supported.stop();
});
