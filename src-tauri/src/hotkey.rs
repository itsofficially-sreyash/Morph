use std::sync::Mutex;

use tauri::{plugin::TauriPlugin, AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::orb;

/// The hotkey Morph currently holds, and why the last attempt failed (if it did).
#[derive(Default)]
pub struct HotkeyState {
    current: Mutex<Option<Shortcut>>,
    error: Mutex<Option<String>>,
}

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri_plugin_global_shortcut::Builder::new()
        // Morph only ever registers one shortcut, so any press means "toggle the orb".
        .with_handler(|app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                orb::toggle(app);
            }
        })
        .build()
}

pub fn parse(hotkey: &str) -> Result<Shortcut, String> {
    hotkey
        .trim()
        .parse::<Shortcut>()
        .map_err(|_| format!("\"{hotkey}\" isn't a valid hotkey. Try something like Ctrl+Shift+Space or Alt+M."))
}

/// Switches to `hotkey`. The old one is released only after the new one is registered,
/// so a failure leaves the previous hotkey working.
pub fn apply<R: Runtime>(app: &AppHandle<R>, hotkey: &str) -> Result<(), String> {
    let state = app.state::<HotkeyState>();
    let result = (|| {
        let next = parse(hotkey)?;
        let mut current = state.current.lock().unwrap();
        // Release builds only, like the other integrations: a shortcut left pointing at a
        // debug build starts it after sign-in, without the dev server its windows load from.
        // Dev builds fall back to the X11 grab (fires while an XWayland window has focus).
        #[cfg(target_os = "linux")]
        if gnome::is_wayland_session() && !cfg!(debug_assertions) {
            // Wayland lets no app grab keys globally; GNOME runs `morph --toggle` for us.
            let exe = crate::shell_integration::launcher_path()?;
            return gnome::install(hotkey, &exe);
        }
        if *current == Some(next) {
            return Ok(());
        }
        let shortcuts = app.global_shortcut();
        shortcuts.register(next).map_err(|e| {
            format!("Couldn't register {hotkey}: {e}. Another app may already be using it.")
        })?;
        if let Some(old) = current.take() {
            let _ = shortcuts.unregister(old);
        }
        *current = Some(next);
        Ok(())
    })();
    *state.error.lock().unwrap() = result.as_ref().err().cloned();
    result
}

/// Removes the system shortcut written for GNOME on Wayland (for the uninstaller).
pub fn remove_system_shortcut() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return gnome::remove();
    #[cfg(not(target_os = "linux"))]
    Ok(())
}

/// GNOME custom keyboard shortcut (Settings → Keyboard → Custom Shortcuts), written with
/// `gsettings`. Each launch of a release build re-points it at this executable, like the
/// right-click entry.
#[cfg(target_os = "linux")]
mod gnome {
    use std::path::Path;
    use std::process::Command;

    const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
    const PATH: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/morph/";

    pub fn is_wayland_session() -> bool {
        let wayland = std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland");
        // Ubuntu reports "ubuntu:GNOME".
        let gnome = std::env::var("XDG_CURRENT_DESKTOP")
            .is_ok_and(|d| d.split(':').any(|p| p.eq_ignore_ascii_case("gnome")));
        wayland && gnome
    }

