//! Finds the external programs some engines need. Nothing is downloaded here:
//! a tool is either next to `morph.exe` (bundled sidecar, step 7), installed, or missing,
//! and the menu says which.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Ffmpeg,
    #[allow(dead_code)] // the PDF actions switch to it when the qpdf sidecar ships (step 7)
    Qpdf,
    LibreOffice,
}

impl Tool {
    fn exe_names(self) -> &'static [&'static str] {
        match self {
            Tool::Ffmpeg => &["ffmpeg"],
            Tool::Qpdf => &["qpdf"],
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
        let _ = Tool::Qpdf.find();
        let _ = Tool::LibreOffice.find();
    }
}
