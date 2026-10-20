//! What the orb shows and how it reacts to input, independent of any window or renderer.
//! Port of the logic in `src/routes/overlay/+page.svelte` + `RadialMenu.svelte`.

use super::catalog::{self, Category};

/// One slice of the ring.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: &'static str,
    pub hint: String,
    pub hue: f32,
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
    /// An action was chosen: show the confirmation, then close shortly after.
    Chosen { category: catalog::CategoryId, action: &'static str },
    Close,
}

#[derive(Debug, Default)]
pub struct Menu {
    /// `None` = the orb is waiting for a drop.
    files: Option<Vec<String>>,
    /// Chosen category (index into `available()`), when there is more than one.
    category: Option<usize>,
    done: Option<String>,
    dragging: bool,
    active: Option<usize>,
}

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

    pub fn is_done(&self) -> bool {
        self.done.is_some()
    }

    pub fn dragging(&self) -> bool {
        self.dragging
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
        if self.done.is_some() || self.dragging {
            return Vec::new();
        }
        match self.current() {
            Some(c) => c
                .actions
                .iter()
                .map(|a| Item { label: a.label, hint: a.hint.to_string(), hue: c.hue })
                .collect(),
            None => self
                .available()
                .iter()
                .map(|c| Item { label: c.label, hint: format!("{} actions", c.actions.len()), hue: c.hue })
                .collect(),
        }
    }

    pub fn active(&self) -> Option<usize> {
        self.active
    }

    pub fn can_go_back(&self) -> bool {
        self.category.is_some() && self.available().len() > 1 && self.done.is_none() && !self.dragging
    }

    pub fn title(&self) -> String {
        if self.dragging {
            "Let go".into()
        } else if self.done.is_some() {
            "✓".into()
        } else if self.files.is_none() {
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
        if let Some(done) = &self.done {
            return done.clone();
        }
        let Some(files) = &self.files else { return "or press Esc".into() };
        if let Some(item) = self.active.and_then(|a| self.items().into_iter().nth(a)) {
            return item.hint;
        }
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
        if index >= items.len() {
            return Outcome::Unchanged;
        }
        match self.current() {
            None => {
                self.category = Some(index);
                self.active = None;
                Outcome::Redraw { new_ring: true }
            }
            Some(c) => {
                let action = &c.actions[index];
                // Step 4 runs the real job; for now the orb just confirms the choice.
                self.done = Some(format!("{} · preview", action.hint));
                self.active = None;
                Outcome::Chosen { category: c.id, action: action.id }
            }
        }
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
        if self.done.is_some() {
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
    fn hover_shows_hint() {
        let mut m = Menu::default();
        m.reset(files(&["a.png"]));
        assert!(m.hover(Some(4)));
        assert_eq!(m.subtitle(), "Resize by width or percentage");
        assert!(!m.hover(Some(4)));
        assert!(m.hover(Some(99)), "out-of-range hover clears the highlight");
        assert_eq!(m.active(), None);
    }

    #[test]
    fn keyboard_flow() {
        let mut m = Menu::default();
        m.reset(files(&["a.png", "b.mp4"]));
        assert_eq!(m.key(Key::Digit(2)), Outcome::Redraw { new_ring: true });
        assert_eq!(m.title(), "Video");
        assert_eq!(m.key(Key::Back), Outcome::Redraw { new_ring: true });
        assert_eq!(m.title(), "Morph");
        m.key(Key::Digit(2));
        assert_eq!(
            m.key(Key::Digit(3)),
            Outcome::Chosen { category: catalog::CategoryId::Video, action: "convert:gif" }
        );
        assert!(m.is_done());
        assert_eq!(m.subtitle(), "Convert to GIF · preview");
        assert_eq!(m.key(Key::Digit(1)), Outcome::Unchanged, "done state ignores further picks");
        assert_eq!(m.key(Key::Escape), Outcome::Close);
    }

    #[test]
    fn arrows_wrap_and_enter_picks() {
        let mut m = Menu::default();
        m.reset(files(&["d.pdf"]));
        m.key(Key::Prev);
        assert_eq!(m.active(), Some(4));
        m.key(Key::Next);
        assert_eq!(m.active(), Some(0));
        assert!(matches!(m.key(Key::Enter), Outcome::Chosen { action: "merge", .. }));
    }

    #[test]
    fn digits_beyond_ring_are_ignored() {
        let mut m = Menu::default();
        m.reset(files(&["d.pdf"]));
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
