// Read-only diagnostic locations, shared by counts, editor navigation and LSP.
use crate::model::{Diagnostic, Provenance, SourceRef};
use crate::store::Store;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Location {
    pub path: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    pub related: bool,
}

pub fn visible(d: &Diagnostic) -> bool {
    d.lifecycle == "open"
        && d.triage.as_deref() != Some("suppressed")
        && matches!(d.severity.as_str(), "error" | "warning" | "info")
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.contains(':')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn source(s: &SourceRef, related: bool, out: &mut BTreeSet<Location>) {
    if safe_path(&s.doc) {
        out.insert(Location {
            path: s.doc.clone(),
            kind: "document".into(),
            section: Some(s.section.clone()),
            quote: (!s.quote.is_empty()).then(|| s.quote.clone()),
            line: None,
            related,
        });
    }
}

fn provenance(
    store: &Store,
    p: Option<&Provenance>,
    seen: &mut BTreeSet<String>,
    out: &mut BTreeSet<Location>,
) {
    match p {
        Some(Provenance::Quote(s)) => source(s, true, out),
        Some(Provenance::Derived { from, .. }) => {
            for id in from {
                walk(store, id, true, seen, out);
            }
        }
        _ => {}
    }
}

fn walk(
    store: &Store,
    id: &str,
    related: bool,
    seen: &mut BTreeSet<String>,
    out: &mut BTreeSet<Location>,
) {
    let id = store.resolve_id(id).to_string();
    if !seen.insert(id.clone()) {
        return;
    }
    if let Some(r) = store.graph.requirements.get(&id) {
        if let Some(s) = &r.source {
            source(s, related, out);
        }
        provenance(store, r.provenance.as_ref(), seen, out);
    } else if let Some(e) = store.graph.entities.get(&id) {
        // The defining mention only: a finding about an entity is not about every
        // sentence that names it. Mirrors docs/compiler/model/diagnostic.md#locations.
        if let Some(s) = e.mentions.first() {
            source(s, related, out);
        }
        provenance(store, e.provenance.as_ref(), seen, out);
    } else if let Some(c) = store.graph.criteria.get(&id) {
        if let Some(s) = &c.source {
            source(s, related, out);
        }
        provenance(store, c.provenance.as_ref(), seen, out);
        walk(store, &c.requirement, true, seen, out);
    } else if let Some(r) = store.graph.relationships.get(&id) {
        for id in r.requirements() {
            walk(store, id, true, seen, out);
        }
    } else if let Some(m) = store.graph.state_machines.get(&id) {
        walk(store, &m.subject, true, seen, out);
        for t in &m.transitions {
            for id in &t.requirements {
                walk(store, id, true, seen, out);
            }
        }
    } else if let Some(v) = store.graph.views.get(&id) {
        provenance(store, v.provenance.as_ref(), seen, out);
        for id in &v.members {
            walk(store, id, true, seen, out);
        }
    } else if let Some((doc, section)) = crate::model::split_section_ref(&id) {
        source(
            &SourceRef {
                doc,
                section,
                quote: String::new(),
            },
            related,
            out,
        );
    } else if safe_path(&id) && (store.docs.contains_key(&id) || id.contains('.')) {
        out.insert(Location {
            path: id.clone(),
            kind: if store.docs.contains_key(&id) || crate::md::supported_format(Path::new(&id)) {
                "document"
            } else {
                "file"
            }
            .into(),
            section: None,
            quote: None,
            line: None,
            related,
        });
    }
}

fn deduplicate(locations: BTreeSet<Location>) -> Vec<Location> {
    let mut out = BTreeMap::<Location, bool>::new();
    for mut l in locations {
        let related = l.related;
        l.related = false;
        out.entry(l)
            .and_modify(|old| *old &= related)
            .or_insert(related);
    }
    out.into_iter()
        .map(|(mut l, related)| {
            l.related = related;
            l
        })
        .collect()
}

pub fn subject_locations(store: &Store, id: &str) -> Vec<Location> {
    let mut out = BTreeSet::new();
    walk(store, id, false, &mut BTreeSet::new(), &mut out);
    deduplicate(out)
}

pub fn document_locations(store: &Store, d: &Diagnostic) -> Vec<Location> {
    let (mut seen, mut out) = (BTreeSet::new(), BTreeSet::new());
    subjects(store, d, &mut seen, &mut out);
    deduplicate(out)
}

fn subjects(
    store: &Store,
    d: &Diagnostic,
    seen: &mut BTreeSet<String>,
    out: &mut BTreeSet<Location>,
) {
    // A missing link target is not another affected file.
    let n = if d.rule == "broken-link" {
        1
    } else {
        d.subjects.len()
    };
    // Resolve direct subjects first so indirect traversal cannot hide a direct quote.
    for id in d.subjects.iter().take(n) {
        let mut subject_seen = BTreeSet::new();
        walk(store, id, false, &mut subject_seen, out);
        seen.extend(subject_seen);
    }
    if let Some(p) = &d.prompt {
        for o in &p.options {
            if let Some(e) = &o.edit {
                source(
                    &SourceRef {
                        doc: e.doc.clone(),
                        section: e.section.clone(),
                        quote: e.old_text.clone(),
                    },
                    true,
                    out,
                );
            }
        }
    }
}

pub fn locations(
    store: &Store,
    root: &Path,
    gs: &crate::gen::GenSettings,
) -> BTreeMap<String, Vec<Location>> {
    let ledger = crate::gen::Ledger::load(&store.out);
    store
        .graph
        .diagnostics
        .iter()
        .map(|(id, d)| {
            let (mut seen, mut out) = (BTreeSet::new(), BTreeSet::new());
            subjects(store, d, &mut seen, &mut out);
            let mut file = |base: &Path, path: &str, line: Option<usize>| {
                if !safe_path(path) {
                    return;
                }
                let full = base.join(path);
                if let Ok(rel) = full.strip_prefix(root) {
                    let path = rel.to_string_lossy().replace('\\', "/");
                    if safe_path(&path) {
                        out.insert(Location {
                            path,
                            kind: "file".into(),
                            section: None,
                            quote: None,
                            line,
                            related: true,
                        });
                    }
                }
            };
            for (eid, entity) in &ledger.entities {
                if seen.contains(store.resolve_id(eid)) {
                    for f in &entity.files {
                        file(&gs.deliverable, f, None);
                    }
                }
            }
            for (rid, row) in &ledger.requirements {
                let owns = seen.contains(store.resolve_id(rid))
                    || seen.contains(store.resolve_id(&row.entity));
                let criteria: Vec<_> = row
                    .tests
                    .iter()
                    .filter(|(cid, _)| seen.contains(store.resolve_id(cid)))
                    .collect();
                if owns || !criteria.is_empty() {
                    for f in &row.files {
                        file(&gs.deliverable, f, None);
                    }
                    for site in &row.sites {
                        file(&gs.deliverable, &site.file, Some(site.line));
                    }
                    let tests =
                        std::iter::once(&row.test).chain(row.tests.values().map(|t| &t.test));
                    for test in tests {
                        let artifact = crate::gen::artifact_path(&store.out, gs, test);
                        if let Ok(rel) = artifact.strip_prefix(root) {
                            file(root, &rel.to_string_lossy().replace('\\', "/"), None);
                        }
                    }
                }
            }
            (id.clone(), deduplicate(out))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Store {
        let mut s = Store::default();
        s.graph.requirements.insert("req:one".into(), serde_json::from_value(json!({
            "statement":"Orders complete", "entities":[], "source":{"doc":"docs/rules.md","section":"/rules","quote":"Orders complete."}
        })).unwrap());
        s.graph.criteria.insert("crit:one".into(), serde_json::from_value(json!({
            "requirement":"req:one", "statement":"Returns 200", "source":{"doc":"docs/examples.md","section":"/example","quote":"Returns 200."}
        })).unwrap());
        s.graph.entities.insert("ent:one".into(), serde_json::from_value(json!({
            "name":"Order", "mentions":[{"doc":"docs/rules.md","section":"/rules","quote":"Orders"},{"doc":"docs/rules.md","section":"/other","quote":"Order"}]
        })).unwrap());
        s
    }
    fn finding(subjects: &[&str]) -> Diagnostic {
        serde_json::from_value(json!({"rule":"contradiction", "severity":"warning", "message":"Conflict", "subjects":subjects})).unwrap()
    }

    #[test]
    fn an_entity_finding_locates_on_the_defining_mention_only() {
        let mut s = fixture();
        s.graph.entities.get_mut("ent:one").unwrap().mentions.push(
            serde_json::from_value(
                json!({"doc":"docs/other.md","section":"/x","quote":"An order"}),
            )
            .unwrap(),
        );
        let found = document_locations(&s, &finding(&["ent:one"]));
        assert_eq!(found.len(), 1);
        assert_eq!(
            (
                found[0].path.as_str(),
                found[0].section.as_deref(),
                found[0].quote.as_deref()
            ),
            ("docs/rules.md", Some("/rules"), Some("Orders"))
        );
        assert!(!found[0].related);
        // An entity no document states has no location: the finding stays project-wide.
        s.graph
            .entities
            .get_mut("ent:one")
            .unwrap()
            .mentions
            .clear();
        assert!(document_locations(&s, &finding(&["ent:one"])).is_empty());
    }

    #[test]
    fn criteria_redirects_and_repeated_subjects_count_each_document_once() {
        let mut s = fixture();
        s.graph
            .redirects
            .insert("crit:old".into(), "crit:one".into());
        s.graph.diagnostics.insert(
            "diag:one".into(),
            finding(&["crit:old", "req:one", "ent:one", "docs/rules.md#/rules"]),
        );
        let counts = crate::webapi::status::diag_counts_by_doc(&s);
        assert_eq!(counts["docs/rules.md"]["warning"], 1);
        assert_eq!(counts["docs/examples.md"]["warning"], 1);
        s.graph.diagnostics.get_mut("diag:one").unwrap().triage = Some("suppressed".into());
        assert!(crate::webapi::status::diag_counts_by_doc(&s).is_empty());
    }

    #[test]
    fn provenance_cycles_and_graph_projections_reach_sources_and_preserve_global_findings() {
        let mut s = fixture();
        s.graph.views.insert("view:one".into(), serde_json::from_value(json!({"kind":"class","title":"Orders", "members":["crit:one"], "provenance":{"derived":{"from":["view:two"],"reasoning":"context"}}})).unwrap());
        s.graph.views.insert("view:two".into(), serde_json::from_value(json!({"kind":"class","title":"Cycle", "provenance":{"derived":{"from":["view:one"],"reasoning":"context"}}})).unwrap());
        s.graph.relationships.insert("rel:one".into(), serde_json::from_value(json!({"members":["ent:one"],"contributions":[{"type":"dependency","a":"ent:one","b":"ent:one","requirements":["req:one"]}]})).unwrap());
        s.graph.state_machines.insert("sm:one".into(), serde_json::from_value(json!({"subject":"ent:one","transitions":[{"from":"new","to":"done","requirements":["req:one"]}]})).unwrap());
        for id in ["view:one", "rel:one", "sm:one"] {
            let loc = document_locations(&s, &finding(&[id]));
            assert!(loc.iter().any(|l| l.path == "docs/rules.md"), "{id}");
            assert!(loc.iter().all(|l| l.related));
        }
        assert!(document_locations(&s, &finding(&[])).is_empty());
    }

    #[test]
    fn aggregate_read_qualifies_diagnostic_ids_without_qualifying_physical_paths() {
        let mut s = fixture();
        s.graph
            .diagnostics
            .insert("diag:one".into(), finding(&["crit:one"]));
        let gs = crate::gen::GenSettings::from_out(Path::new("/project/.jazyk/ds_main"));
        let value = serde_json::to_value(locations(&s, Path::new("/project"), &gs)).unwrap();
        let qualified = crate::webapi::sets::qualify_value(value, "ds_main");
        assert_eq!(
            qualified["diag:ds_main::one"][0]["path"],
            "docs/examples.md"
        );
        assert_eq!(qualified["diag:ds_main::one"][1]["path"], "docs/rules.md");
    }

    #[test]
    fn proposed_edits_add_locations_even_with_a_located_subject_and_missing_targets_are_not_charged(
    ) {
        let s = fixture();
        let mut d = finding(&["req:one"]);
        d.prompt = Some(serde_json::from_value(json!({"question":"Which?", "options":[{"label":"Rewrite", "edit":{"doc":"docs/other.md","section":"/other","old_text":"Old.","new_text":"New."}}],"freeform":true})).unwrap());
        assert!(document_locations(&s, &d)
            .iter()
            .any(|l| l.path == "docs/other.md"));
        d.rule = "broken-link".into();
        d.prompt = None;
        d.subjects = vec!["docs/rules.md".into(), "docs/missing.md".into()];
        assert_eq!(document_locations(&s, &d).len(), 1);
        for severity in ["none", "unknown"] {
            d.severity = severity.into();
            assert!(!visible(&d));
        }
        d.severity = "error".into();
        d.lifecycle = "resolved".into();
        assert!(!visible(&d));
    }

    #[test]
    fn ledger_locations_include_code_sites_and_each_criterion_test_without_path_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mut s = fixture();
        s.out = root.join(".jazyk/ds_main");
        s.graph
            .diagnostics
            .insert("diag:one".into(), finding(&["crit:one"]));
        s.graph.redirects.insert("req:old".into(), "req:one".into());
        s.graph
            .redirects
            .insert("crit:old".into(), "crit:one".into());
        let ledger: crate::gen::Ledger = serde_json::from_value(json!({"requirements":{"req:old":{
            "entity":"ent:one", "files":["src/order.rs","../escape.rs"],"sites":[{"file":"src/order.rs","line":8,"head":"fn order() {}"}],
            "test":{"kind":"programmatic","label":"orders","artifact":"tests/order.rs","name":"orders","run":"cargo test"},
            "tests":{"crit:old":{"test":{"kind":"programmatic","label":"criterion","artifact":"tests/criterion.rs","name":"criterion","run":"cargo test"},"criterion_hash":"h"}},"hashes":{"requirement":"h","test":"h","files":"h"}
        }}})).unwrap();
        ledger.save(&s.out).unwrap();
        let mut gs = crate::gen::GenSettings::from_out(&s.out);
        gs.deliverable = root.join("product");
        let all = locations(&s, root, &gs);
        let paths: BTreeSet<_> = all["diag:one"].iter().map(|l| l.path.as_str()).collect();
        assert!(paths.contains("product/src/order.rs"));
        assert!(paths.contains("product/tests/criterion.rs"));
        assert!(paths.contains("product/tests/order.rs"));
        assert!(all["diag:one"].iter().any(|l| l.line == Some(8)));
        assert!(all["diag:one"].iter().all(|l| !l.path.contains("..")));
    }
}
