//! Original binary files and native projections for format-specific editors.
use super::Refusal;
use crate::project::Project;
use serde_json::Value;

pub fn available() -> bool {
    cfg!(all(
        feature = "native-documents",
        not(target_arch = "wasm32")
    )) && !crate::vfs::is_scoped()
}

#[cfg(all(test, feature = "native-documents", not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde_json::json;

    fn project() -> (tempfile::TempDir, Project) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".jazyk")).unwrap();
        std::fs::write(dir.path().join(".jazyk/jazyk.toml"), "version = 1\n[[sets]]\nid = 'ds_native'\nname = 'Native documents'\ndocs = ['docs/**']\n").unwrap();
        std::fs::create_dir(dir.path().join("docs")).unwrap();
        std::fs::write(
            dir.path().join("docs/spec.docx"),
            include_bytes!("../../tests/fixtures/document-proof/contract.docx"),
        )
        .unwrap();
        let project = Project::load(dir.path());
        (dir, project)
    }

    fn edited() -> Vec<u8> {
        let mut output = std::io::Cursor::new(Vec::new());
        docx_rs::Docx::new()
            .add_paragraph(
                docx_rs::Paragraph::new()
                    .add_run(docx_rs::Run::new().add_text("Edited native contract")),
            )
            .build()
            .pack(&mut output)
            .unwrap();
        output.into_inner()
    }

    #[test]
    fn reads_real_diagnostics_and_saves_original_bytes_with_revision_checks() {
        let (_dir, p) = project();
        let before = read(&p, "docs/spec.docx").unwrap();
        assert_eq!(before["writable"], true);
        assert!(before["projection"]["text"]
            .as_str()
            .unwrap()
            .contains("café"));
        assert!(before["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "broken-link" && !d["locations"].as_array().unwrap().is_empty()));
        let bytes = edited();
        let saved = write(
            &p,
            "docs/spec.docx",
            &json!({"baseHash": before["revision"], "base64": STANDARD.encode(&bytes)}),
        )
        .unwrap();
        assert_eq!(std::fs::read(p.root.join("docs/spec.docx")).unwrap(), bytes);
        assert_eq!(
            read(&p, "docs/spec.docx").unwrap()["projection"]["text"],
            "Edited native contract"
        );
        assert_eq!(
            write(
                &p,
                "docs/spec.docx",
                &json!({"baseHash": before["revision"], "base64": before["base64"]})
            )
            .unwrap_err()
            .0,
            409
        );
        assert_eq!(write(&p, "docs/spec.docx", &json!({"baseHash": saved["revision"], "base64": STANDARD.encode(b"invalid package")})).unwrap_err().0, 422);
        assert_eq!(std::fs::read(p.root.join("docs/spec.docx")).unwrap(), bytes);
    }

    #[test]
    fn imports_exclusively_and_refuses_pdf_replacement_or_paths_outside_documents() {
        let (_dir, p) = project();
        let content = json!({"base64": STANDARD.encode(edited())});
        write(&p, "docs/import.docx", &content).unwrap();
        assert_eq!(write(&p, "docs/import.docx", &content).unwrap_err().0, 409);
        assert_eq!(write(&p, "outside.docx", &content).unwrap_err().0, 400);
        assert_eq!(write(&p, "../escape.docx", &content).unwrap_err().0, 400);
        assert_eq!(read(&p, "../escape.docx").unwrap_err().0, 400);
        std::fs::write(p.root.join("docs/reference.pdf"), b"original pdf bytes").unwrap();
        assert_eq!(
            write(&p, "docs/reference.pdf", &content).unwrap_err().0,
            405
        );
        assert_eq!(
            std::fs::read(p.root.join("docs/reference.pdf")).unwrap(),
            b"original pdf bytes"
        );
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_and_nested_project_writes() {
        let (_dir, p) = project();
        let other = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(other.path(), p.root.join("docs/link")).unwrap();
        let content = json!({"base64": STANDARD.encode(edited())});
        assert!(write(&p, "docs/link/escape.docx", &content).is_err());
        std::fs::create_dir_all(p.root.join("docs/nested/.jazyk")).unwrap();
        std::fs::write(p.root.join("docs/nested/.jazyk/jazyk.toml"), "version=1\n").unwrap();
        assert!(write(&p, "docs/nested/escape.docx", &content).is_err());
        assert!(!other.path().join("escape.docx").exists());
    }

    #[test]
    #[ignore = "requires JAZYK_PDFIUM pointing to the native PDFium library"]
    fn imports_pdf_bytes_and_previews_a_pdf_without_compilable_text() {
        let (_dir, p) = project();
        for (path, bytes) in [
            (
                "docs/reference.pdf",
                include_bytes!("../../tests/fixtures/document-proof/contract.pdf").as_slice(),
            ),
            (
                "docs/blank.pdf",
                include_bytes!("../../tests/fixtures/document-proof/no-text.pdf").as_slice(),
            ),
        ] {
            write(&p, path, &json!({"base64": STANDARD.encode(bytes)})).unwrap();
            let document = read(&p, path).unwrap();
            assert_eq!(document["writable"], false);
            assert_eq!(document["base64"], STANDARD.encode(bytes));
            if path.ends_with("blank.pdf") {
                assert!(document["projection"].is_null());
                assert!(document["extractionError"]
                    .as_str()
                    .unwrap()
                    .contains("OCR"));
            } else {
                assert_eq!(document["diagnostics"][0]["locations"][0]["page"], 0);
            }
        }
    }
}

pub fn read(project: &Project, path: &str) -> Result<Value, Refusal> {
    if !available() {
        return Err((501, "DOCX and PDF need a native Jazyk connection".into()));
    }
    #[cfg(all(feature = "native-documents", not(target_arch = "wasm32")))]
    return native::read(project, path);
    #[cfg(not(all(feature = "native-documents", not(target_arch = "wasm32"))))]
    {
        let _ = (project, path);
        unreachable!()
    }
}

pub fn write(project: &Project, path: &str, body: &Value) -> Result<Value, Refusal> {
    if !available() {
        return Err((501, "DOCX and PDF need a native Jazyk connection".into()));
    }
    #[cfg(all(feature = "native-documents", not(target_arch = "wasm32")))]
    return native::write(project, path, body);
    #[cfg(not(all(feature = "native-documents", not(target_arch = "wasm32"))))]
    {
        let _ = (project, path, body);
        unreachable!()
    }
}

#[cfg(all(feature = "native-documents", not(target_arch = "wasm32")))]
mod native {
    use super::*;
    use crate::{documents, lsp::Lsp};
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde_json::json;
    use std::io::Write;
    use std::path::Path;

    fn position(text: &str, p: &Value) -> Option<usize> {
        let line = p["line"].as_u64()? as usize;
        let col = p["character"].as_u64()? as usize;
        let mut base = 0;
        for (i, text) in text.split('\n').enumerate() {
            if i == line {
                let mut n = 0;
                for (offset, c) in text.char_indices() {
                    if n == col {
                        return Some(base + offset);
                    }
                    n += c.len_utf16();
                }
                return (n == col).then_some(base + text.len());
            }
            base += text.len() + 1;
        }
        None
    }

    pub fn read(project: &Project, path: &str) -> Result<Value, Refusal> {
        if !documents::supported(Path::new(path)) {
            return Err((400, "expected DOCX or PDF".into()));
        }
        let bytes = super::super::projfiles::project_file(&project.root, path)?;
        if bytes.len() > documents::MAX_BYTES {
            return Err((413, "documents must be at most 32 MiB".into()));
        }
        let revision = documents::revision(&bytes);
        let absolute = project.root.join(path);
        let format = Path::new(path)
            .extension()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase();
        let projection = documents::from_bytes(&absolute, &bytes);
        let mut findings = Vec::new();
        if let Ok(p) = &projection {
            let lsp = Lsp::new(
                project.root.clone(),
                project.root.join(".jazyk"),
                crate::gen::GenSettings::resolve(project),
            );
            findings = lsp.document_diagnostics(path);
            for d in &mut findings {
                let range = &d["range"];
                let quote = position(&p.text, &range["start"])
                    .zip(position(&p.text, &range["end"]))
                    .and_then(|(s, e)| p.text.get(s..e))
                    .unwrap_or("");
                d["quote"] = json!(quote);
                d["revision"] = json!(revision);
                d["locations"] = if !quote.is_empty() && p.text.match_indices(quote).count() == 1 {
                    if format == "pdf" {
                        json!(documents::pdf_quote(&absolute, quote).map_err(|e| (422, e))?)
                    } else {
                        json!(p
                            .blocks
                            .iter()
                            .filter(|b| b.text.contains(quote))
                            .map(|b| &b.location)
                            .collect::<Vec<_>>())
                    }
                } else {
                    json!([])
                };
            }
        }
        // Diagnostics perform their own reads. Never label another revision's ranges
        // with the bytes captured at the start of this request.
        let now = std::fs::read(&absolute).map_err(|e| (409, e.to_string()))?;
        if documents::revision(&now) != revision {
            return Err((409, "document changed while being read; reopen it".into()));
        }
        let writable =
            format == "docx" && super::super::docs::safe_doc_path(project, path).is_some();
        let (projection, extraction_error) = match projection {
            Ok(p) => (json!(p), Value::Null),
            Err(e) => (Value::Null, json!(e)),
        };
        Ok(
            json!({"path": path, "format": format, "base64": STANDARD.encode(bytes),
            "revision": revision, "writable": writable, "projection": projection,
            "extractionError": extraction_error, "diagnostics": findings}),
        )
    }

    pub fn write(project: &Project, path: &str, body: &Value) -> Result<Value, Refusal> {
        if !documents::supported(Path::new(path)) {
            return Err((400, "expected DOCX or PDF".into()));
        }
        let absolute =
            super::super::docs::safe_doc_path_why(project, path).map_err(|e| (400, e))?;
        let _lock = crate::document_sets::ProjectLock::acquire(&project.root, "documents.lock")?;
        let current = match std::fs::read(&absolute) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err((409, format!("cannot read current document: {e}"))),
        };
        if current.is_some() && path.to_ascii_lowercase().ends_with(".pdf") {
            return Err((405, "PDF is read-only".into()));
        }
        let expected = body.get("baseHash").and_then(Value::as_str);
        let current_hash = current.as_deref().map(documents::revision);
        if current_hash.as_deref() != expected {
            return Err((
                409,
                "the document changed on disk; reload or download your edited copy".into(),
            ));
        }
        let encoded = body["base64"]
            .as_str()
            .ok_or((400, "base64 is required".into()))?;
        if encoded.len() > documents::MAX_BYTES.div_ceil(3) * 4 {
            return Err((413, "documents must be at most 32 MiB".into()));
        }
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| (400, "invalid base64 document".into()))?;
        documents::validate(&absolute, &bytes).map_err(|e| (422, e))?;
        let parent = absolute
            .parent()
            .ok_or((400, "document has no parent".into()))?;
        std::fs::create_dir_all(parent).map_err(|e| (500, e.to_string()))?;
        super::super::docs::safe_doc_path_why(project, path).map_err(|e| (400, e))?;
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| (500, e.to_string()))?;
        temp.write_all(&bytes)
            .and_then(|_| temp.as_file().sync_all())
            .map_err(|e| (500, e.to_string()))?;
        if current.is_some() {
            let latest = std::fs::read(&absolute).map_err(|e| (409, e.to_string()))?;
            if Some(documents::revision(&latest)).as_deref() != expected {
                return Err((409, "document changed during save".into()));
            }
            let permissions = std::fs::metadata(&absolute)
                .map_err(|e| (409, e.to_string()))?
                .permissions();
            temp.as_file()
                .set_permissions(permissions)
                .map_err(|e| (500, e.to_string()))?;
            temp.persist(&absolute).map_err(|e| (500, e.to_string()))?;
        } else {
            temp.persist_noclobber(&absolute)
                .map_err(|e| (409, format!("cannot create document exclusively: {e}")))?;
        }
        Ok(json!({"path": path, "revision": documents::revision(&bytes)}))
    }
}
