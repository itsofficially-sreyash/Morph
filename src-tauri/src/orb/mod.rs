//! The orb: the radial drop menu that opens at the cursor.
//!
//! On Windows it is a native, per-pixel-transparent popup drawn on the CPU (`win.rs`), so
//! no WebView2 processes stay alive while Morph waits in the tray (AGENTS.md rule 10).
//! Linux still uses the web overlay (`crate::overlay`) until a GTK presenter for the same
//! renderer is built and tested.

pub mod geometry;
pub mod model;
pub mod render;
#[cfg(windows)]
mod win;

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
                #[cfg(windows)]
                let _ = ask_app.run_on_main_thread(move || win::ask(job, question, tx));
                #[cfg(not(windows))]
                let _ = (&ask_app, question, tx.send(jobs::Answer::KeepBoth));
                // If the main thread goes away, never overwrite.
                rx.recv().unwrap_or(jobs::Answer::KeepBoth)
            },
            move |summary| {
                #[cfg(windows)]
                let _ = done_app.run_on_main_thread(move || win::job_finished(job, summary));
                #[cfg(not(windows))]
                let _ = (&done_app, summary);
            },
        );
    }));
}

#[cfg_attr(not(windows), allow(dead_code))]
fn start_job(job: u64, req: Request) {
    match STARTER.get() {
        Some(start) => start(job, req),
        None => eprintln!("orb: job runner not initialised"),
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn open_tool(kind: ToolKind, category: CategoryId, action: &'static str, files: Vec<String>) {
    match OPENER.get() {
        Some(open) => open(kind, category, action, files),
        None => eprintln!("orb: tool windows not initialised"),
    }
}

/// Opens the orb at the cursor. `files: None` shows the empty "Drop files" state.
pub fn show_at_cursor<R: Runtime>(app: &AppHandle<R>, files: Option<Vec<String>>) {
    #[cfg(windows)]
    {
        // The native window lives on the main thread, whose message loop Tauri pumps.
        let _ = app.run_on_main_thread(move || win::show(files));
    }
    #[cfg(not(windows))]
    {
        let _ = crate::overlay::show_at_cursor(app, files);
    }
}

/// Files from the right-click entry (`morph --open`). Explorer starts one launch per
/// selected file, so launches arriving close together are gathered into one orb.
pub fn open_files<R: Runtime>(app: &AppHandle<R>, files: Vec<String>) {
    #[cfg(windows)]
    {
        let _ = app.run_on_main_thread(move || win::open_files(files));
    }
    #[cfg(not(windows))]
    {
        let _ = crate::overlay::show_at_cursor(app, Some(files));
    }
}

/// Hotkey / `--toggle`: open an empty orb at the cursor, or close it if it's open.
pub fn toggle<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(windows)]
    {
        let _ = app.run_on_main_thread(|| {
            if win::is_visible() {
                win::hide();
            } else {
                win::show(None);
            }
        });
    }
    #[cfg(not(windows))]
    {
        crate::overlay::toggle(app);
    }
}
