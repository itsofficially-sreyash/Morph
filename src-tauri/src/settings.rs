use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "settings.json";
const STORE_KEY: &str = "settings";

/// What to do when the output file already exists.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverwritePolicy {
    /// Ask the user each time.
    Ask,
    /// Add a numeric suffix: `photo.converted (2).webp`.
    Rename,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub hotkey: String,
    /// `{name}` is the source file stem, `{ext}` the new extension. Converted files are
    /// always written in the original file's folder (owner rule), so this is a file
    /// name only; folder separators are rejected.
    pub naming_pattern: String,
    pub overwrite: OverwritePolicy,
    /// Start Morph (quietly, in the tray) when the user signs in. On by default (owner rule).
    pub start_at_login: bool,
    /// "Convert with Morph" in the file manager's right-click menu. On by default.
    pub context_menu: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+Space".into(),
            naming_pattern: "{name}.converted.{ext}".into(),
            // AGENTS.md rule 3: never overwrite without asking.
            overwrite: OverwritePolicy::Ask,
            start_at_login: true,
            context_menu: true,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.hotkey.trim().is_empty() {
            return Err("Hotkey can't be empty".into());
        }
        if !self.naming_pattern.contains("{name}") || !self.naming_pattern.contains("{ext}") {
            return Err("File name must contain {name} and {ext}".into());
        }
        if self.naming_pattern.contains(['/', '\\']) || self.naming_pattern.contains("..") {
            return Err("File name can't contain folders: converted files are saved next to the original".into());
        }
        Ok(())
    }

    /// Where a converted copy of `source` with extension `ext` is written: always the
    /// original's own folder. Every engine must use this (AGENTS.md rule 3a).
    #[allow(dead_code)] // first caller: the engines in step 4
    pub fn output_path(&self, source: &Path, ext: &str) -> PathBuf {
        let stem = source.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default();
        let name = self.naming_pattern.replace("{name}", &stem).replace("{ext}", ext.trim_start_matches('.'));
        source.parent().unwrap_or_else(|| Path::new("")).join(name)
    }
}

pub fn load<R: Runtime>(app: &AppHandle<R>) -> Settings {
    app.store(STORE_FILE)
        .ok()
        .and_then(|store| store.get(STORE_KEY))
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

pub fn save<R: Runtime>(app: &AppHandle<R>, settings: &Settings) -> Result<(), String> {
    settings.validate()?;
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    store.set(STORE_KEY, value);
    store.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    load(&app)
}

#[tauri::command]
pub fn set_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    settings.validate()?;
    // Switch the hotkey first: if it can't be registered, nothing is saved and the old one keeps working.
    crate::hotkey::apply(&app, &settings.hotkey)?;
    crate::autostart::apply(&app, settings.start_at_login)?;
    crate::shell_integration::apply(settings.context_menu)?;
    save(&app, &settings)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_safe() {
        let s = Settings::default();
        assert!(s.validate().is_ok());
        assert_eq!(s.overwrite, OverwritePolicy::Ask);
    }

    #[test]
    fn partial_json_falls_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"hotkey":"Alt+M"}"#).unwrap();
        assert_eq!(s.hotkey, "Alt+M");
        assert_eq!(s.naming_pattern, Settings::default().naming_pattern);
    }

    #[test]
    fn old_custom_folder_setting_is_ignored() {
        // Saved by earlier builds, which had a "save in this folder" option.
        let s: Settings =
            serde_json::from_str(r#"{"hotkey":"Alt+M","output":{"kind":"custom","path":"D:\\Out"}}"#).unwrap();
        assert_eq!(s.hotkey, "Alt+M");
        let out = s.output_path(Path::new("C:\\Pics\\a.png"), "webp");
        assert_eq!(out.parent(), Some(Path::new("C:\\Pics")));
    }

    #[test]
    fn output_goes_next_to_the_original() {
        let s = Settings::default();
        let src = Path::new("D:/Photos/2026/holiday.png");
        assert_eq!(s.output_path(src, "webp"), Path::new("D:/Photos/2026/holiday.converted.webp"));
        assert_eq!(s.output_path(src, ".jpg"), Path::new("D:/Photos/2026/holiday.converted.jpg"));
    }

    #[test]
    fn custom_pattern_keeps_the_folder() {
        let s = Settings { naming_pattern: "{name} (small).{ext}".into(), ..Default::default() };
        let out = s.output_path(Path::new("D:/Videos/clip.mp4"), "mp4");
        assert_eq!(out, Path::new("D:/Videos/clip (small).mp4"));
    }

    #[test]
    fn rejects_bad_pattern() {
        for bad in ["{name}.out", "..\\{name}.{ext}", "out/{name}.{ext}", "C:\\Temp\\{name}.{ext}", "{name}..{ext}"] {
            let s = Settings { naming_pattern: bad.into(), ..Default::default() };
            assert!(s.validate().is_err(), "{bad} should be rejected");
        }
    }
}
