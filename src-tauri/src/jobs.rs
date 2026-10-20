//! Runs a chosen action on the dropped files, off the main thread.
//!
//! - Destinations come from `Settings::output_path`: always the original's folder.
//! - If outputs already exist, the user is asked **once per batch** (Replace / Keep both).
//!   An original file is never replaced, whatever the file-name pattern.
//! - Engines write to a hidden temp file that is renamed into place only on success,
//!   so a failed conversion never leaves a half-written file.
//! - At most `half_cores()` files are processed at once across all jobs (AGENTS.md rule 10).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Runtime};

use crate::actions::{self, CategoryId};
use crate::engines::{self, Done};
use crate::settings::{self, OverwritePolicy, Settings};

pub struct Request {
    pub category: CategoryId,
    pub action: &'static str,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Replace,
    KeepBoth,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Summary {
    pub ok: bool,
    pub message: String,
    /// Paths of the files written.
    pub saved: Vec<String>,
}

impl Summary {
    fn failed(message: impl Into<String>) -> Self {
        Summary { ok: false, message: message.into(), saved: Vec::new() }
    }
}

// ---- Global limit on concurrent work ----

struct Slots {
    used: Mutex<usize>,
    freed: Condvar,
}

static SLOTS: Slots = Slots { used: Mutex::new(0), freed: Condvar::new() };

struct Permit;

impl Permit {
    fn acquire() -> Permit {
        let cap = engines::half_cores();
        let mut used = SLOTS.used.lock().unwrap();
        while *used >= cap {
            used = SLOTS.freed.wait(used).unwrap();
        }
        *used += 1;
        Permit
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        *SLOTS.used.lock().unwrap() -= 1;
        SLOTS.freed.notify_one();
    }
}

// ---- Planning destinations ----

/// `photo.converted.webp` → `photo.converted (2).webp`, `(3)`, … until the name is free.
fn unique(dest: &Path, taken: &HashSet<PathBuf>) -> PathBuf {
    let stem = dest.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = dest.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..)
        .map(|n| dest.with_file_name(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists() && !taken.contains(p))
        .expect("an unused name exists")
}

fn temp_for(dest: &Path) -> PathBuf {
    dest.with_file_name(format!(".~{}", engines::name(dest)))
}

struct Item {
    /// Input(s): one file, or all of them for combined actions.
    srcs: Vec<PathBuf>,
    dest: PathBuf,
}

/// Chooses every destination up front so the overwrite question is asked at most once.
fn plan(settings: &Settings, req: &Request, inputs: &[PathBuf], ask: &dyn Fn(String) -> Answer) -> Vec<Item> {
    let wanted: Vec<(Vec<PathBuf>, PathBuf)> = if engines::is_combined(req.category, req.action) {
        let ext = engines::output_ext(req.category, req.action, &inputs[0]);
        vec![(inputs.to_vec(), settings.output_path(&inputs[0], &ext))]
    } else {
        inputs
            .iter()
            .map(|src| {
                let ext = engines::output_ext(req.category, req.action, src);
                (vec![src.clone()], settings.output_path(src, &ext))
            })
            .collect()
    };

    let originals: HashSet<&PathBuf> = inputs.iter().collect();
    let conflicts: Vec<&PathBuf> = wanted.iter().map(|(_, d)| d).filter(|d| d.exists() && !originals.contains(d)).collect();
    let answer = match (conflicts.len(), settings.overwrite) {
        (0, _) => Answer::KeepBoth,
        (_, OverwritePolicy::Rename) => Answer::KeepBoth,
        (1, OverwritePolicy::Ask) => ask(format!("{} already exists", engines::name(conflicts[0]))),
        (n, OverwritePolicy::Ask) => ask(format!("{n} of these files already exist")),
    };

    let mut taken: HashSet<PathBuf> = HashSet::new();
    wanted
        .into_iter()
        .map(|(srcs, dest)| {
            // Never write over an original, and never let two outputs of this batch collide.
            let must_rename = originals.contains(&dest) || taken.contains(&dest);
            let dest = if must_rename || (dest.exists() && answer == Answer::KeepBoth) { unique(&dest, &taken) } else { dest };
            taken.insert(dest.clone());
            Item { srcs, dest }
        })
        .collect()
}

// ---- Running ----

enum Result1 {
    Saved(PathBuf),
    NotSmaller(PathBuf),
    Failed(String),
}

fn execute(req: &Request, item: &Item) -> Result1 {
    let _permit = Permit::acquire();
    let tmp = temp_for(&item.dest);
    let outcome = if engines::is_combined(req.category, req.action) {
        engines::run_combined(req.category, req.action, &item.srcs, &tmp).map(|_| Done::Saved)
    } else {
        engines::run_one(req.category, req.action, &item.srcs[0], &tmp)
    };
    match outcome {
        Ok(Done::Saved) => match std::fs::rename(&tmp, &item.dest) {
            Ok(()) => Result1::Saved(item.dest.clone()),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                Result1::Failed(format!("Couldn't save {}: {e}", engines::name(&item.dest)))
            }
        },
        Ok(Done::NotSmaller) => {
            let _ = std::fs::remove_file(&tmp);
            Result1::NotSmaller(item.srcs[0].clone())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Result1::Failed(e)
        }
    }
}

