//! The "Convert with Morph" entry in the file manager's right-click menu. It runs
//! `morph --open <file>`, which hands the file to the running Morph (`cli.rs`).
//!
//! Like start-at-sign-in, it records this executable's path, so debug builds never
//! register themselves, and each launch of a release build re-points the entry (e.g. after
//! an update). `morph --remove-integration` (run by the uninstaller) removes it.

#[cfg(not(windows))]
mod linux;
#[cfg(windows)]
mod win;

#[cfg(not(windows))]
use linux as platform;
#[cfg(windows)]
use win as platform;

use crate::actions::CATEGORIES;

/// Every extension Morph has actions for, lowercase, without the dot.
pub fn extensions() -> Vec<&'static str> {
    let mut all: Vec<&str> = CATEGORIES.iter().flat_map(|c| c.extensions.iter().copied()).collect();
    all.sort_unstable();
    all.dedup();
    all
}

/// Adds or removes the entry to match `enabled`.
pub fn apply(enabled: bool) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if enabled { platform::install(&exe) } else { platform::remove() }
}

/// Removes the entry whatever the build (for the uninstaller and for tests).
pub fn remove() -> Result<(), String> {
    platform::remove()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_list_is_unique_and_covers_every_category() {
        let exts = extensions();
        let mut sorted = exts.clone();
        sorted.dedup();
        assert_eq!(exts, sorted);
        for ext in ["png", "heic", "mp4", "mp3", "pdf", "docx", "xlsx"] {
            assert!(exts.contains(&ext), "{ext} missing");
        }
    }
}
