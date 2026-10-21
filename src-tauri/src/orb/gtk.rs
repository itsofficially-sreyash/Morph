//! Linux presenter for the native orb: a GTK 3 window with an RGBA visual. The shared
//! renderer draws each frame on the CPU and cairo copies it to the window.
//!
//! Everything here runs on the main thread, where Tauri runs the GTK main loop. Nothing
//! runs while the orb is hidden: no timers, and the pixel buffers are freed (only the
//! window and fonts are kept so the next open is instant). No WebKit is involved, so no
//! web processes stay alive while Morph waits in the tray.
//!
//! GTK runs on XWayland under Wayland (`main.rs`): native Wayland can't keep a window on
//! top or place it at the cursor.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{cairo, gdk, gio, glib};
use tiny_skia::Pixmap;

use super::geometry as g;
use super::model::{Key, Menu, Outcome};
use super::render::{self, Fonts};
use crate::jobs::{Answer, Request, Summary};
use super::placement::{place, Rect};

const OPEN_MS: f32 = 180.0;
/// How long "✓ Saved …" stays before the orb closes. Errors stay until dismissed.
const DONE_MS: u64 = 1500;
/// Launches from one multi-file right-click arrive within this window.
const GATHER_MS: u128 = 1500;

/// The last rendered frame, read by the draw handler. Kept outside `Orb` so drawing never
/// needs the orb state (GTK may draw while it is borrowed).
type Frame = Rc<RefCell<Option<cairo::ImageSurface>>>;

struct Orb {
    window: gtk::Window,
    frame: Frame,
    /// Device-pixel buffer, alive only while the orb is visible.
    pixmap: Option<Pixmap>,
    menu: Menu,
    fonts: Fonts,
    /// GDK's (integer) scale factor of the monitor under the cursor.
    scale: i32,
    dark: bool,
    anim_start: Option<Instant>,
    anim_timer: Option<glib::SourceId>,
    done_timer: Option<glib::SourceId>,
    /// Whether the pointer cursor is the hand (only changed when it flips).
    hand: bool,
    visible: bool,
    /// A running job waits on this for the Replace / Keep both answer.
    pending_answer: Option<Sender<Answer>>,
    /// Id of the job this orb is showing; results of older jobs are ignored.
    job: u64,
    /// When `open_files` last added files, to gather a multi-file right-click.
    last_open: Option<Instant>,
}

thread_local! {
    static ORB: RefCell<Option<Orb>> = const { RefCell::new(None) };
}

/// Runs `f` on the orb state. Returns `None` if the orb doesn't exist or the state is
/// already borrowed (a signal emitted from inside another handler).
fn with_orb<T>(f: impl FnOnce(&mut Orb) -> T) -> Option<T> {
    ORB.with(|cell| cell.try_borrow_mut().ok().and_then(|mut o| o.as_mut().map(f)))
}

impl Orb {
    fn open_progress(&self) -> f32 {
        self.anim_start
            .map_or(1.0, |t| (t.elapsed().as_secs_f32() * 1000.0 / OPEN_MS).min(1.0))
    }

