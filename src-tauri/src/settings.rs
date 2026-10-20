use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "settings.json";
const STORE_KEY: &str = "settings";

/// Where converted files are written.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind", content = "path")]
pub enum OutputLocation {
    /// Next to the source file.
    SameAsSource,
    /// A fixed folder chosen by the user.
    Custom(String),
}

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
    pub output: OutputLocation,
    /// `{name}` is the source file stem, `{ext}` the new extension.
    pub naming_pattern: String,
    pub overwrite: OverwritePolicy,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+Space".into(),
            output: OutputLocation::SameAsSource,
            naming_pattern: "{name}.converted.{ext}".into(),
            // AGENTS.md rule 3: never overwrite without asking.
            overwrite: OverwritePolicy::Ask,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.hotkey.trim().is_empty() {
            return Err("Hotkey can't be empty".into());
        }
        if !self.naming_pattern.contains("{name}") || !self.naming_pattern.contains("{ext}") {
            return Err("Naming pattern must contain {name} and {ext}".into());
        }
        if let OutputLocation::Custom(path) = &self.output {
            if path.trim().is_empty() {
                return Err("Choose an output folder".into());
            }
        }
        Ok(())
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
        assert_eq!(s.output, OutputLocation::SameAsSource);
    }

    #[test]
    fn partial_json_falls_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"hotkey":"Alt+M"}"#).unwrap();
        assert_eq!(s.hotkey, "Alt+M");
        assert_eq!(s.naming_pattern, Settings::default().naming_pattern);
    }

    #[test]
    fn custom_output_roundtrips() {
        let s = Settings {
            output: OutputLocation::Custom("D:\\Out".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
    }

    #[test]
    fn rejects_bad_pattern() {
        let s = Settings {
            naming_pattern: "{name}.out".into(),
            ..Default::default()
        };
        assert!(s.validate().is_err());
    }
}
