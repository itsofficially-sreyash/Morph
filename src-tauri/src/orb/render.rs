//! Draws the orb into a premultiplied RGBA pixmap on the CPU (tiny-skia + ab_glyph).
//! No GPU context: nothing wakes the graphics driver, and nothing runs between frames.

use std::sync::Arc;

use ab_glyph::{point, Font, FontVec, PxScale, ScaleFont};
use tiny_skia::{
    Color, FillRule, GradientStop, Paint, Pixmap, Point, RadialGradient, SpreadMode, Stroke, StrokeDash, Transform,
};

use super::geometry::{self as g, C};
use super::model::Menu;

/// Weights that are missing on a system share one loaded font instead of a second copy.
pub struct Fonts {
    regular: Arc<FontVec>,
    semibold: Arc<FontVec>,
    bold: Arc<FontVec>,
}

fn read_font(paths: &[std::path::PathBuf]) -> Option<Arc<FontVec>> {
    paths
        .iter()
        .find_map(|p| std::fs::read(p).ok())
        .and_then(|bytes| FontVec::try_from_vec(bytes).ok())
        .map(Arc::new)
}

impl Fonts {
    /// System UI fonts: Segoe UI on Windows (ships with every Windows 10/11), common
    /// Linux sans fonts otherwise. Nothing is bundled.
    pub fn load() -> Option<Self> {
        let windir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
        let win = |f: &str| windir.join("Fonts").join(f);
        let linux = |f: &str| std::path::PathBuf::from("/usr/share/fonts").join(f);
        let regular = read_font(&[
            win("segoeui.ttf"),
            linux("truetype/dejavu/DejaVuSans.ttf"),
            linux("TTF/DejaVuSans.ttf"),
            linux("noto/NotoSans-Regular.ttf"),
        ])?;
        let bold = read_font(&[
            win("segoeuib.ttf"),
            linux("truetype/dejavu/DejaVuSans-Bold.ttf"),
            linux("TTF/DejaVuSans-Bold.ttf"),
            linux("noto/NotoSans-Bold.ttf"),
        ]);
        let semibold = read_font(&[win("seguisb.ttf"), linux("noto/NotoSans-SemiBold.ttf")]);
        let bold = bold.unwrap_or_else(|| regular.clone());
        let semibold = semibold.unwrap_or_else(|| bold.clone());
        Some(Fonts { regular, semibold, bold })
    }
}

#[derive(Clone, Copy)]
struct Palette {
    ring_bg: Color,
    ring_edge: Color,
    hub_hi: Color,
    hub_lo: Color,
    slice_l: f32,
    slice_a: f32,
    text: Color,
    muted: Color,
    accent: Color,
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, (a * 255.0).round() as u8)
}

/// Same values as the CSS variables in the web orb (`src/routes/overlay/+page.svelte`).
fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            ring_bg: rgba(24, 22, 30, 0.97),
            ring_edge: rgba(255, 255, 255, 0.1),
            hub_hi: rgba(0x2c, 0x29, 0x35, 1.0),
            hub_lo: rgba(0x1a, 0x18, 0x20, 1.0),
            slice_l: 0.26,
            slice_a: 0.92,
            text: rgba(0xf1, 0xef, 0xf6, 1.0),
            muted: rgba(0xa1, 0x9d, 0xad, 1.0),
            accent: rgba(0x9b, 0x82, 0xff, 1.0),
        }
    } else {
        Palette {
            ring_bg: rgba(250, 249, 246, 0.98),
            ring_edge: rgba(0, 0, 0, 0.1),
            hub_hi: rgba(0xff, 0xff, 0xff, 1.0),
            hub_lo: rgba(0xee, 0xec, 0xe7, 1.0),
            slice_l: 0.90,
            slice_a: 1.0,
            text: rgba(0x1d, 0x1c, 0x1a, 1.0),
            muted: rgba(0x6b, 0x68, 0x62, 1.0),
            accent: rgba(0x6c, 0x4b, 0xf4, 1.0),
        }
    }
}

fn hsl(h: f32, s: f32, l: f32, a: f32) -> Color {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    Color::from_rgba(r + m, g + m, b + m, a).unwrap_or(Color::BLACK)
}

fn faded(mut c: Color, alpha: f32) -> Color {
    c.apply_opacity(alpha);
    c
}

fn paint(color: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color);
    p.anti_alias = true;
    p
}

#[derive(Clone, Copy)]
enum Weight {
    Regular,
    Semibold,
    Bold,
}

struct Text<'a> {
    fonts: &'a Fonts,
    /// Logical → device transform (DPI scale plus the opening animation).
    xf: Transform,
    alpha: f32,
}

