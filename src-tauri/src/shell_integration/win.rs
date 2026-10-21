//! Explorer: a classic shell verb under `HKCU\Software\Classes\*\shell` (per user, no
//! admin rights). On Windows 11 it appears under "Show more options"; the compact
//! top-level menu needs a signed app package (`IExplorerCommand`), which goes with code
//! signing in step 7.

use std::path::Path;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

const KEY: PCWSTR = w!("Software\\Classes\\*\\shell\\MorphConvert");
const COMMAND_KEY: PCWSTR = w!("Software\\Classes\\*\\shell\\MorphConvert\\command");

fn check(err: WIN32_ERROR, what: &str) -> Result<(), String> {
    err.ok().map_err(|e| format!("Couldn't {what}: {}", e.message()))
}

fn create(path: PCWSTR) -> Result<HKEY, String> {
    let mut key = HKEY::default();
    let err = unsafe {
        RegCreateKeyExW(HKEY_CURRENT_USER, path, None, None, REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None)
    };
    check(err, "add the right-click entry")?;
    Ok(key)
}

fn set(key: HKEY, name: Option<&str>, value: &str) -> Result<(), String> {
    let data: Vec<u8> = value.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    let name = name.map(HSTRING::from);
    let name_ptr = name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
    check(unsafe { RegSetValueExW(key, name_ptr, None, REG_SZ, Some(&data)) }, "write the right-click entry")
}

/// Windows Search syntax limiting the entry to files Morph has actions for.
pub fn applies_to(exts: &[&str]) -> String {
    exts.iter().map(|e| format!("System.FileExtension:=.{e}")).collect::<Vec<_>>().join(" OR ")
}

pub fn install(exe: &Path) -> Result<(), String> {
    let exe = exe.display().to_string();
    let verb = create(KEY)?;
    let result = (|| {
        set(verb, Some("MUIVerb"), "Convert with Morph")?;
        set(verb, Some("Icon"), &format!("\"{exe}\",0"))?;
        // No 15-item limit; Explorer starts one `--open` per file and Morph gathers them.
        set(verb, Some("MultiSelectModel"), "Player")?;
        set(verb, Some("AppliesTo"), &applies_to(&super::extensions()))
    })();
    unsafe {
        let _ = RegCloseKey(verb);
    }
    result?;
    let command = create(COMMAND_KEY)?;
    let result = set(command, None, &format!("\"{exe}\" --open \"%1\""));
    unsafe {
        let _ = RegCloseKey(command);
    }
    result?;
    refresh_explorer();
    Ok(())
}

pub fn remove() -> Result<(), String> {
    let err = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, KEY) };
    if err != ERROR_FILE_NOT_FOUND {
        check(err, "remove the right-click entry")?;
    }
    refresh_explorer();
    Ok(())
}

/// The command currently registered, if any.
#[cfg(test)]
pub fn registered_command() -> Option<String> {
    use windows::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_SZ};
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            COMMAND_KEY,
            None,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    err.is_ok().then(|| String::from_utf16_lossy(&buf[..(size as usize / 2).saturating_sub(1)]))
}

/// Tells Explorer the menu changed, so it shows up without signing out.
fn refresh_explorer() {
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_to_syntax() {
        assert_eq!(applies_to(&["png", "pdf"]), "System.FileExtension:=.png OR System.FileExtension:=.pdf");
    }

    /// Writes to the real registry, so it only runs on request:
    /// `cargo test shell_integration -- --ignored`.
    #[test]
    #[ignore]
    fn install_then_remove_round_trip() {
        let had = registered_command();
        let exe = Path::new("C:\\Program Files\\Morph\\morph.exe");
        install(exe).unwrap();
        assert_eq!(registered_command().as_deref(), Some("\"C:\\Program Files\\Morph\\morph.exe\" --open \"%1\""));
        remove().unwrap();
        assert_eq!(registered_command(), None);
        remove().unwrap(); // removing twice is fine
        // Put back whatever was there before the test.
        if let Some(cmd) = had {
            let exe = cmd.trim_start_matches('"').split('"').next().unwrap_or_default().to_string();
            install(Path::new(&exe)).unwrap();
        }
    }
}
