import assert from "node:assert/strict";
import test from "node:test";

await import("../src/polkavm-computer.js");
const {
  ComputerDevices,
  FS_OPEN_READ: READ,
  FS_OPEN_WRITE: WRITE,
  FS_OPEN_CREATE: CREATE,
  FS_OPEN_TRUNCATE: TRUNCATE,
  FS_OPEN_EXCLUSIVE: EXCLUSIVE,
  FS_OPEN_APPEND: APPEND,
  STATUS_INVALID: INVALID,
  STATUS_NOT_FOUND: NOT_FOUND,
  STATUS_DENIED: DENIED,
  STATUS_LIMIT: LIMIT,
  STATUS_EXISTS: EXISTS,
  STATUS_NOT_DIRECTORY: NOT_DIRECTORY,
  STATUS_IS_DIRECTORY: IS_DIRECTORY,
  STATUS_NOT_EMPTY: NOT_EMPTY,
} = globalThis.PolkaVmComputer;
const encode = (text) => new TextEncoder().encode(text);
const decode = (bytes) => new TextDecoder().decode(bytes);

function contents(devices, path) {
  const handle = devices.fsOpen(path, READ);
  assert.ok(handle >= 16);
  const result = devices.fsRead(handle, 1024 * 1024);
  devices.fsClose(handle);
  return decode(result.bytes);
}

function metadata(devices, path) {
  const result = devices.fsMetadata(path);
  assert.equal(result.status, 0);
  assert.equal(result.record.byteLength, 24);
  const view = new DataView(result.record.buffer);
  return {
    kind: view.getUint32(0, true),
    size: view.getUint32(4, true),
    mtime: view.getBigUint64(8, true),
    inode: view.getBigUint64(16, true),
  };
}

function directory(devices, path) {
  const result = devices.fsListDirectory(path);
  assert.equal(result.status, result.record.byteLength);
  const view = new DataView(result.record.buffer);
  const children = [];
  let offset = 4;
  for (let index = 0; index < view.getUint32(0, true); index++) {
    const length = view.getUint32(offset, true);
    children.push([
      decode(result.record.subarray(offset + 4, offset + 4 + length)),
      view.getUint32(offset + 4 + length, true),
    ]);
    offset += 8 + length;
  }
  assert.equal(offset, result.record.byteLength);
  return children;
}

test("shared exclusive creation and append serialize across process-local handles", () => {
  const parent = new ComputerDevices();
  const child = new ComputerDevices(null, parent.filesystem);
  const lock = parent.fsOpen("/home/index.lock", WRITE | CREATE | EXCLUSIVE);
  assert.ok(lock >= 16);
  assert.equal(
    child.fsOpen("/home/index.lock", WRITE | CREATE | EXCLUSIVE | TRUNCATE),
    EXISTS,
  );
  assert.equal(parent.fsWrite(lock, encode("staged")), 6);
  assert.equal(child.fsRemove("/home/index.lock"), DENIED);
  assert.throws(() => child.mountFile("/home/index.lock", encode("overwrite")));
  assert.equal(contents(child, "/home/index.lock"), "staged");
  parent.fsClose(lock);
  assert.equal(child.fsRename("/home/index.lock", "/home/index"), 0);
  assert.equal(parent.fsStat("/home/index.lock"), null);
  assert.equal(contents(parent, "/home/index"), "staged");

  const first = parent.fsOpen("/home/index", WRITE | APPEND);
  const second = child.fsOpen("/home/index", WRITE | APPEND);
  assert.equal(parent.fsSeek(first, 0, 0), 0);
  assert.equal(child.fsWrite(second, encode("-child")), 6);
  assert.equal(parent.fsWrite(first, encode("-parent")), 7);
  assert.equal(contents(child, "/home/index"), "staged-child-parent");
  parent.dispose();
  assert.equal(child.fsRemove("/home/index"), DENIED);
  child.fsClose(second);
  assert.equal(child.fsRemove("/home/index"), 0);
  assert.deepEqual(parent.takeRemovedFiles().sort(), [
    "/home/index",
    "/home/index.lock",
  ]);
  assert.deepEqual(parent.takeModifiedFiles(), []);
  child.dispose();
});

