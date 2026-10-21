//! Engines do the actual conversions. Each writes to the destination it is given (the job
//! runner picks it with `Settings::output_path` and handles overwrite questions), and
//! never touches the original.

pub mod ffmpeg;
pub mod image;
pub mod office;
pub mod pdf;
pub mod pdf_render;
pub mod pdf_tools;

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::actions::CategoryId;

pub fn name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// Half the logical cores, at least one, so conversions never make the computer laggy.
pub fn half_cores() -> usize {
    std::thread::available_parallelism().map(|n| (n.get() / 2).max(1)).unwrap_or(1)
}

/// Extra input collected by a tool window (`tools.rs` windows: Split, Resize, Crop).
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Params {
    #[default]
    None,
    /// Inclusive, 1-based page ranges; empty = every page.
    Split { ranges: Vec<(u32, u32)> },
    /// A percentage, or a width and/or height in pixels (one of them alone keeps the
    /// proportions; both is an exact size). Applied to each image on its own.
    Resize {
        #[serde(default)]
        percent: Option<f32>,
        #[serde(default)]
        width: Option<u32>,
        #[serde(default)]
        height: Option<u32>,
    },
    /// Region in the original image's pixels.
    Crop { x: u32, y: u32, width: u32, height: u32 },
}

/// Actions that turn all the files into one output (in the given order).
pub fn is_combined(category: CategoryId, action: &str) -> bool {
    matches!((category, action), (CategoryId::Image, "to-pdf") | (CategoryId::Pdf, "merge"))
}

/// Actions that turn one file into several outputs.
pub fn is_multi(category: CategoryId, action: &str) -> bool {
    matches!((category, action), (CategoryId::Pdf, "split" | "to-images"))
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
        (CategoryId::Image, "resize" | "crop") => image::edit_ext(src).to_string(),
        (_, "to-pdf") => "pdf".into(),
        (CategoryId::Pdf, "to-images") => "png".into(),
        (CategoryId::Pdf, _) => "pdf".into(),
        (CategoryId::Video | CategoryId::Audio, _) => ffmpeg::output_ext(action, src),
        _ => src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default(),
    }
}

pub enum Done {
    Saved,
    /// "Shrink" made a file that wasn't smaller; the caller deletes it.
    NotSmaller,
}

pub fn run_one(category: CategoryId, action: &str, src: &Path, dst: &Path, params: &Params) -> Result<Done, String> {
    let ext = output_ext(category, action, src);
    let saved = |r: Result<(), String>| r.map(|_| Done::Saved);
    let shrunk = |r: Result<bool, String>| r.map(|smaller| if smaller { Done::Saved } else { Done::NotSmaller });
    match (category, action, params) {
        (CategoryId::Image, "compress", _) => shrunk(image::shrink(src, dst)),
        (CategoryId::Image, "resize", Params::Resize { percent, width, height }) => {
            saved(image::resize(src, dst, image::ResizeSpec { percent: *percent, width: *width, height: *height }))
        }
        (CategoryId::Image, "crop", Params::Crop { x, y, width, height }) => {
            saved(image::crop(src, dst, (*x, *y, *width, *height)))
        }
        (CategoryId::Image, "resize" | "crop", _) => Err(format!("{action} needs its window")),
        (CategoryId::Image, _, _) => saved(image::convert(src, dst, &ext)),
        (CategoryId::Video | CategoryId::Audio, _, _) => saved(ffmpeg::run(category, action, src, dst)),
        (CategoryId::Document, _, _) => saved(office::convert(src, dst, &ext)),
        (CategoryId::Pdf, "compress", _) => shrunk(pdf_tools::shrink(src, dst)),
        (CategoryId::Pdf, "rotate", _) => saved(pdf_tools::rotate(src, dst)),
        (CategoryId::Pdf, _, _) => Err(format!("{action} isn't a single-file PDF action")),
    }
}

pub fn run_combined(category: CategoryId, action: &str, srcs: &[PathBuf], dst: &Path) -> Result<(), String> {
    match (category, action) {
        (CategoryId::Image, "to-pdf") => pdf::images_to_pdf(srcs, dst),
        (CategoryId::Pdf, "merge") => pdf_tools::merge(srcs, dst),
        _ => Err(format!("{action} doesn't combine files")),
    }
}

