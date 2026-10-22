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
        if let Some(entry) = entry_path() {
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
    result.map_err(|e| format!("Couldn't change start at sign-in: {e}"))?;
    #[cfg(target_os = "linux")]
    if enabled {
        add_try_exec_to_entry()?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn entry_path() -> Option<std::path::PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")))?;
    Some(config.join("autostart").join(format!("{NAME}.desktop")))
}

/// Uninstalling Morph can't remove this per-user file (package scripts run as root, and
/// deleting an AppImage runs nothing). With `TryExec`, desktops skip the entry once Morph
/// is gone (checked: GLib, which GNOME's session uses, refuses such an entry).
#[cfg(target_os = "linux")]
fn add_try_exec_to_entry() -> Result<(), String> {
    let (Some(path), Ok(exe)) = (entry_path(), crate::shell_integration::launcher_path()) else { return Ok(()) };
    let fail = |e: std::io::Error| format!("Couldn't change start at sign-in: {e}");
    let text = std::fs::read_to_string(&path).map_err(fail)?;
    let updated = with_try_exec(&text, &exe.to_string_lossy());
    if updated != text {
        std::fs::write(&path, updated).map_err(fail)?;
    }
    Ok(())
}

/// `text` with one `TryExec=<exe>` line right after the `Exec=` line.
#[cfg(any(target_os = "linux", test))]
fn with_try_exec(text: &str, exe: &str) -> String {
    let mut out = String::new();
    for line in text.lines().filter(|l| !l.starts_with("TryExec=")) {
        out.push_str(line);
        out.push('\n');
        if line.starts_with("Exec=") {
            out.push_str(&format!("TryExec={exe}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_exec_follows_exec_once() {
        let entry = "[Desktop Entry]\nType=Application\nName=Morph\nExec=/usr/bin/morph \nTerminal=false";
        let once = with_try_exec(entry, "/usr/bin/morph");
        assert_eq!(once, "[Desktop Entry]\nType=Application\nName=Morph\nExec=/usr/bin/morph \nTryExec=/usr/bin/morph\nTerminal=false\n");
        // Re-pointed after a move, never duplicated.
        let moved = with_try_exec(&once, "/home/u/Morph.AppImage");
        assert_eq!(moved.matches("TryExec=").count(), 1);
        assert!(moved.contains("TryExec=/home/u/Morph.AppImage\n"));
    }
}
