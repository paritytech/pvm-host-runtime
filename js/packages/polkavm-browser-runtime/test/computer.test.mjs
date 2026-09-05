import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

await import("../src/polkavm-computer.js");

const packageRoot = resolve(import.meta.dirname, "..");
const repositoryRoot = resolve(packageRoot, "../../..");
const fixtureRoot = resolve(
  repositoryRoot,
  "rust/crates/polkavm-host-runtime/tests/fixtures",
);

const {
  computerContext,
  ComputerProcess,
  ComputerSupervisor,
  ComputerTranslator,
  WebSocketTcpProvider,
  TTY_MODE_RAW,
} = globalThis.PolkaVmComputer;

const MAX_GAS = 50_000_000;

const translator = await ComputerTranslator.create(
  await readFile(resolve(packageRoot, "dist/polkavm-browser-runtime.wasm")),
);

async function fixture(name) {
  return translator.translate(
    await readFile(resolve(fixtureRoot, `${name}.polkavm`)),
  );
}

const coreContext = await fixture("computer-core-context");
const coreServices = await fixture("computer-core-services");
const roundtrip = await fixture("computer-tty-fs-roundtrip");
const pipeDriver = await fixture("computer-pipe-driver");
const pipeFilter = await fixture("computer-pipe-filter");
const tcpRoundtrip = await fixture("computer-tcp-roundtrip");
const workspaceDriver = await fixture("computer-workspace-driver");
const workspacePane = await fixture("computer-workspace-pane");
const filesystemGuest = await fixture("computer-filesystem");

const text = (bytes) => new TextDecoder().decode(bytes);

function runToExit(target, limit = 10_000) {
  for (let step = 0; step < limit; step++) {
    const status = target.run();
    if (status.kind === "exited") {
      return status.code;
    }
    assert.equal(status.kind, "yielded");
  }
  throw new Error("guest did not exit");
}

test("computer guest reads context and exits with status", () => {
  const context = computerContext(
    ["shell.polkavm", "--login"],
    [
      ["HOME", "/home"],
      ["TERM", "pvm-tty"],
    ],
  );
  const process = new ComputerProcess(coreContext, context, MAX_GAS);
  assert.deepEqual(process.run(), { kind: "exited", code: 23 });
  assert.deepEqual(process.run(), { kind: "exited", code: 23 });
});

test("computer guest reads clocks and secure random", () => {
  const process = new ComputerProcess(
    coreServices,
    computerContext([], []),
    MAX_GAS,
  );
  assert.deepEqual(process.run(), { kind: "exited", code: 31 });
});

test("computer guest roundtrips terminal and filesystem", () => {
  const process = new ComputerProcess(
    roundtrip,
    computerContext([], []),
    MAX_GAS,
  );
  process.setTerminalSize(100, 40);
  process.mountFile("/home/seed.txt", new TextEncoder().encode("seeded"));

  assert.equal(process.run().kind, "yielded");
  assert.equal(text(process.takeTerminalOutput()), "ready:seeded\r\n");
  assert.equal(process.terminalMode(), TTY_MODE_RAW);
  assert.deepEqual(process.takeModifiedFiles(), []);
  assert.deepEqual(process.takeRemovedFiles(), []);

  process.sendTerminalInput(new TextEncoder().encode("hello"));
  assert.equal(process.run().kind, "yielded");
  assert.equal(text(process.takeTerminalOutput()), "HELLO");

  process.sendTerminalInput(new TextEncoder().encode(" pvm"));
  assert.equal(process.run().kind, "yielded");
  assert.equal(text(process.takeTerminalOutput()), " PVM");

  process.sendTerminalInput(new TextEncoder().encode("q"));
  assert.deepEqual(process.run(), { kind: "exited", code: 7 });
  const modified = process.takeModifiedFiles();
  assert.equal(modified.length, 1);
  assert.equal(modified[0][0], "/home/echo.txt");
  assert.equal(text(modified[0][1]), "hello pvm");
  assert.deepEqual(process.takeRemovedFiles(), ["/home/remove.tmp"]);
});

test("foreground programs inherit their pane's terminal dimensions", () => {
  const supervisor = new ComputerSupervisor(
    workspacePane,
    computerContext([], []),
    MAX_GAS,
  );
  supervisor.registerPackage("extra", workspacePane);
  supervisor.setTerminalSize(123, 45);
  supervisor.sendTerminalInput(new TextEncoder().encode("p"));
  assert.equal(supervisor.run().kind, "yielded");
  while (supervisor.takeTerminalOutput() !== null) {}

  supervisor.sendTerminalInput(new TextEncoder().encode("s"));
  assert.equal(supervisor.run().kind, "yielded");
  assert.equal(text(supervisor.takeTerminalOutput()), "123x45");
  supervisor.dispose();
});