test("invalid flags and directory paths cannot create or truncate files", () => {
  const devices = new ComputerDevices();
  devices.mountFile("/home/existing", encode("keep"));
  const before = devices.exportFilesystemMetadata();
  for (const flags of [
    READ | CREATE,
    READ | TRUNCATE,
    WRITE | EXCLUSIVE,
    READ | APPEND,
    WRITE | TRUNCATE | 64,
  ]) {
    assert.equal(devices.fsOpen("/home/existing", flags), INVALID);
    assert.equal(devices.fsOpen("/home/new", flags), INVALID);
  }
  for (const path of [
    "/home//bad",
    "/home/./bad",
    "/home/../bad",
    "/home/bad/",
    "/home/\0bad",
    `/home/${"é".repeat(98)}`,
  ]) {
    assert.equal(devices.fsMkdir(path), INVALID);
    assert.equal(devices.fsOpen(path, WRITE | CREATE), INVALID);
  }
  assert.equal(devices.fsOpen("/home", READ), IS_DIRECTORY);
  assert.equal(devices.fsOpen("/home/missing/file", WRITE | CREATE), NOT_FOUND);
  assert.equal(
    devices.fsOpen("/home/existing/file", WRITE | CREATE),
    NOT_DIRECTORY,
  );
  assert.equal(contents(devices, "/home/existing"), "keep");
  assert.deepEqual(devices.exportFilesystemMetadata(), before);
  assert.equal(devices.takeFilesystemMetadata(), null);
});

test("non-directory ancestors report NOT_DIRECTORY without namespace mutation", () => {
  const devices = new ComputerDevices();
  devices.mountFile("/home/file", encode("unchanged"));
  const before = devices.exportFilesystemMetadata();
  const descendant = "/home/file/missing/child";
  assert.equal(devices.fsOpen(descendant, READ), NOT_DIRECTORY);
  assert.equal(devices.fsOpen(descendant, WRITE | CREATE), NOT_DIRECTORY);
  assert.equal(devices.fsMkdir(descendant), NOT_DIRECTORY);
  assert.equal(devices.fsRemove(descendant), NOT_DIRECTORY);
  assert.equal(devices.fsRmdir(descendant), NOT_DIRECTORY);
  assert.equal(devices.fsRename(descendant, "/home/new"), NOT_DIRECTORY);
  assert.equal(devices.fsRename("/home/file", descendant), NOT_DIRECTORY);
  assert.equal(devices.fsMetadata(descendant).status, NOT_DIRECTORY);
  assert.equal(devices.fsListDirectory(descendant).status, NOT_DIRECTORY);
  assert.equal(contents(devices, "/home/file"), "unchanged");
  assert.deepEqual(devices.exportFilesystemMetadata(), before);
  assert.deepEqual(devices.takeModifiedFiles(), []);
  assert.deepEqual(devices.takeRemovedFiles(), []);
});

test("directory discovery and subtree rename preserve identity and preflight every failure", () => {
  const devices = new ComputerDevices();
  devices.mountFile("/home/tree/sub/file", encode("source"));
  devices.mountFile("/home/destination/keep", encode("destination"));
  assert.equal(devices.fsMkdir("/home/tree/empty"), 0);
  assert.deepEqual(directory(devices, "/home/tree"), [
    ["empty", 2],
    ["sub", 2],
  ]);
  assert.deepEqual(directory(devices, "/home/tree/sub"), [["file", 1]]);
  assert.equal(devices.fsRemove("/home/tree"), IS_DIRECTORY);
  assert.equal(devices.fsRmdir("/home/tree/sub/file"), NOT_DIRECTORY);
  assert.equal(devices.fsRmdir("/home/tree"), NOT_EMPTY);
  const before = devices.exportFilesystemMetadata();
  const fileBefore = metadata(devices, "/home/tree/sub/file");
  assert.equal(devices.fsRename("/home/tree", "/home/destination"), NOT_EMPTY);
  assert.equal(devices.fsRename("/home/tree", "/home/tree/sub/cycle"), INVALID);
  assert.equal(
    devices.fsRename("/home/tree", `/home/${"x".repeat(190)}`),
    INVALID,
  );
  assert.deepEqual(devices.exportFilesystemMetadata(), before);
  assert.equal(contents(devices, "/home/destination/keep"), "destination");

  const child = new ComputerDevices(null, devices.filesystem);
  const handle = child.fsOpen("/home/tree/sub/file", READ);
  assert.ok(handle >= 16);
  assert.equal(devices.fsRename("/home/tree", "/home/moved"), DENIED);
  assert.equal(devices.fsRename("/home/tree", "/home/tree"), 0);
  child.dispose();
  assert.equal(devices.fsRename("/home/tree", "/home/moved"), 0);
  assert.deepEqual(metadata(devices, "/home/moved/sub/file"), fileBefore);
  assert.equal(devices.fsMetadata("/home/tree").status, NOT_FOUND);
  assert.deepEqual(directory(devices, "/home/moved"), [
    ["empty", 2],
    ["sub", 2],
  ]);
  assert.equal(devices.fsRmdir("/home/moved/empty"), 0);
  assert.deepEqual(devices.takeRemovedFiles(), ["/home/tree/sub/file"]);
  assert.equal(decode(devices.takeModifiedFiles()[0][1]), "source");
});

