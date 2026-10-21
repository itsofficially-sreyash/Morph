//! Documents with whatever office suite the machine already has (owner rule: no bundling,
//! use what's installed):
//! - Windows: Microsoft Word / Excel / PowerPoint through COM automation (driven by
//!   Windows PowerShell, which every Windows 10/11 has), else LibreOffice.
//! - Linux: LibreOffice.

use std::path::{Path, PathBuf};

use crate::tools::Tool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Word,
    Excel,
    PowerPoint,
}

fn family(src: &Path) -> Option<Family> {
    let ext = src.extension()?.to_string_lossy().to_lowercase();
    Some(match ext.as_str() {
        "doc" | "docx" | "odt" | "rtf" | "txt" => Family::Word,
        "xls" | "xlsx" | "ods" => Family::Excel,
        "ppt" | "pptx" | "odp" => Family::PowerPoint,
        _ => return None,
    })
}

/// Spreadsheets and slides can only become PDFs; text documents can become anything here.
fn check_target(fam: Family, ext: &str, src: &Path) -> Result<(), String> {
    if ext == "pdf" || fam == Family::Word {
        Ok(())
    } else {
        Err(format!("{} can only be saved as PDF", super::name(src)))
    }
}

#[cfg(windows)]
fn ms_office_has(fam: Family) -> bool {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CLASSES_ROOT, RRF_RT_REG_SZ};
    let key: PCWSTR = match fam {
        Family::Word => w!("Word.Application\\CLSID"),
        Family::Excel => w!("Excel.Application\\CLSID"),
        Family::PowerPoint => w!("PowerPoint.Application\\CLSID"),
    };
    let mut size = 0u32;
    unsafe { RegGetValueW(HKEY_CLASSES_ROOT, key, None, RRF_RT_REG_SZ, None, None, Some(&mut size)) }.is_ok()
}

#[cfg(not(windows))]
fn ms_office_has(_: Family) -> bool {
    false
}

/// `Ok` if some office suite can convert documents here, otherwise what to install.
pub fn available() -> Result<(), &'static str> {
    let ms = [Family::Word, Family::Excel, Family::PowerPoint].into_iter().any(ms_office_has);
    if ms || Tool::LibreOffice.find().is_some() {
        Ok(())
    } else if cfg!(windows) {
        Err("Needs Microsoft Office or LibreOffice")
    } else {
        Err("Install LibreOffice to enable")
    }
}

pub fn convert(src: &Path, dst: &Path, ext: &str) -> Result<(), String> {
    let fam = family(src).ok_or_else(|| format!("{} isn't a document Morph knows", super::name(src)))?;
    check_target(fam, ext, src)?;
    if ms_office_has(fam) {
        return ms_office(fam, src, dst, ext);
    }
    if Tool::LibreOffice.find().is_some() {
        return libreoffice(src, dst, ext);
    }
    let app = match fam {
        Family::Word => "Word",
        Family::Excel => "Excel",
        Family::PowerPoint => "PowerPoint",
    };
    Err(format!("{} needs Microsoft {app} or LibreOffice", super::name(src)))
}

// ---- Microsoft Office (Windows) ----

