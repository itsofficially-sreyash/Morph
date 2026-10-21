//! The orb: the radial drop menu that opens at the cursor.
//!
//! A native, per-pixel-transparent window drawn on the CPU by `render.rs`, so no web
//! processes (WebView2, WebKit) stay alive while Morph waits in the tray (AGENTS.md
//! rule 10). One behaviour model (`model.rs`), one presenter per system: `win.rs` (Win32
//! layered window) and `gtk.rs` (GTK 3 with an RGBA visual).

pub mod geometry;
pub mod model;
pub mod placement;
pub mod render;
#[cfg(target_os = "linux")]
mod gtk;
#[cfg(windows)]
mod win;

#[cfg(target_os = "linux")]
use gtk as native;
#[cfg(windows)]
use win as native;

use std::sync::OnceLock;

use tauri::{AppHandle, Runtime};

use crate::actions::{CategoryId, ToolKind};
use crate::jobs::{self, Request};

type Starter = Box<dyn Fn(u64, Request) + Send + Sync>;
type Opener = Box<dyn Fn(ToolKind, CategoryId, &'static str, Vec<String>) + Send + Sync>;

/// Set once at startup by `init`; the native orb calls it when an action is chosen.
static STARTER: OnceLock<Starter> = OnceLock::new();
/// Opens the window for actions that need more input (Merge, Split, Resize, Crop).
static OPENER: OnceLock<Opener> = OnceLock::new();

/// Connects the orb to the job runner. Jobs run on a worker thread; their questions and
/// results come back to the main thread, where the orb window lives.
pub fn init<R: Runtime>(app: &AppHandle<R>) {
    let opener_app = app.clone();
    let _ = OPENER.set(Box::new(move |kind, category, action, files| {
        // Queued, so the window is built after the orb's input handling has finished.
        let app = opener_app.clone();
        let _ = opener_app.run_on_main_thread(move || crate::toolwin::open(&app, kind, category, action, files));
    }));
    let app = app.clone();
    let _ = STARTER.set(Box::new(move |job, req| {
        let ask_app = app.clone();
        let done_app = app.clone();
        jobs::start(
            app.clone(),
            req,
            move |question| {
                let (tx, rx) = std::sync::mpsc::channel();
                let _ = ask_app.run_on_main_thread(move || native::ask(job, question, tx));
                // If the main thread goes away, never overwrite.
                rx.recv().unwrap_or(jobs::Answer::KeepBoth)
            },
            move |summary| {
                let _ = done_app.run_on_main_thread(move || native::job_finished(job, summary));
            },
        );
    }));
}

fn start_job(job: u64, req: Request) {
    match STARTER.get() {
        Some(start) => start(job, req),
        None => eprintln!("orb: job runner not initialised"),
    }
}

fn open_tool(kind: ToolKind, category: CategoryId, action: &'static str, files: Vec<String>) {
    match OPENER.get() {
        Some(open) => open(kind, category, action, files),
        None => eprintln!("orb: tool windows not initialised"),
    }
}

/// Opens the orb at the cursor. `files: None` shows the empty "Drop files" state.
pub fn show_at_cursor<R: Runtime>(app: &AppHandle<R>, files: Option<Vec<String>>) {
    // The native window lives on the main thread, whose event loop Tauri runs.
    let _ = app.run_on_main_thread(move || native::show(files));
}

/// Files from the right-click entry (`morph --open`). Explorer starts one launch per
/// selected file, so launches arriving close together are gathered into one orb.
pub fn open_files<R: Runtime>(app: &AppHandle<R>, files: Vec<String>) {
    let _ = app.run_on_main_thread(move || native::open_files(files));
}

/// Hotkey / `--toggle`: open an empty orb at the cursor, or close it if it's open.
pub fn toggle<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.run_on_main_thread(|| {
        if native::is_visible() {
            native::hide();
        } else {
            native::show(None);
        }
    });
}
