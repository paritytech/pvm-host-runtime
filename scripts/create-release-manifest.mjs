import { execFileSync } from "node:child_process";
import { mkdir, readFile, stat, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const outputIndex = process.argv.indexOf("--output");
if (outputIndex === -1 || !process.argv[outputIndex + 1]) {
  throw new Error("usage: create-release-manifest.mjs --output <path>");
}

const packageJson = JSON.parse(
  await readFile(resolve(root, "package.json"), "utf8"),
);
const source = await readFile(
  resolve(root, "js/packages/polkavm-browser-runtime/SOURCE"),
  "utf8",
);
const sums = await readFile(
  resolve(root, "js/packages/polkavm-browser-runtime/dist/SHA256SUMS"),
  "utf8",
);
const artifacts = {};
for (const line of sums.trim().split("\n")) {
  const [sha256, file] = line.split(/\s+/, 2);
  artifacts[file] = {
    sha256,
    size: (await stat(resolve(root, "js/packages/polkavm-browser-runtime/dist", file)))
      .size,
  };
}

const revision = label => {
  const match = source.match(new RegExp(`${label}: ([0-9a-f]{40})`));
  if (!match) throw new Error(`SOURCE is missing ${label}`);
  return match[1];
};
const sourceRevision = execFileSync("git", ["rev-parse", "HEAD"], {
  cwd: root,
  encoding: "utf8",
}).trim();
if (!/^[0-9a-f]{40}$/.test(sourceRevision)) {
  throw new Error("git did not return an immutable source revision");
}

const manifest = {
  schemaVersion: 1,
  version: packageJson.version,
  sourceRepository: "https://github.com/paritytech/polkavm-host-runtime",
  sourceRevision,
  rustCrates: {
    "polkavm-host-runtime": packageJson.version,
    "polkavm-gpu-wire": packageJson.version,
    "polkavm-host-runtime-assets": packageJson.version,
  },
  npmPackages: {
    "@parity/polkavm-browser-runtime": packageJson.version,
  },
  polkavm: {
    nativeRevision: revision("PolkaVM native revision"),
    wasmRevision: revision("PolkaVM wasm revision"),
  },
  artifacts,
};
const outputPath = resolve(root, process.argv[outputIndex + 1]);
await mkdir(dirname(outputPath), { recursive: true });
await writeFile(outputPath, `${JSON.stringify(manifest, null, 2)}\n`);