test("same-size writes advance persisted mtime across rollback without changing inode", () => {
  const devices = new ComputerDevices();
  devices.mountFile("/home/file", encode("before"));
  const snapshot = devices.exportFilesystemMetadata();
  snapshot.clockNs = "9000000000000000000";
  snapshot.entries.find((entry) => entry.path === "/home/file").mtimeNs =
    snapshot.clockNs;
  devices.importFilesystemMetadata(snapshot);
  assert.equal(devices.takeFilesystemMetadata(), null);
  const before = metadata(devices, "/home/file");
  const handle = devices.fsOpen("/home/file", READ | WRITE);
  assert.equal(devices.fsWrite(handle, encode("after!")), 6);
  const after = metadata(devices, "/home/file");
  assert.equal(after.size, before.size);
  assert.equal(after.inode, before.inode);
  assert.equal(after.mtime, before.mtime + 1n);
  assert.deepEqual(
    devices.fsFstat(handle).record,
    devices.fsMetadata("/home/file").record,
  );
  assert.equal(devices.fsTruncate(handle, 6), 0);
  assert.equal(metadata(devices, "/home/file").mtime, after.mtime + 1n);
  devices.fsClose(handle);
  const changes = devices.takeModifiedFiles();
  const persisted = devices.takeFilesystemMetadata();
  assert.equal(devices.takeFilesystemMetadata(), null);
  const restored = new ComputerDevices();
  for (const [path, bytes] of changes) restored.mountFile(path, bytes);
  restored.importFilesystemMetadata(persisted);
  assert.deepEqual(restored.exportFilesystemMetadata(), persisted);
  assert.deepEqual(
    metadata(restored, "/home/file"),
    metadata(devices, "/home/file"),
  );
  changes[0][1].fill(0);
  persisted.entries[0].inode = "999";
  assert.equal(contents(devices, "/home/file"), "after!");
  assert.equal(contents(restored, "/home/file"), "after!");
  assert.equal(metadata(restored, "/home").inode, 1n);
});

test("metadata restore is atomic, exact, and prohibited with live child handles", () => {
  const devices = new ComputerDevices();
  devices.mountFile("/home/dir/file", encode("bytes"));
  const before = devices.exportFilesystemMetadata();
  for (const mutate of [
    (data) => data.entries.pop(),
    (data) => {
      data.entries[1].inode = data.entries[0].inode;
    },
    (data) => {
      data.entries[1].mtimeNs = "18446744073709551616";
    },
    (data) => {
      data.nextInode = "01";
    },
    (data) => {
      data.entries[1].kind = 1;
    },
    (data) => {
      data.entries[1].path = "/home//dir";
    },
  ]) {
    const invalid = structuredClone(before);
    mutate(invalid);
    assert.throws(() => devices.importFilesystemMetadata(invalid));
    assert.deepEqual(devices.exportFilesystemMetadata(), before);
    assert.equal(contents(devices, "/home/dir/file"), "bytes");
  }
  const child = new ComputerDevices(null, devices.filesystem);
  assert.throws(() => devices.importFilesystemMetadata(before));
  child.dispose();
  const handle = devices.fsOpen("/home/dir/file", READ);
  assert.throws(() => devices.importFilesystemMetadata(before));
  devices.fsClose(handle);
  devices.importFilesystemMetadata(before);
  assert.deepEqual(devices.exportFilesystemMetadata(), before);
});