fn summarize(results: Vec<Result1>) -> Summary {
    let mut saved = Vec::new();
    let mut not_smaller = Vec::new();
    let mut failed = Vec::new();
    for r in results {
        match r {
            Result1::Saved(p) => saved.push(p),
            Result1::NotSmaller(p) => not_smaller.push(p),
            Result1::Failed(e) => failed.push(e),
        }
    }
    let saved_text = match saved.as_slice() {
        [] => None,
        [one] => Some(format!("Saved {}", engines::name(one))),
        many => Some(format!("Saved {} files", many.len())),
    };
    let small_text = match not_smaller.as_slice() {
        [] => None,
        [one] => Some(format!("{} is already as small as it gets", engines::name(one))),
        many => Some(format!("{} files are already as small as they get", many.len())),
    };
    let message = match (failed.len(), saved_text, small_text) {
        (0, Some(s), None) => s,
        (0, Some(s), Some(n)) => format!("{s}; {n}"),
        (0, None, Some(n)) => n,
        (0, None, None) => "Nothing to do".into(),
        (1, None, None) => failed[0].clone(),
        (n, None, None) => format!("{} (and {} more failed)", failed[0], n - 1),
        (n, s, _) => format!("{}, {n} failed: {}", s.unwrap_or_else(|| "Nothing saved".into()), failed[0]),
    };
    Summary {
        ok: failed.is_empty() && !saved.is_empty(),
        message,
        saved: saved.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
    }
}

/// Runs a request to completion on the calling thread (call it from a worker thread).
pub fn run(settings: &Settings, req: &Request, ask: &dyn Fn(String) -> Answer) -> Summary {
    let category = actions::category(req.category);
    let Some(action) = category.action(req.action) else {
        return Summary::failed(format!("Unknown action {}", req.action));
    };
    if let Err(why) = actions::availability(action) {
        return Summary::failed(why);
    }
    let inputs: Vec<PathBuf> = req.files.iter().filter(|f| category.accepts(f)).map(PathBuf::from).collect();
    if inputs.is_empty() {
        return Summary::failed(format!("None of these files are {} files", category.label.to_lowercase()));
    }
    let items = plan(settings, req, &inputs, ask);

    // Workers pull items in order; how many run at once is also capped globally.
    let workers = engines::max_parallel(req.category).min(items.len()).max(1);
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, Result1)>> = Mutex::new(Vec::with_capacity(items.len()));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                engines::lower_thread_priority();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let r = execute(req, item);
                    results.lock().unwrap().push((i, r));
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(i, _)| *i);
    summarize(results.into_iter().map(|(_, r)| r).collect())
}

