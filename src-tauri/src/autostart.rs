//! Start Morph at sign-in (on by default). Windows: a value under
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

/// The name the plugin registers under: the product name.
const NAME: &str = "Morph";

/// Removes the autostart entry whatever the build (for the uninstaller). Plain registry and
/// file deletes, no Tauri: the MSI runs this outside the user's desktop, where Morph's
/// windows and tray icon can't be created.
pub fn remove() -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::core::{w, HSTRING};
        use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
        use windows::Win32::System::Registry::{RegDeleteKeyValueW, HKEY_CURRENT_USER};
        let name = HSTRING::from(NAME);
        for key in [
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            // Where Task Manager records "Disabled" for the entry.
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run"),
        ] {
            let err = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key, &name) };
            if err != ERROR_FILE_NOT_FOUND {
                err.ok().map_err(|e| format!("Couldn't remove start at sign-in: {}", e.message()))?;
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")));
        if let Some(entry) = config.map(|c| c.join("autostart").join(format!("{NAME}.desktop"))) {
            match std::fs::remove_file(&entry) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(format!("Couldn't remove start at sign-in: {e}"));
                }
                _ => {}
            }
        }
    }
    Ok(())
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
