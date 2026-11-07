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
        if crate::diagnostic_locations::visible(d) {
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

// The same document projection as the LSP and graph location read.
pub fn diag_counts_by_doc(store: &Store) -> BTreeMap<String, BTreeMap<&'static str, usize>> {
    let mut out: BTreeMap<String, BTreeMap<&'static str, usize>> = BTreeMap::new();
    for d in store.graph.diagnostics.values().filter(|d| crate::diagnostic_locations::visible(d)) {
        let sev = match d.severity.as_str() { "error" => "error", "warning" => "warning", _ => "info" };
        let paths: std::collections::BTreeSet<_> = crate::diagnostic_locations::document_locations(store, d)
            .into_iter().filter(|l| l.kind == "document").map(|l| l.path).collect();
        for path in paths { *out.entry(path).or_default().entry(sev).or_default() += 1; }
    }
    out
}
