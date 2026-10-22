//! Updates from GitHub Releases. Release builds look once, 30 s after Morph starts (and
//! Morph usually starts once per sign-in); there's no timer or polling. The download is
//! checked against Morph's own signing key before it's installed. The check is the only
//! network request Morph makes, and it can be turned off in Settings.
//!
//! An update is never installed without asking: the update window shows what's new, with
//! Later, Skip this version, and Update and restart.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_store::StoreExt;
use tauri_plugin_updater::{Update, UpdaterExt};

pub const LABEL: &str = "update";
const STORE_FILE: &str = "settings.json";
const SKIPPED_KEY: &str = "skipped_update";
/// Where people can download Morph by hand if updating in place fails.
pub const RELEASES_URL: &str = "https://github.com/itsofficially-sreyash/Morph/releases/latest";

pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R, tauri_plugin_updater::Config> {
    tauri_plugin_updater::Builder::new().build()
}

/// The update found by the last check, kept so installing doesn't check again.
#[derive(Default)]
pub struct UpdateState(Mutex<Option<Update>>);

#[derive(Clone, Serialize)]
pub struct Info {
    current: String,
    version: String,
    /// Release notes, as written on the GitHub release.
    notes: Option<String>,
}

fn info(update: &Update) -> Info {
    Info { current: update.current_version.clone(), version: update.version.clone(), notes: update.body.clone() }
}

async fn check<R: Runtime>(app: &AppHandle<R>) -> Result<Option<Info>, String> {
    let mut builder = app.updater_builder();
    // For testing an update against a local server (an http:// one only works in the
    // test builds the workflow makes by hand).
    if let Some(url) = std::env::var("MORPH_UPDATE_URL").ok().and_then(|u| u.parse().ok()) {
        builder = builder.endpoints(vec![url]).map_err(|e| e.to_string())?;
    }
    let update = builder.build().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    let found = update.as_ref().map(info);
    *app.state::<UpdateState>().0.lock().unwrap() = update;
    Ok(found)
}

/// Release builds with "Check for updates" on: one look, a little after start, so it
/// never slows down signing in. Errors (offline, GitHub unreachable) are ignored.
pub fn check_after_start<R: Runtime>(app: &AppHandle<R>) {
    if cfg!(debug_assertions) || !crate::settings::load(app).auto_update {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(30));
        let Ok(Some(found)) = tauri::async_runtime::block_on(check(&app)) else { return };
        let skipped = app.store(STORE_FILE).ok().and_then(|s| s.get(SKIPPED_KEY));
        if skipped.as_ref().and_then(|v| v.as_str()) != Some(found.version.as_str()) {
            let _ = app.run_on_main_thread({
                let app = app.clone();
                move || show(&app)
            });
        }
    });
}

/// The update window: it checks by itself when opened from the tray or Settings.
pub fn show<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("/update".into()))
        .title("Morph Update")
        .inner_size(460.0, 420.0)
        .resizable(false)
        .maximizable(false)
        .center()
        .build();
    if let Err(e) = built {
        eprintln!("update window: {e}");
    }
}

/// The update already found, or a fresh check.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<Info>, String> {
    if let Some(update) = app.state::<UpdateState>().0.lock().unwrap().as_ref() {
        return Ok(Some(info(update)));
    }
    check(&app).await
}

#[derive(Clone, Serialize)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

/// Downloads, verifies and installs the update, then restarts Morph. On Windows the
/// installer runs with only a progress bar and starts the new Morph itself.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    let update = app.state::<UpdateState>().0.lock().unwrap().take().ok_or("No update to install")?;
    let mut downloaded = 0u64;
    let result = update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = app.emit_to(LABEL, "update-progress", Progress { downloaded, total });
            },
            || {},
        )
        .await;
    if let Err(e) = result {
        // Keep it for another try.
        *app.state::<UpdateState>().0.lock().unwrap() = Some(update);
        return Err(e.to_string());
    }
    app.restart();
}

#[tauri::command]
pub fn update_skip(app: AppHandle, version: String) {
    if let Ok(store) = app.store(STORE_FILE) {
        store.set(SKIPPED_KEY, version);
        let _ = store.save();
    }
}

#[tauri::command]
pub fn update_open_downloads(app: AppHandle) {
    use tauri_plugin_opener::OpenerExt;
    let _ = app.opener().open_url(RELEASES_URL, None::<&str>);
}
