//! Windows presenter for the native orb: a layered (per-pixel alpha) popup window.
//!
//! Everything here runs on the main thread, whose message loop Tauri already pumps, so
//! the window's messages arrive without any extra thread. Nothing runs while the orb is
//! hidden: no timers, and the pixel buffers are freed (only the window handle and fonts
//! are kept so the next open is instant).

use std::cell::RefCell;
use std::sync::mpsc::Sender;
use std::time::Instant;

use tiny_skia::Pixmap;
use windows::core::{implement, w, Ref, Result as WinResult};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, POINTL, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetMonitorInfoW, MonitorFromPoint, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC,
    HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::{
    IDropTarget, IDropTarget_Impl, OleInitialize, RegisterDragDrop, ReleaseStgMedium, CF_HDROP, DROPEFFECT,
    DROPEFFECT_COPY, DROPEFFECT_NONE,
};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_BACK, VK_DOWN, VK_ESCAPE, VK_LEFT,
    VK_NUMPAD1, VK_NUMPAD9, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_TAB, VK_UP,
};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, IsWindowVisible, KillTimer, LoadCursorW, RegisterClassExW,
    SetCursor, SetForegroundWindow, SetTimer, ShowWindow, UpdateLayeredWindow, CS_HREDRAW, CS_VREDRAW, HTCLIENT,
    IDC_ARROW, IDC_HAND, MA_ACTIVATE, SW_HIDE, SW_SHOW, ULW_ALPHA, WA_INACTIVE, WM_ACTIVATE, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_SETCURSOR, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

/// winuser.h; the `windows` crate files it under a module we don't otherwise need.
const WM_MOUSELEAVE: u32 = 0x02A3;

use super::geometry as g;
use super::model::{Key, Menu, Outcome};
use crate::jobs::{Answer, Request, Summary};
use super::render::{self, Fonts};
use crate::overlay::{place, Rect};

const ANIM_TIMER: usize = 1;
const DONE_TIMER: usize = 2;
const OPEN_MS: f32 = 180.0;
/// How long "✓ Saved …" stays before the orb closes. Errors stay until dismissed.
const DONE_MS: u32 = 1500;

/// Device-pixel buffers, alive only while the orb is visible.
struct Surface {
    pixmap: Pixmap,
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
}