test("guest streams bytes through a piped child and reaps it", () => {
  const supervisor = new ComputerSupervisor(
    pipeDriver,
    computerContext([], []),
    MAX_GAS,
  );
  supervisor.registerPackage("upper", pipeFilter);

  // The driver asserts every contract detail internally (unknown package,
  // bad pids, partial writes, EOF, double reap) and exits nonzero with a
  // distinct code on the first violation.
  assert.equal(runToExit(supervisor), 0);
  assert.equal(text(supervisor.takeTerminalOutput()), "HELLO, PIPES");
});

test("spawn without registration fails from the start", () => {
  const supervisor = new ComputerSupervisor(
    pipeDriver,
    computerContext([], []),
    MAX_GAS,
  );
  assert.equal(runToExit(supervisor), 13);
});

test("open spawn suspends for the embedder and resumes", () => {
  const supervisor = new ComputerSupervisor(
    pipeDriver,
    computerContext([], []),
    MAX_GAS,
    null,
    { packageResolution: true },
  );
  const requested = [];
  for (let step = 0; step < 10_000; step++) {
    const status = supervisor.run();
    if (status.kind === "exited") {
      assert.equal(status.code, 0);
      assert.equal(text(supervisor.takeTerminalOutput()), "HELLO, PIPES");
      // The driver probes one unknown package (rejected -> NOT_FOUND, the
      // same observable as the default path) and pipes through "upper"
      // (provided by the embedder without prior registration).
      assert.ok(requested.includes("upper"));
      assert.ok(requested.some((name) => name !== "upper"));
      return;
    }
    if (status.kind === "package") {
      // Suspension is idempotent until the embedder acts.
      assert.deepEqual(supervisor.run(), status);
      requested.push(status.package);
      if (status.package === "upper") {
        supervisor.providePackage(pipeFilter);
      } else {
        supervisor.rejectPackage();
      }
      continue;
    }
    assert.equal(status.kind, "yielded");
  }
  throw new Error("guest did not exit");
});

test("supervisor terminates the root as interrupted", () => {
  const supervisor = new ComputerSupervisor(
    roundtrip,
    computerContext([], []),
    MAX_GAS,
  );
  supervisor.setTerminalSize(100, 40);
  supervisor.mountFile("/home/seed.txt", new TextEncoder().encode("seeded"));

  assert.equal(supervisor.run().kind, "yielded");
  supervisor.sendTerminalInput(new TextEncoder().encode("hello"));
  assert.equal(supervisor.run().kind, "yielded");

  assert.deepEqual(supervisor.terminateForeground(), {
    kind: "exited",
    code: 130,
  });
  assert.deepEqual(supervisor.takeModifiedFiles(), []);
  // Termination is recorded: the computer stays exited on later runs.
  assert.deepEqual(supervisor.run(), { kind: "exited", code: 130 });
});

test("WebSocket TCP provider negotiates then carries bounded binary streams", () => {
  const instances = [];
  class FakeWebSocket {
    constructor(url) {
      this.url = url;
      this.bufferedAmount = 0;
      this.sent = [];
      instances.push(this);
    }

    send(value) {
      this.sent.push(value);
    }

    close() {
      this.onclose?.();
    }
  }

  let activity = 0;
  const provider = new WebSocketTcpProvider(
    "wss://relay.invalid/tcp",
    () => activity++,
    FakeWebSocket,
  );
  const stream = provider.connect("example.org:443");
  const socket = instances[0];
  socket.onopen();
  assert.deepEqual(JSON.parse(socket.sent[0]), {
    version: 1,
    address: "example.org:443",
  });
  assert.equal(stream.write(new Uint8Array([1])), null);

  socket.onmessage({ data: JSON.stringify({ type: "connected" }) });
  assert.equal(stream.write(new Uint8Array([1, 2])), 2);
  assert.ok(socket.sent[1] instanceof ArrayBuffer);
  socket.onmessage({ data: new Uint8Array([3, 4, 5]) });
  assert.deepEqual(stream.read(2), new Uint8Array([3, 4]));
  assert.deepEqual(stream.read(2), new Uint8Array([5]));
  assert.equal(stream.read(2), null);
  assert.ok(activity >= 2);

  stream.close();
  assert.deepEqual(stream.read(2), new Uint8Array());
});

