// Fetches the FFmpeg that the installers bundle, as src-tauri/binaries/morph-ffmpeg-<target>.
//
//   node scripts/fetch-ffmpeg.mjs
//
// The build is an **LGPL** build from BtbN/FFmpeg-Builds (no
// --enable-gpl / --enable-nonfree; see THIRD_PARTY.md). BtbN rebuilds its "latest" assets regularly, so there's
// no fixed checksum; instead this script refuses any FFmpeg that is GPL or non-free, older
// than 7.1 (needed for iPhone HEIC photos), or missing an encoder Morph uses.
// Run by the release workflow, and by hand before a local `tauri build`.

import { execFileSync } from "node:child_process";
import { copyFileSync, createWriteStream, existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync, chmodSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";

const SERIES = "9.0"; // FFmpeg release series to bundle
const BASE = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest";
const ASSETS = {
  "x86_64-pc-windows-msvc": `ffmpeg-n${SERIES}-latest-win64-lgpl-${SERIES}.zip`,
  "x86_64-unknown-linux-gnu": `ffmpeg-n${SERIES}-latest-linux64-lgpl-${SERIES}.tar.xz`,
};
// Encoders the presets in engines/ffmpeg.rs and engines/image.rs use. MP4 needs one of
// the H.264 ones (h264_mf on Windows; libopenh264 on Linux).
const REQUIRED = ["libvpx-vp9", "libopus", "libmp3lame", "aac", "flac", "gif", "libaom-av1", "pam"];
const H264 = { "x86_64-pc-windows-msvc": "h264_mf", "x86_64-unknown-linux-gnu": "libopenh264" };

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const target = process.argv[2] ?? execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (.+)$/m)[1];
const asset = ASSETS[target];
if (!asset) throw new Error(`No FFmpeg build configured for ${target}`);
const windows = target.includes("windows");
const outDir = join(root, "src-tauri", "binaries");
const out = join(outDir, `morph-ffmpeg-${target}${windows ? ".exe" : ""}`);

const work = mkdtempSync(join(tmpdir(), "morph-ffmpeg-"));
try {
  const archive = join(work, asset);
  // Big download from GitHub's CDN; retry a dropped connection a few times.
  for (let attempt = 1; ; attempt++) {
    try {
      console.log(`Downloading ${BASE}/${asset}`);
      const res = await fetch(`${BASE}/${asset}`);
      if (!res.ok) throw new Error(`Download failed: ${res.status} ${res.statusText}`);
      await pipeline(Readable.fromWeb(res.body), createWriteStream(archive));
      break;
    } catch (e) {
      if (attempt === 4) throw e;
      console.log(`  ${e.cause?.code ?? e.message}; retrying (${attempt}/3)`);
      await new Promise((r) => setTimeout(r, 3000 * attempt));
    }
  }

  // bsdtar (Windows 10+) and GNU tar both read these archives.
  execFileSync("tar", ["-xf", archive, "-C", work], { stdio: "inherit" });
  const top = readdirSync(work).find((n) => n !== asset && statSync(join(work, n)).isDirectory());
  const exe = join(work, top, "bin", windows ? "ffmpeg.exe" : "ffmpeg");
  if (!existsSync(exe)) throw new Error(`ffmpeg not found in ${asset}`);
  if (!windows) chmodSync(exe, 0o755);

  const run = (...args) => execFileSync(exe, ["-hide_banner", ...args], { encoding: "utf8" });
  const conf = run("-buildconf");
  for (const flag of ["--enable-gpl", "--enable-nonfree"]) {
    if (conf.includes(flag)) throw new Error(`Refusing ${asset}: built with ${flag} (see THIRD_PARTY.md)`);
  }
  const version = execFileSync(exe, ["-version"], { encoding: "utf8" }).split("\n")[0];
  const [, major, minor] = version.match(/version n?(\d+)\.(\d+)/) ?? [];
  if (!major || Number(major) * 100 + Number(minor) < 701) throw new Error(`Refusing ${version}: Morph needs FFmpeg 7.1 or newer`);
  const encoders = run("-encoders").split("\n").map((l) => l.trim().split(/\s+/)[1]);
  const missing = [...REQUIRED, H264[target]].filter((e) => !encoders.includes(e));
  if (missing.length) throw new Error(`Refusing ${asset}: missing encoders ${missing.join(", ")}`);
  const decoders = run("-decoders").split("\n").map((l) => l.trim().split(/\s+/)[1]);
  if (!decoders.includes("hevc")) throw new Error(`Refusing ${asset}: no HEVC decoder (HEIC photos)`);

  mkdirSync(outDir, { recursive: true });
  rmSync(out, { force: true });
  copyFileSync(exe, out); // the temp folder may be on another drive
  console.log(`${version}\nLGPL build, all encoders present → ${out}`);
} finally {
  rmSync(work, { recursive: true, force: true });
}