impl Surface {
    fn new(size: u32) -> Option<Self> {
        let pixmap = Pixmap::new(size, size)?;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32), // top-down rows, like the pixmap
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            let old = SelectObject(dc, bitmap.into());
            Some(Surface { pixmap, dc, bitmap, old, bits: bits.cast() })
        }
    }

    /// Copies the premultiplied RGBA pixmap into the premultiplied BGRA DIB.
    fn blit(&mut self) {
        let src = self.pixmap.data();
        let dst = unsafe { std::slice::from_raw_parts_mut(self.bits, src.len()) };
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

struct Orb {
    hwnd: HWND,
    menu: Menu,
    fonts: Fonts,
    surface: Option<Surface>,
    scale: f32,
    dark: bool,
    pos: (i32, i32),
    anim_start: Option<Instant>,
    tracking_leave: bool,
    visible: bool,
    /// A running job waits on this for the Replace / Keep both answer.
    pending_answer: Option<Sender<Answer>>,
    /// Id of the job this orb is showing; results of older jobs are ignored.
    job: u64,
}

thread_local! {
    static ORB: RefCell<Option<Orb>> = const { RefCell::new(None) };
}

/// Runs `f` on the orb state. Returns `None` if the orb doesn't exist or the state is
/// already borrowed (a re-entrant message, e.g. WM_ACTIVATE sent by `ShowWindow`).
fn with_orb<T>(f: impl FnOnce(&mut Orb) -> T) -> Option<T> {
    ORB.with(|cell| cell.try_borrow_mut().ok().and_then(|mut o| o.as_mut().map(f)))
}

impl Orb {
    fn size(&self) -> u32 {
        render::device_size(self.scale)
    }

    fn open_progress(&self) -> f32 {
        self.anim_start
            .map_or(1.0, |t| (t.elapsed().as_secs_f32() * 1000.0 / OPEN_MS).min(1.0))
    }

    fn paint(&mut self) {
        let size = self.size();
        if self.surface.as_ref().is_none_or(|s| s.pixmap.width() != size) {
            self.surface = Surface::new(size);
        }
        let open = self.open_progress();
        let Some(surface) = self.surface.as_mut() else { return };
        render::render(&mut surface.pixmap, &self.menu, self.scale, open, self.dark, &self.fonts);
        surface.blit();
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let pos = POINT { x: self.pos.0, y: self.pos.1 };
        let sz = SIZE { cx: size as i32, cy: size as i32 };
        unsafe {
            let _ = UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&pos),
                Some(&sz),
                Some(surface.dc),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    /// Starts (or restarts) the 180 ms opening animation; frames run only until it settles.
    fn animate(&mut self) {
        self.anim_start = Some(Instant::now());
        unsafe { SetTimer(Some(self.hwnd), ANIM_TIMER, 16, None) };
        self.paint();
    }

    fn logical(&self, lparam: LPARAM) -> (f32, f32) {
        let x = (lparam.0 & 0xffff) as i16 as f32;
        let y = ((lparam.0 >> 16) & 0xffff) as i16 as f32;
        (x / self.scale, y / self.scale)
    }

    /// Applies a model outcome. Returns true if the orb should close.
    fn apply(&mut self, outcome: Outcome) -> bool {
        match outcome {
            Outcome::Unchanged => false,
            Outcome::Redraw { new_ring: true } => {
                self.animate();
                false
            }
            Outcome::Redraw { new_ring: false } => {
                self.paint();
                false
            }
            Outcome::Chosen { category, action, files } => {
                self.job += 1;
                self.paint();
                super::start_job(self.job, Request { category, action, files });
                false
            }
            Outcome::Answered(answer) => {
                if let Some(tx) = self.pending_answer.take() {
                    let _ = tx.send(answer);
                }
                self.paint();
                false
            }
            Outcome::Close => true,
        }
    }
}

fn apps_use_dark_theme() -> bool {
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    ok.is_ok() && value == 0
}

fn key_from_vk(vk: u16) -> Option<Key> {
    let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
    Some(match vk {
        0x31..=0x39 => Key::Digit((vk - 0x30) as u8),
        v if (VK_NUMPAD1.0..=VK_NUMPAD9.0).contains(&v) => Key::Digit((v - VK_NUMPAD1.0 + 1) as u8),
        v if v == VK_RIGHT.0 || v == VK_DOWN.0 || (v == VK_TAB.0 && !shift) => Key::Next,
        v if v == VK_LEFT.0 || v == VK_UP.0 || (v == VK_TAB.0 && shift) => Key::Prev,
        v if v == VK_RETURN.0 => Key::Enter,
        v if v == VK_BACK.0 => Key::Back,
        v if v == VK_ESCAPE.0 => Key::Escape,
        _ => return None,
    })
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Side effects that re-enter this procedure (ShowWindow → WM_ACTIVATE) run after the
    // state borrow is released.
    let mut close = false;
    let handled = match msg {
        WM_MOUSEMOVE => with_orb(|o| {
            if !o.tracking_leave {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                o.tracking_leave = unsafe { TrackMouseEvent(&mut tme) }.is_ok();
            }
            let (x, y) = o.logical(lparam);
            if o.menu.hover(g::hit_test(x, y, o.menu.items().len())) {
                o.paint();
            }
        })
        .is_some(),
        WM_MOUSELEAVE => with_orb(|o| {
            o.tracking_leave = false;
            if o.menu.hover(None) {
                o.paint();
            }
        })
        .is_some(),
        WM_SETCURSOR if (lparam.0 & 0xffff) as u32 == HTCLIENT => with_orb(|o| {
            let mut p = POINT::default();
            unsafe { GetCursorPos(&mut p).ok() };
            let (x, y) = ((p.x - o.pos.0) as f32 / o.scale, (p.y - o.pos.1) as f32 / o.scale);
            let clickable = g::hit_test(x, y, o.menu.items().len()).is_some()
                || (g::in_hub(x, y) && (o.menu.can_go_back() || o.menu.is_finished()));
            unsafe {
                let _ = SetCursor(LoadCursorW(None, if clickable { IDC_HAND } else { IDC_ARROW }).ok());
            }
        })
        .is_some(),
        WM_LBUTTONDOWN => with_orb(|o| {
            let (x, y) = o.logical(lparam);
            let outcome = if o.menu.is_finished() {
                // Any click on a finished orb dismisses it.
                Outcome::Close
            } else if g::in_hub(x, y) {
                o.menu.back()
            } else if let Some(i) = g::hit_test(x, y, o.menu.items().len()) {
                o.menu.pick(i)
            } else {
                Outcome::Unchanged
            };
            close = o.apply(outcome);
        })
        .is_some(),
        WM_KEYDOWN => with_orb(|o| {
            if let Some(key) = key_from_vk(wparam.0 as u16) {
                let outcome = o.menu.key(key);
                close = o.apply(outcome);
            }
        })
        .is_some(),
        WM_TIMER => with_orb(|o| match wparam.0 {
            ANIM_TIMER => {
                o.paint();
                if o.open_progress() >= 1.0 {
                    o.anim_start = None;
                    unsafe { KillTimer(Some(hwnd), ANIM_TIMER).ok() };
                }
            }
            DONE_TIMER => close = true,
            _ => {}
        })
        .is_some(),
        // Clicking outside the orb dismisses it, except while it waits for a drop (the
        // user has to click into a file manager to start dragging) or a job is running.
        WM_ACTIVATE if (wparam.0 & 0xffff) as u32 == WA_INACTIVE => {
            close = with_orb(|o| o.menu.hides_on_blur()).unwrap_or(false);
            true
        }
        WM_MOUSEACTIVATE => return LRESULT(MA_ACTIVATE as isize),
        _ => false,
    };
    if close {
        hide();
    }
    if handled { LRESULT(0) } else { unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) } }
}