    fn paint(&mut self) {
        let scale = self.scale as f32;
        let size = render::device_size(scale);
        if self.pixmap.as_ref().is_none_or(|p| p.width() != size) {
            self.pixmap = Pixmap::new(size, size);
        }
        let open = self.open_progress();
        let Some(pm) = self.pixmap.as_mut() else { return };
        render::render(pm, &self.menu, scale, open, self.dark, &self.fonts);

        let mut frame = self.frame.borrow_mut();
        if frame.as_ref().is_none_or(|s| s.width() != size as i32) {
            *frame = cairo::ImageSurface::create(cairo::Format::ARgb32, size as i32, size as i32).ok();
        }
        let Some(surface) = frame.as_mut() else { return };
        let stride = surface.stride() as usize;
        // Fails only while cairo still holds the surface (never between draws).
        if let Ok(mut data) = surface.data() {
            // Premultiplied RGBA → cairo's premultiplied ARGB32 (BGRA bytes on little-endian).
            for (row, src) in data.chunks_exact_mut(stride).zip(pm.data().chunks_exact(size as usize * 4)) {
                for (d, s) in row.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                    d[0] = s[2];
                    d[1] = s[1];
                    d[2] = s[0];
                    d[3] = s[3];
                }
            }
        }
        surface.mark_dirty();
        surface.set_device_scale(scale as f64, scale as f64);
        drop(frame);
        self.window.queue_draw();
    }

    /// Starts (or restarts) the 180 ms opening animation; frames run only until it settles.
    fn animate(&mut self) {
        self.anim_start = Some(Instant::now());
        if self.anim_timer.is_none() {
            self.anim_timer = Some(glib::timeout_add_local(Duration::from_millis(16), || {
                let more = with_orb(|o| {
                    o.paint();
                    let more = o.open_progress() < 1.0;
                    if !more {
                        o.anim_start = None;
                        o.anim_timer = None;
                    }
                    more
                });
                // Borrowed right now (re-entrant signal): try again on the next tick.
                if more.unwrap_or(true) { glib::ControlFlow::Continue } else { glib::ControlFlow::Break }
            }));
        }
        self.paint();
    }

    fn stop_timers(&mut self) {
        if let Some(id) = self.anim_timer.take() {
            id.remove();
        }
        if let Some(id) = self.done_timer.take() {
            id.remove();
        }
        self.anim_start = None;
    }

    fn update_cursor(&mut self, x: f32, y: f32) {
        let hand = g::hit_test(x, y, self.menu.items().len()).is_some()
            || (g::in_hub(x, y) && (self.menu.can_go_back() || self.menu.is_finished()));
        if hand == self.hand {
            return;
        }
        self.hand = hand;
        if let Some(win) = self.window.window() {
            let cursor = gdk::Cursor::from_name(&win.display(), if hand { "pointer" } else { "default" });
            win.set_cursor(cursor.as_ref());
        }
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
                super::start_job(self.job, Request { category, action, files, params: Default::default() });
                false
            }
            Outcome::OpenTool { kind, category, action, files } => {
                // The window takes over; the orb closes.
                super::open_tool(kind, category, action, files);
                true
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

/// GNOME's "Dark style" (Settings → Appearance), else a dark GTK theme.
fn prefers_dark() -> bool {
    let schema = gio::SettingsSchemaSource::default().and_then(|s| s.lookup("org.gnome.desktop.interface", true));
    if let Some(schema) = schema.filter(|s| s.has_key("color-scheme")) {
        let settings = gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None);
        return settings.string("color-scheme") == "prefer-dark";
    }
    gtk::Settings::default().is_some_and(|s| {
        s.is_gtk_application_prefer_dark_theme()
            || s.gtk_theme_name().is_some_and(|t| t.to_lowercase().contains("dark"))
    })
}

fn key_from(event: &gdk::EventKey) -> Option<Key> {
    use gdk::keys::constants as k;
    let v = event.keyval();
    if let Some(d) = v.to_unicode().and_then(|c| c.to_digit(10)).filter(|d| (1..=9).contains(d)) {
        return Some(Key::Digit(d as u8));
    }
    Some(if v == k::Right || v == k::Down || v == k::Tab {
        Key::Next
    } else if v == k::Left || v == k::Up || v == k::ISO_Left_Tab {
        Key::Prev
    } else if v == k::Return || v == k::KP_Enter {
        Key::Enter
    } else if v == k::BackSpace {
        Key::Back
    } else if v == k::Escape {
        Key::Escape
    } else {
        return None;
    })
}

/// Clicks on the transparent corners go to the window below, as on Windows.
fn round_input_shape(window: &gtk::Window) {
    let size = g::SIZE as i32;
    let rects: Vec<cairo::RectangleInt> = (0..size)
        .filter_map(|y| {
            let dy = (y as f32 + 0.5 - g::C).abs();
            let half = (g::BACKDROP_R * g::BACKDROP_R - dy * dy).sqrt();
            (half > 0.0).then(|| {
                let x0 = (g::C - half).floor() as i32;
                cairo::RectangleInt::new(x0, y, size - 2 * x0, 1)
            })
        })
        .collect();
    window.input_shape_combine_region(Some(&cairo::Region::create_rectangles(&rects)));
}

