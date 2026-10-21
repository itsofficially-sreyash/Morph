//! The actions registry: every action the orb offers, which engine runs it, and whether
//! it can run on this machine right now.

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
    /// In-process Rust image work (`engines::image`), always available.
    Image,
    /// In-process PDF tools (`engines::pdf_tools`), always available.
    Pdf,
    /// The OS's PDF renderer (`engines::pdf_render`).
    PdfRender,
    /// Microsoft Office or LibreOffice, whichever is installed (`engines::office`).
    Office,
    External(Tool),
}

/// Actions that need more input than a click open a small window first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    /// Order the PDFs, then merge.
    Merge,
    /// Every page, or custom ranges like "1-3, 5".
    Split,
    /// Width × height or a percentage.
    Resize,
    /// Drag a box over the image.
    Crop,
}

#[derive(Debug)]
pub struct Action {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub engine: Engine,
    pub window: Option<ToolKind>,
    /// How many matching files the action needs (Merge: 2).
    pub min_files: usize,
    /// Extensions this action is limited to within its category (slides and sheets can
    /// only become PDFs). `None`: every file of the category.
    pub only: Option<&'static [&'static str]>,
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
    Action { id, label, hint, engine, window: None, min_files: 1, only: None }
}

const fn tool(id: &'static str, label: &'static str, hint: &'static str, engine: Engine, kind: ToolKind) -> Action {
    Action { id, label, hint, engine, window: Some(kind), min_files: 1, only: None }
}

/// Documents that can become DOCX/ODT/TXT; slides and spreadsheets only become PDFs.
const TEXT_DOCS: &[&str] = &["docx", "doc", "odt", "rtf", "txt"];

const IMG: Engine = Engine::Image;
const PDF: Engine = Engine::Pdf;
const FF: Engine = Engine::External(Tool::Ffmpeg);
const OFFICE: Engine = Engine::Office;

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
            act("convert:avif", "AVIF", "Convert to AVIF", FF),
            tool("resize", "Resize", "Resize by size or percentage", IMG, ToolKind::Resize),
            tool("crop", "Crop", "Crop to a region you draw", IMG, ToolKind::Crop),
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
            act("normalize", "Level", "Normalize loudness", FF),
        ],
    },
    Category {
        id: CategoryId::Pdf,
        label: "PDF",
        hue: 12.0,
        extensions: &["pdf"],
        actions: &[
            Action { min_files: 2, ..tool("merge", "Merge", "Combine PDFs in the order you choose", PDF, ToolKind::Merge) },
            tool("split", "Split", "Every page, or ranges like 1-3, 5", PDF, ToolKind::Split),
            act("compress", "Shrink", "Make the file smaller (lossless)", PDF),
            act("to-images", "Images", "Save each page as a PNG", Engine::PdfRender),
            act("rotate", "Rotate", "Rotate all pages 90°", PDF),
        ],
    },
    Category {
        id: CategoryId::Document,
        label: "Docs",
        hue: 210.0,
        extensions: &["docx", "doc", "odt", "pptx", "ppt", "odp", "xlsx", "xls", "ods", "rtf", "txt"],
        actions: &[
            act("to-pdf", "PDF", "Save as PDF", OFFICE),
            Action { only: Some(TEXT_DOCS), ..act("convert:docx", "DOCX", "Save as a Word document", OFFICE) },
            Action { only: Some(TEXT_DOCS), ..act("convert:odt", "ODT", "Save as OpenDocument text", OFFICE) },
            Action { only: Some(TEXT_DOCS), ..act("convert:txt", "TXT", "Save as plain text", OFFICE) },
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

    /// The actions offered for these files: those that apply to at least one of them.
    pub fn actions_for<'a>(&'static self, files: impl IntoIterator<Item = &'a String> + Clone) -> Vec<&'static Action> {
        self.actions.iter().filter(|a| files.clone().into_iter().any(|f| a.accepts(self, f))).collect()
    }
}

impl Action {
    /// Whether this action works on `path` (a file of `category`).
    pub fn accepts(&self, category: &Category, path: &str) -> bool {
        let ext = std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase());
        category.accepts(path) && self.only.is_none_or(|only| ext.is_some_and(|e| only.contains(&e.as_str())))
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
        Engine::Image | Engine::Pdf => Ok(()),
        Engine::PdfRender => crate::engines::pdf_render::available(),
        Engine::Office => crate::engines::office::available(),
        Engine::External(tool) if tool.find().is_some() => Ok(()),
        Engine::External(Tool::Ffmpeg) => Err("Needs FFmpeg (included in the installer)"),
        Engine::External(Tool::LibreOffice) => Err("Install LibreOffice to enable"),
    }
}

/// `Ok` if there are enough matching files for the action, otherwise what's missing.
pub fn enough_files(category: &Category, action: &Action, matching: usize) -> Result<(), String> {
    if matching >= action.min_files {
        Ok(())
    } else {
        Err(format!("Drop {} or more {} files", action.min_files, category.label))
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
    fn in_process_actions_are_always_available() {
        let img = category(CategoryId::Image);
        for id in ["convert:png", "convert:jpg", "convert:webp", "compress", "to-pdf", "resize", "crop"] {
            assert_eq!(availability(img.action(id).unwrap()), Ok(()), "{id}");
        }
        let pdf = category(CategoryId::Pdf);
        for id in ["merge", "split", "compress", "rotate"] {
            assert_eq!(availability(pdf.action(id).unwrap()), Ok(()), "{id}");
        }
    }

    #[test]
    fn slides_and_sheets_only_become_pdfs() {
        let docs = category(CategoryId::Document);
        let ids = |files: &[&str]| -> Vec<&str> {
            let files: Vec<String> = files.iter().map(|f| f.to_string()).collect();
            docs.actions_for(&files).iter().map(|a| a.id).collect()
        };
        assert_eq!(ids(&["deck.pptx"]), ["to-pdf"]);
        assert_eq!(ids(&["a.xlsx", "b.ODP"]), ["to-pdf"]);
        assert_eq!(ids(&["notes.docx"]), ["to-pdf", "convert:docx", "convert:odt", "convert:txt"]);
        assert_eq!(ids(&["deck.pptx", "notes.docx"]).len(), 4);
        assert!(!docs.action("convert:docx").unwrap().accepts(docs, "deck.pptx"));
    }

    #[test]
    fn merge_needs_two_pdfs() {
        let pdf = category(CategoryId::Pdf);
        let merge = pdf.action("merge").unwrap();
        assert_eq!(enough_files(pdf, merge, 1), Err("Drop 2 or more PDF files".into()));
        assert!(enough_files(pdf, merge, 2).is_ok());
        assert_eq!(merge.window, Some(ToolKind::Merge));
    }

    #[test]
    fn no_trim_anywhere() {
        assert!(CATEGORIES.iter().flat_map(|c| c.actions).all(|a| a.id != "trim"));
    }

    #[test]
    fn categories_follow_extension_for_missing_files() {
        let files: Vec<String> = ["a.PNG", "b.mp4", "nope.xyz"].map(String::from).to_vec();
        let ids: Vec<_> = categories_for(&files).iter().map(|c| c.id).collect();
        assert_eq!(ids, [CategoryId::Image, CategoryId::Video]);
    }
}
