//! PDF pages → PNG images with what the OS already has: Windows 10/11's built-in PDF
//! renderer (`Windows.Data.Pdf`), or poppler's `pdftoppm` on Linux (preinstalled on
//! Ubuntu). Nothing is bundled.

use std::path::{Path, PathBuf};

/// Renders at 2× the page's size in device-independent pixels (≈ 192 dpi): sharp text
/// without huge files.
const SCALE: f64 = 2.0;

#[cfg(windows)]
pub fn available() -> Result<(), &'static str> {
    Ok(())
}

#[cfg(not(windows))]
pub fn available() -> Result<(), &'static str> {
    which("pdftoppm").map(|_| ()).ok_or("Needs poppler-utils (pdftoppm)")
}

#[cfg(not(windows))]
fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?.to_str()?.split(':').map(|d| Path::new(d).join(name)).find(|p| p.is_file())
}

/// Writes one PNG per page to `dst_for(page_number)` (1-based).
#[cfg(windows)]
pub fn render(src: &Path, dst_for: impl Fn(u32) -> PathBuf) -> Result<Vec<PathBuf>, String> {
    use windows::core::HSTRING;
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    let err = |e: windows::core::Error| format!("Couldn't render {}: {}", super::name(src), e.message());
    // Worker threads need WinRT initialised; "already initialised" is fine.
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
    // StorageFile needs a plain absolute path (no `\\?\` prefix, which `canonicalize` adds).
    let abs = std::path::absolute(src).map_err(|e| e.to_string())?;
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(abs.as_os_str())).and_then(|op| op.join()).map_err(err)?;
    let doc = PdfDocument::LoadFromFileAsync(&file).and_then(|op| op.join()).map_err(|e| {
        if doc_is_protected(src) {
            format!("{} is password-protected", super::name(src))
        } else {
            err(e)
        }
    })?;
    let mut written = Vec::new();
    for i in 0..doc.PageCount().map_err(err)? {
        let page = doc.GetPage(i).map_err(err)?;
        let size = page.Size().map_err(err)?;
        let options = PdfPageRenderOptions::new().map_err(err)?;
        options.SetDestinationWidth((size.Width as f64 * SCALE).round() as u32).map_err(err)?;
        // The default encoder is PNG.
        let stream = InMemoryRandomAccessStream::new().map_err(err)?;
        page.RenderWithOptionsToStreamAsync(&stream, &options).and_then(|op| op.join()).map_err(err)?;
        let len = stream.Size().map_err(err)? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(err)?).map_err(err)?;
        reader.LoadAsync(len).and_then(|op| op.join()).map_err(err)?;
        let mut png = vec![0u8; len as usize];
        reader.ReadBytes(&mut png).map_err(err)?;
        let dst = dst_for(i + 1);
        std::fs::write(&dst, png).map_err(|e| format!("Couldn't write {}: {e}", super::name(&dst)))?;
        written.push(dst);
    }
    Ok(written)
}

#[cfg(windows)]
fn doc_is_protected(src: &Path) -> bool {
    std::fs::read(src).map(|b| b.windows(8).any(|w| w == b"/Encrypt")).unwrap_or(false)
}

#[cfg(not(windows))]
pub fn render(src: &Path, dst_for: impl Fn(u32) -> PathBuf) -> Result<Vec<PathBuf>, String> {
    let tool = which("pdftoppm").ok_or("Needs poppler-utils (pdftoppm)")?;
    let scratch = std::env::temp_dir().join(format!("morph-pdftoppm-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let mut cmd = super::background_command(&tool);
    cmd.args(["-png", "-r", &((96.0 * SCALE) as u32).to_string()]).arg(src).arg(scratch.join("p"));
    let result = super::run_quietly(cmd, "pdftoppm").and_then(|_| {
        // pdftoppm names pages p-1.png, p-01.png … depending on the page count.
        let mut pages: Vec<(u32, PathBuf)> = std::fs::read_dir(&scratch)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter_map(|p| {
                let n = p.file_stem()?.to_str()?.rsplit('-').next()?.parse::<u32>().ok()?;
                Some((n, p))
            })
            .collect();
        pages.sort();
        let mut written = Vec::new();
        for (n, p) in pages {
            let dst = dst_for(n);
            std::fs::rename(&p, &dst).or_else(|_| std::fs::copy(&p, &dst).map(|_| ())).map_err(|e| e.to_string())?;
            written.push(dst);
        }
        Ok(written)
    });
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::image::tests_support::sample;
    use crate::engines::pdf::images_to_pdf;

    #[test]
    fn renders_every_page_to_png() {
        if available().is_err() {
            eprintln!("no PDF renderer here; skipping");
            return;
        }
        let d = std::env::temp_dir().join("morph-pdfrender-tests");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let imgs = [sample(&d, "a.png", false), sample(&d, "b.png", true)];
        let src = d.join("doc.pdf");
        images_to_pdf(&imgs, &src).unwrap();
        let out = render(&src, |n| d.join(format!("page-{n}.png"))).unwrap();
        assert_eq!(out.len(), 2);
        for p in &out {
            let img = image::open(p).unwrap();
            // 120 px images make 90 pt pages; rendered at 2× = 240 px wide.
            assert!(img.width() >= 200, "{} is only {} px wide", p.display(), img.width());
        }
    }
}