/// Runs in Windows PowerShell. Paths arrive in environment variables, never inside the
/// script text, so a file name can't inject commands. If the app was already open, only
/// our document is closed (the user's windows stay), and a dummy password makes protected
/// files fail at once instead of waiting on an invisible password prompt.
///
/// `Quit()` takes no arguments here: Word declares its parameters by reference, and
/// PowerShell refuses `Quit(0)` (which left Word running). The document is already
/// closed without saving and alerts are off, so nothing can prompt.
const OFFICE_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$src = $env:MORPH_SRC; $dst = $env:MORPH_DST; $fmt = $env:MORPH_FMT; $m = [Type]::Missing
$noPass = 'morph-no-password'
switch ($env:MORPH_APP) {
  'word' {
    $was = [bool](Get-Process WINWORD -ErrorAction SilentlyContinue)
    $app = New-Object -ComObject Word.Application
    try {
      $app.DisplayAlerts = 0
      $doc = $app.Documents.Open($src, $false, $true, $false, $noPass)
      try {
        switch ($fmt) {
          'pdf'  { $doc.ExportAsFixedFormat($dst, 17) }
          'docx' { $doc.SaveAs2($dst, 16) }
          'odt'  { $doc.SaveAs2($dst, 23) }
          'txt'  { $doc.SaveAs2($dst, 7, $m, $m, $m, $m, $m, $m, $m, $m, $m, 65001) }
        }
      } finally { $doc.Close(0) }
    } finally {
      if (-not $was) { $app.Quit() }
      [void][Runtime.InteropServices.Marshal]::ReleaseComObject($app)
    }
  }
  'excel' {
    $app = New-Object -ComObject Excel.Application
    try {
      $app.DisplayAlerts = $false
      $book = $app.Workbooks.Open($src, 0, $true, $m, $noPass)
      try { $book.ExportAsFixedFormat(0, $dst) } finally { $book.Close($false) }
    } finally {
      $app.Quit()
      [void][Runtime.InteropServices.Marshal]::ReleaseComObject($app)
    }
  }
  'powerpoint' {
    $was = [bool](Get-Process POWERPNT -ErrorAction SilentlyContinue)
    $app = New-Object -ComObject PowerPoint.Application
    try {
      $deck = $app.Presentations.Open("$src::$noPass", -1, 0, 0)
      try { $deck.SaveAs($dst, 32) } finally { $deck.Close() }
    } finally {
      if (-not $was) { $app.Quit() }
      [void][Runtime.InteropServices.Marshal]::ReleaseComObject($app)
    }
  }
}
"#;

/// `-EncodedCommand` takes Base64 of UTF-16LE text.
#[cfg(windows)]
fn encode_command(script: &str) -> String {
    let utf16le: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    crate::b64::encode(&utf16le)
}

#[cfg(windows)]
fn ms_office(fam: Family, src: &Path, dst: &Path, ext: &str) -> Result<(), String> {
    let powershell = std::env::var_os("SystemRoot")
        .map(|r| PathBuf::from(r).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("powershell.exe"));
    // Office wants full paths; it resolves relative ones against its own folder.
    let abs = |p: &Path| std::path::absolute(p).map_err(|e| e.to_string());
    let mut cmd = super::background_command(&powershell);
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"])
        .arg(encode_command(OFFICE_SCRIPT))
        .env("MORPH_SRC", abs(src)?)
        .env("MORPH_DST", abs(dst)?)
        .env("MORPH_FMT", ext)
        .env(
            "MORPH_APP",
            match fam {
                Family::Word => "word",
                Family::Excel => "excel",
                Family::PowerPoint => "powerpoint",
            },
        );
    super::run_with_timeout(cmd, "Microsoft Office", std::time::Duration::from_secs(120))?;
    if dst.is_file() {
        Ok(())
    } else {
        Err(format!("Microsoft Office didn't save {}", super::name(dst)))
    }
}

#[cfg(not(windows))]
fn ms_office(_: Family, _: &Path, _: &Path, _: &str) -> Result<(), String> {
    Err("Microsoft Office isn't available here".into())
}

// ---- LibreOffice (Linux, or Windows without Office) ----

/// LibreOffice's `--convert-to` filter for each target extension.
fn filter(ext: &str) -> Result<&'static str, String> {
    Ok(match ext {
        "pdf" => "pdf",
        "docx" => "docx:MS Word 2007 XML",
        "odt" => "odt",
        "txt" => "txt:Text (encoded):UTF8",
        other => return Err(format!("LibreOffice can't make .{other} files here")),
    })
}

/// A private LibreOffice profile, so conversions work even while the user has
/// LibreOffice open (a shared profile would be locked).
fn profile_url() -> String {
    let dir = std::env::temp_dir().join("morph-libreoffice-profile");
    let path = dir.to_string_lossy().replace('\\', "/");
    if path.starts_with('/') { format!("file://{path}") } else { format!("file:///{path}") }
}