/// Runs an action that writes several files. `dst_for(label)` gives the path for each
/// piece, e.g. "pages 1-3" or "page 2".
pub fn run_multi(
    category: CategoryId,
    action: &str,
    src: &Path,
    params: &Params,
    dst_for: &dyn Fn(&str) -> PathBuf,
) -> Result<Vec<PathBuf>, String> {
    match (category, action) {
        (CategoryId::Pdf, "split") => {
            let ranges = match params {
                Params::Split { ranges } if !ranges.is_empty() => ranges.clone(),
                _ => pdf_tools::every_page(pdf_tools::page_count(src)?),
            };
            pdf_tools::split(src, &ranges, |r| dst_for(&pdf_tools::range_label(r)))
        }
        (CategoryId::Pdf, "to-images") => pdf_render::render(src, |n| dst_for(&format!("page {n}"))),
        _ => Err(format!("{action} doesn't make several files")),
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
pub fn run_quietly(cmd: Command, what: &str) -> Result<(), String> {
    run(cmd, what, false).map(|_| ())
}

/// Like `run_quietly`, but kills the program if it runs longer than `limit` (Office can
/// hang on an unexpected dialog). Polls only while the job runs.
pub fn run_with_timeout(mut cmd: Command, what: &str, limit: std::time::Duration) -> Result<(), String> {
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't start {what}: {e}"))?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => {
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    use std::io::Read;
                    let _ = pipe.read_to_string(&mut stderr);
                }
                if status.success() {
                    return Ok(());
                }
                return Err(format!("{what} failed: {}", readable_error(&stderr)));
            }
            None if started.elapsed() > limit => {
                let _ = child.kill();
                return Err(format!("{what} took too long and was stopped"));
            }
            None => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
}

/// The useful line of a tool's stderr. PowerShell writes errors as CLIXML when its output
/// is redirected (`#< CLIXML <Objs …><S S="Error">…</S>`); this pulls the message out.
fn readable_error(stderr: &str) -> String {
    if stderr.contains("<Objs") {
        let mut parts = Vec::new();
        let mut rest = stderr;
        while let Some(start) = rest.find("<S S=\"Error\">") {
            let after = &rest[start + 13..];
            let end = after.find("</S>").unwrap_or(after.len());
            parts.push(after[..end].replace("_x000D__x000A_", " "));
            rest = &after[end..];
        }
        let text = parts.concat().split_whitespace().collect::<Vec<_>>().join(" ");
        // The first sentence is the error; the rest is PowerShell's position info.
        let first = text.split(" At line:").next().unwrap_or(&text).trim().to_string();
        if !first.is_empty() {
            return first;
        }
    }
    stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error").trim().to_string()
}

/// Like `run_quietly`, but returns what the program wrote to stdout.
pub fn run_capture(cmd: Command, what: &str) -> Result<Vec<u8>, String> {
    run(cmd, what, true)
}

fn run(mut cmd: Command, what: &str, capture: bool) -> Result<Vec<u8>, String> {
    let stdout = if capture { std::process::Stdio::piped() } else { std::process::Stdio::null() };
    let out = cmd
        .stdin(std::process::Stdio::null())
        .stdout(stdout)
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Couldn't start {what}: {e}"))?;
    if out.status.success() {
        return Ok(out.stdout);
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
    fn powershell_clixml_errors_become_one_line() {
        let clixml = r#"#< CLIXML
<Objs Version="1.1.0.1" xmlns="http://schemas.microsoft.com/powershell/2004/04"><S S="Error">Argument: '1' should be a System.Management.Automation.PSReference. Use [ref]._x000D__x000A_</S><S S="Error">At line:18 char:62_x000D__x000A_</S></Objs>"#;
        assert_eq!(
            readable_error(clixml),
            "Argument: '1' should be a System.Management.Automation.PSReference. Use [ref]."
        );
        assert_eq!(readable_error("first\nlast line\n\n"), "last line");
    }

    #[test]
    fn half_cores_is_at_least_one() {
        assert!(half_cores() >= 1);
    }
}
