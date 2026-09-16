#!/usr/bin/env node
// Prepares everything `tauri build` needs beside the app:
//   src-tauri/binaries/ffmpeg-<triple>[.exe]        (downloaded, cached)
//   src-tauri/binaries/hark-diarize-<triple>[.exe]  (built from crates/hark-diarize-cli)
//   src-tauri/resources/*.dll|*.dylib               (llama.cpp + sherpa-onnx runtime libs)
// Usage: node scripts/prepare-bundle.mjs [--features cuda|metal] [--profile release] [--target <triple>]
import { execFileSync } from "node:child_process";
import { createWriteStream, existsSync, mkdirSync, readdirSync, copyFileSync, statSync, rmSync } from "node:fs";
import { pipeline } from "node:stream/promises";
import path from "node:path";
import os from "node:os";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const features = args.includes("--features") ? args[args.indexOf("--features") + 1] : "";
const profile = args.includes("--profile") ? args[args.indexOf("--profile") + 1] : "release";
const hostTriple = execFileSync("rustc", ["-vV"]).toString().match(/host: (\S+)/)[1];
const triple = args.includes("--target") ? args[args.indexOf("--target") + 1] : hostTriple;
const targetArgs = triple === hostTriple ? [] : ["--target", triple];
const win = process.platform === "win32";
const mac = process.platform === "darwin";
const exe = win ? ".exe" : "";
const binDir = path.join(root, "src-tauri", "binaries");
const resDir = path.join(root, "src-tauri", "resources");
mkdirSync(binDir, { recursive: true });
mkdirSync(resDir, { recursive: true });

const log = (m) => console.log(`[prepare-bundle] ${m}`);

// 1. ffmpeg
const ffmpegOut = path.join(binDir, `ffmpeg-${triple}${exe}`);
if (!existsSync(ffmpegOut)) {
  const cache = path.join(os.homedir(), ".cache", "hark");
  mkdirSync(cache, { recursive: true });
  if (win) {
    const zip = path.join(cache, "ffmpeg-essentials.zip");
    if (!existsSync(zip)) {
      log("downloading ffmpeg (gyan.dev essentials)...");
      await download("https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip", zip);
    }
    const dir = path.join(cache, "ffmpeg-win");
    rmSync(dir, { recursive: true, force: true });
    execFileSync("powershell", ["-NoProfile", "-Command", `Expand-Archive -LiteralPath '${zip}' -DestinationPath '${dir}' -Force`]);
    const found = findFile(dir, "ffmpeg.exe");
    copyFileSync(found, ffmpegOut);
  } else if (mac) {
    const arch = triple.startsWith("aarch64") ? "arm64" : "amd64";
    const zip = path.join(cache, `ffmpeg-mac-${arch}.zip`);
    if (!existsSync(zip)) {
      log(`downloading ffmpeg (ffmpeg.martin-riedl.de, ${arch} static build)...`);
      await download(`https://ffmpeg.martin-riedl.de/redirect/latest/macos/${arch}/release/ffmpeg.zip`, zip);
    }
    const dir = path.join(cache, "ffmpeg-mac");
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    execFileSync("unzip", ["-o", "-q", zip, "-d", dir]);
    copyFileSync(path.join(dir, "ffmpeg"), ffmpegOut);
    execFileSync("chmod", ["+x", ffmpegOut]);
  } else {
    const sys = execFileSync("which", ["ffmpeg"]).toString().trim();
    copyFileSync(sys, ffmpegOut);
  }
  log(`ffmpeg -> ${ffmpegOut} (${(statSync(ffmpegOut).size / 1e6).toFixed(0)} MB)`);
} else {
  log("ffmpeg present");
}

// 2. diarize sidecar
const profileArgs = profile === "release" ? ["--release"] : ["--profile", profile];
const featArgs = features ? ["--features", features] : [];
// The sidecar always uses sherpa's prebuilt CPU binaries (its CUDA feature conflicts with them,
// and diarization is quick on CPU).
log(`building hark-diarize sidecar (${profile})...`);
execFileSync("cargo", ["build", "-p", "hark-diarize-cli", ...profileArgs, ...targetArgs], { stdio: "inherit", cwd: root });
// Honour CARGO_TARGET_DIR so a second variant (e.g. a CUDA build in target-cuda/) is
// collected from its own tree and never mixes with another variant's libraries.
const targetRoot = process.env.CARGO_TARGET_DIR ? path.resolve(process.env.CARGO_TARGET_DIR) : path.join(root, "target");
const targetDir = triple === hostTriple ? path.join(targetRoot, profile) : path.join(targetRoot, triple, profile);
copyFileSync(path.join(targetDir, `hark-diarize${exe}`), path.join(binDir, `hark-diarize-${triple}${exe}`));
log("hark-diarize sidecar copied");

