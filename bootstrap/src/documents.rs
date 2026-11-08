//! Native format readers. See docs/compiler/binary-documents.md.
use pdfium_render::prelude::*;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Projection {
    pub format: String,
    pub revision: String,
    pub text: String,
    pub blocks: Vec<Block>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub text: String,
    // Byte positions in this particular projection, never persistent identities.
    pub start: usize,
    pub end: usize,
    pub location: Value,
}

pub fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("docx") || e.eq_ignore_ascii_case("pdf"))
}

pub fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn read(path: &Path) -> Result<Projection, String> {
    let bytes = crate::vfs::read(path).map_err(|e| e.to_string())?;
    from_bytes(path, &bytes)
}

pub fn from_bytes(path: &Path, bytes: &[u8]) -> Result<Projection, String> {
    if bytes.len() > MAX_BYTES {
        return Err("documents must be at most 32 MiB".into());
    }
    let format = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut p = Projection {
        format: format.clone(),
        revision: revision(bytes),
        text: String::new(),
        blocks: vec![],
        limitations: vec![],
    };
    match format.as_str() {
        "docx" => {
            check_docx_package(bytes)?;
            let doc = docx_rs::read_docx(bytes).map_err(|e| format!("DOCX reader: {e}"))?;
            // Traverse the library's typed model, not OOXML or ZIP bytes. The JSON
            // view preserves document order and also covers nested table cells.
            let value = serde_json::to_value(&doc.document).map_err(|e| e.to_string())?;
            let styles = serde_json::to_value(&doc.styles).map_err(|e| e.to_string())?;
            let cx = Docx {
                links: doc
                    .hyperlinks
                    .iter()
                    .map(|(id, target, _)| (id.clone(), target.clone()))
                    .collect(),
                levels: heading_styles(&styles),
            };
            docx_blocks(&value, "body", &cx, &mut p)?;
            p.limitations.push("Extraction profile: body paragraphs and table cells only. Headers, footers, notes, shapes, numbering, and Word layout are not certified. Original bytes are never rewritten by this reader.".into());
        }
        "pdf" => {
            let _guard = PDF_LOCK.lock().map_err(|e| e.to_string())?;
            let pdfium = pdfium()?;
            let doc = pdfium
                .load_pdf_from_byte_slice(bytes, None)
                .map_err(|e| format!("PDF reader: {e}"))?;
            for (i, page) in doc.pages().iter().enumerate() {
                let text = page.text().map_err(|e| e.to_string())?;
                if text.all().trim().is_empty() {
                    return Err(format!(
                        "PDF page {} has no extractable text; OCR is required for scanned pages",
                        i + 1
                    ));
                }
                for segment in text.segments().iter() {
                    let b = segment.bounds();
                    p.push(segment.text(), serde_json::json!({
                        "page": i, "pageWidth": page.width().value, "pageHeight": page.height().value,
                        "rect": { "x": b.left().value, "y": page.height().value - b.top().value,
                            "width": b.width().value, "height": b.height().value }
                    }));
                }
            }
            p.limitations.push("Extraction profile: text-bearing PDF pages. Complex reading order, tables, OCR, rotated pages, and vertical writing are not certified.".into());
        }
        _ => return Err("document proof expects DOCX or PDF".into()),
    }
    Ok(p)
}

pub const MAX_BYTES: usize = 32 * 1024 * 1024;

static PDF_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn pdfium() -> Result<&'static Pdfium, String> {
    // All callers hold PDF_LOCK. Keep the library alive across compiler and API reads.
    static INSTANCE: std::sync::OnceLock<Pdfium> = std::sync::OnceLock::new();
    if let Some(instance) = INSTANCE.get() {
        return Ok(instance);
    }
    let configured = std::env::var("JAZYK_PDFIUM")
        .or_else(|_| std::env::var("JAZYK_PROOF_PDFIUM"))
        .ok();
    let bindings = if let Some(path) = configured {
        Pdfium::bind_to_library(path)
    } else {
        let adjacent = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.join("lib")));
        adjacent.and_then(|dir| Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&dir)).ok())
            .map(Ok).unwrap_or_else(Pdfium::bind_to_system_library)
    }.map_err(|e| format!("PDFium is unavailable: {e}. Install PDFium beside jazyk in lib/ or set JAZYK_PDFIUM to its library path."))?;
    let _ = INSTANCE.set(Pdfium::new(bindings));
    Ok(INSTANCE.get().expect("PDFium initialized under lock"))
}

