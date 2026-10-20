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

    #[test]
    fn rejects_garbage() {
        for bad in ["", "Ctrl+", "Banana+Q", "Ctrl+Shift+Shift+NotAKey"] {
            assert!(parse(bad).is_err(), "{bad} should be rejected");
        }
    }
}
