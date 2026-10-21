//! What the orb shows and how it reacts to input, independent of any window or renderer.
//! Both presenters (`win.rs`, `gtk.rs`) drive this one model.

use crate::actions::{self as catalog, Category, CategoryId, ToolKind};
use crate::jobs::Answer;

/// One slice of the ring.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: &'static str,
    /// Shown in the hub on hover. For a disabled item, the reason it can't run.
    pub hint: String,
    pub hue: f32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// `1`–`9`.
    Digit(u8),
    Next,
    Prev,
    Enter,
    Back,
    Escape,
}

/// What the window should do after an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing visible changed.
    Unchanged,
    /// Repaint; a new ring appeared if `new_ring` (replay the opening animation).
    Redraw { new_ring: bool },
    /// Start this job; the orb now shows "Working…".
    Chosen { category: CategoryId, action: &'static str, files: Vec<String> },
    /// This action needs more input: close the orb and open its window.
    OpenTool { kind: ToolKind, category: CategoryId, action: &'static str, files: Vec<String> },
    /// The user answered the "already exists" question.
    Answered(Answer),
    Close,
}

#[derive(Debug, Clone, Default, PartialEq)]
enum Status {
    #[default]
    Idle,
    /// A job is running; the text is what it's doing.
    Working(String),
    /// A running job asks whether to replace existing outputs.
    Asking(String),
    Finished { ok: bool, message: String },
}

#[derive(Debug, Default)]
pub struct Menu {
    /// `None` = the orb is waiting for a drop.
    files: Option<Vec<String>>,
    /// Chosen category (index into `available()`), when there is more than one.
    category: Option<usize>,
    status: Status,
    dragging: bool,
    active: Option<usize>,
}

const ANSWERS: [(&str, &str, f32, Answer); 2] = [
    ("Replace", "Overwrite the existing file(s)", 12.0, Answer::Replace),
    ("Keep both", "Save as a new numbered copy", 172.0, Answer::KeepBoth),
];

impl Menu {
    pub fn reset(&mut self, files: Option<Vec<String>>) {
        *self = Menu { files, ..Menu::default() };
    }

    /// Files dropped on an open orb join the ones already there (no duplicates).
    pub fn add_files(&mut self, dropped: Vec<String>) {
        let mut files = self.files.take().unwrap_or_default();
        for f in dropped {
            if !files.contains(&f) {
                files.push(f);
            }
        }
        self.reset(Some(files));
    }

    pub fn set_dragging(&mut self, over: bool) -> bool {
        std::mem::replace(&mut self.dragging, over) != over
    }

    pub fn is_waiting_for_drop(&self) -> bool {
        self.files.is_none()
    }

    /// A job is running or asking a question.
    pub fn is_busy(&self) -> bool {
        matches!(self.status, Status::Working(_) | Status::Asking(_))
    }

    pub fn is_finished(&self) -> bool {
        matches!(self.status, Status::Finished { .. })
    }

    pub fn succeeded(&self) -> bool {
        matches!(self.status, Status::Finished { ok: true, .. })
    }

    /// Losing focus closes the orb, except while it waits for a drop (the user has to click
    /// into a file manager to start dragging) or while a job is running.
    pub fn hides_on_blur(&self) -> bool {
        !self.is_waiting_for_drop() && !self.is_busy()
    }

    pub fn dragging(&self) -> bool {
        self.dragging
    }

    pub fn ask(&mut self, question: String) {
        self.status = Status::Asking(question);
        self.active = None;
    }

    pub fn finish(&mut self, ok: bool, message: String) {
        self.status = Status::Finished { ok, message };
        self.active = None;
    }

    fn available(&self) -> Vec<&'static Category> {
        self.files.as_deref().map(catalog::categories_for).unwrap_or_default()
    }

    /// The category whose actions are showing. With a single kind of file there is
    /// nothing to choose at the top level, so it opens directly.
    fn current(&self) -> Option<&'static Category> {
        let available = self.available();
        match self.category {
            Some(i) => available.get(i).copied(),
            None if available.len() == 1 => Some(available[0]),
            None => None,
        }
    }

    pub fn items(&self) -> Vec<Item> {
        if self.dragging {
            return Vec::new();
        }
        match &self.status {
            Status::Working(_) | Status::Finished { .. } => return Vec::new(),
            Status::Asking(_) => {
                return ANSWERS
                    .iter()
                    .map(|&(label, hint, hue, _)| Item { label, hint: hint.into(), hue, enabled: true })
                    .collect();
            }
            Status::Idle => {}
        }
        match self.current() {
            Some(c) => {
                let files = self.files.iter().flatten();
                c.actions_for(files.clone())
                    .into_iter()
                    .map(|a| {
                        let matching = files.clone().filter(|f| a.accepts(c, f)).count();
                        let ready = catalog::availability(a)
                            .map_err(str::to_string)
                            .and_then(|_| catalog::enough_files(c, a, matching));
                        match ready {
                            Ok(()) => Item { label: a.label, hint: a.hint.to_string(), hue: c.hue, enabled: true },
                            Err(why) => Item { label: a.label, hint: why, hue: c.hue, enabled: false },
                        }
                    })
                    .collect()
            }
            None => self
                .available()
                .iter()
                .map(|c| Item { label: c.label, hint: format!("{} actions", c.actions.len()), hue: c.hue, enabled: true })
                .collect(),
        }
    }

    pub fn active(&self) -> Option<usize> {
        self.active
    }

    pub fn can_go_back(&self) -> bool {
        self.category.is_some() && self.available().len() > 1 && self.status == Status::Idle && !self.dragging
    }

    /// The hub shows a drawn check mark instead of a title.
    pub fn shows_check(&self) -> bool {
        self.succeeded() && !self.dragging
    }

    pub fn title(&self) -> String {
        if self.dragging {
            return "Let go".into();
        }
        match &self.status {
            Status::Working(_) => return "Working…".into(),
            Status::Asking(_) => return "Already there".into(),
            Status::Finished { ok: true, .. } => return "✓".into(),
            Status::Finished { ok: false, .. } => return "Couldn't finish".into(),
            Status::Idle => {}
        }
        if self.files.is_none() {
            "Drop files".into()
        } else {
            self.current().map_or("Morph", |c| c.label).into()
        }
    }

    pub fn subtitle(&self) -> String {
        if self.dragging {
            return if self.files.as_ref().is_some_and(|f| !f.is_empty()) {
                "to add to these files".into()
            } else {
                "to open them in Morph".into()
            };
        }
        if let Some(item) = self.active.and_then(|a| self.items().into_iter().nth(a)) {
            return item.hint;
        }
        match &self.status {
            Status::Working(text) | Status::Asking(text) => return text.clone(),
            Status::Finished { message, .. } => return message.clone(),
            Status::Idle => {}
        }
        let Some(files) = &self.files else { return "or press Esc".into() };
        if self.available().is_empty() {
            return "Nothing to convert".into();
        }
        let relevant: Vec<&String> = match self.current() {
            Some(c) => files.iter().filter(|f| c.accepts(f)).collect(),
            None => files.iter().collect(),
        };
        match relevant.as_slice() {
            [one] => catalog::file_name(one).to_string(),
            many => format!("{} files", many.len()),
        }
    }

    /// Pointer moved over slice `index` (or off the ring). Returns true if it changed.
    pub fn hover(&mut self, index: Option<usize>) -> bool {
        let index = index.filter(|&i| i < self.items().len());
        std::mem::replace(&mut self.active, index) != index
    }

    pub fn pick(&mut self, index: usize) -> Outcome {
        let items = self.items();
        let Some(item) = items.get(index) else { return Outcome::Unchanged };
        if let Status::Asking(_) = self.status {
            self.status = Status::Working("Saving…".into());
            self.active = None;
            return Outcome::Answered(ANSWERS[index].3);
        }
        if self.status != Status::Idle {
            return Outcome::Unchanged;
        }
        let Some(c) = self.current() else {
            self.category = Some(index);
            self.active = None;
            return Outcome::Redraw { new_ring: true };
        };
        if !item.enabled {
            // Show why it can't run (its hint is the reason).
            self.active = Some(index);
            return Outcome::Redraw { new_ring: false };
        }
        let action = c.actions_for(self.files.iter().flatten())[index];
        let files: Vec<String> = self.files.iter().flatten().filter(|f| action.accepts(c, f)).cloned().collect();
        if let Some(kind) = action.window {
            return Outcome::OpenTool { kind, category: c.id, action: action.id, files };
        }
        self.status = Status::Working(format!("{}…", action.hint));
        self.active = None;
        Outcome::Chosen { category: c.id, action: action.id, files }
    }

    pub fn back(&mut self) -> Outcome {
        if self.can_go_back() {
            self.category = None;
            self.active = None;
            Outcome::Redraw { new_ring: true }
        } else {
            Outcome::Unchanged
        }
    }

    pub fn key(&mut self, key: Key) -> Outcome {
        if key == Key::Escape {
            return Outcome::Close;
        }
        if matches!(self.status, Status::Working(_) | Status::Finished { .. }) {
            return Outcome::Unchanged;
        }
        let n = self.items().len();
        match key {
            Key::Digit(d) if (1..=9).contains(&d) && (d as usize) <= n => self.pick(d as usize - 1),
            Key::Next | Key::Prev if n > 0 => {
                let step = if key == Key::Next { 1 } else { n - 1 };
                self.active = Some(self.active.map_or(if key == Key::Next { 0 } else { n - 1 }, |a| (a + step) % n));
                Outcome::Redraw { new_ring: false }
            }
            Key::Enter => match self.active {
                Some(a) => self.pick(a),
                None => Outcome::Unchanged,
            },
            Key::Back => self.back(),
            _ => Outcome::Unchanged,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(names: &[&str]) -> Option<Vec<String>> {
        Some(names.iter().map(|s| s.to_string()).collect())
    }

    fn labels(m: &Menu) -> Vec<&'static str> {
        m.items().iter().map(|i| i.label).collect()
    }

    #[test]
    fn empty_orb_waits_for_drop() {
        let mut m = Menu::default();
        m.reset(None);
        assert!(m.is_waiting_for_drop());
        assert!(!m.hides_on_blur());
        assert_eq!((m.title().as_str(), m.subtitle().as_str()), ("Drop files", "or press Esc"));
        assert!(m.items().is_empty());
    }

    #[test]
    fn mixed_files_show_categories_then_actions() {
        let mut m = Menu::default();
        m.reset(files(&["a.png", "b.mp4", "c.mp3", "d.pdf", "e.docx"]));
        assert_eq!(labels(&m), ["Image", "Video", "Audio", "PDF", "Docs"]);
        assert_eq!(m.subtitle(), "5 files");
        assert_eq!(m.pick(0), Outcome::Redraw { new_ring: true });
        assert_eq!(m.title(), "Image");
        assert_eq!(m.subtitle(), "a.png");
        assert_eq!(labels(&m).len(), 8);
        assert!(m.can_go_back());
    }

    #[test]
    fn single_kind_opens_actions_directly() {
        let mut m = Menu::default();
        m.reset(files(&["a.png", "b.JPG"]));
        assert_eq!(m.title(), "Image");
        assert_eq!(m.subtitle(), "2 files");
        assert!(!m.can_go_back());
    }

    #[test]
    fn slides_offer_only_pdf() {
        let mut m = Menu::default();
        m.reset(files(&["deck.pptx"]));
        assert_eq!(labels(&m), ["PDF"]);
        m.reset(files(&["deck.pptx", "notes.docx"]));
        assert_eq!(labels(&m), ["PDF", "DOCX", "ODT", "TXT"]);
        // Picking DOCX sends only the text document.
        match m.pick(1) {
            Outcome::Chosen { action, files, .. } => assert_eq!((action, files), ("convert:docx", vec!["notes.docx".to_string()])),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn hover_shows_hint() {
        let mut m = Menu::default();
        m.reset(files(&["a.png"]));
        assert!(m.hover(Some(2)));
        assert_eq!(m.subtitle(), "Convert to WEBP");
        assert!(!m.hover(Some(2)));
        assert!(m.hover(Some(99)), "out-of-range hover clears the highlight");
        assert_eq!(m.active(), None);
    }

    #[test]
    fn choosing_an_action_starts_a_job_with_only_matching_files() {
        let mut m = Menu::default();
        m.reset(files(&["a.png", "b.mp4", "c.jpg"]));
        m.pick(0); // Image
        let out = m.pick(2); // WEBP
        assert_eq!(
            out,
            Outcome::Chosen { category: CategoryId::Image, action: "convert:webp", files: vec!["a.png".into(), "c.jpg".into()] }
        );
        assert!(m.is_busy() && !m.hides_on_blur());
        assert_eq!((m.title().as_str(), m.subtitle().as_str()), ("Working…", "Convert to WEBP…"));
        assert!(m.items().is_empty());
        assert_eq!(m.key(Key::Digit(1)), Outcome::Unchanged, "no picks while working");
        m.finish(true, "Saved 2 files".into());
        assert!(m.shows_check() && m.hides_on_blur());
        assert_eq!(m.subtitle(), "Saved 2 files");
        assert_eq!(m.key(Key::Escape), Outcome::Close);
    }

    #[test]
    fn merge_is_dimmed_with_one_pdf_and_opens_a_window_with_two() {
        let mut m = Menu::default();
        m.reset(files(&["a.pdf"]));
        let merge = labels(&m).iter().position(|&l| l == "Merge").unwrap();
        assert!(!m.items()[merge].enabled);
        assert_eq!(m.pick(merge), Outcome::Redraw { new_ring: false });
        assert_eq!(m.subtitle(), "Drop 2 or more PDF files");
        assert!(!m.is_busy());

        m.reset(files(&["b.pdf", "a.pdf"]));
        assert_eq!(
            m.pick(merge),
            Outcome::OpenTool {
                kind: ToolKind::Merge,
                category: CategoryId::Pdf,
                action: "merge",
                files: vec!["b.pdf".into(), "a.pdf".into()]
            }
        );
    }

    #[test]
    fn crop_opens_its_window() {
        let mut m = Menu::default();
        m.reset(files(&["a.png"]));
        let crop = labels(&m).iter().position(|&l| l == "Crop").unwrap();
        assert!(matches!(m.pick(crop), Outcome::OpenTool { kind: ToolKind::Crop, .. }));
    }

    #[test]
    fn asking_offers_replace_or_keep_both() {
        let mut m = Menu::default();
        m.reset(files(&["a.png"]));
        m.pick(1);
        m.ask("a.converted.jpg already exists".into());
        assert_eq!(labels(&m), ["Replace", "Keep both"]);
        assert_eq!(m.title(), "Already there");
        assert_eq!(m.subtitle(), "a.converted.jpg already exists");
        assert!(!m.hides_on_blur());
        assert_eq!(m.key(Key::Digit(2)), Outcome::Answered(Answer::KeepBoth));
        assert_eq!(m.title(), "Working…");
    }

    #[test]
    fn failure_shows_the_reason() {
        let mut m = Menu::default();
        m.reset(files(&["a.png"]));
        m.pick(0);
        m.finish(false, "a.png is in a format Morph can't read yet".into());
        assert!(!m.shows_check());
        assert_eq!(m.title(), "Couldn't finish");
        assert!(m.subtitle().contains("can't read"));
    }

    #[test]
    fn keyboard_navigation() {
        let mut m = Menu::default();
        m.reset(files(&["a.png", "b.mp4"]));
        assert_eq!(m.key(Key::Digit(2)), Outcome::Redraw { new_ring: true });
        assert_eq!(m.title(), "Video");
        assert_eq!(m.key(Key::Back), Outcome::Redraw { new_ring: true });
        assert_eq!(m.title(), "Morph");
        m.key(Key::Prev);
        assert_eq!(m.active(), Some(1));
        m.key(Key::Next);
        assert_eq!(m.active(), Some(0));
        assert_eq!(m.key(Key::Digit(9)), Outcome::Unchanged);
    }

    #[test]
    fn drops_merge_without_duplicates() {
        let mut m = Menu::default();
        m.reset(None);
        m.add_files(vec!["holiday.png".into(), "beach.jpg".into()]);
        m.add_files(vec!["clip.mp4".into(), "beach.jpg".into()]);
        assert_eq!(m.subtitle(), "3 files");
        assert_eq!(labels(&m), ["Image", "Video"]);
    }

    #[test]
    fn dragging_hides_ring_and_changes_text() {
        let mut m = Menu::default();
        m.reset(None);
        assert!(m.set_dragging(true));
        assert!(!m.set_dragging(true));
        assert!(m.items().is_empty());
        assert_eq!((m.title().as_str(), m.subtitle().as_str()), ("Let go", "to open them in Morph"));
        m.add_files(vec!["a.png".into()]);
        m.set_dragging(true);
        assert_eq!(m.subtitle(), "to add to these files");
    }

    #[test]
    fn unknown_files_say_nothing_to_convert() {
        let mut m = Menu::default();
        m.reset(files(&["setup.exe", "folder"]));
        assert!(m.items().is_empty());
        assert_eq!(m.subtitle(), "Nothing to convert");
    }
}
