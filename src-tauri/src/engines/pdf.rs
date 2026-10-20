//! Images → one PDF, one page per image (in drop order). A tiny hand-written PDF 1.4
//! writer: each image is embedded as a JPEG (`/DCTDecode`), and pages are streamed to
//! disk one at a time so a big batch never sits in memory at once.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use super::image as img;

/// Counts bytes written, for the cross-reference table.
struct Counting<W: Write> {
    inner: W,
    pos: u64,
}

impl<W: Write> Write for Counting<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.pos += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

pub fn images_to_pdf(srcs: &[PathBuf], dst: &Path) -> Result<(), String> {
    if srcs.is_empty() {
        return Err("No images to put in the PDF".into());
    }
    let io = |e: std::io::Error| format!("Couldn't write {}: {e}", super::name(dst));
    let file = File::create(dst).map_err(io)?;
    let mut w = Counting { inner: BufWriter::new(file), pos: 0 };
    // Objects: 1 catalog, 2 page tree, then per page i: page 3+3i, content 4+3i, image 5+3i.
    let n = srcs.len();
    let mut offsets: Vec<u64> = vec![0; 3 + 3 * n];

    w.write_all(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n").map_err(io)?;
    offsets[1] = w.pos;
    w.write_all(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n").map_err(io)?;
    offsets[2] = w.pos;
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + 3 * i)).collect();
    write!(w, "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {n} >>\nendobj\n", kids.join(" ")).map_err(io)?;

    for (i, src) in srcs.iter().enumerate() {
        let (page, content, image) = (3 + 3 * i, 4 + 3 * i, 5 + 3 * i);
        let decoded = img::open(src)?;
        let rgb = img::flatten(&decoded);
        drop(decoded);
        let (pw, ph) = (rgb.width(), rgb.height());
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode_image(&rgb)
            .map_err(|e| e.to_string())?;
        drop(rgb);
        // Page size in points at 96 px per inch, so photos open at a sensible size.
        let (wpt, hpt) = (pw as f64 * 0.75, ph as f64 * 0.75);

        offsets[page] = w.pos;
        write!(
            w,
            "{page} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {wpt:.2} {hpt:.2}] \
             /Resources << /XObject << /Im0 {image} 0 R >> >> /Contents {content} 0 R >>\nendobj\n"
        )
        .map_err(io)?;
        let ops = format!("q {wpt:.2} 0 0 {hpt:.2} 0 0 cm /Im0 Do Q\n");
        offsets[content] = w.pos;
        write!(w, "{content} 0 obj\n<< /Length {} >>\nstream\n{ops}endstream\nendobj\n", ops.len()).map_err(io)?;
        offsets[image] = w.pos;
        write!(
            w,
            "{image} 0 obj\n<< /Type /XObject /Subtype /Image /Width {pw} /Height {ph} /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            jpeg.len()
        )
        .map_err(io)?;
        w.write_all(&jpeg).map_err(io)?;
        w.write_all(b"\nendstream\nendobj\n").map_err(io)?;
    }

    let xref = w.pos;
    let count = offsets.len();
    write!(w, "xref\n0 {count}\n0000000000 65535 f \n").map_err(io)?;
    for off in &offsets[1..] {
        write!(w, "{off:010} 00000 n \n").map_err(io)?;
    }
    write!(w, "trailer\n<< /Size {count} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").map_err(io)?;
    w.flush().map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_structurally_valid_pdf() {
        let d = std::env::temp_dir().join("morph-pdf-tests");
        let a = super::super::image::tests_support::sample(&d, "a.png", true);
        let b = super::super::image::tests_support::sample(&d, "b.jpg", false);
        let out = d.join("out.pdf");
        images_to_pdf(&[a, b], &out).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(text.trim_end().ends_with("%%EOF"));
        assert_eq!(text.matches("/Type /Page ").count(), 2);
        assert!(text.contains("/Count 2"));
        // Every xref entry must point exactly at "N 0 obj". Work on raw bytes: the JPEG data
        // isn't UTF-8, so text offsets would drift.
        let tail_at = bytes.windows(10).rposition(|w| w == b"startxref\n").unwrap() + 10;
        let tail = std::str::from_utf8(&bytes[tail_at..]).unwrap();
        let xref_at: usize = tail.lines().next().unwrap().parse().unwrap();
        let table = std::str::from_utf8(&bytes[xref_at..tail_at]).unwrap();
        assert!(table.starts_with("xref\n0 9\n"), "{table}");
        for (n, line) in table.lines().skip(3).take(8).enumerate() {
            let off: usize = line[..10].parse().unwrap();
            let expect = format!("{} 0 obj", n + 1);
            assert!(bytes[off..].starts_with(expect.as_bytes()), "object {} not at its xref offset", n + 1);
        }
    }

    #[test]
    fn empty_input_is_an_error() {
        assert!(images_to_pdf(&[], Path::new("x.pdf")).is_err());
    }
}
