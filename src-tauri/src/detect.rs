//! What kind of file is this? Magic bytes first (a renamed `.png` that is really a JPEG
//! still works), the extension as a fallback (and for paths that don't exist, e.g. tests).

use std::io::Read;
use std::path::Path;

use crate::actions::{CategoryId, CATEGORIES};

/// Reads the first bytes of `path` and maps the detected MIME type to a category.
fn sniff(path: &Path) -> Option<CategoryId> {
    let mut head = [0u8; 512];
    let n = std::fs::File::open(path).ok()?.read(&mut head).ok()?;
    let kind = infer::get(&head[..n])?;
    let mime = kind.mime_type();
    Some(match mime {
        m if m.starts_with("image/") => CategoryId::Image,
        m if m.starts_with("video/") => CategoryId::Video,
        m if m.starts_with("audio/") => CategoryId::Audio,
        "application/pdf" => CategoryId::Pdf,
        "application/msword"
        | "application/rtf"
        | "application/vnd.oasis.opendocument.text"
        | "application/vnd.oasis.opendocument.spreadsheet"
        | "application/vnd.oasis.opendocument.presentation"
        | "application/vnd.ms-excel"
        | "application/vnd.ms-powerpoint" => CategoryId::Document,
        m if m.starts_with("application/vnd.openxmlformats-officedocument") => CategoryId::Document,
        // ZIP-based formats `infer` can't tell apart: let the extension decide.
        _ => return None,
    })
}

fn by_extension(path: &Path) -> Option<CategoryId> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    CATEGORIES.iter().find(|c| c.extensions.contains(&ext.as_str())).map(|c| c.id)
}

pub fn category_of(path: &str) -> Option<CategoryId> {
    let p = Path::new(path);
    if p.is_dir() {
        return None;
    }
    sniff(p).or_else(|| by_extension(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("morph-detect-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn magic_bytes_beat_a_wrong_extension() {
        // A JPEG saved as ".pdf" is still an image.
        let p = temp("really-a-jpeg.pdf", &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10, b'J', b'F', b'I', b'F', 0]);
        assert_eq!(category_of(p.to_str().unwrap()), Some(CategoryId::Image));
    }

    #[test]
    fn pdf_by_magic() {
        let p = temp("doc", b"%PDF-1.7\n");
        assert_eq!(category_of(p.to_str().unwrap()), Some(CategoryId::Pdf));
    }

    #[test]
    fn extension_fallback_and_unknowns() {
        assert_eq!(category_of("C:\\nope\\clip.MOV"), Some(CategoryId::Video));
        assert_eq!(category_of("/nope/notes.txt"), Some(CategoryId::Document));
        assert_eq!(category_of("/nope/setup.exe"), None);
        assert_eq!(category_of(std::env::temp_dir().to_str().unwrap()), None, "folders aren't files");
    }
}