fn check_docx_package(bytes: &[u8]) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("Invalid DOCX package: {e}"))?;
    if archive.len() > 10_000 {
        return Err("DOCX has too many package entries".into());
    }
    let mut expanded = 0u64;
    for i in 0..archive.len() {
        expanded = expanded.saturating_add(archive.by_index(i).map_err(|e| e.to_string())?.size());
        if expanded > 128 * 1024 * 1024 {
            return Err("Expanded DOCX package exceeds 128 MiB".into());
        }
    }
    for part in ["[Content_Types].xml", "word/document.xml"] {
        archive
            .by_name(part)
            .map_err(|_| format!("DOCX is missing {part}"))?;
    }
    Ok(())
}

/// Package validation is distinct from the supported prose extraction profile.
/// A valid document with a drawing can be edited even when compilation refuses it.
pub fn validate(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_BYTES {
        return Err("documents must be at most 32 MiB".into());
    }
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "docx" => {
            check_docx_package(bytes)?;
            docx_rs::read_docx(bytes).map_err(|e| format!("DOCX reader: {e}"))?;
        }
        "pdf" => {
            let _guard = PDF_LOCK.lock().map_err(|e| e.to_string())?;
            pdfium()?
                .load_pdf_from_byte_slice(bytes, None)
                .map_err(|e| format!("PDF reader: {e}"))?;
        }
        _ => return Err("expected DOCX or PDF".into()),
    }
    Ok(())
}

/// Locate a unique quote in the PDF's native text layer, in top-left page points.
pub fn pdf_quote(path: &Path, quote: &str) -> Result<Vec<Value>, String> {
    if quote.trim().is_empty() {
        return Ok(vec![]);
    }
    // Read through the filesystem seam, so a hosted workspace's in-memory tree serves
    // the same bytes the projection read. See docs/frontends/wasm.md#filesystem.
    let bytes = crate::vfs::read(path).map_err(|e| e.to_string())?;
    let _guard = PDF_LOCK.lock().map_err(|e| e.to_string())?;
    let pdfium = pdfium()?;
    let doc = pdfium
        .load_pdf_from_byte_slice(&bytes, None)
        .map_err(|e| e.to_string())?;
    let mut hits = vec![];
    for (i, page) in doc.pages().iter().enumerate() {
        let text = page.text().map_err(|e| e.to_string())?;
        let search = text
            .search(quote, &PdfSearchOptions::new().match_case(true))
            .map_err(|e| e.to_string())?;
        for hit in search.iter(PdfSearchDirection::SearchForward) {
            hits.push(hit.iter().map(|s| {
                let b = s.bounds();
                serde_json::json!({ "page": i, "pageWidth": page.width().value, "pageHeight": page.height().value,
                    "rect": {"x": b.left().value, "y": page.height().value - b.top().value,
                        "width": b.width().value, "height": b.height().value} })
            }).collect::<Vec<_>>());
        }
    }
    Ok(if hits.len() == 1 {
        hits.remove(0)
    } else {
        vec![]
    })
}

impl Projection {
    fn push(&mut self, text: String, location: Value) {
        let text = text.replace("\r\n", "\n");
        if text.trim().is_empty() {
            return;
        }
        if !self.text.is_empty() {
            self.text.push_str("\n\n");
        }
        let start = self.text.len();
        self.text.push_str(&text);
        self.blocks.push(Block {
            text,
            start,
            end: self.text.len(),
            location,
        });
    }
}

// What the reader needs from the package beyond the body: hyperlink relationship
// targets by id, and the heading level of each paragraph style.
struct Docx {
    links: std::collections::HashMap<String, String>,
    levels: std::collections::HashMap<String, usize>,
}

// A heading level from an outline level (0 is level 1; Word's 9 is body text).
fn outline_level(v: &Value) -> Option<usize> {
    v.as_u64().map(|n| n as usize + 1).filter(|n| *n <= 6)
}

// A heading level from a style name or id: `Title`, `Heading1`, `heading 1`.
fn named_level(name: &str) -> Option<usize> {
    let n = name.to_ascii_lowercase().replace(' ', "");
    if n == "title" {
        return Some(1);
    }
    n.strip_prefix("heading")?
        .parse::<usize>()
        .ok()
        .filter(|l| (1..=6).contains(l))
}

fn heading_styles(styles: &Value) -> std::collections::HashMap<String, usize> {
    let mut out = std::collections::HashMap::new();
    for s in styles["styles"].as_array().into_iter().flatten() {
        let Some(id) = s["styleId"].as_str() else {
            continue;
        };
        let level = outline_level(&s["paragraphProperty"]["outlineLvl"])
            .or_else(|| s["name"].as_str().and_then(named_level))
            .or_else(|| named_level(id));
        if let Some(level) = level {
            out.insert(id.to_string(), level);
        }
    }
    out
}

