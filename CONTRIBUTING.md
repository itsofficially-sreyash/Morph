# Contributing to Morph

Thanks for helping! Bug reports, ideas and pull requests are all welcome.

## Principles every change keeps

1. **Local only.** No uploads, no telemetry, no network calls for conversions.
2. **Never overwrite the user's files** without asking. Results go next to the original
   (`Settings::output_path` in `src-tauri/src/settings.rs`); an original is never replaced.
3. **Light when idle.** Morph runs from sign-in to shutdown, so nothing may run while it waits in
   the tray: no timers, no polling, no hidden web pages. The orb is drawn natively, and other
   windows are created when opened and destroyed when closed.
4. **Never make the computer laggy.** Conversions run at below-normal priority on at most half the
   cores (`engines::background_command`, `engines::half_cores`).
5. **Windows and Linux.** A feature works on both, or the menu says why it can't. Put platform
   code behind `#[cfg(...)]`.
6. **Licences.** Morph is GPL-3.0-or-later. Anything bundled must be compatible and listed in
   [THIRD_PARTY.md](THIRD_PARTY.md). FFmpeg must be an LGPL build.

## Setting up

You need [Rust](https://rustup.rs), [Node.js](https://nodejs.org) 20+, and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/). On Ubuntu/Debian:

```bash
sudo apt install build-essential pkg-config libssl-dev libwebkit2gtk-4.1-dev libgtk-3-dev \
  libxdo-dev libayatana-appindicator3-dev librsvg2-dev ffmpeg poppler-utils
```

Then:

```bash
npm install
npm run tauri dev           # the app, with live reload for the web windows
cd src-tauri && cargo test  # all tests
npm run check               # type-check the Svelte/TypeScript side
```

Only one Morph runs at a time, so quit any installed copy before `npm run tauri dev`. Development
builds don't register start-at-sign-in, the right-click entry or system shortcuts.

## Where things are

| Path | What |
|---|---|
| `src-tauri/src/orb/` | The radial orb: `model.rs` (behaviour, unit-tested), `render.rs` (drawing with tiny-skia), `win.rs` and `gtk.rs` (the native window on each system) |
| `src-tauri/src/actions.rs` | Every category and action, and when each is available |
| `src-tauri/src/jobs.rs` | Runs a conversion: plans output names, asks before replacing, temp files, results |
| `src-tauri/src/engines/` | The converters: images, PDFs, FFmpeg, office documents |
| `src-tauri/src/shell_integration/` | The right-click entry (Explorer; Files, Nemo, Dolphin, Thunar) |
| `src-tauri/src/hotkey.rs`, `autostart.rs`, `tray.rs` | Shortcut, start at sign-in, tray icon |
| `src/routes/` | Web windows opened on demand: Settings (`+page.svelte`), welcome, and the tool windows (Merge, Split, Resize, Crop) |
| `scripts/fetch-ffmpeg.mjs` | Fetches and checks the LGPL FFmpeg bundled with the installers |

To add an action, add it to `actions.rs`, handle it in the engine it names, and add a test.
Tests that need real tools (FFmpeg, Office, LibreOffice) skip themselves when the tool is missing.

## Pull requests

- Keep each pull request to one change, and explain what it fixes or adds and why.
- `cargo test` and `npm run check` pass, and there are no new compiler warnings.
- Say what you tested by hand, and on which system (Windows, or which Linux desktop).
- Write commit messages as a short summary line, a blank line, then the details.

## Reporting a bug

[Open an issue](https://github.com/itsofficially-sreyash/Morph/issues/new/choose) with your
system (Windows version or Linux distribution and desktop), Morph's version, what you did, what
you expected, and what happened. For a failed conversion, the message the orb showed helps a lot.

By contributing, you agree that your contribution is licensed under the GPL-3.0-or-later.
