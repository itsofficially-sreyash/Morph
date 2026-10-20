//! The actions registry: every action the orb offers, which engine runs it, and whether
//! it can run on this machine right now. The Linux web orb mirrors the labels in
//! `src/lib/mockActions.ts` and asks Rust to run them (`jobs::run_action`).

use crate::detect;
use crate::tools::Tool;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CategoryId {
    Image,
    Video,
    Audio,
    Pdf,
    Document,
}

/// Who does the work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// In-process Rust (`engines::image`), always available.
    Image,
    External(Tool),
    /// Needs a parameter popover (step 5) or an engine that doesn't exist yet.
    NotYet,
}

#[derive(Debug)]
pub struct Action {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub engine: Engine,
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

const fn act(id: &'static str, label: &'static str, hint: &'static str, engine: Engine) -> Action {
    Action { id, label, hint, engine }
}

const IMG: Engine = Engine::Image;
const FF: Engine = Engine::External(Tool::Ffmpeg);
const LO: Engine = Engine::External(Tool::LibreOffice);
const SOON: Engine = Engine::NotYet;

pub static CATEGORIES: &[Category] = &[
    Category {
        id: CategoryId::Image,
        label: "Image",
        hue: 262.0,
        extensions: &["png", "jpg", "jpeg", "webp", "avif", "heic", "heif", "bmp", "tif", "tiff", "gif", "ico"],
        actions: &[
            act("convert:png", "PNG", "Convert to PNG", IMG),
            act("convert:jpg", "JPG", "Convert to JPG", IMG),
            act("convert:webp", "WEBP", "Convert to WEBP", IMG),
            act("convert:avif", "AVIF", "Convert to AVIF", SOON),
            act("resize", "Resize", "Resize by width or percentage", SOON),
            act("crop", "Crop", "Crop to a region", SOON),
            act("compress", "Shrink", "Compress to a smaller file", IMG),
            act("to-pdf", "PDF", "Put images into one PDF", IMG),
        ],
    },
    Category {
        id: CategoryId::Video,
        label: "Video",
        hue: 335.0,
        extensions: &["mp4", "mkv", "webm", "mov", "avi", "m4v", "wmv"],
        actions: &[
            act("convert:mp4", "MP4", "Convert to MP4", FF),
            act("convert:webm", "WEBM", "Convert to WEBM", FF),
            act("convert:gif", "GIF", "Convert to GIF", FF),
            act("trim", "Trim", "Cut the start or end", SOON),
            act("compress", "Shrink", "Compress with a quality preset", FF),
            act("extract-audio", "Audio", "Extract the soundtrack", FF),
            act("mute", "Mute", "Remove the sound", FF),
        ],
    },
    Category {
        id: CategoryId::Audio,
        label: "Audio",
        hue: 172.0,
        extensions: &["mp3", "wav", "flac", "ogg", "m4a", "opus", "aac", "wma"],
        actions: &[
            act("convert:mp3", "MP3", "Convert to MP3", FF),
            act("convert:wav", "WAV", "Convert to WAV", FF),
            act("convert:flac", "FLAC", "Convert to FLAC", FF),
            act("convert:m4a", "M4A", "Convert to M4A", FF),
            act("trim", "Trim", "Cut the start or end", SOON),
            act("normalize", "Level", "Normalize loudness", FF),
        ],
    },
    Category {
        id: CategoryId::Pdf,
        label: "PDF",
        hue: 12.0,
        extensions: &["pdf"],
        // The PDF engine (qpdf sidecar) arrives with packaging; until then these are honest "soon"s.
        actions: &[
            act("merge", "Merge", "Combine into one PDF", SOON),
            act("split", "Split", "One PDF per page", SOON),
            act("compress", "Shrink", "Compress images inside", SOON),
            act("to-images", "Images", "Export each page as PNG", SOON),
            act("rotate", "Rotate", "Rotate all pages 90°", SOON),
        ],
    },
    Category {
        id: CategoryId::Document,
        label: "Docs",
        hue: 210.0,
        extensions: &["docx", "doc", "odt", "pptx", "ppt", "odp", "xlsx", "xls", "ods", "rtf", "txt"],
        actions: &[
            act("to-pdf", "PDF", "Export as PDF", LO),
            act("convert:docx", "DOCX", "Convert to DOCX", LO),
            act("convert:odt", "ODT", "Convert to ODT", LO),
            act("convert:txt", "TXT", "Convert to TXT", LO),
        ],
    },
];

impl Category {
    pub fn accepts(&self, path: &str) -> bool {
        detect::category_of(path) == Some(self.id)
    }

    pub fn action(&'static self, id: &str) -> Option<&'static Action> {
        self.actions.iter().find(|a| a.id == id)
    }
}

impl CategoryId {
    /// The lowercase names the web orb uses (`src/lib/mockActions.ts`).
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "image" => CategoryId::Image,
            "video" => CategoryId::Video,
            "audio" => CategoryId::Audio,
            "pdf" => CategoryId::Pdf,
            "document" => CategoryId::Document,
            _ => return None,
        })
    }
}

pub fn category(id: CategoryId) -> &'static Category {
    CATEGORIES.iter().find(|c| c.id == id).expect("every CategoryId is in CATEGORIES")
}

/// Categories that apply to at least one of `files`, in catalog order.
pub fn categories_for(files: &[String]) -> Vec<&'static Category> {
    let found: Vec<Option<CategoryId>> = files.iter().map(|f| detect::category_of(f)).collect();
    CATEGORIES.iter().filter(|c| found.contains(&Some(c.id))).collect()
}

/// `Ok` if the action can run on this machine now, otherwise the reason (shown in the orb).
pub fn availability(action: &Action) -> Result<(), &'static str> {
    match action.engine {
        Engine::Image => Ok(()),
        Engine::NotYet => Err("Coming soon"),
        Engine::External(tool) if tool.find().is_some() => Ok(()),
        Engine::External(Tool::Ffmpeg) => Err("Needs FFmpeg (included in the installer)"),
        Engine::External(Tool::Qpdf) => Err("Needs qpdf (included in the installer)"),
        Engine::External(Tool::LibreOffice) => Err("Install LibreOffice to enable"),
    }
}

pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_has_actions_and_unique_ids() {
        for c in CATEGORIES {
            assert!(!c.actions.is_empty());
            let mut ids: Vec<_> = c.actions.iter().map(|a| a.id).collect();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), c.actions.len(), "duplicate action id in {}", c.label);
        }
    }

    #[test]
    fn image_actions_are_always_available() {
        let img = category(CategoryId::Image);
        for id in ["convert:png", "convert:jpg", "convert:webp", "compress", "to-pdf"] {
            assert_eq!(availability(img.action(id).unwrap()), Ok(()), "{id}");
        }
        assert_eq!(availability(img.action("crop").unwrap()), Err("Coming soon"));
    }

    #[test]
    fn categories_follow_extension_for_missing_files() {
        let files: Vec<String> = ["a.PNG", "b.mp4", "nope.xyz"].map(String::from).to_vec();
        let ids: Vec<_> = categories_for(&files).iter().map(|c| c.id).collect();
        assert_eq!(ids, [CategoryId::Image, CategoryId::Video]);
    }
}
