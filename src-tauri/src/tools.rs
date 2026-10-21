//! Finds the external programs some engines need. Nothing is downloaded here:
//! a tool is either next to `morph.exe` (bundled sidecar), installed, or missing,
//! and the menu says which.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Ffmpeg,
    LibreOffice,
}

impl Tool {
    fn exe_names(self) -> &'static [&'static str] {
        match self {
            // The installer's copy is `morph-ffmpeg`, so it never clashes with (or shadows)
            // a system `ffmpeg`, e.g. both in /usr/bin with the .deb.
            Tool::Ffmpeg => &["morph-ffmpeg", "ffmpeg"],
            Tool::LibreOffice => &["soffice", "libreoffice"],
        }
    }

    /// Usual install folders that aren't on PATH.
    fn known_dirs(self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if self == Tool::LibreOffice {
            for var in ["ProgramFiles", "ProgramFiles(x86)"] {
                if let Some(base) = std::env::var_os(var) {
                    dirs.push(Path::new(&base).join("LibreOffice").join("program"));
                }
            }
            dirs.push("/usr/lib/libreoffice/program".into());
            dirs.push("/opt/libreoffice/program".into());
        }
        dirs
    }

    pub fn find(self) -> Option<PathBuf> {
        let exe = |dir: &Path, name: &str| {
            let p = dir.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() });
            p.is_file().then_some(p)
        };
        // 1. Bundled next to Morph (release builds).
        let beside = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
        // 2. PATH, 3. known install folders.
        let path_dirs = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
        beside
            .into_iter()
            .chain(path_dirs)
            .chain(self.known_dirs())
            .find_map(|dir| self.exe_names().iter().find_map(|n| exe(&dir, n)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_tool_is_none_not_a_panic() {
        // Whatever is installed on the test machine, lookup must not panic.
        let _ = Tool::Ffmpeg.find();
        let _ = Tool::LibreOffice.find();
    }
}
