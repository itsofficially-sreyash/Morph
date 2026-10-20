//! The orb: the radial drop menu that opens at the cursor.
//!
//! On Windows it is a native, per-pixel-transparent popup drawn on the CPU (`win.rs`), so
//! no WebView2 processes stay alive while Morph waits in the tray (AGENTS.md rule 10).
//! Linux still uses the web overlay (`crate::overlay`) until a GTK presenter for the same
//! renderer is built and tested.

pub mod catalog;
pub mod geometry;
pub mod model;
pub mod render;
#[cfg(windows)]
mod win;

use tauri::{AppHandle, Runtime};

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
