//! Optional, native-only document evaluation. See docs/compiler/document-proof.md.
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
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("document proof accepts files up to 32 MiB".into());
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
            let doc = docx_rs::read_docx(bytes).map_err(|e| format!("DOCX reader: {e}"))?;
            // Traverse the library's typed model, not OOXML or ZIP bytes. The JSON
            // view preserves document order and also covers nested table cells.
            let value = serde_json::to_value(&doc.document).map_err(|e| e.to_string())?;
            docx_blocks(&value, "body", &mut p)?;
            p.limitations.push("Evaluation profile: body paragraphs and table cells only. Headers, footers, notes, shapes, numbering, and Word layout are not certified. Original bytes are never rewritten by this reader.".into());
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
            p.limitations.push("Evaluation profile: text-bearing PDF pages. Complex reading order, tables, OCR, rotated pages, and vertical writing are not certified.".into());
        }
        _ => return Err("document proof expects DOCX or PDF".into()),
    }
    Ok(p)
}

static PDF_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn pdfium() -> Result<&'static Pdfium, String> {
    // pdfium-render installs bindings once per process. Keep the corresponding
    // engine alive across parse, checks, LSP reads, and geometry queries.
    static INSTANCE: std::sync::OnceLock<Result<Pdfium, String>> = std::sync::OnceLock::new();
    INSTANCE
        .get_or_init(|| {
            let lib = std::env::var("JAZYK_PROOF_PDFIUM")
                .map_err(|_| "set JAZYK_PROOF_PDFIUM to the PDFium library path".to_string())?;
            Pdfium::bind_to_library(lib)
                .map(Pdfium::new)
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Locate a unique quote in the PDF's native text layer, in top-left page points.
pub fn pdf_quote(path: &Path, quote: &str) -> Result<Vec<Value>, String> {
    if quote.trim().is_empty() {
        return Ok(vec![]);
    }
    let _guard = PDF_LOCK.lock().map_err(|e| e.to_string())?;
    let pdfium = pdfium()?;
    let doc = pdfium
        .load_pdf_from_file(path, None)
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

fn docx_blocks(v: &Value, path: &str, p: &mut Projection) -> Result<(), String> {
    if v["type"] == "paragraph" {
        let mut text = String::new();
        inline(&v["data"], &mut text)?;
        p.push(
            text,
            serde_json::json!({"part": "word/document.xml", "block": path}),
        );
        return Ok(());
    }
    if let Some(a) = v.as_array() {
        for (i, child) in a.iter().enumerate() {
            docx_blocks(child, &format!("{path}/{i}"), p)?;
        }
    } else if let Some(o) = v.as_object() {
        for key in ["data", "children", "rows", "cells"] {
            if let Some(child) = o.get(key) {
                docx_blocks(child, &format!("{path}/{key}"), p)?;
            }
        }
    }
    Ok(())
}

fn inline(v: &Value, out: &mut String) -> Result<(), String> {
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
        Some("insert" | "delete" | "moveFrom" | "moveTo") => {
            return Err(
                "DOCX proof refuses tracked changes until the revision interpretation is defined"
                    .into(),
            )
        }
        Some("drawing" | "shape" | "footnoteReference" | "sym") => {
            return Err(
                "DOCX proof encountered unsupported inline content; refusing incomplete prose"
                    .into(),
            )
        }
        _ => {}
    }
    if let Some(a) = v.as_array() {
        for c in a {
            inline(c, out)?;
        }
    } else if let Some(o) = v.as_object() {
        for key in ["data", "children"] {
            if let Some(c) = o.get(key) {
                inline(c, out)?;
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
    fn a_docx_only_set_is_dirty_and_hashes_the_compiler_projection() {
        let root =
            std::env::temp_dir().join(format!("jazyk-docx-set-{}", crate::sys::process_id()));
        std::fs::create_dir_all(root.join(".jazyk")).unwrap();
        std::fs::write(root.join("contract.docx"), DOCX).unwrap();
        std::fs::write(root.join(".jazyk/jazyk.toml"), "version = 1\n[[sets]]\nid = 'ds_proof'\nname = 'Proof'\ndocs = ['contract.docx']\nroots = ['contract.docx']\n").unwrap();
        let workspace = crate::document_sets::Workspace::load(&root).unwrap();
        assert_eq!(workspace.summary()["sets"][0]["dirty"], true);
        let project = workspace.resolve(Some("ds_proof")).unwrap();
        let hashes = crate::control::current_doc_hashes(&project);
        let projection = from_bytes(Path::new("contract.docx"), DOCX).unwrap();
        assert_eq!(
            hashes["contract.docx"],
            crate::model::hash_hex(&projection.text)
        );
        std::fs::remove_dir_all(root).unwrap();
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
