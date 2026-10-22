mod actions;
mod autostart;
mod b64;
mod cli;
mod detect;
mod engines;
mod hotkey;
mod jobs;
mod orb;
mod settings;
mod shell_integration;
mod tools;
mod toolwin;
mod tray;
mod welcome;

use tauri::{AppHandle, Manager, RunEvent, Runtime, WindowEvent};

fn run_command<R: Runtime>(app: &AppHandle<R>, command: cli::Command, already_running: bool) {
    match command {
        cli::Command::Open(files) => orb::open_files(app, files),
        cli::Command::Toggle => orb::toggle(app),
        cli::Command::RemoveIntegration => {
            remove_integration();
            app.exit(0);
        }
        // Launching Morph again from the Start menu opens Settings; a first launch
        // (e.g. at sign-in) stays quietly in the tray.
        cli::Command::None if already_running => tray::show_settings(app),
        cli::Command::None => {}
    }
}

/// Removes everything Morph adds to the system outside its own folder: the right-click
/// entry, start at sign-in and (GNOME) the hotkey shortcut. Needs no window or tray icon.
fn remove_integration() {
    if let Err(e) = shell_integration::remove() {
        eprintln!("shell integration: {e}");
    }
    if let Err(e) = hotkey::remove_system_shortcut() {
        eprintln!("hotkey: {e}");
    }
    if let Err(e) = autostart::remove() {
        eprintln!("autostart: {e}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    let cwd = std::env::current_dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    let command = cli::parse(&args, &cwd);
    // The uninstallers run this. Clean up first, before Tauri starts: the MSI runs it
    // outside the user's desktop, where creating the tray icon fails and Morph would stop
    // before reaching any command. Starting Tauri afterwards still hands the command to a
    // running Morph, which cleans up too and quits.
    let removing = matches!(command, cli::Command::RemoveIntegration);
    if removing {
        remove_integration();
    }

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
        .plugin(autostart::plugin())
        .manage(hotkey::HotkeyState::default())
        .manage(toolwin::ToolState::default())
        .invoke_handler(tauri::generate_handler![
            settings::get_settings,
            settings::set_settings,
            hotkey::hotkey_status,
            toolwin::tool_info,
            toolwin::tool_parse_ranges,
            toolwin::tool_preview,
            toolwin::tool_run,
            welcome::welcome_info,
            welcome::welcome_try_orb,
            welcome::welcome_open_settings
        ])
        .setup(move |app| {
            if removing {
                // Already cleaned up above, and no Morph was running to hand it to.
                app.handle().exit(0);
                return Ok(());
            }
            tray::create(app.handle())?;
            orb::init(app.handle());
            // A taken or unsupported hotkey must not stop Morph from starting; the
            // settings window shows the reason (see `hotkey_status`).
            let saved = settings::load(app.handle());
            if let Err(e) = hotkey::apply(app.handle(), &saved.hotkey) {
                eprintln!("hotkey: {e}");
            }
            // Keeps the sign-in entry pointing at this executable (e.g. after an update).
            if let Err(e) = autostart::apply(app.handle(), saved.start_at_login) {
                eprintln!("autostart: {e}");
            }
            if let Err(e) = shell_integration::apply(saved.context_menu) {
                eprintln!("shell integration: {e}");
            }
            // The first launch can carry a command too (e.g. right-click while Morph isn't running).
            let plain = matches!(command, cli::Command::None);
            run_command(app.handle(), command, false);
            // First plain launch ever (after installing, or at the first sign-in): say how
            // Morph works. Not over a right-click launch, which opens the orb instead.
            if plain {
                welcome::show_once(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing the settings window destroys it (see `tray::show_settings`); the
            // run loop below keeps Morph alive in the tray.
            WindowEvent::Destroyed if window.label().starts_with("tool-") => {
                toolwin::closed(window.app_handle(), window.label());
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
