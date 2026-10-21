//! Small windows for actions that need more input than a click: Merge (order the PDFs),
//! Split (page ranges), Resize (size or %), Crop (draw a box). Each is a web window
//! created on demand and destroyed on close (nothing stays in memory), running `src/routes/tool`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::actions::{self, CategoryId, ToolKind};
use crate::engines::{self, pdf_tools, Params};
use crate::jobs::{self, Answer, Request, Summary};
use crate::settings;

struct Pending {
    kind: ToolKind,
    category: CategoryId,
    action: &'static str,
    files: Vec<String>,
}

/// What each open tool window was opened for, by window label.
#[derive(Default)]
pub struct ToolState(Mutex<HashMap<String, Pending>>);

static NEXT: AtomicU32 = AtomicU32::new(1);

pub fn open<R: Runtime>(app: &AppHandle<R>, kind: ToolKind, category: CategoryId, action: &'static str, files: Vec<String>) {
    let label = format!("tool-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let (title, w, h, resizable) = match kind {
        ToolKind::Merge => ("Merge PDFs", 480.0, 540.0, true),
        ToolKind::Split => ("Split PDF", 460.0, 420.0, false),
        ToolKind::Resize => ("Resize", 440.0, 440.0, false),
        ToolKind::Crop => ("Crop", 960.0, 720.0, true),
    };
    app.state::<ToolState>().0.lock().unwrap().insert(label.clone(), Pending { kind, category, action, files });
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("/tool".into()))
        .title(format!("Morph · {title}"))
        .inner_size(w, h)
        .min_inner_size(360.0, 320.0)
        .resizable(resizable)
        .center()
        .focused(true)
        .build();
    if let Err(e) = built {
        app.state::<ToolState>().0.lock().unwrap().remove(&label);
        eprintln!("tool window: {e}");
    }
}

/// Forgets a tool window's request when it closes.
pub fn closed<R: Runtime>(app: &AppHandle<R>, label: &str) {
    app.state::<ToolState>().0.lock().unwrap().remove(label);
}

#[derive(Serialize)]
pub struct FileInfo {
    path: String,
    name: String,
    /// PDFs: page count. Images: pixel size.
    pages: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
    /// Why this file can't be used (e.g. a password-protected PDF).
    error: Option<String>,
}

#[derive(Serialize)]
pub struct ToolInfo {
    kind: ToolKind,
    files: Vec<FileInfo>,
}

fn info(kind: ToolKind, path: &str) -> FileInfo {
    let p = Path::new(path);
    let mut f = FileInfo {
        path: path.into(),
        name: engines::name(p),
        pages: None,
        width: None,
        height: None,
        error: None,
    };
    match kind {
        ToolKind::Merge | ToolKind::Split => match pdf_tools::page_count(p) {
            Ok(n) => f.pages = Some(n),
            Err(e) => f.error = Some(e),
        },
        ToolKind::Resize | ToolKind::Crop => match image_size(p) {
            Ok((w, h)) => (f.width, f.height) = (Some(w), Some(h)),
            Err(e) => f.error = Some(e),
        },
    }
    f
}

/// Reads just the header where possible; HEIC/AVIF need a full decode.
fn image_size(p: &Path) -> Result<(u32, u32), String> {
    image::image_dimensions(p).or_else(|_| {
        let img = engines::image::open(p)?;
        Ok((img.width(), img.height()))
    })
}

#[tauri::command]
pub async fn tool_info(window: WebviewWindow, state: tauri::State<'_, ToolState>) -> Result<ToolInfo, String> {
    let (kind, files) = {
        let map = state.0.lock().unwrap();
        let p = map.get(window.label()).ok_or("This window has nothing to work on")?;
        (p.kind, p.files.clone())
    };
    tauri::async_runtime::spawn_blocking(move || ToolInfo { kind, files: files.iter().map(|f| info(kind, f)).collect() })
        .await
        .map_err(|e| e.to_string())
}

/// Live check of a ranges text like "1-3, 5" for the Split window.
#[tauri::command]
pub fn tool_parse_ranges(text: String, pages: u32) -> Result<Vec<(u32, u32)>, String> {
    pdf_tools::parse_ranges(&text, pages)
}

/// A downscaled JPEG of the image as a data URL, for the Crop window. Works for HEIC too,
/// which the web view can't show itself.
#[tauri::command]
pub async fn tool_preview(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let img = engines::image::open(Path::new(&path))?;
        let small = img.thumbnail(1600, 1600);
        drop(img);
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85)
            .encode_image(&engines::image::flatten(&small))
            .map_err(|e| e.to_string())?;
        Ok(format!("data:image/jpeg;base64,{}", crate::b64::encode(&jpeg)))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Runs the window's action on `files` (in this order) with `params`. Outputs never
/// replace existing files here: they get a numbered name instead.
#[tauri::command]
pub async fn tool_run(
    app: AppHandle,
    window: WebviewWindow,
    state: tauri::State<'_, ToolState>,
    files: Vec<String>,
    params: Params,
) -> Result<Summary, String> {
    let (category, action, allowed) = {
        let map = state.0.lock().unwrap();
        let p = map.get(window.label()).ok_or("This window has nothing to work on")?;
        (p.category, p.action, p.files.clone())
    };
    // Only the files the window was opened with (in any order, any subset).
    if let Some(f) = files.iter().find(|f| !allowed.contains(f)) {
        return Err(format!("{} wasn't part of this request", actions::file_name(f)));
    }
    let settings = settings::load(&app);
    let req = Request { category, action, files, params };
    tauri::async_runtime::spawn_blocking(move || jobs::run(&settings, &req, &|_| Answer::KeepBoth))
        .await
        .map_err(|e| e.to_string())
}
