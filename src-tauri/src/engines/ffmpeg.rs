//! Video and audio through FFmpeg (installed, or bundled next to Morph in release builds).

use std::ffi::OsString;
use std::path::Path;

use crate::actions::CategoryId;
use crate::tools::Tool;

fn src_ext(src: &Path) -> String {
    src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn output_ext(action: &str, src: &Path) -> String {
    match action {
        "compress" => "mp4".into(),
        "extract-audio" => "mp3".into(),
        a if a.starts_with("convert:") => a["convert:".len()..].into(),
        // mute, normalize: same container as the input.
        _ => src_ext(src),
    }
}

/// Codec options for each action (everything between the input and the output file).
fn options(category: CategoryId, action: &str) -> Result<Vec<&'static str>, String> {
    Ok(match (category, action) {
        (CategoryId::Video, "convert:mp4") => vec![
            "-c:v", "libx264", "-preset", "medium", "-crf", "23", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "160k",
            "-movflags", "+faststart",
        ],
        (CategoryId::Video, "convert:webm") => {
            vec!["-c:v", "libvpx-vp9", "-crf", "32", "-b:v", "0", "-row-mt", "1", "-c:a", "libopus", "-b:a", "128k"]
        }
        (CategoryId::Video, "convert:gif") => vec![
            "-an",
            "-vf",
            "fps=12,scale='min(640,iw)':-2:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse",
            "-loop",
            "0",
        ],
        (CategoryId::Video, "compress") => vec![
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "28", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "128k",
            "-movflags", "+faststart",
        ],
        (CategoryId::Video, "extract-audio") => vec!["-vn", "-c:a", "libmp3lame", "-q:a", "2"],
        (CategoryId::Video, "mute") => vec!["-an", "-c:v", "copy"],
        (CategoryId::Audio, "convert:mp3") => vec!["-vn", "-c:a", "libmp3lame", "-q:a", "2"],
        (CategoryId::Audio, "convert:wav") => vec!["-vn", "-c:a", "pcm_s16le"],
        (CategoryId::Audio, "convert:flac") => vec!["-vn", "-c:a", "flac"],
        (CategoryId::Audio, "convert:m4a") => vec!["-vn", "-c:a", "aac", "-b:a", "192k"],
        (CategoryId::Audio, "normalize") => vec!["-vn", "-af", "loudnorm=I=-16:TP=-1.5:LRA=11"],
        _ => return Err(format!("FFmpeg can't do {action} yet")),
    })
}

pub fn args(category: CategoryId, action: &str, src: &Path, dst: &Path, threads: usize) -> Result<Vec<OsString>, String> {
    let mut a: Vec<OsString> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"].map(OsString::from).to_vec();
    a.push(src.into());
    a.extend(options(category, action)?.into_iter().map(OsString::from));
    // Cap encoder threads at half the cores so a long encode can't make the PC laggy.
    a.extend(["-threads".into(), threads.to_string().into()]);
    a.push(dst.into());
    Ok(a)
}

pub fn run(category: CategoryId, action: &str, src: &Path, dst: &Path) -> Result<(), String> {
    let ffmpeg = Tool::Ffmpeg.find().ok_or("FFmpeg isn't installed")?;
    let mut cmd = super::background_command(&ffmpeg);
    cmd.args(args(category, action, src, dst, super::half_cores())?);
    super::run_quietly(cmd, "FFmpeg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_put_input_first_and_output_last() {
        let a = args(CategoryId::Video, "convert:gif", Path::new("in.mp4"), Path::new("out.gif"), 4).unwrap();
        let i = a.iter().position(|x| x == "-i").unwrap();
        assert_eq!(a[i + 1], "in.mp4");
        assert_eq!(a.last().unwrap(), "out.gif");
        assert!(a.windows(2).any(|w| w[0] == "-threads" && w[1] == "4"));
    }

    #[test]
    fn every_ffmpeg_action_in_the_registry_has_options() {
        use crate::actions::{Engine, CATEGORIES};
        // Image actions that use FFmpeg (AVIF) go through `engines::image` instead.
        for c in CATEGORIES.iter().filter(|c| matches!(c.id, CategoryId::Video | CategoryId::Audio)) {
            for act in c.actions {
                if act.engine == Engine::External(Tool::Ffmpeg) {
                    assert!(options(c.id, act.id).is_ok(), "{:?} {} has no FFmpeg options", c.id, act.id);
                }
            }
        }
    }

    /// End-to-end with the real FFmpeg, if it's installed: make a 1 s test clip, then
    /// convert it. Skipped (not failed) on machines without FFmpeg.
    #[test]
    fn real_conversions_when_ffmpeg_is_available() {
        let Some(ff) = Tool::Ffmpeg.find() else {
            eprintln!("FFmpeg not found; skipping");
            return;
        };
        let d = std::env::temp_dir().join("morph-ffmpeg-tests");
        std::fs::create_dir_all(&d).unwrap();
        let clip = d.join("clip.mp4");
        let status = std::process::Command::new(&ff)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=10:duration=1"])
            .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-shortest"])
            .arg(&clip)
            .status()
            .unwrap();
        assert!(status.success());
        for (action, ext) in [("convert:gif", "gif"), ("extract-audio", "mp3"), ("mute", "mp4")] {
            let out = d.join(format!("out-{}.{ext}", action.replace(':', "-")));
            run(CategoryId::Video, action, &clip, &out).unwrap_or_else(|e| panic!("{action}: {e}"));
            assert!(std::fs::metadata(&out).unwrap().len() > 0, "{action} produced an empty file");
        }
        let mp3 = d.join("out-extract-audio.mp3");
        let wav = d.join("back.wav");
        run(CategoryId::Audio, "convert:wav", &mp3, &wav).unwrap();
        assert!(std::fs::metadata(&wav).unwrap().len() > 1000);
    }
}
