//! Documents through LibreOffice in headless mode, only if the user has it installed
//! (never bundled: AGENTS.md rule 4). Not yet tested end to end on the dev machine,
//! which has no LibreOffice.

use std::path::{Path, PathBuf};

use crate::tools::Tool;

/// LibreOffice's `--convert-to` filter for each target extension.
fn filter(ext: &str) -> Result<&'static str, String> {
    Ok(match ext {
        "pdf" => "pdf",
        "docx" => "docx:MS Word 2007 XML",
        "odt" => "odt",
        "txt" => "txt:Text (encoded):UTF8",
        other => return Err(format!("LibreOffice can't make .{other} files here")),
    })
}

/// A private LibreOffice profile, so conversions work even while the user has
/// LibreOffice open (a shared profile would be locked).
fn profile_url() -> String {
    let dir = std::env::temp_dir().join("morph-libreoffice-profile");
    let path = dir.to_string_lossy().replace('\\', "/");
    if path.starts_with('/') { format!("file://{path}") } else { format!("file:///{path}") }
}

pub fn convert(src: &Path, dst: &Path, ext: &str) -> Result<(), String> {
    let soffice = Tool::LibreOffice.find().ok_or("LibreOffice isn't installed")?;
    // LibreOffice picks the output name itself, so convert into a scratch folder first.
    let scratch: PathBuf = std::env::temp_dir().join(format!(
        "morph-office-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let mut cmd = super::background_command(&soffice);
    cmd.arg(format!("-env:UserInstallation={}", profile_url()))
        .args(["--headless", "--norestore", "--nolockcheck", "--convert-to", filter(ext)?, "--outdir"])
        .arg(&scratch)
        .arg(src);
    let result = super::run_quietly(cmd, "LibreOffice").and_then(|_| {
        let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let produced = scratch.join(format!("{stem}.{ext}"));
        if !produced.is_file() {
            return Err(format!("LibreOffice didn't produce a .{ext} file"));
        }
        // Rename fails across drives (temp is usually on C:), so fall back to copying.
        std::fs::rename(&produced, dst)
            .or_else(|_| std::fs::copy(&produced, dst).map(|_| ()))
            .map_err(|e| format!("Couldn't save {}: {e}", super::name(dst)))
    });
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_for_supported_targets() {
        assert_eq!(filter("pdf").unwrap(), "pdf");
        assert!(filter("docx").unwrap().starts_with("docx"));
        assert!(filter("exe").is_err());
    }

    #[test]
    fn profile_is_a_file_url() {
        assert!(profile_url().starts_with("file:///"), "{}", profile_url());
        assert!(!profile_url().contains('\\'));
    }
}