// ---- Drag and drop (OLE), so dragging from Explorer highlights the orb before the drop. ----

#[implement(IDropTarget)]
struct DropTarget;

fn has_files(data: &IDataObject) -> bool {
    unsafe { data.QueryGetData(&hdrop_format()) }.is_ok()
}

fn hdrop_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

fn dropped_paths(data: &IDataObject) -> Vec<String> {
    let mut paths = Vec::new();
    unsafe {
        let Ok(mut medium) = data.GetData(&hdrop_format()) else { return paths };
        let hdrop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(hdrop, u32::MAX, None);
        for i in 0..count {
            let len = DragQueryFileW(hdrop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(hdrop, i, Some(&mut buf));
            paths.push(String::from_utf16_lossy(&buf[..len]));
        }
        ReleaseStgMedium(&mut medium);
    }
    paths
}

fn set_effect(effect: *mut DROPEFFECT, value: DROPEFFECT) {
    if !effect.is_null() {
        unsafe { *effect = value };
    }
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> WinResult<()> {
        let files = data.ok().is_ok_and(has_files);
        set_effect(effect, if files { DROPEFFECT_COPY } else { DROPEFFECT_NONE });
        if files {
            with_orb(|o| {
                if o.menu.set_dragging(true) {
                    o.paint();
                }
            });
        }
        Ok(())
    }

    fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> WinResult<()> {
        let dragging = with_orb(|o| o.menu.dragging()).unwrap_or(false);
        set_effect(effect, if dragging { DROPEFFECT_COPY } else { DROPEFFECT_NONE });
        Ok(())
    }

    fn DragLeave(&self) -> WinResult<()> {
        with_orb(|o| {
            if o.menu.set_dragging(false) {
                o.paint();
            }
        });
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> WinResult<()> {
        let paths = data.ok().map(dropped_paths).unwrap_or_default();
        set_effect(effect, if paths.is_empty() { DROPEFFECT_NONE } else { DROPEFFECT_COPY });
        let hwnd = with_orb(|o| {
            o.menu.set_dragging(false);
            if !paths.is_empty() {
                o.menu.add_files(paths);
            }
            o.animate();
            o.hwnd
        });
        // Give keyboard control (1–9, Enter) to the orb after a drop, where Windows allows it.
        if let Some(hwnd) = hwnd {
            unsafe {
                let _ = SetForegroundWindow(hwnd);
                let _ = SetFocus(Some(hwnd));
            }
        }
        Ok(())
    }
}

// ---- Window lifecycle ----

fn create() -> Option<Orb> {
    let fonts = Fonts::load()?;
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = w!("MorphOrb");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).ok()?,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class,
            w!("Morph"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;
        // COM is already in single-threaded mode on Tauri's main thread (WebView2 needs it);
        // this call is then a harmless no-op.
        let _ = OleInitialize(None);
        let target: IDropTarget = DropTarget.into();
        if let Err(e) = RegisterDragDrop(hwnd, &target) {
            eprintln!("orb: drag and drop unavailable: {e}");
        }
        Some(Orb {
            hwnd,
            menu: Menu::default(),
            fonts,
            surface: None,
            scale: 1.0,
            dark: true,
            pos: (0, 0),
            anim_start: None,
            tracking_leave: false,
            visible: false,
            pending_answer: None,
            job: 0,
        })
    }
}

/// Monitor work area (excludes the taskbar) and DPI scale under the cursor.
fn monitor_at(cursor: POINT) -> (Rect, f32) {
    unsafe {
        let mon = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut info);
        let r = info.rcWork;
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let rect = Rect { x: r.left, y: r.top, width: (r.right - r.left) as u32, height: (r.bottom - r.top) as u32 };
        (rect, dx as f32 / 96.0)
    }
}