fn connect_signals(window: &gtk::Window, frame: Frame) {
    window.connect_draw(move |_, cr| {
        cr.set_operator(cairo::Operator::Source);
        match frame.borrow().as_ref() {
            Some(surface) if cr.set_source_surface(surface, 0.0, 0.0).is_ok() => {}
            _ => cr.set_source_rgba(0.0, 0.0, 0.0, 0.0),
        }
        let _ = cr.paint();
        glib::Propagation::Stop
    });
    // Alt+F4 and the like hide the orb; the window is kept for the next open.
    window.connect_delete_event(|_, _| {
        hide();
        glib::Propagation::Stop
    });
    window.connect_motion_notify_event(|_, e| {
        let (x, y) = e.position();
        with_orb(|o| {
            if o.menu.hover(g::hit_test(x as f32, y as f32, o.menu.items().len())) {
                o.paint();
            }
            o.update_cursor(x as f32, y as f32);
        });
        glib::Propagation::Proceed
    });
    window.connect_leave_notify_event(|_, _| {
        with_orb(|o| {
            if o.menu.hover(None) {
                o.paint();
            }
        });
        glib::Propagation::Proceed
    });
    window.connect_button_press_event(|_, e| {
        if e.button() != 1 || e.event_type() != gdk::EventType::ButtonPress {
            return glib::Propagation::Proceed;
        }
        let (x, y) = e.position();
        let (x, y) = (x as f32, y as f32);
        let close = with_orb(|o| {
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
            o.apply(outcome)
        });
        if close == Some(true) {
            hide();
        }
        glib::Propagation::Stop
    });
    window.connect_key_press_event(|_, e| {
        let Some(key) = key_from(e) else { return glib::Propagation::Proceed };
        let close = with_orb(|o| {
            let outcome = o.menu.key(key);
            o.apply(outcome)
        });
        if close == Some(true) {
            hide();
        }
        glib::Propagation::Stop
    });
    // Clicking elsewhere dismisses the orb, except while it waits for a drop (the user has
    // to click into a file manager to start dragging) or a job is running.
    window.connect_focus_out_event(|_, _| {
        if with_orb(|o| o.visible && o.menu.hides_on_blur()) == Some(true) {
            hide();
        }
        glib::Propagation::Proceed
    });

    // Drag and drop (XDND), so dragging from a file manager highlights the orb first.
    let targets = [gtk::TargetEntry::new("text/uri-list", gtk::TargetFlags::OTHER_APP, 0)];
    window.drag_dest_set(gtk::DestDefaults::ALL, &targets, gdk::DragAction::COPY);
    window.connect_drag_motion(|_, _, _, _, _| {
        with_orb(|o| {
            if o.menu.set_dragging(true) {
                o.paint();
            }
        });
        true
    });
    window.connect_drag_leave(|_, _, _| {
        with_orb(|o| {
            if o.menu.set_dragging(false) {
                o.paint();
            }
        });
    });
    window.connect_drag_data_received(|_, _, _, _, data, _, _| {
        let paths: Vec<String> = data
            .uris()
            .iter()
            .filter_map(|uri| glib::filename_from_uri(uri).ok())
            .map(|(path, _)| path.to_string_lossy().into_owned())
            .collect();
        let window = with_orb(|o| {
            o.menu.set_dragging(false);
            if !paths.is_empty() {
                o.menu.add_files(paths);
            }
            o.animate();
            o.window.clone()
        });
        // Give keyboard control (1–9, Enter) to the orb after a drop.
        if let Some(window) = window {
            window.present();
        }
    });
}

