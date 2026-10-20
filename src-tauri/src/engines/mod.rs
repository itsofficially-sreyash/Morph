//! Engines do the actual conversions. Each writes to the destination it is given (the job
//! runner picks it with `Settings::output_path` and handles overwrite questions), and
//! never touches the original.

pub mod ffmpeg;
pub mod image;
pub mod office;
pub mod pdf;

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::actions::CategoryId;

pub fn name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// Half the logical cores, at least one (AGENTS.md rule 10).
pub fn half_cores() -> usize {
    std::thread::available_parallelism().map(|n| (n.get() / 2).max(1)).unwrap_or(1)
}

/// Actions that turn all dropped files into one output.
pub fn is_combined(category: CategoryId, action: &str) -> bool {
    category == CategoryId::Image && action == "to-pdf"
}

/// How many files of this action may be processed at once. In-process image work is
/// single-threaded per file; external encoders already use several threads each.
pub fn max_parallel(category: CategoryId) -> usize {
    match category {
        CategoryId::Image => half_cores(),
        _ => 1,
    }
}

/// Extension of the file the action produces for `src`.
pub fn output_ext(category: CategoryId, action: &str, src: &Path) -> String {
    if let Some(target) = action.strip_prefix("convert:") {
        return target.to_string();
    }
    match (category, action) {
        (CategoryId::Image, "compress") => image::shrink_ext(src).to_string(),
        (_, "to-pdf") => "pdf".into(),
        (CategoryId::Video | CategoryId::Audio, _) => ffmpeg::output_ext(action, src),
        _ => src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default(),
    }
}

pub enum Done {
    Saved,
    /// "Shrink" made a file that wasn't smaller; the caller deletes it.
    NotSmaller,
}

pub fn run_one(category: CategoryId, action: &str, src: &Path, dst: &Path) -> Result<Done, String> {
    let ext = output_ext(category, action, src);
    match category {
        CategoryId::Image if action == "compress" => {
            Ok(if image::shrink(src, dst)? { Done::Saved } else { Done::NotSmaller })
        }
        CategoryId::Image => image::convert(src, dst, &ext).map(|_| Done::Saved),
        CategoryId::Video | CategoryId::Audio => ffmpeg::run(category, action, src, dst).map(|_| Done::Saved),
        CategoryId::Document => office::convert(src, dst, &ext).map(|_| Done::Saved),
        CategoryId::Pdf => Err("The PDF tools are coming soon".into()),
    }
}

pub fn run_combined(category: CategoryId, action: &str, srcs: &[PathBuf], dst: &Path) -> Result<(), String> {
    match (category, action) {
        (CategoryId::Image, "to-pdf") => pdf::images_to_pdf(srcs, dst),
        _ => Err(format!("{action} doesn't combine files")),
    }
}

/// A command for a background conversion: below-normal priority, and on Windows no
/// console window flashing up.
pub fn background_command(program: &Path) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        let mut cmd = Command::new(program);
        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
        cmd
    }
    #[cfg(not(windows))]
    {
        if Path::new("/usr/bin/nice").is_file() {
            let mut cmd = Command::new("/usr/bin/nice");
            cmd.args(["-n", "10"]).arg(program);
            cmd
        } else {
            Command::new(program)
        }
    }
}

/// Runs `cmd`, turning a failure into its last stderr line.
pub fn run_quietly(mut cmd: Command, what: &str) -> Result<(), String> {
    let out = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Couldn't start {what}: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error").trim();
    Err(format!("{what} failed: {last}"))
}

/// Lowers the current thread's priority, for in-process conversions.
pub fn lower_thread_priority() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_extensions() {
        let p = Path::new("x.PNG");
        assert_eq!(output_ext(CategoryId::Image, "convert:webp", p), "webp");
        assert_eq!(output_ext(CategoryId::Image, "compress", p), "png");
        assert_eq!(output_ext(CategoryId::Image, "to-pdf", p), "pdf");
        assert_eq!(output_ext(CategoryId::Video, "extract-audio", Path::new("c.mov")), "mp3");
        assert_eq!(output_ext(CategoryId::Video, "mute", Path::new("c.MOV")), "mov");
        assert_eq!(output_ext(CategoryId::Document, "to-pdf", Path::new("r.docx")), "pdf");
    }

    #[test]
    fn half_cores_is_at_least_one() {
        assert!(half_cores() >= 1);
    }
}
