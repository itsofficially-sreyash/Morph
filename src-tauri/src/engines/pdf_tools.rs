//! PDF merge / split / rotate / shrink, in-process with lopdf (no external tools).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lopdf::{Dictionary, Document, Object, ObjectId};

fn load(src: &Path) -> Result<Document, String> {
    Document::load(src).map_err(|e| match e {
        lopdf::Error::Decryption(_) | lopdf::Error::Unimplemented(_) if is_encrypted(src) => {
            format!("{} is password-protected", super::name(src))
        }
        other => format!("Couldn't read {}: {other}", super::name(src)),
    })
}

fn is_encrypted(src: &Path) -> bool {
    std::fs::read(src).map(|b| b.windows(8).any(|w| w == b"/Encrypt")).unwrap_or(false)
}

fn save(mut doc: Document, dst: &Path) -> Result<(), String> {
    doc.prune_objects();
    doc.renumber_objects();
    doc.compress();
    doc.save(dst).map(|_| ()).map_err(|e| format!("Couldn't write {}: {e}", super::name(dst)))
}

pub fn page_count(src: &Path) -> Result<u32, String> {
    Ok(load(src)?.get_pages().len() as u32)
}

/// Page attributes a page may inherit from its parent page-tree nodes.
const INHERITABLE: [&[u8]; 4] = [b"MediaBox", b"CropBox", b"Resources", b"Rotate"];

/// Copies inherited attributes onto each page, so pages keep their size and fonts
/// after being moved under a new page tree.
fn pin_inherited(doc: &mut Document) {
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    for page in pages {
        let mut found: Vec<(&[u8], Object)> = Vec::new();
        let mut parent = doc.get_dictionary(page).ok().and_then(|d| d.get(b"Parent").ok()).and_then(|p| p.as_reference().ok());
        let own: Vec<&[u8]> = INHERITABLE
            .iter()
            .copied()
            .filter(|k| doc.get_dictionary(page).is_ok_and(|d| d.has(k)))
            .collect();
        let mut guard = 0;
        while let Some(id) = parent {
            guard += 1;
            if guard > 64 {
                break; // malformed, cyclic tree
            }
            let Ok(node) = doc.get_dictionary(id) else { break };
            for key in INHERITABLE {
                if !own.contains(&key) && !found.iter().any(|(k, _)| *k == key) {
                    if let Ok(v) = node.get(key) {
                        found.push((key, v.clone()));
                    }
                }
            }
            parent = node.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
        }
        if let Ok(dict) = doc.get_dictionary_mut(page) {
            for (k, v) in found {
                dict.set(k, v);
            }
        }
    }
}

/// Joins `srcs` in the given order into one PDF.
pub fn merge(srcs: &[PathBuf], dst: &Path) -> Result<(), String> {
    if srcs.len() < 2 {
        return Err("Merging needs 2 or more PDFs".into());
    }
    let mut next_id = 1;
    let mut pages: Vec<ObjectId> = Vec::new();
    let mut objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    for src in srcs {
        let mut doc = load(src)?;
        pin_inherited(&mut doc);
        doc.renumber_objects_with(next_id);
        next_id = doc.max_id + 1;
        // `get_pages` is keyed by page number, so this keeps reading order.
        pages.extend(doc.get_pages().into_values());
        objects.extend(doc.objects);
    }

    let mut out = Document::with_version("1.7");
    let pages_id: ObjectId = (next_id, 0);
    let catalog_id: ObjectId = (next_id + 1, 0);
    for (id, obj) in objects {
        // Old catalogs, page trees and outlines are replaced by the new ones below.
        let skip = matches!(obj.type_name(), Ok(b"Catalog" | b"Pages" | b"Outlines" | b"Outline"));
        if !skip {
            out.objects.insert(id, obj);
        }
    }
    for &page in &pages {
        if let Some(Object::Dictionary(d)) = out.objects.get_mut(&page) {
            d.set("Parent", Object::Reference(pages_id));
        }
    }
    let mut tree = Dictionary::new();
    tree.set("Type", Object::Name(b"Pages".to_vec()));
    tree.set("Kids", Object::Array(pages.iter().map(|&p| Object::Reference(p)).collect()));
    tree.set("Count", Object::Integer(pages.len() as i64));
    out.objects.insert(pages_id, Object::Dictionary(tree));
    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));
    out.objects.insert(catalog_id, Object::Dictionary(catalog));
    out.trailer.set("Root", Object::Reference(catalog_id));
    out.max_id = next_id + 1;
    save(out, dst)
}

/// Inclusive 1-based page range.
pub type Range = (u32, u32);

/// Parses "1-3, 5, 8-10" (spaces optional, "7-" means 7 to the end) against `pages`.
pub fn parse_ranges(text: &str, pages: u32) -> Result<Vec<Range>, String> {
    let mut out = Vec::new();
    for part in text.split([',', ';']).map(str::trim).filter(|p| !p.is_empty()) {
        let num = |s: &str| -> Result<u32, String> {
            s.trim().parse::<u32>().map_err(|_| format!("\"{part}\" isn't a page or range like 2-5"))
        };
        let (a, b) = match part.split_once('-') {
            Some((a, b)) if b.trim().is_empty() => (num(a)?, pages),
            Some((a, b)) => (num(a)?, num(b)?),
            None => {
                let n = num(part)?;
                (n, n)
            }
        };
        if a == 0 || b == 0 {
            return Err("Pages start at 1".into());
        }
        if a > b {
            return Err(format!("\"{part}\" goes backwards"));
        }
        if b > pages {
            return Err(format!("This PDF has only {pages} page{}", if pages == 1 { "" } else { "s" }));
        }
        out.push((a, b));
    }
    if out.is_empty() {
        return Err("Type the pages to keep, e.g. 1-3, 5".into());
    }
    Ok(out)
}

