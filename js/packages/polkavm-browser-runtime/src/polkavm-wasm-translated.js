/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

"use strict";

(() => {
  const STATUS_FINISHED = -1;
  const STATUS_ECALL = -2;
  const STATUS_TRAP = -3;
  const STATUS_OUT_OF_GAS = -4;
  const INPUT_EVENT_BYTES = 8;
  const MOTION_SAMPLE_BYTES = 48;
  const MOTION_STATUS_UNAVAILABLE = 0;
  const MOTION_STATUS_AVAILABLE = 1;
  const MOTION_STATUS_PERMISSION_DENIED = 2;
  const MOTION_ERROR_UNAVAILABLE = -1;
  const MOTION_ERROR_PERMISSION_DENIED = -2;
  const MOTION_ERROR_INVALID_GUEST_RANGE = -3;
  const MOTION_ERROR_BUFFER_TOO_SMALL = -4;
  const INPUT_POINTER_CAPTURE = 15;
  const POINTER_CAPTURE_IMPORT = "host_pointer_capture";
  const POINTER_CAPTURE_RELEASE = 0;
  const POINTER_CAPTURE_ARM = 1;
  const POINTER_CAPTURE_RELEASED = 0;
  const POINTER_CAPTURE_ARMED = 1;
  const POINTER_CAPTURE_ACTIVE = 2;
  const POINTER_CAPTURE_UNSUPPORTED = -1;
  const POINTER_CAPTURE_INVALID_REQUEST = -2;
  const MAX_INPUT_EVENTS = 4096;
  const MAX_HOSTCALLS_PER_INIT = 1024 * 1024;
  const MAX_HOSTCALLS_PER_UPDATE = 65536;
  const MAX_HOSTCALL_BYTES = 32 * 1024 * 1024;
  const MAX_LOG_BYTES = 4 * 1024;
  const MAX_SAVE_BYTES = 1024 * 1024;
  const MAX_AUDIO_SAMPLES = 48000 * 2;
  const MAX_FRAME_BYTES = 16 * 1024 * 1024;
  const MAX_TRI2D_BYTES = 8 * 1024 * 1024;
  const MAX_UI_SEMANTICS_BYTES = 256 * 1024;
  const MAX_UI_SEMANTIC_NODES = 1024;
  const MAX_UI_SEMANTIC_STRING_BYTES = 1024;
  const UI_OUTPUT_HEADER_BYTES = 48;
  const UI_OUTPUT_COMMAND_HEADER_BYTES = 8;
  const MAX_UI_OUTPUT_BYTES = 256 * 1024;
  const MAX_UI_OUTPUT_COMMANDS = 64;
  const MAX_UI_COPY_TEXT_BYTES = 64 * 1024;
  const MAX_UI_OPEN_URL_BYTES = 8 * 1024;
  const UI_CURSOR_ICONS = Object.freeze([
    "default",
    "none",
    "context-menu",
    "help",
    "pointer",
    "progress",
    "wait",
    "cell",
    "crosshair",
    "text",
    "vertical-text",
    "alias",
    "copy",
    "move",
    "no-drop",
    "not-allowed",
    "grab",
    "grabbing",
    "all-scroll",
    "ew-resize",
    "nesw-resize",
    "nwse-resize",
    "ns-resize",
    "e-resize",
    "se-resize",
    "s-resize",
    "sw-resize",
    "w-resize",
    "nw-resize",
    "n-resize",
    "ne-resize",
    "col-resize",
    "row-resize",
    "zoom-in",
    "zoom-out",
  ]);
  const MAX_GPU_BATCH_BYTES = 4 * 1024 * 1024;
  const MAX_GPU_EVENT_BYTES = 64 * 1024;
  const MAX_GPU_EVENTS = 256;
  const MAX_GPU_SUBMITS_PER_UPDATE = 8;
  const MAX_HOST_FRAME_BYTES = 1024 * 1024;
  const MAX_HOST_FRAMES = 32;
  const MAX_QUEUED_HOST_FRAME_BYTES = 4 * 1024 * 1024;
  const MAX_GPU_COMMANDS = 16_384;
  const GPU_ERROR_MALFORMED_BATCH = -2;
  const GPU_ERROR_QUOTA_EXCEEDED = -3;
  const GPU_ERROR_INVALID_STATE = -5;
  const IOV_MAX = 1024n;
  const AT_FDCWD = BigInt.asUintN(64, -100n);
  const ENOSYS = 38;
  const EFAULT = 14;
  const ENOENT = 2;
  const EBADF = 9;
  const EACCES = 13;
  const EINVAL = 22;
  const SYS_OPENAT = 56n;
  const SYS_CLOSE = 57n;
  const SYS_LSEEK = 62n;
  const SYS_READ = 63n;
  const SYS_READV = 65n;
  const SYS_WRITEV = 66n;
  const SYS_EXIT = 93n;
  const decoder = new TextDecoder();
  const encoder = new TextEncoder();
  const strictDecoder = new TextDecoder("utf-8", { fatal: true });

  function isWebGpuProfile(profile) {
    return profile === "webgpu-raster" || profile === "webgpu";
  }

  function isComputeOpcode(opcode) {
    return opcode >= 24 && opcode <= 29;
  }

  function validUiSemantics(bytes) {
    let snapshot;
    try {
      snapshot = JSON.parse(decoder.decode(bytes));
    } catch {
      return false;
    }
    if (
      snapshot?.version !== 1 ||
      !Number.isSafeInteger(snapshot.generation) ||
      snapshot.generation < 0 ||
      !Array.isArray(snapshot.nodes) ||
      !snapshot.nodes.length ||
      snapshot.nodes.length > MAX_UI_SEMANTIC_NODES
    ) {
      return false;
    }
    const ids = new Set();
    let roots = 0;
    for (const node of snapshot.nodes) {
      if (
        typeof node?.id !== "string" ||
        !/^[0-9a-f]{1,16}$/.test(node.id) ||
        ids.has(node.id) ||
        !Array.isArray(node.bounds) ||
        node.bounds.length !== 4 ||
        !node.bounds.every(Number.isFinite) ||
        node.bounds[2] < node.bounds[0] ||
        node.bounds[3] < node.bounds[1] ||
        typeof node.name !== "string" ||
        encoder.encode(node.name).byteLength > MAX_UI_SEMANTIC_STRING_BYTES ||
        typeof node.value !== "string" ||
        encoder.encode(node.value).byteLength > MAX_UI_SEMANTIC_STRING_BYTES
      ) {
        return false;
      }
      ids.add(node.id);
      if (node.parent === null) {
        roots++;
      }
    }
    return (
      roots === 1 &&
      snapshot.nodes.every(
        (node) => node.parent === null || ids.has(node.parent),
      )
    );
  }

  function decodeUiOutput(bytes) {
    if (
      !(bytes instanceof Uint8Array) ||
      bytes.byteLength < UI_OUTPUT_HEADER_BYTES ||
      bytes.byteLength > MAX_UI_OUTPUT_BYTES
    ) {
      return null;
    }
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    if (
      bytes[0] !== 0x50 ||
      bytes[1] !== 0x55 ||
      bytes[2] !== 0x49 ||
      bytes[3] !== 0x31 ||
      view.getUint16(4, true) !== 1 ||
      view.getUint16(6, true) !== UI_OUTPUT_HEADER_BYTES ||
      view.getUint32(8, true) !== bytes.byteLength
    ) {
      return null;
    }
    const commandCount = view.getUint16(12, true);
    const cursorIcon = UI_CURSOR_ICONS[bytes[14]];
    const flags = bytes[15];
    if (
      commandCount > MAX_UI_OUTPUT_COMMANDS ||
      cursorIcon === undefined ||
      (flags & ~3) !== 0
    ) {
      return null;
    }
    const readRect = (offset) => [
      view.getFloat32(offset, true),
      view.getFloat32(offset + 4, true),
      view.getFloat32(offset + 8, true),
      view.getFloat32(offset + 12, true),
    ];
    const validRect = (rect) =>
      rect.every(Number.isFinite) && rect[2] >= rect[0] && rect[3] >= rect[1];
    let ime = null;
    if (flags & 2) {
      const rect = readRect(16);
      const cursorRect = readRect(32);
      if (!validRect(rect) || !validRect(cursorRect)) {
        return null;
      }
      ime = { rect, cursorRect };
    } else if (bytes.subarray(16, UI_OUTPUT_HEADER_BYTES).some((byte) => byte)) {
      return null;
    }

    const commands = [];
    let offset = UI_OUTPUT_HEADER_BYTES;
    try {
      for (let index = 0; index < commandCount; index++) {
        if (offset + UI_OUTPUT_COMMAND_HEADER_BYTES > bytes.byteLength) {
          return null;
        }
        const opcode = bytes[offset];
        const commandFlags = bytes[offset + 1];
        if (bytes[offset + 2] !== 0 || bytes[offset + 3] !== 0) {
          return null;
        }
        const payloadLength = view.getUint32(offset + 4, true);
        const payloadStart = offset + UI_OUTPUT_COMMAND_HEADER_BYTES;
        const payloadEnd = payloadStart + payloadLength;
        if (payloadEnd > bytes.byteLength) {
          return null;
        }
        const payload = strictDecoder.decode(
          bytes.subarray(payloadStart, payloadEnd),
        );
        if (opcode === 1) {
          if (commandFlags !== 0 || payloadLength > MAX_UI_COPY_TEXT_BYTES) {
            return null;
          }
          commands.push({ type: "copy-text", text: payload });
        } else if (opcode === 2) {
          if (
            (commandFlags & ~1) !== 0 ||
            payloadLength === 0 ||
            payloadLength > MAX_UI_OPEN_URL_BYTES
          ) {
            return null;
          }
          commands.push({
            type: "open-url",
            url: payload,
            newSurface: (commandFlags & 1) !== 0,
          });
        } else {
          return null;
        }
        offset = payloadEnd;
      }
    } catch {
      return null;
    }
    if (offset !== bytes.byteLength) {
      return null;
    }
    return {
      cursorIcon,
      mutableTextUnderCursor: (flags & 1) !== 0,
      ime,
      commands,
    };
  }

  function readMetadata(module) {
    const sections = WebAssembly.Module.customSections(
      module,
      "epoca.pvm.meta",
    );
    if (sections.length !== 1) {
      throw new Error("translated PolkaVM module has invalid metadata");
    }
    const bytes = new Uint8Array(sections[0]);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    let offset = 0;
    const requireBytes = (length) => {
      if (offset + length > bytes.byteLength) {
        throw new Error("translated PolkaVM metadata is truncated");
      }
    };
    const readU16 = () => {
      requireBytes(2);
      const value = view.getUint16(offset, true);
      offset += 2;
      return value;
    };
    const readU32 = () => {
      requireBytes(4);
      const value = view.getUint32(offset, true);
      offset += 4;
      return value;
    };
    const readString = (length) => {
      requireBytes(length);
      const value = decoder.decode(bytes.subarray(offset, offset + length));
      offset += length;
      return value;
    };
    requireBytes(4);
    if (decoder.decode(bytes.subarray(0, 4)) !== "EPM2") {
      throw new Error("translated PolkaVM metadata has an incompatible ABI");
    }
    offset = 4;
    const is64Bit = readU32() !== 0;
    const names = [
      "roAddress",
      "roSize",
      "roPhysical",
      "rwAddress",
      "rwSize",
      "rwPhysical",
      "heapBase",
      "heapLimit",
      "stackLow",
      "stackHigh",
      "stackPhysical",
    ];
    const layout = {};
    for (const name of names) {
      layout[name] = readU32();
    }
    const imports = [];
    const importCount = readU32();
    for (let index = 0; index < importCount; index++) {
      const length = readU16();
      imports.push(length ? readString(length) : null);
    }
    const exports = new Map();
    const exportCount = readU32();
    for (let index = 0; index < exportCount; index++) {
      const name = readString(readU16());
      const block = readU32();
      if (!name || exports.has(name)) {
        throw new Error("translated PolkaVM metadata has invalid exports");
      }
      exports.set(name, block);
    }
    if (offset !== bytes.byteLength) {
      throw new Error("translated PolkaVM metadata has trailing data");
    }
    return { is64Bit, layout, imports, exports };
  }

  function errno(code) {
    return BigInt.asUintN(64, -BigInt(code));
  }

  function normalizedPath(path) {
    while (path.startsWith("./")) {
      path = path.slice(2);
    }
    return path.replace(/^\/+/, "");
  }

  function hidToCoreVm(code) {
    if (code >= 0x04 && code <= 0x1d) {
      return 0x61 + code - 0x04;
    }
    if (code >= 0x1e && code <= 0x26) {
      return 0x31 + code - 0x1e;
    }
    const keys = new Map([
      [0x27, 0x30],
      [0x28, 0x0a],
      [0x58, 0x0a],
      [0x29, 0x1b],
      [0x2a, 0x08],
      [0x2b, 0x09],
      [0x2c, 0x20],
      [0x2d, 0x2d],
      [0x56, 0x2d],
      [0x2e, 0x3d],
      [0x2f, 0x5b],
      [0x30, 0x5d],
      [0x31, 0x5c],
      [0x33, 0x3b],
      [0x34, 0x27],
      [0x35, 0x60],
      [0x36, 0x2c],
      [0x37, 0x2e],
      [0x63, 0x2e],
      [0x38, 0x2f],
      [0x54, 0x2f],
      [0x46, 0x91],
      [0x47, 0x92],
      [0x48, 0x93],
      [0x49, 0x94],
      [0x4a, 0x96],
      [0x4b, 0x98],
      [0x4c, 0x95],
      [0x4d, 0x97],
      [0x4e, 0x99],
      [0x4f, 0x82],
      [0x50, 0x83],
      [0x51, 0x81],
      [0x52, 0x80],
      [0x55, 0x2a],
      [0x57, 0x2b],
      [0x59, 0x97],
      [0x5a, 0x81],
      [0x5b, 0x99],
      [0x5c, 0x83],
      [0x5d, 0x35],
      [0x5e, 0x82],
      [0x5f, 0x96],
      [0x60, 0x80],
      [0x61, 0x98],
      [0x62, 0x2e],
      [0xe0, 0x9c],
      [0xe1, 0x9a],
      [0xe2, 0x9e],
      [0xe4, 0x9d],
      [0xe5, 0x9b],
      [0xe6, 0x9f],
    ]);
    if (code >= 0x3a && code <= 0x45) {
      return 0x84 + code - 0x3a;
    }
    return keys.get(code);
  }

  function validMotionSample(bytes) {
    if (
      !(bytes instanceof Uint8Array) ||
      bytes.byteLength !== MOTION_SAMPLE_BYTES
    ) {
      return false;
    }
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const flags = view.getUint16(6, true);
    if (
      bytes[0] !== 0x50 ||
      bytes[1] !== 0x4d ||
      bytes[2] !== 0x4f ||
      bytes[3] !== 0x31 ||
      view.getUint16(4, true) !== 1 ||
      !flags ||
      flags & ~7 ||
      (flags & 4 && !(flags & 2)) ||
      view.getUint32(8, true) !== MOTION_SAMPLE_BYTES ||
      view.getUint32(12, true) === 0
    ) {
      return false;
    }
    const timestamp = view.getFloat64(16, true);
    if (!Number.isFinite(timestamp) || timestamp < 0) {
      return false;
    }
    for (let offset = 24; offset < MOTION_SAMPLE_BYTES; offset += 4) {
      if (!Number.isFinite(view.getFloat32(offset, true))) {
        return false;
      }
    }
    return true;
  }

  class TranslatedPolkaVmRuntime {
    constructor(
      module,
      assets,
      emit,
      maxGas,
      audioEnabled,
      graphicsProfile,
      gpuCapabilities = null,
      motionAvailability = MOTION_STATUS_UNAVAILABLE,
    ) {
      this.metadata = readMetadata(module);
      this.instance = new WebAssembly.Instance(module, {});
      this.pvm = this.instance.exports;
      this.memory = this.pvm.memory;
      if (!(this.memory instanceof WebAssembly.Memory)) {
        throw new Error("translated PolkaVM module is missing guest memory");
      }
      this.assets = new Map(
        assets.map((asset) => [
          normalizedPath(asset.path),
          new Uint8Array(asset.bytes),
        ]),
      );
      this.emit = emit;
      this.audioEnabled = audioEnabled;
      if (
        !["framebuffer", "tri2d", "webgpu-raster", "webgpu"].includes(
          graphicsProfile,
        )
      ) {
        throw new Error(
          `translated PolkaVM runtime has invalid graphics profile ${graphicsProfile}`,
        );
      }
      this.graphicsProfile = graphicsProfile;
      if (
        isWebGpuProfile(graphicsProfile) &&
        !(gpuCapabilities instanceof Uint8Array)
      ) {
        throw new Error(
          "WebGPU capabilities are required before PolkaVM initialization",
        );
      }
      this.gpuCapabilities =
        gpuCapabilities instanceof Uint8Array ? gpuCapabilities.slice() : null;
      this.gpuEvents = [];
      this.gpuSubmits = 0;
      this.gpuLastSequence = 0n;
      this.hostFrameRequests = 0;
      this.hostFrameRequestBytes = 0;
      this.hostFrameResponses = [];
      this.hostFrameResponseBytes = 0;
      this.tri2dSubmitted = false;
      this.uiSemanticsSubmitted = false;
      this.uiOutputSubmitted = false;
      this.maxGas = BigInt(maxGas);
      this.input = [];
      this.coreInput = [];
      this.epocaInput = [];
      this.pointer = null;
      this.setMotionAvailability(motionAvailability);
      this.motionSample = null;
      this.pointerCapture = {
        supported: false,
        armed: false,
        active: false,
        request: null,
      };
      this.timeMs = null;
      this.clockStartedAt = performance.now();
      this.hostcalls = 0;
      this.hostcallBytes = 0;
      this.resumePending = false;
      this.stopped = false;
      this.coreVm = this.metadata.exports.has("_pvm_start");
      if (this.coreVm && graphicsProfile !== "framebuffer") {
        throw new Error(
          "CoreVM guests require the framebuffer graphics profile",
        );
      }
      this.coreVmStarted = false;
      this.palette = new Uint32Array(256);
      this.palette.fill(0xffffffff);
      this.audioChannels = 0;
      this.audioSampleRate = 0;
      this.fds = new Map();
      this.nextFd = 3;
      this.imports = this.metadata.imports;
      this.exports = this.metadata.exports;
      for (let index = 0; index < 13; index++) {
        if (!(this.pvm[`r${index}`] instanceof WebAssembly.Global)) {
          throw new Error("translated PolkaVM module is missing registers");
        }
      }
    }

    initialize() {
      this.#resetBudget(MAX_HOSTCALLS_PER_INIT);
      if (this.coreVm) {
        this.#setupCoreVm();
        return;
      }
      const init = this.exports.get("init");
      if (init !== undefined) {
        this.#run(init, false);
      }
    }

    usesMotion() {
      return this.imports.includes("host_motion_read");
    }

    usesPointerCapture() {
      return this.imports.includes(POINTER_CAPTURE_IMPORT);
    }

    setPointerCaptureSupported(supported) {
      this.pointerCapture.supported = supported === true;
      if (!this.pointerCapture.supported) {
        this.pointerCapture.armed = false;
        this.pointerCapture.request = null;
      }
    }

    setPointerCaptureActive(active) {
      const next = active === true;
      if (this.pointerCapture.active === next) {
        return;
      }
      this.pointerCapture.active = next;
      if (next) {
        this.pointerCapture.armed = false;
      }
      const record = new Uint8Array(INPUT_EVENT_BYTES);
      record[0] = INPUT_POINTER_CAPTURE;
      record[1] = next ? 1 : 0;
      this.sendInput(record);
    }

    takePointerCaptureRequest() {
      const request = this.pointerCapture.request;
      this.pointerCapture.request = null;
      return request;
    }

    #requestPointerCapture(request) {
      const state = this.pointerCapture;
      if (!state.supported) {
        return POINTER_CAPTURE_UNSUPPORTED;
      }
      if (request === POINTER_CAPTURE_ARM) {
        state.armed = true;
        state.request = true;
      } else if (request === POINTER_CAPTURE_RELEASE) {
        state.armed = false;
        state.request = false;
      } else {
        return POINTER_CAPTURE_INVALID_REQUEST;
      }
      if (state.active) {
        return POINTER_CAPTURE_ACTIVE;
      }
      return state.armed ? POINTER_CAPTURE_ARMED : POINTER_CAPTURE_RELEASED;
    }

    update(timeMs) {
      if (this.stopped) {
        return;
      }
      this.timeMs = timeMs;
      this.gpuSubmits = 0;
      this.hostFrameRequests = 0;
      this.uiSemanticsSubmitted = false;
      this.uiOutputSubmitted = false;
      this.hostFrameRequestBytes = 0;
      this.#resetBudget(
        this.coreVm && !this.coreVmStarted
          ? MAX_HOSTCALLS_PER_INIT
          : MAX_HOSTCALLS_PER_UPDATE,
      );
      if (this.coreVm) {
        this.#run(this.exports.get("_pvm_start"), true);
        this.coreVmStarted = true;
        return;
      }
      const update = this.exports.get("update");
      if (update === undefined) {
        throw new Error("translated PolkaVM guest has no update export");
      }
      this.#run(update, false);
    }

    sendInput(bytes) {
      if (this.stopped || bytes.byteLength !== INPUT_EVENT_BYTES) {
        return;
      }
      if (!this.coreVm) {
        if (this.input.length === MAX_INPUT_EVENTS) {
          this.input.shift();
        }
        this.input.push(bytes.slice());
        return;
      }
      const view = new DataView(
        bytes.buffer,
        bytes.byteOffset,
        bytes.byteLength,
      );
      const type = bytes[0];
      if (
        type === 6 &&
        (Math.abs(view.getInt16(2, true)) > 127 ||
          Math.abs(view.getInt16(4, true)) > 127)
      ) {
        return;
      }
      if (this.imports.includes("pvm_fetch_epoca_inputs")) {
        this.#queueEpocaInput(bytes);
        return;
      }
      if (type === 1 || type === 2) {
        const key = hidToCoreVm(bytes[1]);
        if (key !== undefined) {
          this.#queueCoreInput(key, type === 1 ? 1 : 0);
        }
      } else if (type === 3 || type === 4) {
        const key =
          bytes[1] >= 1 && bytes[1] <= 3 ? 0x9f + bytes[1] : undefined;
        if (key !== undefined) {
          this.#queueCoreInput(key, type === 3 ? 1 : 0);
        }
      } else if (type === 5) {
        const current = [view.getUint16(2, true), view.getUint16(4, true)];
        if (this.pointer) {
          this.#queueCoreInput(
            0xa3,
            Math.max(-128, Math.min(127, current[0] - this.pointer[0])) & 0xff,
          );
          this.#queueCoreInput(
            0xa4,
            Math.max(-128, Math.min(127, current[1] - this.pointer[1])) & 0xff,
          );
        }
        this.pointer = current;
      } else if (type === 6) {
        this.#queueCoreInput(
          0xa3,
          Math.max(-128, Math.min(127, view.getInt16(2, true))) & 0xff,
        );
        this.#queueCoreInput(
          0xa4,
          Math.max(-128, Math.min(127, view.getInt16(4, true))) & 0xff,
        );
      }
    }

    setMotionAvailability(availability) {
      if (
        !Number.isInteger(availability) ||
        availability < MOTION_STATUS_UNAVAILABLE ||
        availability > MOTION_STATUS_PERMISSION_DENIED
      ) {
        throw new Error(
          "translated PolkaVM runtime has invalid motion availability",
        );
      }
      this.motionAvailability = availability;
      if (availability !== MOTION_STATUS_AVAILABLE) {
        this.motionSample = null;
      }
    }

    sendMotionSample(bytes) {
      if (this.stopped || !validMotionSample(bytes)) {
        throw new Error(
          "translated PolkaVM runtime received an invalid motion sample",
        );
      }
      this.motionAvailability = MOTION_STATUS_AVAILABLE;
      this.motionSample = bytes.slice();
    }

    setGpuCapabilities(bytes) {
      if (
        this.stopped ||
        !(bytes instanceof Uint8Array) ||
        bytes.byteLength < 56 ||
        bytes.byteLength > 4096
      ) {
        throw new Error("invalid translated WebGPU capabilities");
      }
      this.gpuCapabilities = bytes.slice();
    }

    sendGpuEvent(bytes) {
      if (
        this.stopped ||
        !(bytes instanceof Uint8Array) ||
        bytes.byteLength < 24 ||
        bytes.byteLength > MAX_GPU_EVENT_BYTES ||
        decoder.decode(bytes.subarray(0, 4)) !== "EGE1"
      ) {
        return;
      }
      if (this.gpuEvents.length === MAX_GPU_EVENTS) {
        this.gpuEvents.shift();
      }
      this.gpuEvents.push(bytes.slice());
    }

    sendHostFrameResponse(bytes) {
      if (
        this.stopped ||
        !(bytes instanceof Uint8Array) ||
        !bytes.byteLength ||
        bytes.byteLength > MAX_HOST_FRAME_BYTES
      ) {
        throw new Error("invalid translated host-frame response");
      }
      if (
        this.hostFrameResponses.length === MAX_HOST_FRAMES ||
        this.hostFrameResponseBytes + bytes.byteLength > MAX_QUEUED_HOST_FRAME_BYTES
      ) {
        throw new Error("translated host-frame response queue overflow");
      }
      this.hostFrameResponses.push(bytes.slice());
      this.hostFrameResponseBytes += bytes.byteLength;
    }

    stop() {
      this.stopped = true;
      this.input.length = 0;
      this.coreInput.length = 0;
      this.hostFrameRequests = 0;
      this.hostFrameRequestBytes = 0;
      this.gpuEvents.length = 0;
      this.hostFrameResponses.length = 0;
      this.hostFrameResponseBytes = 0;
    }

    #resetBudget(hostcalls) {
      this.hostcalls = hostcalls;
      this.hostcallBytes = MAX_HOSTCALL_BYTES;
      this.tri2dSubmitted = false;
      this.pvm.pvm_set_gas(this.maxGas);
    }

    #run(entry, yieldOnFrame) {
      let status;
      if (this.resumePending) {
        this.resumePending = false;
        status = this.pvm.pvm_resume();
      } else {
        if (entry === undefined) {
          throw new Error("translated PolkaVM entrypoint is missing");
        }
        status = this.pvm.pvm_begin(entry, this.maxGas);
      }
      for (;;) {
        if (status === STATUS_FINISHED) {
          if (this.coreVm) {
            throw new Error("CoreVM guest exited");
          }
          return;
        }
        if (status === STATUS_TRAP) {
          throw new Error(
            `translated PolkaVM execution trapped at ${this.pvm.trap_pc.value}`,
          );
        }
        if (status === STATUS_OUT_OF_GAS) {
          throw new Error("translated PolkaVM guest ran out of gas");
        }
        if (status !== STATUS_ECALL) {
          throw new Error(
            `translated PolkaVM returned invalid status ${status}`,
          );
        }
        const importIndex = this.pvm.ecall.value >>> 0;
        const name = this.imports[importIndex];
        if (!name) {
          throw new Error(
            `translated PolkaVM called unknown import ${importIndex}`,
          );
        }
        this.hostcalls--;
        const yielded = this.coreVm
          ? this.#handleCoreVmCall(name)
          : this.#handleCooperativeCall(name);
        if (yielded && yieldOnFrame) {
          this.resumePending = true;
          return;
        }
        if (this.hostcalls === 0) {
          // Resume on the next worker tick after completing this ECALL. Large
          // assets can require more than one bounded hostcall slice, while
          // returning here keeps each slice capped and the worker responsive.
          this.resumePending = true;
          return;
        }
        status = this.pvm.pvm_resume();
      }
    }

    #reg(index) {
      return BigInt.asUintN(64, this.pvm[`r${index}`].value);
    }

    #setReg(index, value) {
      const normalized = this.metadata.is64Bit
        ? BigInt.asIntN(64, BigInt(value))
        : BigInt.asUintN(32, BigInt(value));
      this.pvm[`r${index}`].value = normalized;
    }
    #setU64Result(value) {
      const normalized = BigInt.asUintN(64, BigInt(value));
      this.#setReg(7, normalized);
      if (!this.metadata.is64Bit) {
        this.#setReg(8, normalized >> 32n);
      }
    }


    #u32(value) {
      return Number(value & 0xffffffffn) >>> 0;
    }

    #range(address, length, write = false) {
      address >>>= 0;
      length >>>= 0;
      const end = address + length;
      if (end > 0x100000000) {
        throw new Error(
          "translated PolkaVM guest memory access is out of range",
        );
      }
      const { layout } = this.metadata;
      let physical;
      if (address >= layout.stackLow && end <= layout.stackHigh) {
        physical = layout.stackPhysical + address - layout.stackLow;
      } else if (
        address >= layout.rwAddress &&
        end <=
          layout.rwAddress + this.memory.buffer.byteLength - layout.rwPhysical
      ) {
        physical = layout.rwPhysical + address - layout.rwAddress;
      } else if (
        !write &&
        address >= layout.roAddress &&
        end <= layout.roAddress + layout.roSize
      ) {
        physical = layout.roPhysical + address - layout.roAddress;
      } else {
        throw new Error(
          "translated PolkaVM guest memory access is out of range",
        );
      }
      return new Uint8Array(this.memory.buffer, physical, length);
    }

    #read(address, length) {
      this.#chargeBytes(length);
      return this.#range(address, length).slice();
    }

    #write(address, bytes) {
      this.#chargeBytes(bytes.byteLength);
      this.#range(address, bytes.byteLength, true).set(bytes);
    }

    #chargeBytes(length) {
      if (
        !Number.isSafeInteger(length) ||
        length < 0 ||
        length > this.hostcallBytes
      ) {
        throw new Error(
          "translated PolkaVM guest exceeded hostcall byte budget",
        );
      }
      this.hostcallBytes -= length;
    }

    #readU64(address) {
      const bytes = this.#range(address >>> 0, 8);
      return new DataView(bytes.buffer, bytes.byteOffset, 8).getBigUint64(
        0,
        true,
      );
    }

    #writeU64(address, value) {
      const bytes = this.#range(address >>> 0, 8, true);
      new DataView(bytes.buffer, bytes.byteOffset, 8).setBigUint64(
        0,
        BigInt.asUintN(64, value),
        true,
      );
    }

    #readCString(address) {
      const output = [];
      for (let offset = 0; offset < 255; offset++) {
        let byte;
        try {
          byte = this.#range((address + offset) >>> 0, 1)[0];
        } catch {
          return null;
        }
        if (!byte) {
          return new Uint8Array(output);
        }
        output.push(byte);
      }
      return null;
    }

    // eslint-disable-next-line complexity -- Flat hostcall dispatch mirrors the guest ABI.
    #handleCooperativeCall(name) {
      const a0 = this.#reg(7);
      const a1 = this.#reg(8);
      const a2 = this.#reg(9);
      const a3 = this.#reg(10);
      const a4 = this.#reg(11);
      switch (name) {
        case "host_present_frame": {
          const width = this.#u32(a1);
          const height = this.#u32(a2);
          const stride = this.#u32(a3);
          const rowBytes = width * 4;
          const length = rowBytes * height;
          if (
            !width ||
            !height ||
            stride !== rowBytes ||
            length > MAX_FRAME_BYTES
          ) {
            this.#setReg(7, 1n);
            return false;
          }
          if (this.graphicsProfile !== "framebuffer") {
            this.#setReg(7, 3n);
            return false;
          }
          const source = this.#read(this.#u32(a0), length);
          const pixels = new Uint8Array(length);
          for (let index = 0; index < length; index += 4) {
            pixels[index] = source[index + 2];
            pixels[index + 1] = source[index + 1];
            pixels[index + 2] = source[index];
            pixels[index + 3] = source[index + 3];
          }
          this.emit({ type: "frame", width, height, pixels }, [pixels.buffer]);
          this.#setReg(7, 0n);
          return false;
        }
        case "host_tri2d_submit": {
          const length = this.#u32(a1);
          if (!length || length > MAX_TRI2D_BYTES) {
            this.#setReg(7, 1n);
            return false;
          }
          if (this.graphicsProfile !== "tri2d") {
            this.#setReg(7, 3n);
            return false;
          }
          if (this.tri2dSubmitted) {
            this.#setReg(7, 2n);
            return false;
          }
          const bytes = this.#read(this.#u32(a0), length);
          this.emit({ type: "tri2d", bytes }, [bytes.buffer]);
          this.tri2dSubmitted = true;
          this.#setReg(7, 0n);
          return false;
        }
        case "host_ui_semantics_submit": {
          const length = this.#u32(a1);
          if (!length || length > MAX_UI_SEMANTICS_BYTES) {
            this.#setReg(7, 1n);
            return false;
          }
          if (this.uiSemanticsSubmitted) {
            this.#setReg(7, 2n);
            return false;
          }
          const bytes = this.#read(this.#u32(a0), length);
          if (!validUiSemantics(bytes)) {
            this.#setReg(7, 1n);
            return false;
          }
          this.emit({ type: "ui-semantics", bytes }, [bytes.buffer]);
          this.uiSemanticsSubmitted = true;
          this.#setReg(7, 0n);
          return false;
        }
        case "host_ui_output_submit": {
          const length = this.#u32(a1);
          if (
            length < UI_OUTPUT_HEADER_BYTES ||
            length > MAX_UI_OUTPUT_BYTES
          ) {
            this.#setReg(7, 1n);
            return false;
          }
          this.#chargeBytes(length);
          if (this.uiOutputSubmitted) {
            this.#setReg(7, 2n);
            return false;
          }
          let bytes;
          try {
            bytes = this.#range(this.#u32(a0), length).slice();
          } catch {
            this.#setReg(7, 1n);
            return false;
          }
          const output = decodeUiOutput(bytes);
          if (output === null) {
            this.#setReg(7, 1n);
            return false;
          }
          this.emit({ type: "ui-output", output });
          this.uiOutputSubmitted = true;
          this.#setReg(7, 0n);
          return false;
        }
        case "host_gpu_capabilities": {
          if (!isWebGpuProfile(this.graphicsProfile)) {
            this.#setReg(7, BigInt(GPU_ERROR_INVALID_STATE));
            return false;
          }
          if (this.gpuCapabilities === null) {
            this.#setReg(7, BigInt(GPU_ERROR_INVALID_STATE));
            return false;
          }
          const capacity = this.#u32(a1);
          const required = this.gpuCapabilities.byteLength;
          if (capacity < required) {
            this.#setReg(7, BigInt(-required));
            return false;
          }
          this.#write(this.#u32(a0), this.gpuCapabilities);
          this.#setReg(7, BigInt(required));
          return false;
        }
        case "host_gpu_submit": {
          if (!isWebGpuProfile(this.graphicsProfile)) {
            this.#setReg(7, BigInt(GPU_ERROR_INVALID_STATE));
            return false;
          }
          const length = this.#u32(a1);
          if (this.gpuCapabilities === null) {
            this.#setReg(7, BigInt(GPU_ERROR_INVALID_STATE));
            return false;
          }
          if (
            !length ||
            length > MAX_GPU_BATCH_BYTES ||
            this.gpuSubmits === MAX_GPU_SUBMITS_PER_UPDATE
          ) {
            this.#setReg(
              7,
              BigInt(
                this.gpuSubmits === MAX_GPU_SUBMITS_PER_UPDATE
                  ? GPU_ERROR_QUOTA_EXCEEDED
                  : GPU_ERROR_MALFORMED_BATCH,
              ),
            );
            return false;
          }
          const bytes = this.#read(this.#u32(a0), length);
          const sequence = this.#gpuBatchSequence(bytes);
          if (sequence === null) {
            this.#setReg(7, BigInt(GPU_ERROR_MALFORMED_BATCH));
            return false;
          }
          if (
            this.graphicsProfile !== "webgpu" &&
            this.#gpuBatchUsesCompute(bytes)
          ) {
            this.#setReg(7, BigInt(GPU_ERROR_INVALID_STATE));
            return false;
          }
          if (sequence <= this.gpuLastSequence) {
            this.#setReg(7, BigInt(GPU_ERROR_INVALID_STATE));
            return false;
          }
          this.gpuSubmits++;
          this.gpuLastSequence = sequence;
          this.emit({ type: "gpu-batch", bytes }, [bytes.buffer]);
          this.#setReg(7, 0n);
          return false;
        }
        case "host_gpu_receive": {
          if (!isWebGpuProfile(this.graphicsProfile)) {
            this.#setReg(7, BigInt(GPU_ERROR_INVALID_STATE));
            return false;
          }
          const event = this.gpuEvents[0];
          if (event === undefined) {
            this.#setReg(7, 0n);
            return false;
          }
          const capacity = this.#u32(a1);
          if (capacity < event.byteLength) {
            this.#setReg(7, BigInt(-event.byteLength));
            return false;
          }
          this.#write(this.#u32(a0), event);
          this.gpuEvents.shift();
          this.#setReg(7, BigInt(event.byteLength));
          return false;
        }
        case "host_poll_input": {
          const capacity = this.#u32(a1);
          const count = Math.min(
            Math.floor(capacity / INPUT_EVENT_BYTES),
            this.input.length,
          );
          const output = new Uint8Array(count * INPUT_EVENT_BYTES);
          for (let index = 0; index < count; index++) {
            output.set(this.input.shift(), index * INPUT_EVENT_BYTES);
          }
          this.#write(this.#u32(a0), output);
          this.#setReg(7, BigInt(output.byteLength));
          return false;
        }
        case "host_motion_read":
          return this.#readMotion();
        case POINTER_CAPTURE_IMPORT: {
          const status = this.#requestPointerCapture(this.#u32(a0));
          this.#setReg(7, BigInt(status));
          return false;
        }
        case "host_time_ms": {
          const timeMs = this.timeMs ?? performance.now() - this.clockStartedAt;
          this.#setU64Result(BigInt(Math.max(0, Math.trunc(timeMs))));
          return false;
        }
        case "host_sleep_ms":
          this.timeMs =
            (this.timeMs ?? performance.now() - this.clockStartedAt) +
            Math.min(this.#u32(a0), 50);
          return false;
        case "host_audio_submit": {
          if (!this.audioEnabled) {
            this.#setReg(7, 3n);
            return false;
          }
          const sampleCount = this.#u32(a1);
          if (
            !sampleCount ||
            sampleCount % 2 ||
            sampleCount > MAX_AUDIO_SAMPLES
          ) {
            this.#setReg(7, 1n);
            return false;
          }
          const samples = this.#read(this.#u32(a0), sampleCount * 2);
          this.emit(
            { type: "audio", sampleRate: 48000, channels: 2, samples },
            [samples.buffer],
          );
          this.#setReg(7, 0n);
          return false;
        }
        case "host_frame_send": {
          const length = this.#u32(a1);
          if (!length || length > MAX_HOST_FRAME_BYTES) {
            this.#setReg(7, 1n);
            return false;
          }
          if (
            this.hostFrameRequests === MAX_HOST_FRAMES ||
            this.hostFrameRequestBytes + length > MAX_QUEUED_HOST_FRAME_BYTES
          ) {
            this.#setReg(7, 2n);
            return false;
          }
          const bytes = this.#read(this.#u32(a0), length);
          this.emit({ type: "host-frame-request", bytes }, [bytes.buffer]);
          this.hostFrameRequests++;
          this.hostFrameRequestBytes += length;
          this.#setReg(7, 0n);
          return false;
        }
        case "host_frame_poll": {
          const response = this.hostFrameResponses[0];
          if (response === undefined) {
            this.#setReg(7, 0n);
            return false;
          }
          const capacity = this.#u32(a1);
          if (capacity < response.byteLength) {
            this.#setReg(7, BigInt(-response.byteLength));
            return false;
          }
          this.#write(this.#u32(a0), response);
          this.hostFrameResponses.shift();
          this.hostFrameResponseBytes -= response.byteLength;
          this.#setReg(7, BigInt(response.byteLength));
          return false;
        }
        case "host_asset_read": {
          const nameLength = this.#u32(a1);
          const offset = this.#u32(a2);
          const destination = this.#u32(a3);
          const capacity = this.#u32(a4);
          if (!nameLength || nameLength > 1024) {
            this.#setReg(7, 0n);
            return false;
          }
          const assetName = decoder.decode(
            this.#read(this.#u32(a0), nameLength),
          );
          const asset = this.assets.get(assetName);
          if (!asset || offset >= asset.byteLength) {
            this.#setReg(7, 0n);
            return false;
          }
          const length = Math.min(
            capacity,
            asset.byteLength - offset,
            16 * 1024 * 1024,
          );
          this.#write(destination, asset.subarray(offset, offset + length));
          this.#setReg(7, BigInt(length));
          return false;
        }
        case "host_save_submit": {
          const length = this.#u32(a1);
          if (!length || length > MAX_SAVE_BYTES) {
            this.#setReg(7, 1n);
            return false;
          }
          const bytes = this.#read(this.#u32(a0), length);
          this.emit({ type: "save", bytes }, [bytes.buffer]);
          this.#setReg(7, 0n);
          return false;
        }
        case "host_log": {
          const length = Math.min(this.#u32(a1), MAX_LOG_BYTES);
          const message = decoder.decode(this.#read(this.#u32(a0), length));
          this.emit({ type: "log", message });
          return false;
        }
        default:
          throw new Error(
            `translated PolkaVM guest uses unsupported import ${name}`,
          );
      }
    }

    #gpuBatchSequence(bytes) {
      if (
        bytes.byteLength < 24 ||
        decoder.decode(bytes.subarray(0, 4)) !== "EPG1"
      ) {
        return null;
      }
      const view = new DataView(
        bytes.buffer,
        bytes.byteOffset,
        bytes.byteLength,
      );
      if (
        view.getUint16(4, true) !== 1 ||
        view.getUint16(6, true) !== 0 ||
        view.getUint32(8, true) !== bytes.byteLength
      ) {
        return null;
      }
      const commandCount = view.getUint32(12, true);
      const sequence = view.getBigUint64(16, true);
      if (sequence === 0n || commandCount > MAX_GPU_COMMANDS) {
        return null;
      }
      let offset = 24;
      for (let index = 0; index < commandCount; index++) {
        if (
          offset + 8 > bytes.byteLength ||
          view.getUint16(offset + 2, true) !== 0
        ) {
          return null;
        }
        const commandBytes = view.getUint32(offset + 4, true);
        if (
          commandBytes < 8 ||
          commandBytes % 4 ||
          commandBytes > bytes.byteLength - offset
        ) {
          return null;
        }
        offset += commandBytes;
      }
      return offset === bytes.byteLength ? sequence : null;
    }

    #gpuBatchUsesCompute(bytes) {
      const view = new DataView(
        bytes.buffer,
        bytes.byteOffset,
        bytes.byteLength,
      );
      const commandCount = view.getUint32(12, true);
      let offset = 24;
      for (let index = 0; index < commandCount; index++) {
        if (isComputeOpcode(view.getUint16(offset, true))) {
          return true;
        }
        offset += view.getUint32(offset + 4, true);
      }
      return false;
    }

    #setupCoreVm() {
      let sp = BigInt(this.metadata.layout.stackHigh);
      const argc = 1n;
      sp -= (1n + argc + 1n + 0n + 1n + 4n) * 8n;
      const addressInit = sp;
      let pointer = sp;
      this.#writeU64(Number(pointer), argc);
      pointer += 8n;
      const argument = encoder.encode("./quake\0");
      sp -= BigInt(argument.byteLength);
      this.#write(Number(sp), argument);
      this.#writeU64(Number(pointer), sp);
      pointer += 16n;
      pointer += 8n;
      this.#writeU64(Number(pointer), 6n);
      this.#writeU64(Number(pointer + 8n), 4096n);
      this.#setReg(1, sp);
      this.#setReg(7, addressInit);
    }

    #readMotion() {
      if (this.motionAvailability === MOTION_STATUS_UNAVAILABLE) {
        this.#setReg(7, BigInt(MOTION_ERROR_UNAVAILABLE));
        return false;
      }
      if (this.motionAvailability === MOTION_STATUS_PERMISSION_DENIED) {
        this.#setReg(7, BigInt(MOTION_ERROR_PERMISSION_DENIED));
        return false;
      }
      if (this.motionSample === null) {
        this.#setReg(7, 0n);
        return false;
      }
      if (this.#u32(this.#reg(8)) < MOTION_SAMPLE_BYTES) {
        this.#setReg(7, BigInt(MOTION_ERROR_BUFFER_TOO_SMALL));
        return false;
      }
      try {
        this.#write(this.#u32(this.#reg(7)), this.motionSample);
      } catch {
        this.#setReg(7, BigInt(MOTION_ERROR_INVALID_GUEST_RANGE));
        return false;
      }
      this.motionSample = null;
      this.#setReg(7, BigInt(MOTION_SAMPLE_BYTES));
      return false;
    }

    #queueEpocaInput(bytes) {
      const event = bytes.slice();
      if (event[0] === 5 || event[0] === 6) {
        const existing = this.epocaInput.findIndex(
          (queued) => queued[0] === event[0],
        );
        if (existing !== -1) {
          this.epocaInput[existing] = event;
          return;
        }
      }
      if (this.epocaInput.length === 256) {
        this.epocaInput.shift();
      }
      this.epocaInput.push(event);
    }

    #queueCoreInput(key, value) {
      if (!value && (key === 0xa3 || key === 0xa4)) {
        return;
      }
      if (key === 0xa3 || key === 0xa4) {
        const existing = this.coreInput.find((event) => event[0] === key);
        if (existing) {
          existing[1] = value;
          return;
        }
      }
      if (this.coreInput.length === 256) {
        this.coreInput.shift();
      }
      this.coreInput.push([key, value]);
    }

    // eslint-disable-next-line complexity -- Flat hostcall dispatch mirrors the guest ABI.
    #handleCoreVmCall(name) {
      switch (name) {
        case "host_frame_send":
        case "host_frame_poll":
        case "host_motion_read":
        case POINTER_CAPTURE_IMPORT:
          return this.#handleCooperativeCall(name);
        case "pvm_set_palette": {
          const palette = this.#read(this.#u32(this.#reg(7)), 256 * 3);
          for (let index = 0; index < 256; index++) {
            const offset = index * 3;
            this.palette[index] =
              palette[offset] |
              (palette[offset + 1] << 8) |
              (palette[offset + 2] << 16) |
              0xff000000;
          }
          return false;
        }
        case "pvm_display": {
          const width = this.#u32(this.#reg(7));
          const height = this.#u32(this.#reg(8));
          const length = width * height;
          if (!width || !height || length > MAX_FRAME_BYTES / 4) {
            throw new Error("CoreVM guest supplied invalid frame dimensions");
          }
          const indices = this.#read(this.#u32(this.#reg(9)), length);
          const pixels = new Uint8Array(length * 4);
          const rgba = new Uint32Array(pixels.buffer);
          for (let index = 0; index < length; index++) {
            rgba[index] = this.palette[indices[index]];
          }
          this.emit({ type: "frame", width, height, pixels }, [pixels.buffer]);
          return true;
        }
        case "pvm_fetch_epoca_inputs": {
          const count = Math.min(
            this.#u32(this.#reg(8)),
            this.epocaInput.length,
          );
          const output = new Uint8Array(count * INPUT_EVENT_BYTES);
          for (let index = 0; index < count; index++) {
            output.set(this.epocaInput.shift(), index * INPUT_EVENT_BYTES);
          }
          this.#write(this.#u32(this.#reg(7)), output);
          this.#setReg(7, BigInt(count));
          return false;
        }
        case "pvm_fetch_inputs": {
          const count = Math.min(
            this.#u32(this.#reg(8)),
            this.coreInput.length,
          );
          const output = new Uint8Array(count * 2);
          for (let index = 0; index < count; index++) {
            output.set(this.coreInput.shift(), index * 2);
          }
          this.#write(this.#u32(this.#reg(7)), output);
          this.#setReg(7, BigInt(count));
          return false;
        }
        case "pvm_asset_read": {
          const nameLength = this.#u32(this.#reg(8));
          const offset = this.#u32(this.#reg(9));
          const destination = this.#u32(this.#reg(10));
          const capacity = this.#u32(this.#reg(11));
          if (!nameLength || nameLength > 1024) {
            this.#setReg(7, 0n);
            return false;
          }
          const assetName = decoder.decode(
            this.#read(this.#u32(this.#reg(7)), nameLength),
          );
          const asset = this.assets.get(assetName);
          if (!asset || offset >= asset.byteLength) {
            this.#setReg(7, 0n);
            return false;
          }
          const length = Math.min(
            capacity,
            asset.byteLength - offset,
            16 * 1024 * 1024,
          );
          this.#write(destination, asset.subarray(offset, offset + length));
          this.#setReg(7, BigInt(length));
          return false;
        }
        case "host_audio_submit": {
          if (!this.audioEnabled) {
            this.#setReg(7, 3n);
            return false;
          }
          const sampleCount = this.#u32(this.#reg(8));
          if (
            !sampleCount ||
            sampleCount % 2 ||
            sampleCount > MAX_AUDIO_SAMPLES
          ) {
            this.#setReg(7, 1n);
            return false;
          }
          const samples = this.#read(this.#u32(this.#reg(7)), sampleCount * 2);
          this.emit(
            { type: "audio", sampleRate: 48000, channels: 2, samples },
            [samples.buffer],
          );
          this.#setReg(7, 0n);
          return false;
        }
        case "pvm_time_ms": {
          const timeMs = this.timeMs ?? performance.now() - this.clockStartedAt;
          this.#setU64Result(BigInt(Math.max(0, Math.trunc(timeMs))));
          return false;
        }
        case "host_log": {
          const length = Math.min(this.#u32(this.#reg(8)), MAX_LOG_BYTES);
          const message = decoder.decode(
            this.#read(this.#u32(this.#reg(7)), length),
          );
          this.emit({ type: "log", message });
          return false;
        }
        case "pvm_yield":
          return true;
        case "pvm_init_audio": {
          const channels = this.#u32(this.#reg(7));
          const bitsPerSample = this.#u32(this.#reg(8));
          const sampleRate = this.#u32(this.#reg(9));
          if (
            bitsPerSample !== 16 ||
            channels < 1 ||
            channels > 2 ||
            sampleRate < 8000 ||
            sampleRate > 96000
          ) {
            this.#setReg(7, 0n);
          } else {
            this.audioChannels = channels;
            this.audioSampleRate = sampleRate;
            this.#setReg(7, 1n);
          }
          return false;
        }
        case "pvm_output_audio": {
          const frames = this.#u32(this.#reg(8));
          const sampleCount = Math.min(frames * this.audioChannels, 1024 * 64);
          if (this.audioChannels && sampleCount) {
            const samples = this.#read(
              this.#u32(this.#reg(7)),
              sampleCount * 2,
            );
            this.emit(
              {
                type: "audio",
                sampleRate: this.audioSampleRate,
                channels: this.audioChannels,
                samples,
              },
              [samples.buffer],
            );
          }
          return false;
        }
        case "pvm_syscall":
          return this.#handleCoreVmSyscall();
        default:
          throw new Error(
            `translated CoreVM guest uses unsupported import ${name}`,
          );
      }
    }

    #handleCoreVmSyscall() {
      const syscall = this.#reg(7);
      const a1 = this.#reg(8);
      const a2 = this.#reg(9);
      const a3 = this.#reg(10);
      if (syscall === SYS_READ) {
        this.#setReg(7, this.#readFile(a1, a2, a3));
      } else if (syscall === SYS_READV || syscall === SYS_WRITEV) {
        if (!a3 || a3 > IOV_MAX) {
          this.#setReg(7, errno(EINVAL));
          return false;
        }
        let total = 0n;
        for (let index = 0n; index < a3; index++) {
          let address;
          let length;
          try {
            address = this.#readU64(this.#u32(a2 + index * 16n));
            length = this.#readU64(this.#u32(a2 + index * 16n + 8n));
          } catch {
            this.#setReg(7, errno(EFAULT));
            return false;
          }
          const result =
            syscall === SYS_READV
              ? this.#readFile(a1, address, length)
              : this.#writeFile(a1, address, length);
          if (BigInt.asIntN(64, result) < 0n) {
            this.#setReg(7, result);
            return false;
          }
          total += length;
        }
        this.#setReg(7, total);
      } else if (syscall === SYS_EXIT) {
        if (a1 === 0n) {
          throw new Error("CoreVM guest exited");
        }
        throw new Error(`CoreVM guest exited with status ${a1}`);
      } else if (syscall === SYS_OPENAT) {
        if (a1 !== AT_FDCWD) {
          this.#setReg(7, errno(ENOSYS));
          return false;
        }
        const pathBytes = this.#readCString(this.#u32(a2));
        if (!pathBytes) {
          this.#setReg(7, errno(EFAULT));
          return false;
        }
        const path = normalizedPath(decoder.decode(pathBytes));
        const flags = a3;
        const asset = this.assets.get(path);
        if (!asset) {
          this.#setReg(7, errno(ENOENT));
        } else if (flags & 3n) {
          this.#setReg(7, errno(EACCES));
        } else {
          const fd = this.nextFd++;
          this.fds.set(fd, { bytes: asset, position: 0n });
          this.#setReg(7, BigInt(fd));
        }
      } else if (syscall === SYS_LSEEK) {
        this.#setReg(7, this.#seekFile(a1, a2, a3));
      } else if (syscall === SYS_CLOSE) {
        const fd = this.#fdNumber(a1);
        if (fd === null || !this.fds.delete(fd)) {
          this.#setReg(7, errno(EBADF));
        } else {
          this.#setReg(7, 0n);
        }
      } else {
        this.#setReg(7, errno(ENOSYS));
      }
      return false;
    }

    #fdNumber(value) {
      return value <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(value) : null;
    }

    #readFile(fdValue, address, length) {
      const fd = this.#fdNumber(fdValue);
      const file = fd === null ? null : this.fds.get(fd);
      if (!file) {
        return errno(EBADF);
      }
      if (
        address + length > 0x100000000n ||
        length > BigInt(MAX_HOSTCALL_BYTES)
      ) {
        return errno(EFAULT);
      }
      const end =
        file.position + length < BigInt(file.bytes.byteLength)
          ? file.position + length
          : BigInt(file.bytes.byteLength);
      if (file.position >= end) {
        return 0n;
      }
      const count = Number(end - file.position);
      try {
        this.#write(
          this.#u32(address),
          file.bytes.subarray(
            Number(file.position),
            Number(file.position) + count,
          ),
        );
      } catch {
        return errno(EFAULT);
      }
      file.position += BigInt(count);
      return BigInt(count);
    }

    #writeFile(fdValue, address, length) {
      if (fdValue !== 1n && fdValue !== 2n) {
        return errno(EBADF);
      }
      if (
        address + length > 0x100000000n ||
        length > BigInt(MAX_HOSTCALL_BYTES)
      ) {
        return errno(EFAULT);
      }
      try {
        const byteLength = Number(length);
        this.#chargeBytes(byteLength);
        const bytes = this.#range(this.#u32(address), byteLength);
        const message = decoder.decode(bytes.subarray(0, MAX_LOG_BYTES));
        if (message) {
          this.emit({ type: "log", message });
        }
      } catch {
        return errno(EFAULT);
      }
      return length;
    }

    #seekFile(fdValue, offsetValue, whence) {
      const fd = this.#fdNumber(fdValue);
      const file = fd === null ? null : this.fds.get(fd);
      if (!file) {
        return errno(EBADF);
      }
      const offset = BigInt.asIntN(64, offsetValue);
      const fileLength = BigInt(file.bytes.byteLength);
      if (whence === 0n) {
        file.position = BigInt.asUintN(64, offset);
      } else if (whence === 1n) {
        file.position = BigInt.asUintN(
          64,
          BigInt.asIntN(64, file.position) + offset,
        );
        if (file.position > fileLength) {
          file.position = fileLength;
        }
      } else if (whence === 2n) {
        file.position = BigInt.asUintN(
          64,
          BigInt.asIntN(64, fileLength) + offset,
        );
        if (file.position > fileLength) {
          file.position = fileLength;
        }
      } else {
        return errno(EINVAL);
      }
      return file.position;
    }
  }

  globalThis.decodePolkaVmUiOutput = decodeUiOutput;
  globalThis.TranslatedPolkaVmRuntime = TranslatedPolkaVmRuntime;
})();
