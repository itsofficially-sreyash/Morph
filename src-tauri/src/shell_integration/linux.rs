//! Linux file managers. Each gets its own small file, written only for the file managers
//! installed here; `remove` deletes all of them. Every entry runs `morph --open <files>`.
//! - Files (Nautilus): a script, shown under right-click → Scripts (a top-level item would
//!   need the optional nautilus-python package).
//! - Nemo: a `.nemo_action`, limited to Morph's extensions.
//! - Dolphin: a KIO service menu, limited by MIME type.
//! - Thunar: one `<action>` inside the user's `uca.xml`, which also holds their own actions.

use std::fs;
use std::path::{Path, PathBuf};

const LABEL: &str = "Convert with Morph";
const THUNAR_ID: &str = "morph-convert";

/// Where the entries live (`$XDG_DATA_HOME`, `$XDG_CONFIG_HOME`); separate for tests.
pub struct Dirs {
    pub data: PathBuf,
    pub config: PathBuf,
}

impl Dirs {
    fn from_env() -> Result<Self, String> {
        let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME isn't set")?;
        let xdg = |var: &str, fallback: &str| {
            std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(fallback))
        };
        Ok(Dirs { data: xdg("XDG_DATA_HOME", ".local/share"), config: xdg("XDG_CONFIG_HOME", ".config") })
    }

    fn nautilus(&self) -> PathBuf {
        self.data.join("nautilus/scripts").join(LABEL)
    }
    fn nemo(&self) -> PathBuf {
        self.data.join("nemo/actions/morph-convert.nemo_action")
    }
    fn dolphin(&self) -> PathBuf {
        self.data.join("kio/servicemenus/morph-convert.desktop")
    }
    fn thunar(&self) -> PathBuf {
        self.config.join("Thunar/uca.xml")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Manager {
    Nautilus,
    Nemo,
    Dolphin,
    Thunar,
}

const ALL: [Manager; 4] = [Manager::Nautilus, Manager::Nemo, Manager::Dolphin, Manager::Thunar];

fn installed_managers() -> Vec<Manager> {
    let on_path = |name: &str| {
        std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(name).is_file()))
    };
    ALL.into_iter()
        .filter(|m| {
            on_path(match m {
                Manager::Nautilus => "nautilus",
                Manager::Nemo => "nemo",
                Manager::Dolphin => "dolphin",
                Manager::Thunar => "thunar",
            })
        })
        .collect()
}

/// Single-quoted for `sh`.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Double-quoted as the desktop-entry spec (and Nemo, Thunar) parse `Exec` lines.
fn exec_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The MIME types Dolphin matches against, one per kind of file Morph handles.
const MIME_TYPES: &[&str] = &[
    "image/*",
    "video/*",
    "audio/*",
    "application/pdf",
    "text/plain",
    "application/rtf",
    "application/msword",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.oasis.opendocument.text",
    "application/vnd.ms-powerpoint",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/vnd.oasis.opendocument.presentation",
    "application/vnd.ms-excel",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.oasis.opendocument.spreadsheet",
];

fn write(path: &Path, text: &str, executable: bool) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let fail = |e: std::io::Error| format!("Couldn't add the right-click entry ({}): {e}", path.display());
    fs::create_dir_all(path.parent().expect("entry paths have a folder")).map_err(fail)?;
    fs::write(path, text).map_err(fail)?;
    if executable {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(fail)?;
    }
    Ok(())
}

fn remove_file(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("Couldn't remove {}: {e}", path.display()))
        }
        _ => Ok(()),
    }
}

/// `uca.xml` without Morph's `<action>` (the user's own actions stay as they were).
fn thunar_without_morph(xml: &str) -> String {
    let marker = format!("<unique-id>{THUNAR_ID}</unique-id>");
    let Some(at) = xml.find(&marker) else { return xml.to_string() };
    let (Some(start), Some(end)) = (xml[..at].rfind("<action>"), xml[at..].find("</action>")) else {
        return xml.to_string();
    };
    let end = at + end + "</action>".len();
    // Also drop the line break after the block, so repeated installs don't add blank lines.
    let end = if xml[end..].starts_with('\n') { end + 1 } else { end };
    let start = xml[..start].rfind('\n').map_or(start, |nl| if xml[nl + 1..start].trim().is_empty() { nl + 1 } else { start });
    format!("{}{}", &xml[..start], &xml[end..])
}

fn thunar_action(exe: &str, exts: &[&str]) -> String {
    let patterns: Vec<String> = exts.iter().flat_map(|e| [format!("*.{e}"), format!("*.{}", e.to_uppercase())]).collect();
    format!(
        "<action>\n\t<icon>morph</icon>\n\t<name>{LABEL}</name>\n\t<unique-id>{THUNAR_ID}</unique-id>\n\
         \t<command>{}</command>\n\t<description>Convert or edit with Morph</description>\n\
         \t<patterns>{}</patterns>\n\t<image-files/>\n\t<video-files/>\n\t<audio-files/>\n\t<text-files/>\n\t<other-files/>\n</action>\n",
        xml_escape(&format!("{} --open %F", exec_quote(exe))),
        patterns.join(";")
    )
}

