#!/usr/bin/env node
// Copies the llama.cpp / sherpa-onnx shared libraries next to the test binaries
// (target/<profile>/deps and target/<profile>). Their build scripts do this when
// they run, but a warm CI cache skips them, so tests would fail with DLL/dylib
// not found. Usage: node scripts/stage-runtime-libs.mjs [--profile debug]
import { copyFileSync, mkdirSync, readdirSync, statSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const profile = args.includes("--profile") ? args[args.indexOf("--profile") + 1] : "debug";
const libExt = process.platform === "win32" ? ".dll" : process.platform === "darwin" ? ".dylib" : ".so";
const targetDir = path.join(root, "target", profile);
const found = new Map();
const consider = (p) => {
  const name = path.basename(p);
  if (!name.includes(libExt) || name.startsWith("hark") || name.startsWith("libhark")) return;
  let st;
  try { st = statSync(p); } catch { return; }
  if (!st.isFile()) return;
  const prev = found.get(name);
  if (!prev || st.mtimeMs > prev.mtime) found.set(name, { path: p, mtime: st.mtimeMs });
};
const walk = (dir, depth, visit) => {
  if (depth < 0) return;
  let entries;
  try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
  for (const e of entries) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, depth - 1, visit);
    else visit(p);
  }
};
walk(path.join(targetDir, "build"), 5, (p) => /llama-cpp-sys-2-|sherpa-rs-sys-/.test(p) && consider(p));
for (const cache of [path.join(os.homedir(), ".cache", "sherpa-rs"), path.join(os.homedir(), "Library", "Caches", "sherpa-rs"), path.join(os.homedir(), "AppData", "Local", "sherpa-rs")]) {
  walk(cache, 6, consider);
}
for (const dest of [path.join(targetDir, "deps"), targetDir]) {
  mkdirSync(dest, { recursive: true });
  for (const [name, { path: p }] of found) {
    if (path.dirname(p) !== dest) copyFileSync(p, path.join(dest, name));
  }
}
console.log(`[stage-runtime-libs] ${found.size} libraries -> target/${profile}/{deps,}`);
