//! Placeholder action catalog for the radial menu, until the real `actions` registry
//! lands in step 4. Mirrors `src/lib/mockActions.ts`, which the Linux (web) orb still uses.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryId {
    Image,
    Video,
    Audio,
    Pdf,
    Document,
}

#[derive(Debug)]
pub struct Action {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
}

#[derive(Debug)]
pub struct Category {
    pub id: CategoryId,
    pub label: &'static str,
    /// Hue (degrees) used to tint this category's slices.
    pub hue: f32,
    pub extensions: &'static [&'static str],
    pub actions: &'static [Action],
}

const fn act(id: &'static str, label: &'static str, hint: &'static str) -> Action {
    Action { id, label, hint }
}

pub static CATEGORIES: &[Category] = &[
    Category {
        id: CategoryId::Image,
        label: "Image",
        hue: 262.0,
        extensions: &["png", "jpg", "jpeg", "webp", "avif", "heic", "bmp", "tiff", "gif", "ico"],
        actions: &[
            act("convert:png", "PNG", "Convert to PNG"),
            act("convert:jpg", "JPG", "Convert to JPG"),
            act("convert:webp", "WEBP", "Convert to WEBP"),
            act("convert:avif", "AVIF", "Convert to AVIF"),
            act("resize", "Resize", "Resize by width or percentage"),
            act("crop", "Crop", "Crop to a region"),
            act("compress", "Shrink", "Compress to a smaller file"),
            act("to-pdf", "PDF", "Put images into one PDF"),
        ],
    },
    Category {
        id: CategoryId::Video,
        label: "Video",
        hue: 335.0,
        extensions: &["mp4", "mkv", "webm", "mov", "avi"],
        actions: &[
            act("convert:mp4", "MP4", "Convert to MP4"),
            act("convert:webm", "WEBM", "Convert to WEBM"),
            act("convert:gif", "GIF", "Convert to GIF"),
            act("trim", "Trim", "Cut the start or end"),
            act("compress", "Shrink", "Compress with a quality preset"),
            act("extract-audio", "Audio", "Extract the soundtrack"),
            act("mute", "Mute", "Remove the sound"),
        ],
    },
    Category {
        id: CategoryId::Audio,
        label: "Audio",
        hue: 172.0,
        extensions: &["mp3", "wav", "flac", "ogg", "m4a", "opus"],
        actions: &[
            act("convert:mp3", "MP3", "Convert to MP3"),
            act("convert:wav", "WAV", "Convert to WAV"),
            act("convert:flac", "FLAC", "Convert to FLAC"),
            act("convert:m4a", "M4A", "Convert to M4A"),
            act("trim", "Trim", "Cut the start or end"),
            act("normalize", "Level", "Normalize loudness"),
        ],
    },
    Category {
        id: CategoryId::Pdf,
        label: "PDF",
        hue: 12.0,
        extensions: &["pdf"],
        actions: &[
            act("merge", "Merge", "Combine into one PDF"),
            act("split", "Split", "One PDF per page"),
            act("compress", "Shrink", "Compress images inside"),
            act("to-images", "Images", "Export each page as PNG"),
            act("rotate", "Rotate", "Rotate all pages 90°"),
        ],
    },
    Category {
        id: CategoryId::Document,
        label: "Docs",
        hue: 210.0,
        extensions: &["docx", "doc", "odt", "pptx", "xlsx", "rtf", "txt"],
        actions: &[
            act("to-pdf", "PDF", "Export as PDF (needs LibreOffice)"),
            act("convert:docx", "DOCX", "Convert to DOCX"),
            act("convert:odt", "ODT", "Convert to ODT"),
            act("convert:txt", "TXT", "Convert to TXT"),
        ],
    },
];

pub fn extension(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

impl Category {
    pub fn accepts(&self, path: &str) -> bool {
        self.extensions.contains(&extension(path).as_str())
    }
}

/// Categories that apply to at least one of `files`, in catalog order.
pub fn categories_for(files: &[String]) -> Vec<&'static Category> {
    CATEGORIES
        .iter()
        .filter(|c| files.iter().any(|f| c.accepts(f)))
        .collect()
}

pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}