test("TCP relay buffering is bounded and a closed stream cannot reopen", () => {
  let socket;
  class FakeWebSocket {
    constructor() {
      socket = this;
      this.bufferedAmount = 0;
    }

    send() {}

    close() {
      this.onclose?.();
    }
  }
  const provider = new WebSocketTcpProvider(
    "wss://relay.invalid/tcp",
    null,
    FakeWebSocket,
  );
  const stream = provider.connect("example.org:443");
  const connected = { data: JSON.stringify({ type: "connected" }) };
  socket.onmessage(connected);
  socket.bufferedAmount = 1024 * 1024;
  assert.equal(stream.write(new Uint8Array([1])), null);
  socket.bufferedAmount--;
  assert.equal(stream.write(new Uint8Array([1])), 1);

  socket.onmessage({ data: new Uint8Array(1024 * 1024) });
  assert.equal(stream.read(1).byteLength, 1);
  socket.onmessage({ data: new Uint8Array([42]) });
  assert.equal(stream.read(1024 * 1024).at(-1), 42);
  socket.onmessage({ data: new Uint8Array(1024 * 1024 + 1) });
  assert.throws(() => stream.read(1));
  socket.onmessage(connected);
  assert.throws(() => stream.write(new Uint8Array([1])));

  const chunks = provider.connect("example.org:443");
  socket.onmessage(connected);
  for (let index = 0; index < 1025; index++) {
    socket.onmessage({ data: new Uint8Array([1]) });
  }
  assert.throws(() => chunks.read(1));

  const closed = provider.connect("example.org:443");
  closed.close();
  socket.onmessage(connected);
  socket.onmessage({ data: new Uint8Array([1]) });
  assert.deepEqual(closed.read(1), new Uint8Array());
  assert.throws(() => closed.write(new Uint8Array([1])));
});

test("process exit, fault and cancellation release granted network streams", () => {
  let closed = 0;
  const provider = {
    connect() {
      return {
        read: () => null,
        write: () => null,
        close() {
          closed++;
          if (closed === 1) throw new Error("provider teardown failed");
        },
      };
    },
  };
  const process = new ComputerProcess(
    coreServices,
    computerContext([], []),
    MAX_GAS,
    null,
    provider,
  );
  process.setNetworkEnabled(true);
  process.devices.netTcpConnect("example.org:443");
  process.devices.netTcpConnect("example.org:443");
  assert.deepEqual(process.run(), { kind: "exited", code: 31 });
  assert.equal(closed, 2);
  process.dispose();
  assert.equal(closed, 2);

  const fault = new ComputerProcess(
    coreServices,
    computerContext([], []),
    0,
    null,
    provider,
  );
  fault.setNetworkEnabled(true);
  fault.devices.netTcpConnect("example.org:443");
  assert.throws(() => fault.run());
  assert.equal(closed, 3);

  const supervisor = new ComputerSupervisor(
    coreServices,
    computerContext([], []),
    MAX_GAS,
    null,
    { networkProvider: provider },
  );
  supervisor.setNetworkEnabled(true);
  supervisor.stack[0].devices.netTcpConnect("example.org:443");
  assert.deepEqual(supervisor.terminateForeground(), { kind: "exited", code: 130 });
  assert.equal(closed, 4);
  assert.deepEqual(supervisor.run(), { kind: "exited", code: 130 });
});

test("network capability roundtrips through a Host byte-stream provider", () => {
  let closed = false;
  const provider = {
    connect(address) {
      assert.equal(address, "fixture.invalid:443");
      const incoming = [];
      return {
        read(capacity) {
          if (incoming.length === 0) return null;
          return Uint8Array.from(incoming.splice(0, capacity));
        },
        write(bytes) {
          for (const byte of bytes) {
            incoming.push(
              byte >= 0x61 && byte <= 0x7a ? byte - (0x61 - 0x41) : byte,
            );
          }
          return bytes.byteLength;
        },
        close() {
          closed = true;
        },
      };
    },
  };
  const process = new ComputerProcess(
    tcpRoundtrip,
    computerContext([], [["NET_TARGET", "fixture.invalid:443"]]),
    MAX_GAS,
    null,
    provider,
  );
  process.setNetworkEnabled(true);
  assert.equal(runToExit(process), 0);
  assert.equal(closed, true);
});