pub fn install_into(dirs: &Dirs, managers: &[Manager], exe: &Path, exts: &[&str]) -> Result<(), String> {
    let exe = exe.to_string_lossy();
    for m in managers {
        match m {
            Manager::Nautilus => write(
                &dirs.nautilus(),
                // Nautilus passes the selected files as arguments, relative to the folder it
                // starts the script in; `--open` resolves them against that folder.
                &format!("#!/bin/sh\n# Added by Morph; `morph --remove-integration` removes it.\nexec {} --open \"$@\"\n", sh_quote(&exe)),
                true,
            )?,
            Manager::Nemo => write(
                &dirs.nemo(),
                &format!(
                    "[Nemo Action]\nName={LABEL}\nComment=Convert or edit with Morph\nExec={} --open %F\n\
                     Icon-Name=morph\nSelection=notnone\nExtensions={};\nQuote=double\n",
                    exec_quote(&exe),
                    exts.join(";")
                ),
                false,
            )?,
            Manager::Dolphin => write(
                &dirs.dolphin(),
                &format!(
                    "[Desktop Entry]\nType=Service\nMimeType={};\nActions=morphConvert;\nX-KDE-Priority=TopLevel\n\n\
                     [Desktop Action morphConvert]\nName={LABEL}\nIcon=morph\nExec={} --open %F\n",
                    MIME_TYPES.join(";"),
                    exec_quote(&exe)
                ),
                // Dolphin (KF6) ignores service menus that aren't executable.
                true,
            )?,
            Manager::Thunar => {
                let path = dirs.thunar();
                let current = fs::read_to_string(&path)
                    .unwrap_or_else(|_| "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<actions>\n</actions>\n".into());
                let mut xml = thunar_without_morph(&current);
                let Some(close) = xml.rfind("</actions>") else {
                    return Err(format!("{} isn't a Thunar actions file; left it alone", path.display()));
                };
                xml.insert_str(close, &thunar_action(&exe, exts));
                write(&path, &xml, false)?;
            }
        }
    }
    Ok(())
}

pub fn remove_from(dirs: &Dirs) -> Result<(), String> {
    remove_file(&dirs.nautilus())?;
    remove_file(&dirs.nemo())?;
    remove_file(&dirs.dolphin())?;
    let path = dirs.thunar();
    if let Ok(xml) = fs::read_to_string(&path) {
        let cleaned = thunar_without_morph(&xml);
        if cleaned != xml {
            write(&path, &cleaned, false)?;
        }
    }
    Ok(())
}

pub fn install(exe: &Path) -> Result<(), String> {
    let dirs = Dirs::from_env()?;
    // Entries for file managers that were uninstalled since are cleared too.
    remove_from(&dirs)?;
    install_into(&dirs, &installed_managers(), exe, &super::extensions())
}

pub fn remove() -> Result<(), String> {
    remove_from(&Dirs::from_env()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(name: &str) -> Dirs {
        let root = std::env::temp_dir().join("morph-shell-tests").join(name);
        let _ = fs::remove_dir_all(&root);
        Dirs { data: root.join("data"), config: root.join("config") }
    }

    #[test]
    fn installs_every_entry_and_removes_them_again() {
        let d = dirs("all");
        let exe = Path::new("/opt/My Apps/morph");
        install_into(&d, &ALL, exe, &["png", "pdf"]).unwrap();

        let script = fs::read_to_string(d.nautilus()).unwrap();
        assert!(script.contains("exec '/opt/My Apps/morph' --open \"$@\""), "{script}");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(d.nautilus()).unwrap().permissions().mode() & 0o111, 0o111);
        let nemo = fs::read_to_string(d.nemo()).unwrap();
        assert!(nemo.contains("Exec=\"/opt/My Apps/morph\" --open %F") && nemo.contains("Extensions=png;pdf;"), "{nemo}");
        assert!(fs::read_to_string(d.dolphin()).unwrap().contains("MimeType=image/*;"));
        let uca = fs::read_to_string(d.thunar()).unwrap();
        assert!(uca.contains("<patterns>*.png;*.PNG;*.pdf;*.PDF</patterns>"), "{uca}");

        remove_from(&d).unwrap();
        assert!(!d.nautilus().exists() && !d.nemo().exists() && !d.dolphin().exists());
        assert!(!fs::read_to_string(d.thunar()).unwrap().contains(THUNAR_ID));
    }

    #[test]
    fn thunar_keeps_the_users_own_actions() {
        let d = dirs("thunar");
        let own = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<actions>\n<action>\n\t<name>Open Terminal Here</name>\n\t<unique-id>1</unique-id>\n</action>\n</actions>\n";
        write(&d.thunar(), own, false).unwrap();
        // Installing twice must not add a second entry.
        install_into(&d, &[Manager::Thunar], Path::new("/usr/bin/morph"), &["png"]).unwrap();
        install_into(&d, &[Manager::Thunar], Path::new("/usr/bin/morph"), &["png"]).unwrap();
        let xml = fs::read_to_string(d.thunar()).unwrap();
        assert_eq!(xml.matches(THUNAR_ID).count(), 1);
        assert!(xml.contains("Open Terminal Here"));
        remove_from(&d).unwrap();
        assert_eq!(fs::read_to_string(d.thunar()).unwrap(), own);
    }

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("/a/it's"), r"'/a/it'\''s'");
        assert_eq!(exec_quote("/a b/$x\"y"), r#""/a b/\$x\"y""#);
        assert_eq!(xml_escape("a & <b>"), "a &amp; &lt;b&gt;");
    }
}
