//! Where the orb opens: centred on the cursor, kept fully inside the monitor's work area.
//! Shared by both presenters (Windows: physical pixels; GTK: logical pixels).

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Top-left corner that centres a `size`×`size` window on the cursor,
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