test("network capability reports denied on the web host", () => {
  const process = new ComputerProcess(
    tcpRoundtrip,
    computerContext([], [["NET_TARGET", "127.0.0.1:1"]]),
    MAX_GAS,
  );
  // The tcp fixture maps a DENIED connect to its distinct exit code 21.
  assert.equal(runToExit(process), 21);
});

/**
 * Drives the workspace driver to completion, playing the Host: at the
 * driver's `mount:ready` checkpoint it mounts `/home/seed.txt` (proving
 * live parent->child mount propagation), and it settles open package
 * resolutions through `resolve` (returning undefined rejects as -4).
 */
function runDriver(supervisor, resolve = () => undefined) {
  let output = "";
  let mounted = false;
  const resolutions = [];
  for (let step = 0; step < 10_000; step++) {
    const status = supervisor.run();
    const bytes = supervisor.takeTerminalOutput();
    if (bytes) {
      output += text(bytes);
    }
    if (status.kind === "exited") {
      return { code: status.code, output, resolutions };
    }
    if (status.kind === "package") {
      resolutions.push(status.package);
      const module = resolve(status.package);
      if (module === undefined) {
        supervisor.rejectPackage(-4);
      } else {
        supervisor.providePackage(module);
      }
      continue;
    }
    assert.equal(status.kind, "yielded");
    if (!mounted && output.includes("mount:ready")) {
      mounted = true;
      supervisor.mountFile("/home/seed.txt", new TextEncoder().encode("seed"));
      supervisor.sendTerminalInput(new TextEncoder().encode("g"));
    }
  }
  throw new Error("driver did not exit");
}

test("workspace guest supervises an independent child", () => {
  const supervisor = new ComputerSupervisor(
    workspaceDriver,
    computerContext([], []),
    MAX_GAS,
  );
  supervisor.registerPackage("pane", workspacePane);
  supervisor.registerPackage("extra", coreServices);
  supervisor.setWorkspaceEnabled(true);

  // The driver asserts every contract detail internally (bad handles,
  // unknown package, invalid geometry, banner, byte roundtrip, resize
  // observability, nested denial, persistence, live seed mounts, exit
  // reporting, EOF after drain, close-once) and exits nonzero with a
  // distinct code on the first violation.
  const { code, output, resolutions } = runDriver(supervisor);
  assert.equal(code, 0, `driver output: ${output}`);
  assert.ok(output.endsWith("workspace:ok"), `driver output: ${output}`);
  assert.deepEqual(resolutions, []);

  // The pane's `/home` write surfaced through the parent supervisor.
  const modified = supervisor.takeModifiedFiles();
  assert.ok(
    modified.some(
      ([path, bytes]) =>
        path === "/home/pane.txt" && text(bytes) === "from-pane",
    ),
    `pane write should merge into the parent /home: ${modified.map(([path]) => path)}`,
  );
});

test("workspace operations are denied without the grant", () => {
  const supervisor = new ComputerSupervisor(
    workspaceDriver,
    computerContext([], []),
    MAX_GAS,
  );
  supervisor.registerPackage("pane", workspacePane);
  supervisor.registerPackage("extra", coreServices);

  // Without setWorkspaceEnabled the driver's first probe observes DENIED
  // and exits with its distinct gating code.
  assert.equal(runDriver(supervisor).code, 41);
});

test("open resolution supplies packages anywhere in the tree", () => {
  // Nothing is pre-registered: the workspace spawn of `pane` and the
  // pane's own foreground run of `extra` both suspend for the embedder,
  // and the driver's unknown-package probe resolves through rejection.
  const supervisor = new ComputerSupervisor(
    workspaceDriver,
    computerContext([], []),
    MAX_GAS,
    null,
    { packageResolution: true },
  );
  supervisor.setWorkspaceEnabled(true);

  const { code, output, resolutions } = runDriver(supervisor, (name) => {
    if (name === "pane") return workspacePane;
    if (name === "extra") return coreServices;
    return undefined;
  });
  assert.equal(code, 0, `driver output: ${output}`);
  assert.ok(output.endsWith("workspace:ok"), `driver output: ${output}`);
  assert.deepEqual(resolutions, ["no-such-package", "pane", "extra"]);
});

