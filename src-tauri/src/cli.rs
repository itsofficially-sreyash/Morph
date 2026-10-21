//! Command-line entry points. Both reach the already-running Morph through the
//! single-instance hand-off, so a launch costs only a short-lived process.
//!
//! - `morph --open <files…>`: what the right-click "Convert with Morph…" entry runs (step 6).
//! - `morph --toggle`: for desktops without global hotkeys (GNOME on Wayland) to bind a
//!   system shortcut to.

use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Open(Vec<String>),
    Toggle,
    /// `morph --remove-integration`: for the uninstaller. Removes the right-click entry
    /// and the sign-in entry, then quits (a running Morph quits too).
    RemoveIntegration,
    /// Plain launch (e.g. from the Start menu): show Settings if Morph is already running.
    None,
}

/// Parses `args` as passed to the process (the first one is the program path). Relative
/// file paths are resolved against `cwd`, the directory the command was run from.
pub fn parse(args: &[String], cwd: &str) -> Command {
    let mut rest = args.iter().skip(1);
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--toggle" => return Command::Toggle,
            "--remove-integration" => return Command::RemoveIntegration,
            "--open" => {
                let files: Vec<String> = rest
                    .filter(|a| !a.is_empty())
                    .map(|f| {
                        let p = Path::new(f);
                        if p.is_absolute() { f.clone() } else { Path::new(cwd).join(p).to_string_lossy().into_owned() }
                    })
                    .collect();
                return if files.is_empty() { Command::Toggle } else { Command::Open(files) };
            }
            _ => {}
        }
    }
    Command::None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plain_launch_does_nothing_special() {
        assert_eq!(parse(&args(&["morph.exe"]), "C:\\"), Command::None);
    }

    #[test]
    fn toggle() {
        assert_eq!(parse(&args(&["morph", "--toggle"]), "/"), Command::Toggle);
    }

    #[test]
    fn remove_integration() {
        assert_eq!(parse(&args(&["morph.exe", "--remove-integration"]), "/"), Command::RemoveIntegration);
    }

    #[test]
    fn open_keeps_absolute_paths() {
        let abs = if cfg!(windows) { "C:\\Users\\me\\a.png" } else { "/home/me/a.png" };
        assert_eq!(parse(&args(&["morph", "--open", abs]), "/x"), Command::Open(vec![abs.into()]));
    }

    #[test]
    fn open_resolves_relative_paths_against_cwd() {
        let cwd = if cfg!(windows) { "D:\\Pics" } else { "/pics" };
        let Command::Open(files) = parse(&args(&["morph", "--open", "a.png", "sub/b.mp4"]), cwd) else {
            panic!("expected Open");
        };
        assert!(files[0].starts_with(cwd) && files[0].ends_with("a.png"), "{files:?}");
        assert!(files[1].ends_with("b.mp4"), "{files:?}");
    }

    #[test]
    fn open_without_files_just_opens_the_orb() {
        assert_eq!(parse(&args(&["morph", "--open"]), "/"), Command::Toggle);
    }
}
