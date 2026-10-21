//! Video and audio through FFmpeg (installed, or bundled next to Morph in release builds).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

/// H.264 encoders Morph knows, best first. The bundled FFmpeg is an LGPL build, so it has
/// no libx264 (GPL); a user's own FFmpeg often does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H264 {
    X264,
    /// Windows' own encoder (Media Foundation; hardware when the GPU has one).
    MediaFoundation,
    OpenH264,
}

impl H264 {
    const ALL: [(H264, &'static str); 3] =
        [(H264::X264, "libx264"), (H264::MediaFoundation, "h264_mf"), (H264::OpenH264, "libopenh264")];

    /// Picks from `ffmpeg -encoders` output.
    fn pick(encoders: &str) -> Option<H264> {
        let names: Vec<&str> = encoders.lines().filter_map(|l| l.split_whitespace().nth(1)).collect();
        Self::ALL.iter().find(|(_, name)| names.contains(name)).map(|(e, _)| *e)
    }

    /// Video options for an MP4 at normal quality, or smaller for Shrink. The
    /// Media Foundation values were matched to x264's by SSIM and file size on a 720p clip.
    fn options(self, compress: bool) -> &'static [&'static str] {
        match (self, compress) {
            (H264::X264, false) => &["-c:v", "libx264", "-preset", "medium", "-crf", "23"],
            (H264::X264, true) => &["-c:v", "libx264", "-preset", "veryfast", "-crf", "28"],
            (H264::MediaFoundation, false) => &["-c:v", "h264_mf", "-rate_control", "quality", "-quality", "60"],
            (H264::MediaFoundation, true) => &["-c:v", "h264_mf", "-rate_control", "quality", "-quality", "35"],
            // Not tested yet (no FFmpeg with OpenH264 on the dev machines).
            (H264::OpenH264, false) => &["-c:v", "libopenh264", "-rc_mode", "quality", "-b:v", "5M"],
            (H264::OpenH264, true) => &["-c:v", "libopenh264", "-rc_mode", "quality", "-b:v", "1500k"],
        }
    }
}

/// The H.264 encoder `ffmpeg` has, asked once per FFmpeg (one short run, at the first
/// MP4 job only).
fn h264_encoder(ffmpeg: &Path) -> Option<H264> {
    static CACHE: Mutex<Option<(PathBuf, Option<H264>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, enc)) = cache.as_ref().filter(|(p, _)| p == ffmpeg) {
        return *enc;
    }
    let mut cmd = super::background_command(ffmpeg);
    cmd.args(["-hide_banner", "-encoders"]);
    let enc = super::run_capture(cmd, "FFmpeg").ok().and_then(|out| H264::pick(&String::from_utf8_lossy(&out)));
    *cache = Some((ffmpeg.to_path_buf(), enc));
    enc
}

fn needs_h264(category: CategoryId, action: &str) -> bool {
    category == CategoryId::Video && matches!(action, "convert:mp4" | "compress")
}

/// Codec options for each action (everything between the input and the output file).
fn options(category: CategoryId, action: &str, h264: Option<H264>) -> Result<Vec<&'static str>, String> {
    let mp4 = |compress: bool, audio: &'static str| -> Result<Vec<&'static str>, String> {
        let enc = h264.ok_or("This FFmpeg has no H.264 encoder")?;
        let mut o = enc.options(compress).to_vec();
        o.extend(["-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", audio, "-movflags", "+faststart"]);
        Ok(o)
    };
    Ok(match (category, action) {
        (CategoryId::Video, "convert:mp4") => mp4(false, "160k")?,
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
        (CategoryId::Video, "compress") => mp4(true, "128k")?,
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

pub fn args(
    category: CategoryId,
    action: &str,
    src: &Path,
    dst: &Path,
    threads: usize,
    h264: Option<H264>,
) -> Result<Vec<OsString>, String> {
    let mut a: Vec<OsString> = ["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"].map(OsString::from).to_vec();
    a.push(src.into());
    a.extend(options(category, action, h264)?.into_iter().map(OsString::from));
    // Cap encoder threads at half the cores so a long encode can't make the PC laggy.
    a.extend(["-threads".into(), threads.to_string().into()]);
    a.push(dst.into());
    Ok(a)
}

pub fn run(category: CategoryId, action: &str, src: &Path, dst: &Path) -> Result<(), String> {
    let ffmpeg = Tool::Ffmpeg.find().ok_or("FFmpeg isn't installed")?;
    let h264 = if needs_h264(category, action) { h264_encoder(&ffmpeg) } else { None };
    let mut cmd = super::background_command(&ffmpeg);
    cmd.args(args(category, action, src, dst, super::half_cores(), h264)?);
    super::run_quietly(cmd, "FFmpeg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_put_input_first_and_output_last() {
        let a = args(CategoryId::Video, "convert:gif", Path::new("in.mp4"), Path::new("out.gif"), 4, None).unwrap();
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
                    assert!(options(c.id, act.id, Some(H264::MediaFoundation)).is_ok(), "{:?} {} has no FFmpeg options", c.id, act.id);
                }
            }
        }
    }

    #[test]
    fn picks_the_best_h264_encoder_listed() {
        let lgpl = " V....D h264_amf             AMD AMF H.264 Encoder (codec h264)
 V....D h264_mf              H264 via MediaFoundation (codec h264)
";
        assert_eq!(H264::pick(lgpl), Some(H264::MediaFoundation));
        let gpl = format!("{lgpl} V....D libx264              libx264 H.264 (codec h264)
");
        assert_eq!(H264::pick(&gpl), Some(H264::X264));
        assert_eq!(H264::pick(" V....D libx264rgb  libx264 RGB
"), None);
        // Without an encoder, MP4 says why instead of running FFmpeg.
        assert!(options(CategoryId::Video, "convert:mp4", None).is_err());
        assert!(options(CategoryId::Video, "convert:webm", None).is_ok());
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
        for (action, ext) in [("convert:gif", "gif"), ("extract-audio", "mp3"), ("mute", "mp4"), ("convert:mp4", "mp4"), ("compress", "mp4")] {
            let out = d.join(format!("out-{}.{ext}", action.replace(':', "-")));
            run(CategoryId::Video, action, &clip, &out).unwrap_or_else(|e| panic!("{action}: {e}"));
            assert!(std::fs::metadata(&out).unwrap().len() > 0, "{action} produced an empty file");
        }
        // Windows' own encoder, which the bundled LGPL FFmpeg uses, whatever this FFmpeg prefers.
        let mut list = super::super::background_command(&ff);
        list.args(["-hide_banner", "-encoders"]);
        let encoders = String::from_utf8_lossy(&super::super::run_capture(list, "FFmpeg").unwrap()).into_owned();
        if encoders.contains(" h264_mf ") {
            for action in ["convert:mp4", "compress"] {
                let out = d.join(format!("mf-{}.mp4", action.replace(':', "-")));
                let mut cmd = std::process::Command::new(&ff);
                cmd.args(args(CategoryId::Video, action, &clip, &out, 2, Some(H264::MediaFoundation)).unwrap());
                assert!(cmd.status().unwrap().success(), "h264_mf {action}");
                assert!(std::fs::metadata(&out).unwrap().len() > 0);
            }
        }
        let mp3 = d.join("out-extract-audio.mp3");
        let wav = d.join("back.wav");
        run(CategoryId::Audio, "convert:wav", &mp3, &wav).unwrap();
        assert!(std::fs::metadata(&wav).unwrap().len() > 1000);
    }
}
