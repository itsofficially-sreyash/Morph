# Third-party software in Morph

Morph's installers include the programs below (AGENTS.md rule 4). Everything else Morph uses
comes from the user's system and is not shipped: WebView2 or WebKitGTK, system fonts, the
Windows PDF renderer, `pdftoppm`, Microsoft Office and LibreOffice.

## Bundled programs

### FFmpeg (`morph-ffmpeg`, `morph-ffmpeg.exe`)
- **Licence:** GNU LGPL 2.1 or later. Built **without** `--enable-gpl` and `--enable-nonfree`;
  `scripts/fetch-ffmpeg.mjs` refuses any build that has either.
- **Build:** the `n9.0` LGPL builds from [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds)
  (win64 and linux64, static).
- **Source:** FFmpeg at <https://git.ffmpeg.org/ffmpeg.git> (tag `n9.0.x`, as printed by
  `morph-ffmpeg -version`), with the build scripts and library versions at
  <https://github.com/BtbN/FFmpeg-Builds>. On request we provide the exact source for any
  released version of Morph.
- Morph runs FFmpeg as a separate program and doesn't link to it. Users may replace
  `morph-ffmpeg` with their own build.
- **Libraries inside that build**, each under its own licence: libvpx, libaom, dav1d, Opus,
  OpenH264 (BSD-style); LAME (LGPL); and others. `morph-ffmpeg -buildconf` lists them all.
- **Patents:** H.264 and AAC are patent-encumbered formats. On Windows, Morph encodes H.264
  with Windows' own Media Foundation encoder. The OpenH264 in these builds is compiled from
  source, so Cisco's patent licence for its binaries does not cover it.

## Rust crates and npm packages
Morph is built from open-source crates (Tauri, tiny-skia, ab_glyph, image, lopdf, infer,
windows-rs, gtk-rs and their dependencies) and npm packages (Svelte, SvelteKit, Vite,
@tauri-apps). They are under MIT, Apache-2.0, BSD, Zlib and similar permissive licences,
recorded in `src-tauri/Cargo.lock` and `package-lock.json`.
