//! In-process image conversions (the `image` crate). No external programs.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, ImageEncoder, ImageReader, RgbImage, Rgba};

/// Decodes any supported image, with a readable error for formats we can't open yet.
pub fn open(src: &Path) -> Result<DynamicImage, String> {
    let reader = ImageReader::open(src)
        .map_err(|e| format!("Couldn't open {}: {e}", super::name(src)))?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    reader.decode().map_err(|e| match e {
        image::ImageError::Unsupported(_) => {
            format!("{} is in a format Morph can't read yet (e.g. HEIC/AVIF)", super::name(src))
        }
        other => format!("Couldn't read {}: {other}", super::name(src)),
    })
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
    let rgba = img.to_rgba8();
    let has_alpha = img.color().has_alpha();
    let enc = PngEncoder::new_with_quality(&mut w, compression, filter);
    let result = if has_alpha {
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

/// Converts `src` to the format named by `ext` ("png", "jpg", "webp").
pub fn convert(src: &Path, dst: &Path, ext: &str) -> Result<(), String> {
    let img = open(src)?;
    match ext {
        "png" => write_png(&img, dst, false),
        "jpg" => write_jpeg(&img, dst, 90),
        "webp" => write_webp(&img, dst),
        other => Err(format!("Can't write .{other} images")),
    }
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
    fn unreadable_file_gives_a_clear_error() {
        let d = dir("bad");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("fake.png");
        std::fs::write(&p, b"not an image").unwrap();
        let err = convert(&p, &d.join("o.jpg"), "jpg").unwrap_err();
        assert!(err.contains("fake.png"), "{err}");
    }
}
