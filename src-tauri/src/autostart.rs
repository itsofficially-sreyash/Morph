//! Start Morph at sign-in (owner rule: on by default). Windows: a value under
//! `HKCU\...\Run`; Linux: an XDG autostart entry. Both point at the running executable,
//! so after an update or reinstall the next launch re-points them.
//!
//! Debug builds never register themselves: a dev build starting at every sign-in would
//! be a surprise, and its path is a build folder.

use tauri::{plugin::TauriPlugin, AppHandle, Runtime};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    // No arguments: a launch at sign-in stays quietly in the tray (see `cli::Command::None`).
    tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None::<Vec<&str>>)
}

/// Makes the system's autostart entry match `enabled`.
pub fn apply<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    let launcher = app.autolaunch();
    let result = if enabled { launcher.enable() } else { launcher.disable() };
    result.map_err(|e| format!("Couldn't change start at sign-in: {e}"))
}
