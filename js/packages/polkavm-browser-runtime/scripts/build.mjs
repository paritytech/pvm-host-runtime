import { createHash } from "node:crypto";
import { copyFile, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { homedir } from "node:os";

const packageRoot = resolve(import.meta.dirname, "..");
const repositoryRoot = resolve(packageRoot, "../../..");
const dist = resolve(packageRoot, "dist");
const source = resolve(packageRoot, "src");

function embeddedSource(bytes, name) {
  const source = bytes.toString("utf8");
  const marker = '"use strict";\n\n';
  if (!source.includes(marker)) {
    throw new Error(`${name} is missing its strict-mode prologue`);
  }
  return Buffer.from(source.replace(marker, ""), "utf8");
}

const wasm =
  process.env.POLKAVM_HOST_RUNTIME_WASM ??
  resolve(
    repositoryRoot,
    "target/wasm32-unknown-unknown/release/polkavm_host_runtime.wasm",
  );

if (process.env.POLKAVM_HOST_RUNTIME_WASM === undefined) {
  const rustcVersion = spawnSync("rustc", ["-vV"], {
    encoding: "utf8",
  });
  if (rustcVersion.status !== 0) process.exit(rustcVersion.status ?? 1);
  const commit = /^commit-hash: ([0-9a-f]+)$/m.exec(rustcVersion.stdout)?.[1];
  if (!commit) throw new Error("rustc did not report its commit hash");
  const rustcSysroot = spawnSync("rustc", ["--print", "sysroot"], {
    encoding: "utf8",
  });
  if (rustcSysroot.status !== 0) process.exit(rustcSysroot.status ?? 1);
  const rustLibrarySource = resolve(
    rustcSysroot.stdout.trim(),
    "lib/rustlib/src/rust/library",
  );
  const canonicalRustLibrary = `/rustc/${commit}/library`;

  const build = spawnSync(
    "cargo",
    [
      "build",
      "--locked",
      "--release",
      "--target",
      "wasm32-unknown-unknown",
      "-p",
      "polkavm-host-runtime",
    ],
    {
      cwd: repositoryRoot,
      stdio: "inherit",
      env: {
        ...process.env,
        RUSTFLAGS: [
          process.env.RUSTFLAGS,
          `--remap-path-prefix=${repositoryRoot}=/workspace`,
          `--remap-path-prefix=${process.env.CARGO_HOME ?? resolve(homedir(), ".cargo")}=/cargo`,
          `--remap-path-prefix=${process.env.RUSTUP_HOME ?? resolve(homedir(), ".rustup")}=/rustup`,
          `--remap-path-prefix=${rustLibrarySource}=${canonicalRustLibrary}`,
        ]
          .filter(Boolean)
          .join(" "),
        SOURCE_DATE_EPOCH: "1",
      },
    },
  );
  if (build.status !== 0) process.exit(build.status ?? 1);
}

await rm(dist, { recursive: true, force: true });
await mkdir(dist, { recursive: true });
const translated = await readFile(resolve(source, "polkavm-wasm-translated.js"));
const runtimeCore = await readFile(resolve(source, "polkavm-runtime-core.js"));
const workerEntry = await readFile(resolve(source, "polkavm-wasm-worker-entry.js"));
await copyFile(wasm, resolve(dist, "polkavm-browser-runtime.wasm"));
await copyFile(
  resolve(source, "polkavm-gpu-worker.js"),
  resolve(dist, "polkavm-gpu-worker.js"),
);
await copyFile(
  resolve(source, "polkavm-wasm-translated.js"),
  resolve(dist, "polkavm-wasm-translated.js"),
);
await copyFile(
  resolve(source, "polkavm-runtime-core.js"),
  resolve(dist, "polkavm-runtime-core.js"),
);
await copyFile(
  resolve(source, "polkavm-wasm-worker-entry.js"),
  resolve(dist, "polkavm-wasm-worker-entry.js"),
);
await copyFile(
  resolve(source, "polkavm-computer.js"),
  resolve(dist, "polkavm-computer.js"),
);
await writeFile(
  resolve(dist, "polkavm-worker.js"),
  Buffer.concat([
    translated,
    Buffer.from("\n"),
    embeddedSource(runtimeCore, "polkavm-runtime-core.js"),
    Buffer.from("\n"),
    embeddedSource(workerEntry, "polkavm-wasm-worker-entry.js"),
  ]),
);

const files = [
  "polkavm-browser-runtime.wasm",
  "polkavm-worker.js",
  "polkavm-gpu-worker.js",
  "polkavm-wasm-translated.js",
  "polkavm-runtime-core.js",
  "polkavm-wasm-worker-entry.js",
  "polkavm-computer.js",
];
const sums = [];
for (const file of files) {
  const bytes = await readFile(resolve(dist, file));
  sums.push(`${createHash("sha256").update(bytes).digest("hex")}  ${file}`);
}
await writeFile(resolve(dist, "SHA256SUMS"), `${sums.join("\n")}\n`);
