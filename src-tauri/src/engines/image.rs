//! In-process image conversions (the `image` crate). No external programs.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, ImageEncoder, ImageReader, RgbImage, Rgba};

use crate::tools::Tool;

/// Formats the `image` crate can't decode; FFmpeg reads them (HEVC/AV1 decoders built in,
/// including iPhone-style tiled HEIC). Windows' own HEIC support needs a paid extension.
const VIA_FFMPEG: &[&str] = &["heic", "heif", "avif"];

fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Decodes any supported image, with a readable error for formats we can't open.
pub fn open(src: &Path) -> Result<DynamicImage, String> {
    if VIA_FFMPEG.contains(&ext_of(src).as_str()) {
        return open_with_ffmpeg(src);
    }
    let reader = ImageReader::open(src)
        .map_err(|e| format!("Couldn't open {}: {e}", super::name(src)))?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    match reader.decode() {
        Ok(img) => Ok(img),
        // A HEIC/AVIF saved under another extension: FFmpeg may still read it.
        Err(image::ImageError::Unsupported(_)) if Tool::Ffmpeg.find().is_some() => open_with_ffmpeg(src),
        Err(image::ImageError::Unsupported(_)) => {
            Err(format!("{} is in a format Morph can't read", super::name(src)))
        }
        Err(other) => Err(format!("Couldn't read {}: {other}", super::name(src))),
    }
}

/// FFmpeg decodes to raw RGBA (PAM) on stdout: no temp file, and no PNG compression,
/// which took twice as long for a 24-megapixel photo.
fn open_with_ffmpeg(src: &Path) -> Result<DynamicImage, String> {
    let ff = Tool::Ffmpeg
        .find()
        .ok_or_else(|| format!("{} needs FFmpeg to open (HEIC/AVIF)", super::name(src)))?;
    let mut cmd = super::background_command(&ff);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(src)
        .args(["-frames:v", "1", "-pix_fmt", "rgba", "-c:v", "pam", "-f", "image2pipe", "-"]);
    let raw = super::run_capture(cmd, "FFmpeg")
        .map_err(|e| format!("Couldn't read {}: {e}", super::name(src)))?;
    image::load_from_memory_with_format(&raw, image::ImageFormat::Pnm)
        .map_err(|e| format!("Couldn't read {}: {e}", super::name(src)))
}

/// AVIF through FFmpeg's libaom (BSD licence, fine to bundle). Still-picture mode,
/// quality similar to JPEG 90 at a much smaller size.
pub fn write_avif(src: &Path, dst: &Path) -> Result<(), String> {
    let ff = Tool::Ffmpeg.find().ok_or("Saving AVIF needs FFmpeg")?;
    let mut cmd = super::background_command(&ff);
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"])
        .arg(src)
        .args(["-frames:v", "1", "-c:v", "libaom-av1", "-still-picture", "1", "-crf", "28", "-cpu-used", "6"])
        .args(["-pix_fmt", "yuv420p", "-threads", &super::half_cores().to_string()])
        .arg(dst);
    super::run_quietly(cmd, "FFmpeg")
}

/// JPEG and PDF have no transparency: composite onto white so transparent areas don't turn black.
pub fn flatten(img: &DynamicImage) -> RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let Rgba([r, g, b, a]) = *rgba.get_pixel(x, y);
        let a = a as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        image::Rgb([mix(r), mix(g), mix(b)])
    })
}

fn writer(dst: &Path) -> Result<BufWriter<File>, String> {
    File::create(dst).map(BufWriter::new).map_err(|e| format!("Couldn't write {}: {e}", super::name(dst)))
}

pub fn write_jpeg(img: &DynamicImage, dst: &Path, quality: u8) -> Result<(), String> {
    let rgb = flatten(img);
    let mut w = writer(dst)?;
    JpegEncoder::new_with_quality(&mut w, quality)
        .encode_image(&rgb)
        .map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())
}

pub fn write_png(img: &DynamicImage, dst: &Path, best: bool) -> Result<(), String> {
    let mut w = writer(dst)?;
    let (compression, filter) =
        if best { (CompressionType::Best, FilterType::Adaptive) } else { (CompressionType::Default, FilterType::Adaptive) };
    let enc = PngEncoder::new_with_quality(&mut w, compression, filter);
    // Convert only to the layout that gets written (a 12 MP RGBA copy is 48 MB).
    let result = if img.color().has_alpha() {
        let rgba = img.to_rgba8();
        enc.write_image(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
    } else {
        let rgb = img.to_rgb8();
        enc.write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
    };
    result.map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())
}

/// WebP from the `image` crate is lossless (exact pixels, usually much smaller than PNG).
pub fn write_webp(img: &DynamicImage, dst: &Path) -> Result<(), String> {
    let rgba = img.to_rgba8();
    let mut w = writer(dst)?;
    WebPEncoder::new_lossless(&mut w)
        .encode(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())
}