fn docx_blocks(v: &Value, path: &str, cx: &Docx, p: &mut Projection) -> Result<(), String> {
    if v["type"] == "paragraph" {
        let mut text = String::new();
        inline(&v["data"], cx, &mut text)?;
        let property = &v["data"]["property"];
        let level = outline_level(&property["outlineLvl"]).or_else(|| {
            let style = property["style"].as_str()?;
            cx.levels.get(style).copied().or_else(|| named_level(style))
        });
        if let Some(level) = level.filter(|_| !text.trim().is_empty()) {
            text = format!("{} {}", "#".repeat(level), text.trim());
        }
        p.push(
            text,
            serde_json::json!({"part": "word/document.xml", "block": path}),
        );
        return Ok(());
    }
    if let Some(a) = v.as_array() {
        for (i, child) in a.iter().enumerate() {
            docx_blocks(child, &format!("{path}/{i}"), cx, p)?;
        }
    } else if let Some(o) = v.as_object() {
        for key in ["data", "children", "rows", "cells"] {
            if let Some(child) = o.get(key) {
                docx_blocks(child, &format!("{path}/{key}"), cx, p)?;
            }
        }
    }
    Ok(())
}

fn inline(v: &Value, cx: &Docx, out: &mut String) -> Result<(), String> {
    match v["type"].as_str() {
        Some("text") => {
            if let Some(s) = v["data"]["text"].as_str() {
                out.push_str(s);
            }
            return Ok(());
        }
        Some("tab" | "ptab") => {
            out.push('\t');
            return Ok(());
        }
        Some("break" | "carriageReturn") => {
            out.push('\n');
            return Ok(());
        }
        Some("hyperlink") => {
            let link = &v["data"];
            let mut text = String::new();
            inline(&link["children"], cx, &mut text)?;
            let target = match link["type"].as_str() {
                Some("anchor") => link["anchor"].as_str().map(|a| format!("#{a}")),
                _ => link["rid"]
                    .as_str()
                    .and_then(|id| cx.links.get(id).cloned()),
            };
            match target.filter(|t| !t.is_empty() && !text.trim().is_empty()) {
                Some(t) => out.push_str(&format!("[{text}]({})", t.replace(' ', "%20"))),
                None => out.push_str(&text),
            }
            return Ok(());
        }
        Some("insert" | "delete" | "moveFrom" | "moveTo") => return Err(
            "DOCX extraction refuses tracked changes until the revision interpretation is defined"
                .into(),
        ),
        Some("drawing" | "shape" | "footnoteReference" | "sym") => {
            return Err(
                "DOCX extraction encountered unsupported inline content; refusing incomplete prose"
                    .into(),
            )
        }
        _ => {}
    }
    if let Some(a) = v.as_array() {
        for c in a {
            inline(c, cx, out)?;
        }
    } else if let Some(o) = v.as_object() {
        for key in ["data", "children"] {
            if let Some(c) = o.get(key) {
                inline(c, cx, out)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const DOCX: &[u8] = include_bytes!("../tests/fixtures/document-proof/contract.docx");
    const PDF: &[u8] = include_bytes!("../tests/fixtures/document-proof/contract.pdf");

    #[test]
    fn docx_reads_body_table_unicode_and_repeatable_locations() {
        let a = from_bytes(Path::new("contract.docx"), DOCX).unwrap();
        let b = from_bytes(Path::new("contract.DOCX"), DOCX).unwrap();
        assert!(a.text.contains("café & tea < 5; 😀."));
        assert!(a.text.contains("Property\n\nValue\n\nTimeout\n\n3 seconds"));
        assert_eq!(a.revision, b.revision);
        for (x, y) in a.blocks.iter().zip(&b.blocks) {
            assert_eq!(x.location, y.location);
            assert_eq!(&a.text[x.start..x.end], x.text);
        }
    }

    #[test]
    fn docx_headings_and_hyperlinks_project_as_markdown_structure() {
        use docx_rs::{Hyperlink, HyperlinkType, Paragraph, Run};
        let mut bytes = std::io::Cursor::new(Vec::new());
        docx_rs::Docx::new()
            .add_paragraph(
                Paragraph::new()
                    .style("Heading1")
                    .add_run(Run::new().add_text("Gateway")),
            )
            .add_paragraph(
                Paragraph::new()
                    .style("Heading2")
                    .add_run(Run::new().add_text("Timeouts")),
            )
            .add_paragraph(
                Paragraph::new()
                    .add_run(Run::new().add_text("See the "))
                    .add_hyperlink(
                        Hyperlink::new("notes.md", HyperlinkType::External)
                            .add_run(Run::new().add_text("operator notes")),
                    )
                    .add_run(Run::new().add_text(" and "))
                    .add_hyperlink(
                        Hyperlink::new("limits", HyperlinkType::Anchor)
                            .add_run(Run::new().add_text("limits")),
                    )
                    .add_run(Run::new().add_text(".")),
            )
            .build()
            .pack(&mut bytes)
            .unwrap();
        let p = from_bytes(Path::new("gateway.docx"), &bytes.into_inner()).unwrap();
        assert_eq!(
            p.text,
            "# Gateway\n\n## Timeouts\n\nSee the [operator notes](notes.md) and [limits](#limits)."
        );
    }

    #[test]
    fn a_word_package_projects_its_heading_styles_and_relationship_links() {
        let docx = include_bytes!("../tests/fixtures/document-proof/structured.docx");
        let p = from_bytes(Path::new("gateway.docx"), docx).unwrap();
        assert!(p.text.starts_with("# Gateway contract\n\n"));
        assert!(p.text.contains("\n\n## Timeouts\n\n"));
        assert!(p
            .text
            .contains("Operators follow the [operator notes](notes.md)."));
        assert!(p.text.contains("[payment reference](reference.pdf)"));
        assert!(p.text.contains("[jazyk.org](https://jazyk.org)"));
    }

    #[test]
    fn a_docx_only_set_is_dirty_and_hashes_original_bytes() {
        let root =
            std::env::temp_dir().join(format!("jazyk-docx-set-{}", crate::sys::process_id()));
        std::fs::create_dir_all(root.join(".jazyk")).unwrap();
        std::fs::write(root.join("contract.docx"), DOCX).unwrap();
        std::fs::write(root.join(".jazyk/jazyk.toml"), "version = 1\n[[sets]]\nid = 'ds_proof'\nname = 'Proof'\ndocs = ['contract.docx']\nroots = ['contract.docx']\n").unwrap();
        let workspace = crate::document_sets::Workspace::load(&root).unwrap();
        assert_eq!(workspace.summary()["sets"][0]["dirty"], true);
        let project = workspace.resolve(Some("ds_proof")).unwrap();
        let hashes = crate::control::current_doc_hashes(&project);
        assert_eq!(hashes["contract.docx"], crate::task_runtime::digest(DOCX));
        std::fs::remove_dir_all(root).unwrap();
    }

    // A quote lookup reads the PDF through the filesystem seam: inside a hosted
    // workspace a file only the in-memory tree holds is found, a missing one is an
    // ordinary not-found error before the PDF reader is touched.
    #[test]
    fn pdf_quote_reads_through_the_vfs() {
        let mut files = std::collections::BTreeMap::new();
        files.insert("present.pdf".to_string(), PDF.to_vec());
        let _scope = crate::vfs::Scope::enter(&files);
        let err = pdf_quote(Path::new("/project/missing.pdf"), "contract").unwrap_err();
        assert!(
            !err.contains("PDFium") && !err.to_lowercase().contains("pdfium"),
            "{}",
            err
        );
        assert!(crate::vfs::read(Path::new("/project/present.pdf")).is_ok());
    }

    #[test]
    #[ignore = "requires JAZYK_PROOF_PDFIUM pointing to the native PDFium library"]
    fn pdf_quote_finds_a_quote_in_a_workspace_only_pdf() {
        let mut files = std::collections::BTreeMap::new();
        files.insert("contract.pdf".to_string(), PDF.to_vec());
        let _scope = crate::vfs::Scope::enter(&files);
        let hits = pdf_quote(Path::new("/project/contract.pdf"), "contract").unwrap();
        assert!(!hits.is_empty());
    }

    #[test]
    fn malformed_documents_fail_without_a_projection() {
        assert!(from_bytes(Path::new("bad.docx"), b"not a docx").is_err());
        assert!(from_bytes(Path::new("bad.txt"), b"plain text").is_err());
    }

    #[test]
    #[ignore = "requires JAZYK_PROOF_PDFIUM pointing to the native PDFium library"]
    fn pdf_supports_repeated_reads_and_refuses_pages_without_text() {
        for _ in 0..3 {
            let p = from_bytes(Path::new("contract.pdf"), PDF).unwrap();
            assert!(p.text.contains("[contract](missing.md)"));
            assert!(p
                .blocks
                .iter()
                .all(|b| b.location["rect"]["width"].as_f64().unwrap() > 0.));
        }
        let err = from_bytes(
            Path::new("scan.pdf"),
            include_bytes!("../tests/fixtures/document-proof/no-text.pdf"),
        )
        .unwrap_err();
        assert!(err.contains("OCR"));
    }
}