    fn gsettings(args: &[&str]) -> Result<String, String> {
        let out = Command::new("gsettings")
            .args(args)
            .output()
            .map_err(|e| format!("Couldn't set the GNOME shortcut (gsettings: {e})"))?;
        if !out.status.success() {
            return Err(format!("Couldn't set the GNOME shortcut: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// "Ctrl+Shift+Space" → "<Control><Shift>space".
    pub fn accelerator(hotkey: &str) -> String {
        let parts: Vec<&str> = hotkey.split('+').map(str::trim).collect();
        let (key, mods) = parts.split_last().expect("split always yields one part");
        let mut accel = String::new();
        for m in mods {
            accel.push_str(match m.to_lowercase().as_str() {
                "ctrl" | "control" | "commandorcontrol" | "cmdorctrl" => "<Control>",
                "shift" => "<Shift>",
                "alt" | "option" => "<Alt>",
                _ => "<Super>", // super, meta, cmd, win
            });
        }
        let key = match key.to_lowercase().as_str() {
            "enter" => "Return".to_string(),
            "esc" | "escape" => "Escape".to_string(),
            "tab" => "Tab".to_string(),
            k if k.len() > 1 && k.starts_with('f') && k[1..].parse::<u8>().is_ok() => k.to_uppercase(),
            k => k.to_string(),
        };
        accel + &key
    }

    /// GVariant text: `@as []` or `['/a/', '/b/']` → the paths.
    pub fn parse_list(text: &str) -> Vec<String> {
        let inner = text.trim_start_matches("@as").trim().trim_start_matches('[').trim_end_matches(']');
        inner.split(',').map(|p| p.trim().trim_matches('\'').to_string()).filter(|p| !p.is_empty()).collect()
    }

    fn format_list(paths: &[String]) -> String {
        let quoted: Vec<String> = paths.iter().map(|p| format!("'{p}'")).collect();
        format!("[{}]", quoted.join(", "))
    }

    /// A GVariant string literal.
    fn literal(s: &str) -> String {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }

    fn set_list(f: impl FnOnce(&mut Vec<String>)) -> Result<(), String> {
        let mut paths = parse_list(&gsettings(&["get", SCHEMA, "custom-keybindings"])?);
        let before = paths.clone();
        f(&mut paths);
        if paths != before {
            gsettings(&["set", SCHEMA, "custom-keybindings", &format_list(&paths)])?;
        }
        Ok(())
    }

    pub fn install(hotkey: &str, exe: &Path) -> Result<(), String> {
        let key = format!("{SCHEMA}.custom-keybinding:{PATH}");
        // GNOME splits the command like a shell, so quote the path.
        let command = format!("'{}' --toggle", exe.to_string_lossy().replace('\'', r"'\''"));
        gsettings(&["set", &key, "name", "'Morph'"])?;
        gsettings(&["set", &key, "command", &literal(&command)])?;
        gsettings(&["set", &key, "binding", &literal(&accelerator(hotkey))])?;
        set_list(|paths| {
            if !paths.iter().any(|p| p == PATH) {
                paths.push(PATH.to_string());
            }
        })
    }

    pub fn remove() -> Result<(), String> {
        if Command::new("gsettings").arg("--version").output().is_err() {
            return Ok(()); // not GNOME: nothing was written
        }
        set_list(|paths| paths.retain(|p| p != PATH))?;
        let key = format!("{SCHEMA}.custom-keybinding:{PATH}");
        for field in ["name", "command", "binding"] {
            gsettings(&["reset", &key, field])?;
        }
        Ok(())
    }
}

#[tauri::command]
pub fn hotkey_status(state: tauri::State<HotkeyState>) -> Option<String> {
    state.error.lock().unwrap().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkey_parses() {
        assert!(parse("Ctrl+Shift+Space").is_ok());
    }

    #[test]
    fn accepts_common_forms() {
        for ok in ["Alt+M", "CommandOrControl+Shift+K", "ctrl+alt+f9", " Ctrl+Shift+Space "] {
            assert!(parse(ok).is_ok(), "{ok} should parse");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn gnome_accelerators_and_lists() {
        assert_eq!(gnome::accelerator("Ctrl+Shift+Space"), "<Control><Shift>space");
        assert_eq!(gnome::accelerator("alt+M"), "<Alt>m");
        assert_eq!(gnome::accelerator("Super+f9"), "<Super>F9");
        assert!(gnome::parse_list("@as []").is_empty());
        assert_eq!(gnome::parse_list("['/a/', '/b/']"), ["/a/", "/b/"]);
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "Ctrl+", "Banana+Q", "Ctrl+Shift+Shift+NotAKey"] {
            assert!(parse(bad).is_err(), "{bad} should be rejected");
        }
    }
}