pub fn show(files: Option<Vec<String>>) {
    ORB.with(|cell| {
        if let Ok(mut o) = cell.try_borrow_mut() {
            if o.is_none() {
                *o = create();
            }
        }
    });
    let Some(hwnd) = with_orb(|o| {
        let mut cursor = POINT::default();
        unsafe { GetCursorPos(&mut cursor).ok() };
        let (area, scale) = monitor_at(cursor);
        o.scale = scale;
        o.dark = apps_use_dark_theme();
        o.pos = place((cursor.x as f64, cursor.y as f64), o.size(), area);
        o.menu.reset(files);
        // A fresh orb no longer shows any earlier job.
        o.job += 1;
        o.visible = true;
        o.animate();
        o.hwnd
    }) else {
        eprintln!("orb: couldn't create the window (system fonts missing?)");
        return;
    };
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
    }
}

pub fn hide() {
    let Some(hwnd) = with_orb(|o| {
        unsafe {
            let _ = KillTimer(Some(o.hwnd), ANIM_TIMER);
            let _ = KillTimer(Some(o.hwnd), DONE_TIMER);
        }
        o.anim_start = None;
        // Free ~2 × (size² × 4) bytes of pixel buffers while idle.
        o.surface = None;
        o.menu.reset(None);
        o.visible = false;
        // Nobody can answer now: keep both (never overwrite without an answer).
        if let Some(tx) = o.pending_answer.take() {
            let _ = tx.send(Answer::KeepBoth);
        }
        o.hwnd
    }) else {
        return;
    };
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

pub fn is_visible() -> bool {
    with_orb(|o| unsafe { IsWindowVisible(o.hwnd) }.as_bool()).unwrap_or(false)
}

/// A job asks whether to replace existing outputs. Answered "Keep both" right away if
/// the orb showing that job is gone.
pub fn ask(job: u64, question: String, tx: Sender<Answer>) {
    let shown = with_orb(|o| {
        if !o.visible || o.job != job {
            return false;
        }
        o.menu.ask(question.clone());
        o.pending_answer = Some(tx.clone());
        o.animate();
        true
    });
    if shown != Some(true) {
        let _ = tx.send(Answer::KeepBoth);
    }
}

/// A job finished: show "✓ Saved …" (closes by itself) or the error (stays until dismissed).
pub fn job_finished(job: u64, summary: Summary) {
    with_orb(|o| {
        if !o.visible || o.job != job {
            return;
        }
        o.menu.finish(summary.ok, summary.message);
        o.paint();
        if summary.ok {
            unsafe { SetTimer(Some(o.hwnd), DONE_TIMER, DONE_MS, None) };
        }
    });
}
