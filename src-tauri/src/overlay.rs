use serde::Serialize;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Runtime, WebviewWindow};

pub const LABEL: &str = "overlay";
/// Logical (DPI-independent) size of the overlay window; matches tauri.conf.json.
const SIZE: f64 = 380.0;

/// Sent to the overlay page each time it is shown.
#[derive(Debug, Clone, Serialize)]
pub struct ShownPayload {
    /// Files already chosen (context menu, preview). `None` means "waiting for a drop".
    pub files: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Top-left corner (physical pixels) that centres a `size`×`size` window on the cursor,
/// shifted so it stays fully inside `monitor`.
pub fn place(cursor: (f64, f64), size: u32, monitor: Rect) -> (i32, i32) {
    let half = size as f64 / 2.0;
    let clamp = |want: f64, start: i32, len: u32| {
        let max = start as f64 + len as f64 - size as f64;
        // If the monitor is smaller than the window, pin to its start.
        want.min(max).max(start as f64).round() as i32
    };
    (
        clamp(cursor.0 - half, monitor.x, monitor.width),
        clamp(cursor.1 - half, monitor.y, monitor.height),
    )
}

fn window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    app.get_webview_window(LABEL)
}

pub fn show_at_cursor<R: Runtime>(app: &AppHandle<R>, files: Option<Vec<String>>) -> tauri::Result<()> {
    let Some(win) = window(app) else { return Ok(()) };
    let cursor = app.cursor_position()?;
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)?
        .or(app.primary_monitor()?);

    if let Some(m) = monitor {
        let scale = m.scale_factor();
        // Re-apply the logical size first: moving between monitors with different
        // scaling must not leave the orb too big or too small.
        win.set_size(LogicalSize::new(SIZE, SIZE))?;
        let size_px = (SIZE * scale).round() as u32;
        let area = Rect {
            x: m.position().x,
            y: m.position().y,
            width: m.size().width,
            height: m.size().height,
        };
        let (x, y) = place((cursor.x, cursor.y), size_px, area);
        win.set_position(PhysicalPosition::new(x, y))?;
    }

    win.show()?;
    win.set_focus()?;
    win.emit_to(LABEL, "overlay://shown", ShownPayload { files })?;
    Ok(())
}

pub fn hide<R: Runtime>(app: &AppHandle<R>) {
    if let Some(win) = window(app) {
        let _ = win.hide();
    }
}

#[tauri::command]
pub fn hide_overlay(app: AppHandle) {
    hide(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    const FHD: Rect = Rect { x: 0, y: 0, width: 1920, height: 1080 };

    #[test]
    fn centres_on_cursor() {
        assert_eq!(place((960.0, 540.0), 380, FHD), (770, 350));
    }

    #[test]
    fn clamps_to_top_left_corner() {
        assert_eq!(place((10.0, 5.0), 380, FHD), (0, 0));
    }

    #[test]
    fn clamps_to_bottom_right_corner() {
        assert_eq!(place((1919.0, 1079.0), 380, FHD), (1540, 700));
    }

    #[test]
    fn works_on_secondary_monitor_left_of_primary() {
        let left = Rect { x: -2560, y: -200, width: 2560, height: 1440 };
        assert_eq!(place((-2550.0, 1200.0), 570, left), (-2560, 670));
    }

    #[test]
    fn tiny_monitor_pins_to_start() {
        let tiny = Rect { x: 100, y: 100, width: 200, height: 200 };
        assert_eq!(place((200.0, 200.0), 380, tiny), (100, 100));
    }
}