// 3. runtime libs (llama.cpp dynamic + sherpa-onnx). Their build scripts copy shared
//    libraries into target/<profile>, but only when they actually run - on a warm CI cache
//    that step is skipped - so collect from the build-script output dirs and the sherpa
//    download cache instead, and fail if anything expected is missing.
log(`building hark-llm (${profile}) to ensure runtime libraries exist...`);
execFileSync("cargo", ["build", "-p", "hark-llm", ...profileArgs, ...featArgs, ...targetArgs], { stdio: "inherit", cwd: root });
const libExt = win ? ".dll" : mac ? ".dylib" : ".so";
const isLib = (f) => f.includes(libExt) && !f.startsWith("hark_lib") && !f.startsWith("libhark_lib");
// Start clean: a library left over from a previous variant (CPU ggml.dll next to a CUDA
// build, say) would otherwise ship and silently disable the GPU.
for (const f of safeReaddir(resDir)) {
  if (f.includes(libExt)) rmSync(path.join(resDir, f), { force: true });
}
const found = new Map(); // name -> path (newest wins)
const consider = (p) => {
  const name = path.basename(p);
  if (!isLib(name)) return;
  let st;
  try { st = statSync(p); } catch { return; } // dangling symlink
  if (!st.isFile()) return;
  const prev = found.get(name);
  if (!prev || st.mtimeMs > prev.mtime) found.set(name, { path: p, mtime: st.mtimeMs });
};
for (const f of safeReaddir(targetDir)) consider(path.join(targetDir, f));
walk(path.join(targetDir, "build"), 5, (p) => /llama-cpp-sys-2-|sherpa-rs-sys-/.test(p) && consider(p));
for (const cache of [path.join(os.homedir(), ".cache", "sherpa-rs"), path.join(os.homedir(), "Library", "Caches", "sherpa-rs"), path.join(os.homedir(), "AppData", "Local", "sherpa-rs")]) {
  walk(cache, 6, consider);
}
// CUDA builds: ggml-cuda.dll imports cuBLAS, which only exists on machines with the
// toolkit installed - ship the runtime DLLs (redistributable) from CUDA_PATH.
if (win && /cuda/.test(features)) {
  const cudaPath = process.env.CUDA_PATH;
  if (!cudaPath) {
    console.error("[prepare-bundle] --features cuda needs CUDA_PATH to collect cublas/cudart runtime DLLs");
    process.exit(1);
  }
  for (const dir of [path.join(cudaPath, "bin"), path.join(cudaPath, "bin", "x64")]) {
    for (const f of safeReaddir(dir)) {
      if (/^(cublas64_|cublasLt64_|cudart64_)\d+\.dll$/i.test(f)) consider(path.join(dir, f));
    }
  }
  const cublas = [...found.keys()].some((k) => /^cublas64_/i.test(k));
  if (!cublas) {
    console.error("[prepare-bundle] cublas64_*.dll not found under CUDA_PATH");
    process.exit(1);
  }
}
let n = 0;
for (const [name, { path: p }] of found) {
  copyFileSync(p, path.join(resDir, name));
  n++;
}
const required = win
  ? ["llama.dll", "ggml.dll", "ggml-base.dll", "ggml-cpu.dll", "sherpa-onnx-c-api.dll", "onnxruntime.dll"]
  : mac
    ? ["libllama.dylib", "libggml.dylib", "libggml-base.dylib", "libggml-cpu.dylib", "libsherpa-onnx-c-api.dylib", "libonnxruntime.dylib"]
    : [];
const missing = required.filter((r) => !found.has(r) && ![...found.keys()].some((k) => k.startsWith(r.replace(/\.(dll|dylib)$/, ""))));
if (missing.length) {
  console.error(`[prepare-bundle] missing runtime libraries: ${missing.join(", ")}`);
  process.exit(1);
}
log(`${n} runtime libraries -> src-tauri/resources`);

function safeReaddir(dir) {
  try { return readdirSync(dir); } catch { return []; }
}

function walk(dir, depth, visit) {
  if (depth < 0) return;
  let entries;
  try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
  for (const e of entries) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, depth - 1, visit);
    else visit(p);
  }
}

async function download(url, dest) {
  const res = await fetch(url, { redirect: "follow", headers: { "user-agent": "hark-prepare-bundle" } });
  if (!res.ok) throw new Error(`download ${url}: ${res.status}`);
  await pipeline(res.body, createWriteStream(dest));
}

function findFile(dir, name) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) {
      const r = findFile(p, name);
      if (r) return r;
    } else if (e.name === name) return p;
  }
  return null;
}
