mod hotkey;
mod overlay;
mod settings;
mod tray;

use tauri::{Manager, RunEvent, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    // Must be registered first so a second launch is caught before anything else starts.
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // `morph --toggle` lets desktops without global hotkeys (GNOME on Wayland) bind
            // their own system shortcut. TODO(step 6): route `--open <files>` too.
            if args.iter().any(|a| a == "--toggle") {
                overlay::toggle(app);
            } else {
                tray::show_settings(app);
            }
        }));
    }

    let app = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(hotkey::plugin())
        .manage(hotkey::HotkeyState::default())
        .invoke_handler(tauri::generate_handler![
            settings::get_settings,
            settings::set_settings,
            overlay::hide_overlay,
            hotkey::hotkey_status
        ])
        .setup(|app| {
            tray::create(app.handle())?;
            // A taken or unsupported hotkey must not stop Morph from starting; the
            // settings window shows the reason (see `hotkey_status`).
            let saved = settings::load(app.handle());
            if let Err(e) = hotkey::apply(app.handle(), &saved.hotkey) {
                eprintln!("hotkey: {e}");
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the settings window destroys it (see `tray::show_settings`); the
            // run loop below keeps Morph alive in the tray.
            // Clicking outside the orb dismisses it, except while it waits for a drop.
            WindowEvent::Focused(false) if window.label() == overlay::LABEL && overlay::hides_on_blur() => {
                overlay::hide(window.app_handle());
            }
            WindowEvent::DragDrop(drag) if window.label() == overlay::LABEL => {
                overlay::on_drag_drop(window, drag);
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