test("file and directory quota failures leave namespace and destination intact", () => {
  const devices = new ComputerDevices();
  for (let index = 0; index < 64; index++)
    devices.mountFile(`/home/f${index}`, encode(String(index)));
  let before = devices.exportFilesystemMetadata();
  assert.equal(devices.fsOpen("/home/extra", WRITE | CREATE), LIMIT);
  assert.throws(() =>
    devices.mountFile("/home/not-created/extra", encode("overflow")),
  );
  assert.deepEqual(devices.exportFilesystemMetadata(), before);
  assert.equal(devices.fsRename("/home/f0", "/home/f1"), 0);
  assert.equal(contents(devices, "/home/f1"), "0");
  for (let index = 0; index < 256; index++)
    assert.equal(devices.fsMkdir(`/home/d${index}`), 0);
  before = devices.exportFilesystemMetadata();
  assert.equal(devices.fsMkdir("/home/extra"), LIMIT);
  assert.throws(() =>
    devices.mountFile("/home/missing/file", encode("overflow")),
  );
  assert.deepEqual(devices.exportFilesystemMetadata(), before);
  assert.equal(devices.fsRename("/home/d0", "/home/d1"), 0);
  assert.equal(devices.fsMkdir("/home/extra"), 0);
});

test("open destination and handle or byte limits never destroy existing data", () => {
  const devices = new ComputerDevices();
  devices.mountFile("/home/source", encode("source"));
  devices.mountFile("/home/destination", encode("destination"));
  const child = new ComputerDevices(null, devices.filesystem);
  const destination = child.fsOpen("/home/destination", READ);
  assert.ok(destination >= 16);
  assert.equal(devices.fsRename("/home/source", "/home/destination"), DENIED);
  assert.equal(contents(devices, "/home/destination"), "destination");
  child.dispose();
  const handles = Array.from({ length: 16 }, () =>
    devices.fsOpen("/home/source", WRITE),
  );
  assert.ok(handles.every((handle) => handle >= 16));
  assert.equal(devices.fsOpen("/home/destination", WRITE | TRUNCATE), LIMIT);
  assert.equal(devices.fsOpen("/home/new", WRITE | CREATE), LIMIT);
  for (const handle of handles) devices.fsClose(handle);
  assert.equal(contents(devices, "/home/destination"), "destination");
  assert.equal(devices.fsMetadata("/home/new").status, NOT_FOUND);
  const handle = devices.fsOpen("/home/source", WRITE);
  assert.equal(devices.fsSeek(handle, 1024 * 1024, 0), 1024 * 1024);
  const before = metadata(devices, "/home/source");
  assert.equal(devices.fsWrite(handle, encode("x")), LIMIT);
  assert.deepEqual(metadata(devices, "/home/source"), before);
  assert.equal(contents(devices, "/home/source"), "source");
  devices.dispose();
});

test("exhausted metadata clocks roundtrip and reject mutations atomically", () => {
  const devices = new ComputerDevices();
  devices.mountFile("/home/file", encode("keep"));
  const snapshot = devices.exportFilesystemMetadata();
  snapshot.clockNs = "18446744073709551615";
  snapshot.nextInode = "18446744073709551615";
  devices.importFilesystemMetadata(snapshot);
  assert.deepEqual(devices.exportFilesystemMetadata(), snapshot);
  const handle = devices.fsOpen("/home/file", WRITE);
  assert.ok(handle >= 16);
  assert.equal(devices.fsWrite(handle, encode("loss")), LIMIT);
  assert.equal(devices.fsTruncate(handle, 0), LIMIT);
  devices.fsClose(handle);
  assert.equal(devices.fsRename("/home/file", "/home/renamed"), LIMIT);
  assert.equal(devices.fsRemove("/home/file"), LIMIT);
  assert.equal(devices.fsMkdir("/home/new"), LIMIT);
  assert.equal(contents(devices, "/home/file"), "keep");
  assert.deepEqual(devices.exportFilesystemMetadata(), snapshot);
  assert.deepEqual(devices.takeModifiedFiles(), []);
  assert.deepEqual(devices.takeRemovedFiles(), []);
  assert.equal(devices.takeFilesystemMetadata(), null);
});
