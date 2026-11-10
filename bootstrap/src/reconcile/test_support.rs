use super::*;
use crate::model::{hash_hex, DocRecord, Requirement, SourceRef};
use crate::project::Project;
use crate::store::Store;
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn seed_doc(store: &mut Store, doc: &str, text: &str) {
    store.docs.insert(
        doc.to_string(),
        DocRecord {
            content_hash: hash_hex(text),
            sections: crate::md::parse_sections(text),
            coverage: BTreeMap::new(),
        },
    );
}

pub(super) fn req(
    statement: &str,
    entity: &str,
    doc: &str,
    section: &str,
    quote: &str,
) -> Requirement {
    Requirement {
        statement: statement.into(),
        entities: vec![entity.into()],
        edges: Vec::new(),
        source: Some(SourceRef {
            doc: doc.into(),
            section: section.into(),
            quote: quote.into(),
        }),
        ..Default::default()
    }
}

pub(super) fn rules_for<'a>(f: &'a [Finding], rule: &str) -> Vec<&'a Finding> {
    f.iter().filter(|(r, _, _, _, _)| r == rule).collect()
}

pub(super) fn journal_dir() -> std::path::PathBuf {
    // A counter, not the clock: parallel tests read the same clock value.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "jazyk-flip-test-{}-{}",
        crate::sys::process_id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    crate::vfs::create_dir_all(dir.join("journal")).unwrap();
    dir
}

pub(super) fn write_entry(
    dir: &Path,
    generation: u64,
    batch: &[&str],
    mutations: Vec<Value>,
    justification: &str,
) {
    let entry = JournalEntry {
        build: format!("g{}", generation),
        generation,
        kind: "session".into(),
        batch: batch.iter().map(|s| s.to_string()).collect(),
        mutations,
        resolved_goals: batch
            .iter()
            .map(|g| Resolved {
                goal: g.to_string(),
                justification: justification.into(),
                evidence: Value::Null,
            })
            .collect(),
        ..Default::default()
    };
    crate::vfs::write(
        dir.join("journal").join(format!("g{}.yaml", generation)),
        serde_norway::to_string(&entry).unwrap(),
    )
    .unwrap();
}

// A two-level tree with every level view in place: two «system» roots (the scope
// root's level view is the component form) and a backend holding two children.
pub(super) fn level_store() -> Store {
    let mut s = Store::default();
    let ent = |name: &str, parent: Option<&str>, stereotype: Option<&str>| Entity {
        name: name.into(),
        parent: parent.map(String::from),
        stereotype: stereotype.map(String::from),
        mentions: vec![SourceRef {
            doc: "arch.md".into(),
            section: "/arch".into(),
            quote: name.into(),
        }],
        ..Default::default()
    };
    s.graph
        .entities
        .insert("ent:frontend".into(), ent("Frontend", None, Some("system")));
    s.graph
        .entities
        .insert("ent:backend".into(), ent("Backend", None, Some("system")));
    s.graph.entities.insert(
        "ent:server".into(),
        ent("Server", Some("ent:backend"), None),
    );
    s.graph.entities.insert(
        "ent:database".into(),
        ent("Database", Some("ent:backend"), None),
    );
    let view = |kind: &str, title: &str, members: &[&str]| View {
        kind: kind.into(),
        title: title.into(),
        members: members.iter().map(|m| m.to_string()).collect(),
        default: true,
        ..Default::default()
    };
    s.graph.views.insert(
        "view:component/public".into(),
        view("component", "Public", &["ent:frontend", "ent:backend"]),
    );
    s.graph.views.insert(
        "view:class/backend".into(),
        view("class", "Backend", &["ent:server", "ent:database"]),
    );
    s
}

pub(super) fn level_shape(s: &Store) -> Vec<Finding> {
    rules_for(&checks(s, &Project::default()), "level-shape")
        .into_iter()
        .cloned()
        .collect()
}
