// Mirrors `Settings` in src-tauri/src/settings.rs. Converted files are always saved
// next to the original, so there is no output-folder setting.
export interface Settings {
  hotkey: string;
  naming_pattern: string;
  overwrite: "ask" | "rename";
}
