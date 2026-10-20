//! Ring geometry in logical units (the orb is `SIZE`×`SIZE`, centre `C`).
//! Angles are degrees, 0° = 12 o'clock, clockwise. Port of `src/lib/radial.ts`.

use tiny_skia::{Path, PathBuilder};

pub const SIZE: f32 = 380.0;
pub const C: f32 = SIZE / 2.0;
pub const HUB: f32 = 56.0;
pub const R0: f32 = 66.0;
pub const R1: f32 = 170.0;
pub const LABEL_R: f32 = (R0 + R1) / 2.0;
pub const BACKDROP_R: f32 = R1 + 6.0;
/// Text box inside the hub, kept clear of the circle's curved edge.
pub const HUB_TEXT_W: f32 = 92.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sector {
    pub start: f32,
    pub end: f32,
    /// Middle of the sector, used to place its label.
    pub mid: f32,
}

/// Splits the circle into `count` equal sectors, the first one centred at 12 o'clock.
pub fn sectors(count: usize, gap_deg: f32) -> Vec<Sector> {
    if count == 0 {
        return Vec::new();
    }
    let span = 360.0 / count as f32;
    let gap = if count > 1 { gap_deg } else { 0.0 };
    (0..count)
        .map(|i| {
            let mid = i as f32 * span;
            let half = (span - gap) / 2.0;
            Sector { start: mid - half, end: mid + half, mid }
        })
        .collect()
}

pub fn polar(r: f32, deg: f32) -> (f32, f32) {
    let rad = (deg - 90.0).to_radians();
    (C + r * rad.cos(), C + r * rad.sin())
}

/// Appends an arc of radius `r` from `a0` to `a1` as cubic Béziers (≤ 90° each).
fn arc(pb: &mut PathBuilder, r: f32, a0: f32, a1: f32) {
    let segments = ((a1 - a0).abs() / 90.0).ceil().max(1.0) as usize;
    let step = (a1 - a0) / segments as f32;
    for i in 0..segments {
        let s = a0 + step * i as f32;
        let e = s + step;
        // Standard cubic approximation of a circular arc.
        let k = 4.0 / 3.0 * ((e - s).to_radians() / 4.0).tan();
        let (sx, sy) = polar(r, s);
        let (ex, ey) = polar(r, e);
        let (sr, er) = ((s - 90.0).to_radians(), (e - 90.0).to_radians());
        pb.cubic_to(
            sx - k * r * sr.sin(),
            sy + k * r * sr.cos(),
            ex + k * r * er.sin(),
            ey - k * r * er.cos(),
            ex,
            ey,
        );
    }
}

/// Annular (donut) slice between radii `r0` and `r1`.
pub fn annular(r0: f32, r1: f32, s: Sector) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let (x, y) = polar(r1, s.start);
    pb.move_to(x, y);
    arc(&mut pb, r1, s.start, s.end);
    let (x, y) = polar(r0, s.end);
    pb.line_to(x, y);
    arc(&mut pb, r0, s.end, s.start);
    pb.close();
    pb.finish()
}

pub fn circle(r: f32) -> Option<Path> {
    PathBuilder::from_circle(C, C, r)
}

/// Slice under a point (logical units), or `None` inside the hub / outside the ring.
pub fn hit_test(x: f32, y: f32, count: usize) -> Option<usize> {
    let (dx, dy) = (x - C, y - C);
    let dist = dx.hypot(dy);
    if count == 0 || dist < R0 || dist > R1 {
        return None;
    }
    let deg = (dy.atan2(dx).to_degrees() + 90.0 + 360.0) % 360.0;
    let span = 360.0 / count as f32;
    Some((((deg + span / 2.0) % 360.0) / span) as usize % count)
}

pub fn in_hub(x: f32, y: f32) -> bool {
    (x - C).hypot(y - C) <= HUB
}

/// Inside the visible disc. Pixels outside it are fully transparent and click-through
/// (Windows routes clicks on alpha-0 pixels of a layered window to whatever is below).
#[cfg(test)]
pub fn in_orb(x: f32, y: f32) -> bool {
    (x - C).hypot(y - C) <= BACKDROP_R
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sector_is_centred_at_top() {
        let s = sectors(4, 2.0);
        assert_eq!(s[0].mid, 0.0);
        assert_eq!((s[0].start, s[0].end), (-44.0, 44.0));
        assert_eq!(s[2].mid, 180.0);
    }

    #[test]
    fn hit_test_maps_directions_to_slices() {
        // 4 slices: top, right, bottom, left.
        assert_eq!(hit_test(C, C - 100.0, 4), Some(0));
        assert_eq!(hit_test(C + 100.0, C, 4), Some(1));
        assert_eq!(hit_test(C, C + 100.0, 4), Some(2));
        assert_eq!(hit_test(C - 100.0, C, 4), Some(3));
        // Just left of 12 o'clock still belongs to the top slice.
        assert_eq!(hit_test(C - 5.0, C - 100.0, 4), Some(0));
    }

    #[test]
    fn hub_and_outside_are_not_slices() {
        assert_eq!(hit_test(C, C, 5), None);
        assert_eq!(hit_test(C, C - (R1 + 1.0), 5), None);
        assert!(in_hub(C + 10.0, C));
        assert!(!in_orb(0.0, 0.0), "window corners are outside the disc");
    }

    #[test]
    fn annular_path_bounds_match_radii() {
        let p = annular(R0, R1, sectors(1, 0.0)[0]).unwrap();
        let b = p.bounds();
        assert!((b.left() - (C - R1)).abs() < 0.5 && (b.right() - (C + R1)).abs() < 0.5);
    }
}
