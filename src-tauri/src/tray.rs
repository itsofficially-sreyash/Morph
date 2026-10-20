use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder,
};

use crate::overlay;

pub fn create<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open_orb", "Open orb", true, None::<&str>)?;
    let preview = MenuItem::with_id(app, "preview_orb", "Preview with sample files", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Morph", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &preview,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id("morph-tray")
        .tooltip("Morph")
        .icon(app.default_window_icon().cloned().expect("bundle has an icon"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open_orb" => {
                let _ = overlay::show_at_cursor(app, None);
            }
            // Until real drops land (step 3), this shows the radial menu with made-up files.
            "preview_orb" => {
                let sample = ["holiday.png", "clip.mp4", "song.mp3", "notes.pdf", "report.docx"];
                let _ = overlay::show_at_cursor(app, Some(sample.map(String::from).to_vec()));
            }
            "settings" => show_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_settings(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub const SETTINGS_LABEL: &str = "main";

/// Opens (or focuses) the settings window. It is created on demand and destroyed when
/// closed, so its web page process isn't kept alive while Morph sits in the tray.
pub fn show_settings<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("/".into()))
        .title("Morph Settings")
        .inner_size(560.0, 520.0)
        .resizable(false)
        .center()
        .build();
    if let Err(e) = built {
        eprintln!("settings window: {e}");
    }
}