/// Starts a request on a background thread. `ask` is called (from that thread) if outputs
/// already exist; `done` receives the summary.
pub fn start<R: Runtime>(
    app: AppHandle<R>,
    req: Request,
    ask: impl Fn(String) -> Answer + Send + 'static,
    done: impl FnOnce(Summary) + Send + 'static,
) {
    let settings = settings::load(&app);
    let spawned = std::thread::Builder::new().name("morph-job".into()).spawn(move || {
        done(run(&settings, &req, &ask));
    });
    if let Err(e) = spawned {
        eprintln!("jobs: couldn't start a worker thread: {e}");
    }
}

/// For the web orb (Linux): runs an action and returns the summary. Existing outputs are
/// kept (Keep both) because the web orb can't ask yet.
#[tauri::command]
pub async fn run_action(app: AppHandle, category: String, action: String, files: Vec<String>) -> Result<Summary, String> {
    let id = CategoryId::parse(&category).ok_or_else(|| format!("Unknown category {category}"))?;
    let action = actions::category(id).action(&action).ok_or_else(|| format!("Unknown action {action}"))?.id;
    let settings = settings::load(&app);
    let req = Request { category: id, action, files };
    tauri::async_runtime::spawn_blocking(move || run(&settings, &req, &|_| Answer::KeepBoth))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::image::tests_support::sample;
    use std::cell::Cell;

    fn fresh_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join("morph-jobs-tests").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn req(category: CategoryId, action: &'static str, files: &[&Path]) -> Request {
        Request { category, action, files: files.iter().map(|p| p.to_string_lossy().into_owned()).collect() }
    }

    fn never_ask(_: String) -> Answer {
        panic!("should not ask")
    }

    #[test]
    fn converts_next_to_the_original() {
        let d = fresh_dir("next-to");
        let a = sample(&d, "holiday.png", true);
        let s = run(&Settings::default(), &req(CategoryId::Image, "convert:webp", &[&a]), &never_ask);
        assert!(s.ok, "{}", s.message);
        assert_eq!(s.message, "Saved holiday.converted.webp");
        assert!(d.join("holiday.converted.webp").is_file());
        // No temp files left behind.
        assert!(std::fs::read_dir(&d).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".~")));
    }

    #[test]
    fn asks_once_and_keep_both_adds_a_number() {
        let d = fresh_dir("keep-both");
        let a = sample(&d, "a.png", false);
        let b = sample(&d, "b.png", false);
        std::fs::write(d.join("a.converted.jpg"), b"old").unwrap();
        std::fs::write(d.join("b.converted.jpg"), b"old").unwrap();
        let asked = Cell::new(0);
        let s = run(&Settings::default(), &req(CategoryId::Image, "convert:jpg", &[&a, &b]), &|q| {
            asked.set(asked.get() + 1);
            assert_eq!(q, "2 of these files already exist");
            Answer::KeepBoth
        });
        assert_eq!(asked.get(), 1);
        assert!(s.ok, "{}", s.message);
        assert_eq!(std::fs::read(d.join("a.converted.jpg")).unwrap(), b"old", "kept the old file");
        assert!(d.join("a.converted (2).jpg").is_file());
        assert!(d.join("b.converted (2).jpg").is_file());
    }

    #[test]
    fn replace_overwrites_only_after_asking() {
        let d = fresh_dir("replace");
        let a = sample(&d, "a.png", false);
        std::fs::write(d.join("a.converted.jpg"), b"old").unwrap();
        let s = run(&Settings::default(), &req(CategoryId::Image, "convert:jpg", &[&a]), &|q| {
            assert_eq!(q, "a.converted.jpg already exists");
            Answer::Replace
        });
        assert!(s.ok);
        assert_ne!(std::fs::read(d.join("a.converted.jpg")).unwrap(), b"old");
        assert!(!d.join("a.converted (2).jpg").exists());
    }

    #[test]
    fn rename_policy_never_asks() {
        let d = fresh_dir("rename-policy");
        let a = sample(&d, "a.png", false);
        std::fs::write(d.join("a.converted.webp"), b"old").unwrap();
        let settings = Settings { overwrite: OverwritePolicy::Rename, ..Default::default() };
        let s = run(&settings, &req(CategoryId::Image, "convert:webp", &[&a]), &never_ask);
        assert!(s.ok);
        assert!(d.join("a.converted (2).webp").is_file());
    }

    #[test]
    fn never_replaces_the_original_even_with_a_bare_pattern() {
        let d = fresh_dir("original-safe");
        let a = sample(&d, "pic.png", false);
        let before = std::fs::read(&a).unwrap();
        let settings = Settings { naming_pattern: "{name}.{ext}".into(), ..Default::default() };
        let s = run(&settings, &req(CategoryId::Image, "convert:png", &[&a]), &never_ask);
        assert!(s.ok, "{}", s.message);
        assert_eq!(std::fs::read(&a).unwrap(), before, "original untouched");
        assert!(d.join("pic (2).png").is_file());
    }

    #[test]
    fn outputs_of_one_batch_never_collide() {
        let d = fresh_dir("collide");
        let png = sample(&d, "same.png", false);
        let jpg = sample(&d, "same.jpg", false);
        let s = run(&Settings::default(), &req(CategoryId::Image, "convert:webp", &[&png, &jpg]), &never_ask);
        assert!(s.ok, "{}", s.message);
        assert!(d.join("same.converted.webp").is_file() && d.join("same.converted (2).webp").is_file());
    }

    #[test]
    fn images_to_one_pdf() {
        let d = fresh_dir("pdf");
        let a = sample(&d, "first.png", true);
        let b = sample(&d, "second.jpg", false);
        let s = run(&Settings::default(), &req(CategoryId::Image, "to-pdf", &[&a, &b]), &never_ask);
        assert!(s.ok, "{}", s.message);
        assert_eq!(s.message, "Saved first.converted.pdf");
        assert!(std::fs::read(d.join("first.converted.pdf")).unwrap().starts_with(b"%PDF"));
    }

    #[test]
    fn failures_are_reported_with_the_reason() {
        let d = fresh_dir("fail");
        let good = sample(&d, "good.png", false);
        let bad = d.join("bad.png");
        std::fs::write(&bad, b"not an image").unwrap();
        let s = run(&Settings::default(), &req(CategoryId::Image, "convert:jpg", &[&good, &bad]), &never_ask);
        assert!(!s.ok);
        assert!(s.message.starts_with("Saved good.converted.jpg, 1 failed"), "{}", s.message);
        assert!(s.message.contains("bad.png"), "{}", s.message);
        assert!(!d.join(".~bad.converted.jpg").exists(), "temp file cleaned up");
    }

    #[test]
    fn unavailable_and_unrelated_requests_fail_cleanly() {
        let d = fresh_dir("unavailable");
        let a = sample(&d, "a.png", false);
        let s = run(&Settings::default(), &req(CategoryId::Image, "crop", &[&a]), &never_ask);
        assert_eq!((s.ok, s.message.as_str()), (false, "Coming soon"));
        let s = run(&Settings::default(), &req(CategoryId::Image, "convert:jpg", &[Path::new("song.mp3")]), &never_ask);
        assert!(!s.ok && s.message.contains("image"), "{}", s.message);
    }

    #[test]
    fn summaries_read_naturally() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(summarize(vec![Result1::Saved(p("a.jpg")), Result1::Saved(p("b.jpg"))]).message, "Saved 2 files");
        assert_eq!(summarize(vec![Result1::NotSmaller(p("x.jpg"))]).message, "x.jpg is already as small as it gets");
        let s = summarize(vec![Result1::Failed("boom".into()), Result1::Failed("bang".into())]);
        assert_eq!((s.ok, s.message.as_str()), (false, "boom (and 1 more failed)"));
    }
}
