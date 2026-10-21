//! Linux file managers (Nautilus/Nemo/Dolphin/Thunar) are the Linux phase (owner:
//! Windows first). Until then nothing is installed, and the setting just has no effect
//! (it must not stop the other settings from saving).

use std::path::Path;

pub fn install(_exe: &Path) -> Result<(), String> {
    eprintln!("shell integration: Linux file-manager entries aren't implemented yet");
    Ok(())
}

pub fn remove() -> Result<(), String> {
    Ok(())
}
