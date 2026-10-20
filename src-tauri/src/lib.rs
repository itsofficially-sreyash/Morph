mod overlay;
mod settings;
mod tray;

use tauri::{RunEvent, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    // Must be registered first so a second launch is caught before anything else starts.
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // TODO(step 6): route `--open <files>` / `--toggle` to the overlay.
            tray::show_settings(app);
        }));
    }

    let app = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            settings::get_settings,
            settings::set_settings,
            overlay::hide_overlay
        ])
        .setup(|app| {
            tray::create(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the settings window hides it; Morph keeps running in the tray.
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                api.prevent_close();
                let _ = window.hide();
            }
            // Clicking anywhere outside the orb dismisses it.
            WindowEvent::Focused(false) if window.label() == overlay::LABEL => {
                let _ = window.hide();
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("error while building Morph");

    app.run(|_app, event| {
        // Stay alive with no visible windows; only "Quit Morph" (exit code set) ends the app.
        if let RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
