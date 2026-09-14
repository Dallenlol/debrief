#!/usr/bin/env node
// Prepares everything `tauri build` needs beside the app:
//   src-tauri/binaries/ffmpeg-<triple>[.exe]        (downloaded, cached)
//   src-tauri/binaries/hark-diarize-<triple>[.exe]  (built from crates/hark-diarize-cli)
//   src-tauri/resources/*.dll|*.dylib               (llama.cpp + sherpa-onnx runtime libs)
// Usage: node scripts/prepare-bundle.mjs [--features cuda|metal] [--profile release]
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
const triple = execFileSync("rustc", ["-vV"]).toString().match(/host: (\S+)/)[1];
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
execFileSync("cargo", ["build", "-p", "hark-diarize-cli", ...profileArgs], { stdio: "inherit", cwd: root });
const targetDir = path.join(root, "target", profile);
copyFileSync(path.join(targetDir, `hark-diarize${exe}`), path.join(binDir, `hark-diarize-${triple}${exe}`));
log("hark-diarize sidecar copied");

// 3. runtime libs (llama.cpp dynamic + sherpa-onnx): their sys crates copy the shared
//    libraries into target/<profile> when built.
log(`building hark-llm (${profile}) to collect runtime libraries...`);
execFileSync("cargo", ["build", "-p", "hark-llm", ...profileArgs, ...featArgs], { stdio: "inherit", cwd: root });
const libExt = win ? ".dll" : mac ? ".dylib" : ".so";
let n = 0;
for (const f of readdirSync(targetDir)) {
  if (f.endsWith(libExt) && !f.startsWith("hark_lib")) {
    copyFileSync(path.join(targetDir, f), path.join(resDir, f));
    n++;
  }
}
log(`${n} runtime libraries -> src-tauri/resources`);

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
