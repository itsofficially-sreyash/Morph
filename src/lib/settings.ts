// Mirrors `Settings` in src-tauri/src/settings.rs.
export type OutputLocation = { kind: "same_as_source" } | { kind: "custom"; path: string };

export interface Settings {
  hotkey: string;
  output: OutputLocation;
  naming_pattern: string;
  overwrite: "ask" | "rename";
}