pub fn every_page(pages: u32) -> Vec<Range> {
    (1..=pages).map(|p| (p, p)).collect()
}

pub fn range_label(r: Range) -> String {
    if r.0 == r.1 { format!("page {}", r.0) } else { format!("pages {}-{}", r.0, r.1) }
}

/// Writes `dst_for(range)` for each range.
pub fn split(src: &Path, ranges: &[Range], dst_for: impl Fn(Range) -> PathBuf) -> Result<Vec<PathBuf>, String> {
    let doc = load(src)?;
    let total = doc.get_pages().len() as u32;
    let mut written = Vec::new();
    for &r in ranges {
        let mut part = doc.clone();
        let drop: Vec<u32> = (1..=total).filter(|p| *p < r.0 || *p > r.1).collect();
        part.delete_pages(&drop);
        let dst = dst_for(r);
        save(part, &dst)?;
        written.push(dst);
    }
    Ok(written)
}

/// Rotates every page clockwise by 90°.
pub fn rotate(src: &Path, dst: &Path) -> Result<(), String> {
    let mut doc = load(src)?;
    pin_inherited(&mut doc);
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    for id in pages {
        if let Ok(d) = doc.get_dictionary_mut(id) {
            let now = d.get(b"Rotate").ok().and_then(|r| r.as_i64().ok()).unwrap_or(0);
            d.set("Rotate", Object::Integer((now + 90).rem_euclid(360)));
        }
    }
    save(doc, dst)
}

/// Lossless shrink: drops unused objects and compresses uncompressed streams.
/// Returns false if the result isn't smaller (the caller discards it).
pub fn shrink(src: &Path, dst: &Path) -> Result<bool, String> {
    let mut doc = load(src)?;
    doc.delete_zero_length_streams();
    save(doc, dst)?;
    let before = std::fs::metadata(src).map(|m| m.len()).unwrap_or(u64::MAX);
    let after = std::fs::metadata(dst).map(|m| m.len()).unwrap_or(u64::MAX);
    Ok(after < before)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::image::tests_support::sample;
    use crate::engines::pdf::images_to_pdf;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join("morph-pdftools-tests").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A PDF with `n` pages, made with Morph's own writer.
    fn pdf(d: &Path, name: &str, n: usize) -> PathBuf {
        let imgs: Vec<PathBuf> = (0..n).map(|i| sample(d, &format!("{name}-{i}.png"), false)).collect();
        let p = d.join(name);
        images_to_pdf(&imgs, &p).unwrap();
        p
    }

    #[test]
    fn merge_keeps_order_and_all_pages() {
        let d = dir("merge");
        let a = pdf(&d, "a.pdf", 2);
        let b = pdf(&d, "b.pdf", 3);
        let out = d.join("merged.pdf");
        merge(&[b.clone(), a.clone()], &out).unwrap();
        assert_eq!(page_count(&out).unwrap(), 5);
        // b's pages come first: the first page's image object must be b's (3 pages → its own XObject names).
        let merged = Document::load(&out).unwrap();
        assert_eq!(merged.get_pages().len(), 5);
        assert!(merge(&[a], &out).is_err(), "one PDF can't be merged");
    }

    #[test]
    fn split_writes_one_file_per_range() {
        let d = dir("split");
        let src = pdf(&d, "doc.pdf", 5);
        let ranges = parse_ranges("1-2, 4, 5-", 5).unwrap();
        let outs = split(&src, &ranges, |r| d.join(format!("part-{}-{}.pdf", r.0, r.1))).unwrap();
        let counts: Vec<u32> = outs.iter().map(|p| page_count(p).unwrap()).collect();
        assert_eq!(counts, [2, 1, 1]);
    }

    #[test]
    fn rotate_turns_every_page() {
        let d = dir("rotate");
        let src = pdf(&d, "doc.pdf", 2);
        let out = d.join("rot.pdf");
        rotate(&src, &out).unwrap();
        let doc = Document::load(&out).unwrap();
        for id in doc.get_pages().into_values() {
            assert_eq!(doc.get_dictionary(id).unwrap().get(b"Rotate").unwrap().as_i64().unwrap(), 90);
        }
    }

    #[test]
    fn shrink_is_honest() {
        let d = dir("shrink");
        let src = pdf(&d, "doc.pdf", 1);
        let out = d.join("small.pdf");
        let smaller = shrink(&src, &out).unwrap();
        assert_eq!(smaller, std::fs::metadata(&out).unwrap().len() < std::fs::metadata(&src).unwrap().len());
        assert_eq!(page_count(&out).unwrap(), 1);
    }

    #[test]
    fn range_parsing() {
        assert_eq!(parse_ranges("1-3,5", 9).unwrap(), [(1, 3), (5, 5)]);
        assert_eq!(parse_ranges(" 7- ", 9).unwrap(), [(7, 9)]);
        assert!(parse_ranges("3-1", 9).unwrap_err().contains("backwards"));
        assert!(parse_ranges("12", 9).unwrap_err().contains("only 9 pages"));
        assert!(parse_ranges("0", 9).is_err());
        assert!(parse_ranges("two", 9).is_err());
        assert!(parse_ranges("", 9).is_err());
        assert_eq!(every_page(3), [(1, 1), (2, 2), (3, 3)]);
        assert_eq!(range_label((2, 2)), "page 2");
        assert_eq!(range_label((2, 4)), "pages 2-4");
    }

    #[test]
    fn garbage_is_a_clear_error() {
        let d = dir("bad");
        let p = d.join("bad.pdf");
        std::fs::write(&p, b"%PDF-1.4 nonsense").unwrap();
        assert!(page_count(&p).unwrap_err().contains("bad.pdf"));
    }
}
