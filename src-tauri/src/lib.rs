mod cli;
mod hotkey;
mod orb;
mod overlay;
mod settings;
mod tray;

use tauri::{AppHandle, Manager, RunEvent, Runtime, WindowEvent};

fn run_command<R: Runtime>(app: &AppHandle<R>, command: cli::Command, already_running: bool) {
    match command {
        cli::Command::Open(files) => orb::show_at_cursor(app, Some(files)),
        cli::Command::Toggle => orb::toggle(app),
        // Launching Morph again from the Start menu opens Settings; a first launch
        // (e.g. at sign-in) stays quietly in the tray.
        cli::Command::None if already_running => tray::show_settings(app),
        cli::Command::None => {}
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    // Must be registered first so a second launch is caught before anything else starts.
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            run_command(app, cli::parse(&args, &cwd), true);
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
            // Windows draws the orb natively (no WebView2 at idle); Linux keeps the web orb for now.
            #[cfg(not(windows))]
            overlay::create(app.handle())?;
            // A taken or unsupported hotkey must not stop Morph from starting; the
            // settings window shows the reason (see `hotkey_status`).
            let saved = settings::load(app.handle());
            if let Err(e) = hotkey::apply(app.handle(), &saved.hotkey) {
                eprintln!("hotkey: {e}");
            }
            // The first launch can carry a command too (e.g. right-click while Morph isn't running).
            let args: Vec<String> = std::env::args().collect();
            let cwd = std::env::current_dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
            run_command(app.handle(), cli::parse(&args, &cwd), false);
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