test("network revocation reaches a running workspace pane", () => {
  let closed = 0;
  const provider = {
    connect: () => ({
      read: () => null,
      write: (bytes) => bytes.byteLength,
      close: () => closed++,
    }),
  };
  const supervisor = new ComputerSupervisor(
    workspaceDriver,
    computerContext([], []),
    MAX_GAS,
    null,
    { packageResolution: true, networkProvider: provider },
  );
  supervisor.setWorkspaceEnabled(true);
  supervisor.setNetworkEnabled(true);
  const result = runDriver(supervisor, (name) => {
    if (name === "pane") return workspacePane;
    if (name !== "extra") return undefined;
    const pane = supervisor.workspaceChildren.find((child) => child.exit === null);
    const devices = pane.supervisor.stack[0].devices;
    const socket = devices.netTcpConnect("example.org:443");
    assert.ok(socket >= 0x1000);
    supervisor.setNetworkEnabled(false);
    assert.equal(closed, 1);
    assert.equal(devices.netWrite(socket, new Uint8Array([1])), -2);
    assert.equal(devices.netTcpConnect("example.org:443"), -5);
    return coreServices;
  });
  assert.equal(result.code, 0);
  assert.equal(closed, 1);
});

test("cancelling a nested package requester cannot resume its abandoned spawn", () => {
  const supervisor = new ComputerSupervisor(
    workspacePane,
    computerContext([], []),
    MAX_GAS,
    null,
    { packageResolution: true },
  );
  supervisor.sendTerminalInput(new TextEncoder().encode("p"));
  assert.deepEqual(supervisor.run(), { kind: "package", package: "extra" });
  supervisor.providePackage(pipeDriver);
  assert.equal(supervisor.run().kind, "package");
  assert.deepEqual(supervisor.terminateForeground(), { kind: "yielded" });
  assert.equal(supervisor.pendingPackage(), null);
  assert.throws(() => supervisor.providePackage(pipeFilter));
  assert.equal(supervisor.run().kind, "yielded");
  let output = "";
  for (let bytes = supervisor.takeTerminalOutput(); bytes !== null; bytes = supervisor.takeTerminalOutput()) {
    output += text(bytes);
  }
  assert.ok(output.endsWith("p:130"), output);
  supervisor.dispose();
});

test("workspace revocation cancels direct and routed pending resolutions", () => {
  const direct = new ComputerSupervisor(
    workspaceDriver,
    computerContext([], []),
    MAX_GAS,
    null,
    { packageResolution: true },
  );
  direct.setWorkspaceEnabled(true);
  assert.equal(direct.run().kind, "package");
  direct.setWorkspaceEnabled(false);
  assert.equal(direct.pendingPackage(), null);
  assert.throws(() => direct.providePackage(workspacePane));
  assert.equal(runDriver(direct).code, 42);

  const routed = new ComputerSupervisor(
    workspaceDriver,
    computerContext([], []),
    MAX_GAS,
    null,
    { packageResolution: true },
  );
  routed.registerPackage("pane", workspacePane);
  routed.setWorkspaceEnabled(true);
  assert.equal(routed.run().kind, "package");
  routed.rejectPackage();
  assert.deepEqual(routed.run(), { kind: "package", package: "extra" });
  routed.setWorkspaceEnabled(false);
  assert.equal(routed.pendingPackage(), null);
  assert.notEqual(routed.run().kind, "package");
  routed.dispose();
});

test("workspace cancellation releases shared locks without losing pending writes", () => {
  const supervisor = new ComputerSupervisor(
    workspaceDriver,
    computerContext([], []),
    MAX_GAS,
  );
  supervisor.registerPackage("pane", workspacePane);
  supervisor.registerPackage("extra", coreServices);
  supervisor.setWorkspaceEnabled(true);
  let output = "";
  for (let step = 0; step < 10_000 && !output.includes("mount:ready"); step++) {
    assert.equal(supervisor.run().kind, "yielded");
    const bytes = supervisor.takeTerminalOutput();
    if (bytes) output += text(bytes);
  }
  assert.ok(output.includes("mount:ready"));
  const parent = supervisor.stack[0].devices;
  const child = supervisor.workspaceChildren.find(
    (entry) => entry.exit === null,
  ).supervisor.stack[0].devices;
  const {
    FS_OPEN_WRITE: WRITE,
    FS_OPEN_CREATE: CREATE,
    FS_OPEN_EXCLUSIVE: EXCLUSIVE,
    STATUS_EXISTS,
    STATUS_DENIED,
  } = globalThis.PolkaVmComputer;
  const handle = child.fsOpen("/home/cancel.lock", WRITE | CREATE | EXCLUSIVE);
  assert.ok(handle >= 16);
  assert.equal(child.fsWrite(handle, new TextEncoder().encode("pending")), 7);
  assert.equal(
    parent.fsOpen("/home/cancel.lock", WRITE | CREATE | EXCLUSIVE),
    STATUS_EXISTS,
  );
  assert.equal(
    parent.fsRename("/home/cancel.lock", "/home/committed"),
    STATUS_DENIED,
  );
  supervisor.setWorkspaceEnabled(false);
  assert.equal(parent.fsRename("/home/cancel.lock", "/home/committed"), 0);
  assert.equal(
    text(
      supervisor
        .takeModifiedFiles()
        .find(([path]) => path === "/home/committed")[1],
    ),
    "pending",
  );
  assert.ok(supervisor.takeRemovedFiles().includes("/home/cancel.lock"));
  assert.ok(
    supervisor
      .takeFilesystemMetadata()
      .entries.some((entry) => entry.path === "/home/committed"),
  );
  supervisor.dispose();
});