impl Text<'_> {
    fn font(&self, w: Weight) -> &FontVec {
        match w {
            Weight::Regular => &self.fonts.regular,
            Weight::Semibold => &self.fonts.semibold,
            Weight::Bold => &self.fonts.bold,
        }
    }

    /// Width in logical units.
    fn measure(&self, w: Weight, size: f32, text: &str) -> f32 {
        let font = self.font(w).as_scaled(PxScale::from(size));
        let mut width = 0.0;
        let mut prev = None;
        for ch in text.chars() {
            let id = font.glyph_id(ch);
            if let Some(p) = prev {
                width += font.kern(p, id);
            }
            width += font.h_advance(id);
            prev = Some(id);
        }
        width
    }

    fn ellipsize(&self, w: Weight, size: f32, text: &str, max: f32) -> String {
        if self.measure(w, size, text) <= max {
            return text.to_string();
        }
        let mut chars: Vec<char> = text.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let candidate: String = chars.iter().collect::<String>() + "…";
            if self.measure(w, size, &candidate) <= max {
                return candidate;
            }
        }
        "…".into()
    }

    /// Word-wraps to `max` wide, breaking long words anywhere; the last line gets "…" if cut.
    fn wrap(&self, w: Weight, size: f32, text: &str, max: f32, max_lines: usize) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut line = String::new();
        for word in text.split_whitespace() {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if self.measure(w, size, &candidate) <= max {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            // A single word wider than the box is broken across lines.
            for ch in word.chars() {
                let next = format!("{line}{ch}");
                if !line.is_empty() && self.measure(w, size, &next) > max {
                    lines.push(std::mem::take(&mut line));
                    line.push(ch);
                } else {
                    line = next;
                }
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        if lines.len() > max_lines {
            lines.truncate(max_lines);
            let last = lines.pop().unwrap_or_default() + "…";
            lines.push(self.ellipsize(w, size, &last, max));
        }
        lines
    }

    /// Draws `text` centred on (`cx`, `cy`) in logical units, like SVG
    /// `text-anchor: middle; dominant-baseline: central`.
    fn draw(&self, pm: &mut Pixmap, w: Weight, size: f32, text: &str, cx: f32, cy: f32, color: Color) {
        let device_size = size * self.xf.sx;
        let font = self.font(w).as_scaled(PxScale::from(device_size));
        let width = self.measure(w, size, text) * self.xf.sx;
        let mut origin = Point::from_xy(cx, cy);
        self.xf.map_points(std::slice::from_mut(&mut origin));
        let baseline = origin.y + (font.ascent() + font.descent()) / 2.0;
        let mut x = origin.x - width / 2.0;
        let color = faded(color, self.alpha);
        let mut prev = None;
        for ch in text.chars() {
            let id = font.glyph_id(ch);
            if let Some(p) = prev {
                x += font.kern(p, id);
            }
            let glyph = id.with_scale_and_position(PxScale::from(device_size), point(x, baseline));
            x += font.h_advance(id);
            prev = Some(id);
            if let Some(outline) = self.font(w).outline_glyph(glyph) {
                let b = outline.px_bounds();
                outline.draw(|gx, gy, coverage| {
                    blend(pm, b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32, color, coverage);
                });
            }
        }
    }
}

/// Source-over blend of `color` × `coverage` into one premultiplied RGBA pixel.
fn blend(pm: &mut Pixmap, x: i32, y: i32, color: Color, coverage: f32) {
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    if x < 0 || y < 0 || x >= w || y >= h {
        return;
    }
    let a = color.alpha() * coverage.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let i = ((y * w + x) * 4) as usize;
    let px = &mut pm.data_mut()[i..i + 4];
    let inv = 1.0 - a;
    let src = [color.red(), color.green(), color.blue()];
    for c in 0..3 {
        px[c] = (src[c] * a * 255.0 + px[c] as f32 * inv).round().min(255.0) as u8;
    }
    px[3] = (a * 255.0 + px[3] as f32 * inv).round().min(255.0) as u8;
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Device-pixel size of the orb for a DPI scale factor.
pub fn device_size(scale: f32) -> u32 {
    (g::SIZE * scale).round() as u32
}

/// Renders one frame. `open` runs 0 → 1 during the opening animation (1 = settled).
pub fn render(pm: &mut Pixmap, menu: &Menu, scale: f32, open: f32, dark: bool, fonts: &Fonts) {
    pm.fill(Color::TRANSPARENT);
    let pal = palette(dark);
    let t = ease_out_cubic(open);
    let k = 0.72 + 0.28 * t;
    let alpha = open.clamp(0.0, 1.0);
    let xf = Transform::from_translate(C, C)
        .pre_scale(k, k)
        .pre_translate(-C, -C)
        .post_scale(scale, scale);

    // Backdrop disc (+ dashed glow while files are dragged over the orb).
    if let Some(disc) = g::circle(g::BACKDROP_R) {
        pm.fill_path(&disc, &paint(faded(pal.ring_bg, alpha)), FillRule::Winding, xf, None);
        let (color, stroke) = if menu.dragging() {
            let mut s = Stroke { width: 4.0, ..Stroke::default() };
            s.dash = StrokeDash::new(vec![10.0, 8.0], 0.0);
            (pal.accent, s)
        } else {
            (pal.ring_edge, Stroke { width: 1.0, ..Stroke::default() })
        };
        pm.stroke_path(&disc, &paint(faded(color, alpha)), &stroke, xf, None);
    }

    let text = Text { fonts, xf, alpha };
    let items = menu.items();
    let active = menu.active();
    for (i, (item, s)) in items.iter().zip(g::sectors(items.len(), 2.0)).enumerate() {
        let is_active = active == Some(i);
        // The highlighted slice pops out slightly.
        let slice_xf = if is_active {
            Transform::from_translate(C, C).pre_scale(1.035, 1.035).pre_translate(-C, -C).post_concat(xf)
        } else {
            xf
        };
        if let Some(path) = g::annular(g::R0, g::R1, s) {
            let fill = if is_active { hsl(item.hue, 0.78, 0.58, 1.0) } else { hsl(item.hue, 0.55, pal.slice_l, pal.slice_a) };
            pm.fill_path(&path, &paint(faded(fill, alpha)), FillRule::Winding, slice_xf, None);
            let edge = hsl(item.hue, 0.70, 0.70, 0.18);
            pm.stroke_path(&path, &paint(faded(edge, alpha)), &Stroke::default(), slice_xf, None);
        }
        let (lx, ly) = g::polar(g::LABEL_R, s.mid);
        let label_color = if is_active { Color::WHITE } else { pal.text };
        text.draw(pm, Weight::Semibold, 13.0, item.label, lx, ly, label_color);
        if i < 9 {
            let (kx, ky) = g::polar(g::R1 - 13.0, s.mid);
            let key_color = if is_active { rgba(255, 255, 255, 0.8) } else { pal.muted };
            text.draw(pm, Weight::Regular, 9.0, &(i + 1).to_string(), kx, ky, key_color);
        }
    }

    // Hub: radial gradient like the CSS (centre 50%/35%, radius 70% of its box).
    if let Some(hub) = g::circle(g::HUB) {
        let centre = Point::from_xy(C, C - 0.3 * g::HUB);
        let shader = RadialGradient::new(
            centre,
            centre,
            1.4 * g::HUB,
            vec![GradientStop::new(0.0, faded(pal.hub_hi, alpha)), GradientStop::new(1.0, faded(pal.hub_lo, alpha))],
            SpreadMode::Pad,
            Transform::identity(),
        );
        let mut p = Paint::default();
        p.anti_alias = true;
        if let Some(shader) = shader {
            p.shader = shader;
        }
        pm.fill_path(&hub, &p, FillRule::Winding, xf, None);
        pm.stroke_path(&hub, &paint(faded(pal.ring_edge, alpha)), &Stroke::default(), xf, None);
    }

    // Hub text block, vertically centred: [‹ back] / title / wrapped hint (≤ 3 lines).
    // The gap separates blocks; wrapped lines of the hint sit tight like CSS line-height.
    struct Line {
        weight: Weight,
        size: f32,
        text: String,
        color: Color,
        gap_before: f32,
    }
    const GAP: f32 = 2.0;
    let mut lines: Vec<Line> = Vec::new();
    if menu.can_go_back() {
        lines.push(Line { weight: Weight::Semibold, size: 10.0, text: "‹ back".into(), color: pal.accent, gap_before: 0.0 });
    }
    // The done state's ✓ is drawn as a shape: Segoe UI Bold has no check-mark glyph.
    let check = menu.is_done();
    let title = if check { String::new() } else { text.ellipsize(Weight::Bold, 15.0, &menu.title(), g::HUB_TEXT_W) };
    let gap = if lines.is_empty() { 0.0 } else { GAP };
    lines.push(Line { weight: Weight::Bold, size: 15.0, text: title, color: pal.text, gap_before: gap });
    for (i, l) in text.wrap(Weight::Regular, 10.0, &menu.subtitle(), g::HUB_TEXT_W, 3).into_iter().enumerate() {
        let gap_before = if i == 0 { GAP } else { 0.0 };
        lines.push(Line { weight: Weight::Regular, size: 10.0, text: l, color: pal.muted, gap_before });
    }
    let height: f32 = lines.iter().map(|l| l.gap_before + l.size * 1.2).sum();
    let mut y = C - height / 2.0;
    for l in &lines {
        y += l.gap_before;
        let lh = l.size * 1.2;
        if check && l.size == 15.0 {
            draw_check(pm, C, y + lh / 2.0, faded(pal.accent, alpha), xf);
        } else {
            text.draw(pm, l.weight, l.size, &l.text, C, y + lh / 2.0, l.color);
        }
        y += lh;
    }
}

/// A check mark about one title line tall, centred on (`cx`, `cy`).
fn draw_check(pm: &mut Pixmap, cx: f32, cy: f32, color: Color, xf: Transform) {
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(cx - 7.0, cy);
    pb.line_to(cx - 2.0, cy + 5.0);
    pb.line_to(cx + 8.0, cy - 6.0);
    if let Some(path) = pb.finish() {
        let stroke = Stroke {
            width: 2.6,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..Stroke::default()
        };
        pm.stroke_path(&path, &paint(color), &stroke, xf, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_at(pm: &Pixmap, x: u32, y: u32) -> u8 {
        pm.pixel(x, y).map(|p| p.alpha()).unwrap_or(0)
    }

    /// Renders the main states to PNGs under `target/orb-snapshots/` for a visual check,
    /// and asserts the basics: corners click-through, disc opaque, nothing clipped.
    #[test]
    fn snapshots() {
        let Some(fonts) = Fonts::load() else {
            eprintln!("no system fonts found; skipping");
            return;
        };
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/orb-snapshots");
        std::fs::create_dir_all(&dir).unwrap();
        let scale = 1.75;
        let size = device_size(scale);
        let mut pm = Pixmap::new(size, size).unwrap();

        let mut m = Menu::default();
        let cases: Vec<(&str, Box<dyn Fn(&mut Menu)>, bool)> = vec![
            ("drop-dark", Box::new(|m: &mut Menu| m.reset(None)), true),
            (
                "categories-dark",
                Box::new(|m: &mut Menu| m.reset(Some(["a.png", "b.mp4", "c.mp3", "d.pdf", "e.docx"].map(String::from).to_vec()))),
                true,
            ),
            (
                "image-hover-light",
                Box::new(|m: &mut Menu| {
                    m.reset(Some(vec!["a.png".into(), "b.jpg".into()]));
                    m.hover(Some(4));
                }),
                false,
            ),
            (
                "dragging-dark",
                Box::new(|m: &mut Menu| {
                    m.reset(None);
                    m.set_dragging(true);
                }),
                true,
            ),
            (
                "done-dark",
                Box::new(|m: &mut Menu| {
                    m.reset(Some(vec!["a.png".into()]));
                    m.pick(2);
                }),
                true,
            ),
        ];
        for (name, setup, dark) in cases {
            setup(&mut m);
            render(&mut pm, &m, scale, 1.0, dark, &fonts);
            pm.save_png(dir.join(format!("{name}.png"))).unwrap();
            assert_eq!(alpha_at(&pm, 0, 0), 0, "{name}: corner must be transparent");
            assert_eq!(alpha_at(&pm, size - 1, size - 1), 0, "{name}: corner must be transparent");
            assert!(alpha_at(&pm, size / 2, size / 2) > 200, "{name}: hub must be opaque");
            // The disc's rightmost and bottom edge must be inside the pixmap (the old clipping bug).
            let edge = (C + g::BACKDROP_R - 2.0) * scale;
            assert!(alpha_at(&pm, edge as u32, size / 2) > 100, "{name}: right edge clipped");
            assert!(alpha_at(&pm, size / 2, edge as u32) > 100, "{name}: bottom edge clipped");
        }
    }

    #[test]
    fn opening_animation_starts_small_and_faint() {
        let Some(fonts) = Fonts::load() else { return };
        let mut pm = Pixmap::new(device_size(1.0), device_size(1.0)).unwrap();
        let mut m = Menu::default();
        m.reset(None);
        render(&mut pm, &m, 1.0, 0.0, true, &fonts);
        // At t=0 the disc is scaled to 72% and fully transparent.
        assert_eq!(alpha_at(&pm, 190, 190), 0);
        render(&mut pm, &m, 1.0, 1.0, true, &fonts);
        assert!(alpha_at(&pm, 190, 190) > 200);
    }

    #[test]
    fn long_hints_wrap_to_three_lines() {
        let Some(fonts) = Fonts::load() else { return };
        let t = Text { fonts: &fonts, xf: Transform::identity(), alpha: 1.0 };
        let lines = t.wrap(Weight::Regular, 10.0, "Export as PDF (needs LibreOffice)", g::HUB_TEXT_W, 3);
        assert!(lines.len() >= 2 && lines.len() <= 3, "{lines:?}");
        for l in &lines {
            assert!(t.measure(Weight::Regular, 10.0, l) <= g::HUB_TEXT_W + 0.5, "{l} too wide");
        }
        let cut = t.wrap(Weight::Regular, 10.0, &"word ".repeat(40), g::HUB_TEXT_W, 3);
        assert_eq!(cut.len(), 3);
        assert!(cut[2].ends_with('…'));
    }
}
