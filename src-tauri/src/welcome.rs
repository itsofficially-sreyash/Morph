//! A short "how to use Morph" window, shown once on the first plain launch (after
//! installing, or at the first sign-in) and from the tray afterwards. Like Settings, it's
//! created on demand and destroyed when closed, so no web page stays in memory.

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_store::StoreExt;

pub const LABEL: &str = "welcome";
const STORE_FILE: &str = "settings.json";
const SHOWN_KEY: &str = "welcome_shown";

/// Shows the window if it has never been shown. It counts as shown as soon as it opens,
/// so a crash or a forced quit never brings it back.
pub fn show_once<R: Runtime>(app: &AppHandle<R>) {
    let Ok(store) = app.store(STORE_FILE) else { return };
    if store.get(SHOWN_KEY).and_then(|v| v.as_bool()).unwrap_or(false) {
        return;
    }
    show(app);
    store.set(SHOWN_KEY, true);
    if let Err(e) = store.save() {
        eprintln!("welcome: {e}");
    }
}

pub fn show<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("/welcome".into()))
        .title("Welcome to Morph")
        .inner_size(520.0, 640.0)
        .resizable(false)
        .maximizable(false)
        .center()
        .build();
    if let Err(e) = built {
        eprintln!("welcome window: {e}");
    }
}

/// What the page needs to describe this machine's Morph.
#[derive(Serialize)]
pub struct Info {
    hotkey: String,
    /// Why the hotkey isn't working, if it isn't.
    hotkey_error: Option<String>,
    os: &'static str,
    context_menu: bool,
    start_at_login: bool,
}

#[tauri::command]
pub fn welcome_info(app: AppHandle, hotkeys: tauri::State<crate::hotkey::HotkeyState>) -> Info {
    let settings = crate::settings::load(&app);
    Info {
        hotkey: settings.hotkey,
        hotkey_error: crate::hotkey::hotkey_status(hotkeys),
        os: if cfg!(windows) { "windows" } else { "linux" },
        context_menu: settings.context_menu,
        start_at_login: settings.start_at_login,
    }
}

/// "Try it": the empty orb, waiting for a drop, at the cursor (over the button).
#[tauri::command]
pub fn welcome_try_orb(app: AppHandle) {
    crate::orb::show_at_cursor(&app, None);
}

#[tauri::command]
pub fn welcome_open_settings(app: AppHandle) {
    crate::tray::show_settings(&app);
}
