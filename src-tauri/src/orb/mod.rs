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

use crate::jobs::{self, Request};

type Starter = Box<dyn Fn(u64, Request) + Send + Sync>;

/// Set once at startup by `init`; the native orb calls it when an action is chosen.
static STARTER: OnceLock<Starter> = OnceLock::new();

/// Connects the orb to the job runner. Jobs run on a worker thread; their questions and
/// results come back to the main thread, where the orb window lives.
pub fn init<R: Runtime>(app: &AppHandle<R>) {
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