/// Converts `src` to the format named by `ext` ("png", "jpg", "webp", "avif").
pub fn convert(src: &Path, dst: &Path, ext: &str) -> Result<(), String> {
    if ext == "avif" {
        // FFmpeg reads every input format itself, HEIC included.
        return write_avif(src, dst);
    }
    let img = open(src)?;
    match ext {
        "png" => write_png(&img, dst, false),
        "jpg" => write_jpeg(&img, dst, 90),
        "webp" => write_webp(&img, dst),
        other => Err(format!("Can't write .{other} images")),
    }
}

/// Extension for an edited copy (Resize, Crop): the original's format where Morph can
/// write it; photos Morph can only read (HEIC/HEIF/AVIF) become JPEG.
pub fn edit_ext(src: &Path) -> &'static str {
    match ext_of(src).as_str() {
        "png" => "png",
        "webp" => "webp",
        "bmp" => "bmp",
        "tif" | "tiff" => "tiff",
        "gif" => "gif",
        "ico" => "ico",
        _ => "jpg",
    }
}

/// Writes `img` in the format named by `ext`.
fn write_as(img: &DynamicImage, dst: &Path, ext: &str) -> Result<(), String> {
    match ext {
        "png" => write_png(img, dst, false),
        "jpg" => write_jpeg(img, dst, 92),
        "webp" => write_webp(img, dst),
        other => {
            let format = image::ImageFormat::from_extension(other).ok_or_else(|| format!("Can't write .{other}"))?;
            // ICO holds at most 256 × 256.
            let img = if other == "ico" && (img.width() > 256 || img.height() > 256) {
                img.resize(256, 256, image::imageops::FilterType::Lanczos3)
            } else {
                img.clone()
            };
            img.save_with_format(dst, format).map_err(|e| format!("Couldn't write {}: {e}", super::name(dst)))
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ResizeSpec {
    pub percent: Option<f32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl ResizeSpec {
    /// The new size for an image of `w` × `h`.
    pub fn target(&self, w: u32, h: u32) -> Result<(u32, u32), String> {
        let scale = |n: u32, f: f64| ((n as f64 * f).round() as u32).max(1);
        let size = match (self.percent, self.width, self.height) {
            (Some(p), _, _) if p > 0.0 && p <= 1000.0 => (scale(w, p as f64 / 100.0), scale(h, p as f64 / 100.0)),
            (Some(_), _, _) => return Err("Choose a percentage between 1 and 1000".into()),
            (None, Some(nw), Some(nh)) => (nw, nh),
            (None, Some(nw), None) => (nw, scale(h, nw as f64 / w as f64)),
            (None, None, Some(nh)) => (scale(w, nh as f64 / h as f64), nh),
            (None, None, None) => return Err("Type a width, a height or a percentage".into()),
        };
        if size.0 == 0 || size.1 == 0 || size.0 > 30_000 || size.1 > 30_000 {
            return Err("Choose a size between 1 and 30000 pixels".into());
        }
        Ok(size)
    }
}

pub fn resize(src: &Path, dst: &Path, spec: ResizeSpec) -> Result<(), String> {
    let img = open(src)?;
    let (w, h) = spec.target(img.width(), img.height())?;
    let out = img.resize_exact(w, h, image::imageops::FilterType::Lanczos3);
    drop(img);
    write_as(&out, dst, edit_ext(src))
}

/// Crops to `(x, y, width, height)` in the original's pixels, clamped to the image.
pub fn crop(src: &Path, dst: &Path, (x, y, width, height): (u32, u32, u32, u32)) -> Result<(), String> {
    let img = open(src)?;
    let (iw, ih) = (img.width(), img.height());
    if x >= iw || y >= ih {
        return Err("The crop area is outside the image".into());
    }
    let (w, h) = (width.min(iw - x), height.min(ih - y));
    if w == 0 || h == 0 {
        return Err("The crop area is empty".into());
    }
    write_as(&img.crop_imm(x, y, w, h), dst, edit_ext(src))
}

/// The extension a shrunk copy keeps: JPEGs stay JPEG, PNGs stay PNG (lossless),
/// everything else becomes JPEG.
pub fn shrink_ext(src: &Path) -> &'static str {
    match src.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
        Some("png") => "png",
        _ => "jpg",
    }
}

/// Writes a smaller copy. Returns `Ok(false)` if the result wasn't smaller than the
/// original (the caller then discards it and says so).
pub fn shrink(src: &Path, dst: &Path) -> Result<bool, String> {
    let img = open(src)?;
    match shrink_ext(src) {
        "png" => write_png(&img, dst, true)?,
        _ => write_jpeg(&img, dst, 72)?,
    }
    let before = std::fs::metadata(src).map(|m| m.len()).unwrap_or(u64::MAX);
    let after = std::fs::metadata(dst).map(|m| m.len()).unwrap_or(u64::MAX);
    Ok(after < before)
}

/// Generated sample images for engine tests (no binary fixtures in the repo).
#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    /// A 120×80 gradient; with `alpha`, the left quarter is fully transparent.
    pub fn sample(dir: &Path, name: &str, alpha: bool) -> std::path::PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(name);
        let img = image::RgbaImage::from_fn(120, 80, |x, y| {
            Rgba([(x * 2) as u8, (y * 3) as u8, 128, if alpha && x < 30 { 0 } else { 255 }])
        });
        let img = DynamicImage::ImageRgba8(img);
        // JPEG can't store alpha, so save those as RGB.
        if name.ends_with(".jpg") { DynamicImage::ImageRgb8(img.to_rgb8()).save(&p) } else { img.save(&p) }.unwrap();
        p
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::sample;
    use super::*;

    fn dir(test: &str) -> std::path::PathBuf {
        std::env::temp_dir().join("morph-image-tests").join(test)
    }

    #[test]
    fn converts_between_formats_and_keeps_size() {
        let d = dir("convert");
        let src = sample(&d, "in.png", true);
        for ext in ["jpg", "webp", "png"] {
            let out = d.join(format!("out.{ext}"));
            convert(&src, &out, ext).unwrap();
            let back = image::open(&out).unwrap();
            assert_eq!((back.width(), back.height()), (120, 80), "{ext}");
        }
    }

    #[test]
    fn transparent_pixels_become_white_in_jpeg() {
        let d = dir("flatten");
        let src = sample(&d, "in.png", true);
        let out = d.join("out.jpg");
        convert(&src, &out, "jpg").unwrap();
        let px = image::open(&out).unwrap().to_rgb8().get_pixel(5, 5).0;
        assert!(px.iter().all(|&c| c > 240), "expected white, got {px:?}");
    }

    #[test]
    fn shrink_reports_when_not_smaller() {
        let d = dir("shrink");
        let src = sample(&d, "in.png", false);
        let out = d.join("out.png");
        // Whatever the result, it must not error and must report honestly.
        let smaller = shrink(&src, &out).unwrap();
        let (a, b) = (std::fs::metadata(&src).unwrap().len(), std::fs::metadata(&out).unwrap().len());
        assert_eq!(smaller, b < a);
        assert_eq!(shrink_ext(Path::new("x.JPG")), "jpg");
        assert_eq!(shrink_ext(Path::new("x.bmp")), "jpg");
    }

    #[test]
    fn resize_specs() {
        let s = |percent, width, height| ResizeSpec { percent, width, height };
        assert_eq!(s(Some(50.0), None, None).target(4000, 3000).unwrap(), (2000, 1500));
        assert_eq!(s(None, Some(1000), None).target(4000, 3000).unwrap(), (1000, 750), "width keeps proportions");
        assert_eq!(s(None, None, Some(300)).target(4000, 3000).unwrap(), (400, 300), "height keeps proportions");
        assert_eq!(s(None, Some(10), Some(20)).target(4000, 3000).unwrap(), (10, 20), "both = exact");
        assert!(s(None, None, None).target(10, 10).is_err());
        assert!(s(Some(0.0), None, None).target(10, 10).is_err());
        assert!(s(None, Some(40_000), None).target(10, 10).is_err());
    }

    /// AVIF out and back in exercises the FFmpeg path that HEIC also uses.
    #[test]
    fn avif_round_trip_through_ffmpeg() {
        if Tool::Ffmpeg.find().is_none() {
            eprintln!("FFmpeg not found; skipping");
            return;
        }
        let d = dir("avif");
        let src = sample(&d, "in.png", false);
        let avif = d.join("out.avif");
        convert(&src, &avif, "avif").unwrap();
        assert!(std::fs::read(&avif).unwrap()[4..12].starts_with(b"ftypavif"), "not an AVIF file");
        let back = open(&avif).unwrap();
        assert_eq!((back.width(), back.height()), (120, 80));
    }

    /// Real HEIC photos aren't in the repo. Run with
    /// `MORPH_HEIC_SAMPLE=<path> cargo test heic -- --ignored`.
    #[test]
    #[ignore]
    fn heic_sample_converts_to_jpeg() {
        let Some(path) = std::env::var_os("MORPH_HEIC_SAMPLE") else { return };
        let src = std::path::PathBuf::from(path);
        let img = open(&src).unwrap();
        eprintln!("decoded {} x {}", img.width(), img.height());
        let out = dir("heic").join("sample.jpg");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        convert(&src, &out, "jpg").unwrap();
        let back = image::open(&out).unwrap();
        assert_eq!((back.width(), back.height()), (img.width(), img.height()));
    }

    #[test]
    fn unreadable_file_gives_a_clear_error() {
        let d = dir("bad");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("fake.png");
        std::fs::write(&p, b"not an image").unwrap();
        let err = convert(&p, &d.join("o.jpg"), "jpg").unwrap_err();
        assert!(err.contains("fake.png"), "{err}");
    }
}