test("process exit, fault and supervisor cancellation release all open paths", () => {
  const {
    ComputerDevices,
    FS_OPEN_WRITE: WRITE,
    FS_OPEN_CREATE: CREATE,
  } = globalThis.PolkaVmComputer;
  for (const gas of [MAX_GAS, 1]) {
    const process = new ComputerProcess(
      coreServices,
      computerContext([], []),
      gas,
    );
    const observer = new ComputerDevices(null, process.devices.filesystem);
    assert.ok(process.devices.fsOpen("/home/open", WRITE | CREATE) >= 16);
    if (gas === 1) assert.throws(() => process.run(), /gas/);
    else assert.equal(process.run().kind, "exited");
    assert.equal(observer.fsRemove("/home/open"), 0);
    observer.dispose();
  }
  const supervisor = new ComputerSupervisor(
    coreContext,
    computerContext([], []),
    MAX_GAS,
  );
  const observer = new ComputerDevices(null, supervisor.filesystem);
  assert.ok(
    supervisor.stack[0].devices.fsOpen("/home/open", WRITE | CREATE) >= 16,
  );
  assert.deepEqual(supervisor.terminateForeground(), {
    kind: "exited",
    code: 130,
  });
  assert.equal(observer.fsRemove("/home/open"), 0);
  observer.dispose();
});

function filesystemCheckpoint(supervisor, expected) {
  let output = "";
  for (let step = 0; step < 100; step++) {
    const status = supervisor.run();
    const bytes = supervisor.takeTerminalOutput();
    if (bytes) output += text(bytes);
    if (output.includes(expected)) return;
    assert.equal(status.kind, "yielded", output);
  }
  throw new Error(`missing checkpoint ${expected}: ${output}`);
}

test("real guest filesystem records and cross-process atomic publication", () => {
  const supervisor = new ComputerSupervisor(
    filesystemGuest,
    computerContext([], []),
    MAX_GAS,
  );
  supervisor.registerPackage("fs-child", filesystemGuest);
  filesystemCheckpoint(supervisor, "fs:ready");
  supervisor.sendTerminalInput(new TextEncoder().encode("p"));
  filesystemCheckpoint(supervisor, "fs:published");
  assert.deepEqual(supervisor.run(), { kind: "exited", code: 0 });
  assert.deepEqual(
    supervisor.takeModifiedFiles().map(([path, bytes]) => [path, text(bytes)]),
    [["/home/repo/record", "candidate"]],
  );
});

test("real guest cancellation preserves publication target across metadata restore", () => {
  const supervisor = new ComputerSupervisor(
    filesystemGuest,
    computerContext([], []),
    MAX_GAS,
  );
  supervisor.registerPackage("fs-child", filesystemGuest);
  filesystemCheckpoint(supervisor, "fs:ready");
  supervisor.sendTerminalInput(new TextEncoder().encode("c"));
  filesystemCheckpoint(supervisor, "fs:cancel");
  assert.deepEqual(supervisor.terminateForeground(), {
    kind: "exited",
    code: 130,
  });
  const metadata = supervisor.exportFilesystemMetadata();
  const files = supervisor.takeModifiedFiles();
  const restored = new ComputerSupervisor(
    filesystemGuest,
    computerContext(["check"], []),
    MAX_GAS,
  );
  for (const [path, bytes] of files) restored.mountFile(path, bytes);
  restored.importFilesystemMetadata(metadata);
  assert.deepEqual(restored.exportFilesystemMetadata(), metadata);
  filesystemCheckpoint(restored, "fs:restored");
  assert.deepEqual(restored.run(), { kind: "exited", code: 0 });
});
