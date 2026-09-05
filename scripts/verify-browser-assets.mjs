import { createHash } from "node:crypto";
import { readFile, readdir } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const dist = resolve(root, "js/packages/polkavm-browser-runtime/dist");
const embedded = resolve(root, "rust/crates/polkavm-host-runtime-assets/assets");

const distFiles = (await readdir(dist)).sort();
const embeddedFiles = (await readdir(embedded)).sort();
if (JSON.stringify(distFiles) !== JSON.stringify(embeddedFiles)) {
  throw new Error(
    `browser asset inventory mismatch\ndist: ${distFiles.join(", ")}\nembedded: ${embeddedFiles.join(", ")}`,
  );
}

for (const file of distFiles) {
  const generated = await readFile(resolve(dist, file));
  const packaged = await readFile(resolve(embedded, file));
  const generatedHash = createHash("sha256").update(generated).digest("hex");
  const packagedHash = createHash("sha256").update(packaged).digest("hex");
  if (!generated.equals(packaged)) {
    throw new Error(
      `browser asset differs from source build: ${file}\ngenerated: ${generatedHash}\npackaged: ${packagedHash}`,
    );
  }
  console.log(`${generatedHash}  ${file}`);
}