/// LibreOffice installs Writer, Calc and Impress as separate parts (Ubuntu ships some
/// without Impress). Without the right one it prints "source file could not be loaded"
/// and still exits 0, so check first and say what to install.
fn libreoffice_part(soffice: &Path, fam: Family) -> Result<(), String> {
    let (part, lib, package) = match fam {
        Family::Word => ("Writer", "swlo", "libreoffice-writer"),
        Family::Excel => ("Calc", "sclo", "libreoffice-calc"),
        Family::PowerPoint => ("Impress", "sdlo", "libreoffice-impress"),
    };
    let file = |name: &str| if cfg!(windows) { format!("{name}.dll") } else { format!("lib{name}.so") };
    let Some(program) = std::fs::canonicalize(soffice).ok().and_then(|p| p.parent().map(Path::to_path_buf)) else {
        return Ok(());
    };
    // Only judge the usual layout: one library per part (some builds merge them all).
    let split_layout = ["swlo", "sclo", "sdlo"].iter().any(|l| program.join(file(l)).is_file());
    if !split_layout || program.join(file(lib)).is_file() {
        return Ok(());
    }
    Err(if cfg!(windows) {
        format!("Needs LibreOffice {part} (add it in the LibreOffice installer)")
    } else {
        format!("Needs LibreOffice {part}: sudo apt install {package}")
    })
}

fn libreoffice(src: &Path, dst: &Path, ext: &str) -> Result<(), String> {
    let soffice = Tool::LibreOffice.find().ok_or("LibreOffice isn't installed")?;
    if let Some(fam) = family(src) {
        libreoffice_part(&soffice, fam)?;
    }
    // LibreOffice picks the output name itself, so convert into a scratch folder first.
    let scratch: PathBuf = std::env::temp_dir().join(format!(
        "morph-office-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let mut cmd = super::background_command(&soffice);
    cmd.arg(format!("-env:UserInstallation={}", profile_url()))
        .args(["--headless", "--norestore", "--nolockcheck", "--convert-to", filter(ext)?, "--outdir"])
        .arg(&scratch)
        .arg(src);
    let result = super::run_with_timeout(cmd, "LibreOffice", std::time::Duration::from_secs(120)).and_then(|_| {
        let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let produced = scratch.join(format!("{stem}.{ext}"));
        if !produced.is_file() {
            return Err(format!("LibreOffice couldn't open {} (damaged or password-protected?)", super::name(src)));
        }
        // Rename fails across drives (temp is usually on C:), so fall back to copying.
        std::fs::rename(&produced, dst)
            .or_else(|_| std::fs::copy(&produced, dst).map(|_| ()))
            .map_err(|e| format!("Couldn't save {}: {e}", super::name(dst)))
    });
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn encoded_command_matches_powershell() {
        // [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes("hi!"))
        assert_eq!(encode_command("hi!"), "aABpACEA");
    }

    #[test]
    fn families_and_targets() {
        assert_eq!(family(Path::new("r.DOCX")), Some(Family::Word));
        assert_eq!(family(Path::new("s.xlsx")), Some(Family::Excel));
        assert_eq!(family(Path::new("d.pptx")), Some(Family::PowerPoint));
        assert!(check_target(Family::Excel, "pdf", Path::new("s.xlsx")).is_ok());
        assert!(check_target(Family::Excel, "docx", Path::new("s.xlsx")).unwrap_err().contains("only be saved as PDF"));
        assert!(check_target(Family::Word, "txt", Path::new("r.docx")).is_ok());
    }

    #[test]
    fn libreoffice_filters_and_profile() {
        assert_eq!(filter("pdf").unwrap(), "pdf");
        assert!(filter("docx").unwrap().starts_with("docx"));
        assert!(filter("exe").is_err());
        assert!(profile_url().starts_with("file:///"), "{}", profile_url());
        assert!(!profile_url().contains('\\'));
    }

    /// Real Office round trip; needs Word. Run with
    /// `MORPH_DOCX_SAMPLE=<path> cargo test office -- --ignored`.
    #[test]
    #[ignore]
    fn docx_sample_to_pdf() {
        let Some(path) = std::env::var_os("MORPH_DOCX_SAMPLE") else { return };
        let src = PathBuf::from(path);
        for (ext, magic) in [("pdf", &b"%PDF"[..]), ("docx", &b"PK"[..]), ("txt", &b""[..])] {
            let dst = std::env::temp_dir().join(format!("morph-office-sample.{ext}"));
            let _ = std::fs::remove_file(&dst);
            convert(&src, &dst, ext).unwrap_or_else(|e| panic!("{ext}: {e}"));
            let bytes = std::fs::read(&dst).unwrap();
            assert!(bytes.starts_with(magic) && !bytes.is_empty(), "{ext} output looks wrong");
            eprintln!("{ext}: {} bytes", bytes.len());
        }
    }
}