fn create() -> Option<Orb> {
    let fonts = Fonts::load()?;
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Morph");
    window.set_decorated(false);
    window.set_resizable(false);
    window.set_skip_taskbar_hint(true);
    window.set_skip_pager_hint(true);
    window.set_keep_above(true);
    window.set_type_hint(gdk::WindowTypeHint::Utility);
    window.set_app_paintable(true);
    // Per-pixel alpha needs a compositor (every current desktop has one).
    if let Some(visual) = GtkWindowExt::screen(&window).and_then(|s| s.rgba_visual()) {
        window.set_visual(Some(&visual));
    }
    let size = g::SIZE as i32;
    window.set_default_size(size, size);
    window.set_size_request(size, size);
    window.add_events(
        gdk::EventMask::POINTER_MOTION_MASK
            | gdk::EventMask::BUTTON_PRESS_MASK
            | gdk::EventMask::LEAVE_NOTIFY_MASK
            | gdk::EventMask::KEY_PRESS_MASK
            | gdk::EventMask::FOCUS_CHANGE_MASK,
    );
    let frame: Frame = Rc::new(RefCell::new(None));
    connect_signals(&window, frame.clone());
    round_input_shape(&window);
    Some(Orb {
        window,
        frame,
        pixmap: None,
        menu: Menu::default(),
        fonts,
        scale: 1,
        dark: true,
        anim_start: None,
        anim_timer: None,
        done_timer: None,
        hand: false,
        visible: false,
        pending_answer: None,
        job: 0,
        last_open: None,
    })
}

/// Cursor position, work area (excludes panels) and scale of the monitor under it, all
/// in GDK's logical pixels.
fn cursor_and_monitor() -> Option<((i32, i32), Rect, i32)> {
    let display = gdk::Display::default()?;
    let pointer = display.default_seat()?.pointer()?;
    let (_, x, y) = pointer.position();
    let monitor = display.monitor_at_point(x, y)?;
    let r = monitor.workarea();
    let area = Rect { x: r.x(), y: r.y(), width: r.width().max(0) as u32, height: r.height().max(0) as u32 };
    Some(((x, y), area, monitor.scale_factor().max(1)))
}

pub fn show(files: Option<Vec<String>>) {
    ORB.with(|cell| {
        if let Ok(mut o) = cell.try_borrow_mut() {
            if o.is_none() {
                *o = create();
            }
        }
    });
    let Some(window) = with_orb(|o| {
        if let Some(((x, y), area, scale)) = cursor_and_monitor() {
            o.scale = scale;
            let (px, py) = place((x as f64, y as f64), g::SIZE as u32, area);
            o.window.move_(px, py);
        }
        o.stop_timers();
        o.dark = prefers_dark();
        o.menu.reset(files);
        // A fresh orb no longer shows any earlier job.
        o.job += 1;
        o.visible = true;
        o.animate();
        o.window.clone()
    }) else {
        eprintln!("orb: couldn't create the window (system fonts missing?)");
        return;
    };
    // Outside the borrow: showing emits focus and draw signals.
    window.show();
    window.present();
}

pub fn hide() {
    let Some(window) = with_orb(|o| {
        o.stop_timers();
        // Free the pixel buffers (2 × size² × 4 bytes) while idle.
        o.pixmap = None;
        *o.frame.borrow_mut() = None;
        o.menu.reset(None);
        o.visible = false;
        o.last_open = None;
        // Nobody can answer now: keep both (never overwrite without an answer).
        if let Some(tx) = o.pending_answer.take() {
            let _ = tx.send(Answer::KeepBoth);
        }
        o.window.clone()
    }) else {
        return;
    };
    window.hide();
}

pub fn is_visible() -> bool {
    with_orb(|o| o.visible).unwrap_or(false)
}

/// Right-click entry: add to the orb a moment-ago launch opened, or open a new one.
pub fn open_files(files: Vec<String>) {
    let gathered = with_orb(|o| {
        let recent = o.last_open.is_some_and(|t| t.elapsed().as_millis() < GATHER_MS);
        if o.visible && recent && !o.menu.is_busy() && !o.menu.is_finished() {
            o.menu.add_files(files.clone());
            o.last_open = Some(Instant::now());
            o.animate();
            true
        } else {
            false
        }
    });
    if gathered != Some(true) {
        show(Some(files));
        with_orb(|o| o.last_open = Some(Instant::now()));
    }
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
            o.done_timer = Some(glib::timeout_add_local_once(Duration::from_millis(DONE_MS), || {
                // The source ends itself; forget it so `hide` doesn't remove it again.
                with_orb(|o| o.done_timer = None);
                hide();
            }));
        }
    });
}
