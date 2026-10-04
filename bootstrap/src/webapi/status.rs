// The status value, the document-relative path, and the per-document diagnostic
// counts. Target-neutral (docs/frontends/wasm.md#one-api-two-transports).
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn status_value(store: &Store) -> Value {
    let (mut total, mut covered) = (0usize, 0usize);
    for rec in store.docs.values() {
        for (r, sec) in &rec.sections {
            if sec.raw.lines().skip(1).all(|l| l.trim().is_empty()) {
                continue;
            }
            total += 1;
            if rec.coverage.contains_key(r) {
                covered += 1;
            }
        }
    }
    let mut by_sev: std::collections::BTreeMap<&str, usize> = Default::default();
    for d in store.graph.diagnostics.values() {
        if d.lifecycle == "open" && d.triage.as_deref() != Some("suppressed") {
            *by_sev.entry(d.severity.as_str()).or_default() += 1;
        }
    }
    json!({
        "version": store.status.version,
        "generation": store.status.generation,
        "verdict": store.status.verdict,
        "spent": store.status.spent,
        "parked": store.status.parked,
        "failed": store.status.failed,
        "costs": store.status.costs,
        "counts": {
            "entities": store.graph.entities.len(),
            "requirements": store.graph.requirements.len(),
            "relationships": store.graph.relationships.len(),
            "views": store.graph.views.len(),
        },
        "coverage": { "covered": covered, "total": total },
        "diagnostics": by_sev,
    })
}

// Relative doc path under the project root, forward slashes. The DocRecord key.
pub fn rel_doc(root: &std::path::Path, f: &std::path::Path) -> String {
    f.strip_prefix(root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| f.to_string_lossy().to_string())
}

// Open non-suppressed diagnostics counted per document. A diagnostic counts toward a
// document when a subject anchors there, the same mapping the LSP publishes:
// a requirement whose source is the document, an entity with a mention in it, or a
// section reference into it.
pub fn diag_counts_by_doc(store: &Store) -> BTreeMap<String, BTreeMap<&'static str, usize>> {
    let mut out: BTreeMap<String, BTreeMap<&'static str, usize>> = BTreeMap::new();
    for d in store.graph.diagnostics.values() {
        if d.lifecycle != "open" || d.triage.as_deref() == Some("suppressed") {
            continue;
        }
        let sev: &'static str = match d.severity.as_str() {
            "error" => "error",
            "warning" => "warning",
            "info" => "info",
            _ => "none",
        };
        let mut docs_hit: std::collections::BTreeSet<String> = Default::default();
        if d.rule == "broken-link" {
            // The second subject identifies the missing target, not another
            // source document to charge for this finding.
            if let Some(doc) = d.subjects.first() {
                *out.entry(doc.clone()).or_default().entry(sev).or_default() += 1;
            }
            continue;
        }
        for subject in &d.subjects {
            let resolved = store.resolve_id(subject).to_string();
            if let Some(r) = store.graph.requirements.get(&resolved) {
                if let Some(src) = r.source.as_ref() {
                    docs_hit.insert(src.doc.clone());
                }
            } else if let Some(e) = store.graph.entities.get(&resolved) {
                for m in &e.mentions {
                    docs_hit.insert(m.doc.clone());
                }
            } else if let Some((sdoc, _)) = crate::model::split_section_ref(&resolved) {
                docs_hit.insert(sdoc);
            }
        }
        for doc in docs_hit {
            *out.entry(doc).or_default().entry(sev).or_default() += 1;
        }
    }
    out
}
