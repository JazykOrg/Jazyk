use super::*;

mod entities;
mod files;
mod gc;
mod journal;
mod lock;
mod mutations;
mod neighbors;
mod prose;
mod sync;

// A fresh directory per call: parallel tests each hold a store of their own, so a
// commit never finds another test's generation on disk (`store-moved`).
fn tmp() -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "jazyk-store-test-{}-{}",
        crate::sys::process_id(),
        n
    ));
    crate::vfs::remove_dir_all(&dir).ok();
    crate::vfs::create_dir_all(&dir).ok();
    dir
}

// A directory of its own for a test that reloads from disk (the shared tmp dir is
// stomped by parallel tests).
fn own_dir(label: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("jazyk-{}-test-{}", label, crate::sys::process_id()));
    crate::vfs::remove_dir_all(&dir).ok();
    crate::vfs::create_dir_all(&dir).ok();
    dir
}

fn every_kind_graph() -> Store {
    let mut s = Store::default();
    s.graph.entities.insert("ent:order".into(), entity("Order"));
    s.graph
        .entities
        .insert("ent:payment".into(), entity("Payment"));
    s.graph.entities.insert("ent:čaj".into(), entity("Čaj"));
    let mut r = seeded_req("orders.md", "An order holds a payment.");
    r.entities = vec!["ent:order".into(), "ent:payment".into()];
    r.source = Some(mention("orders.md", "/orders", "An order holds a payment."));
    // As a commit leaves it: the requirement's source is a mention on its entities.
    for e in ["ent:order", "ent:payment"] {
        s.graph
            .entities
            .get_mut(e)
            .unwrap()
            .mentions
            .push(r.source.clone().unwrap());
    }
    r.edges = vec![ReqEdge {
        a: "ent:order".into(),
        b: "ent:payment".into(),
        rel_type: Some("association".into()),
        cardinality: None,
    }];
    r.transition = Some(Transition {
        subject: "ent:order".into(),
        from: "open".into(),
        to: "paid".into(),
        trigger: None,
        guard: None,
    });
    s.graph.requirements.insert("req:orders-1".into(), r);
    s.graph.criteria.insert(
        "crit:orders-1".into(),
        Criterion {
            requirement: "req:orders-1".into(),
            statement: "A paid order shows its payment.".into(),
            ..Default::default()
        },
    );
    s.graph.views.insert(
        "view:class/shop".into(),
        View {
            kind: "class".into(),
            title: "Shop".into(),
            members: vec!["ent:order".into(), "ent:payment".into()],
            ..Default::default()
        },
    );
    s.graph
        .diagnostics
        .insert("diag:lint-1".into(), diag("lint", vec!["req:orders-1"]));
    s.graph
        .redirects
        .insert("ent:purchase".into(), "ent:order".into());
    crate::derive::recompute_relationships(&mut s);
    crate::derive::recompute_state_machines(&mut s);
    s
}

fn tree_bytes(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, into: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(root, &p, into);
            } else {
                let rel = p
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                into.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut all = BTreeMap::new();
    walk(dir, dir, &mut all);
    all
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

fn wi() -> WorkItem {
    WorkItem {
        task: "reconcile-doc".into(),
        target: "t.md".into(),
        dirty_sections: vec![],
        stale_anchors: vec![],
        proposals: Vec::new(),
    }
}

fn session() -> Commit {
    wi().commit(1, None)
}

fn mention(doc: &str, sec: &str, quote: &str) -> SourceRef {
    SourceRef {
        doc: doc.into(),
        section: sec.into(),
        quote: quote.into(),
    }
}

fn seed_doc(store: &mut Store, doc: &str, text: &str) {
    let sections = crate::md::parse_sections(text);
    store.docs.insert(
        doc.to_string(),
        DocRecord {
            content_hash: hash_hex(text),
            sections,
            coverage: BTreeMap::new(),
        },
    );
}

// A fixture entity the sweep keeps. The sweep runs behind every commit and deletes
// an entity with nothing on it (no mention, requirement, attribute, child, or
// provenance); a decreed attribute stands in for the mention a real one carries.
fn entity(name: &str) -> Entity {
    Entity {
        name: name.into(),
        attributes: vec![Attribute {
            name: "fixture".into(),
            r#type: None,
            value: None,
            provenance: Provenance::Decree {
                author: "test".into(),
                at: String::new(),
                note: None,
            },
        }],
        ..Default::default()
    }
}

fn create(id: &str, name: &str) -> Op {
    Op::CreateEntity {
        id: id.into(),
        entity: entity(name),
    }
}

fn derived(from: &[&str]) -> Provenance {
    Provenance::Derived {
        from: from.iter().map(|f| f.to_string()).collect(),
        reasoning: "invented structure".into(),
    }
}

fn journal_entry(s: &Store, generation: u64) -> JournalEntry {
    let path = s.out.join("journal").join(format!("g{}.yaml", generation));
    serde_norway::from_str(&crate::vfs::read_to_string(&path).unwrap()).unwrap()
}

// A stated entity with a mention in the seeded section, under an optional parent.
fn stated(name: &str, quote: &str, parent: Option<&str>) -> Entity {
    Entity {
        name: name.into(),
        mentions: vec![mention("t.md", "/t", quote)],
        parent: parent.map(String::from),
        ..Default::default()
    }
}

// A grouping: derived from its members, a definition, no mentions.
fn grouping(name: &str, members: &[&str], parent: Option<&str>) -> Entity {
    Entity {
        name: name.into(),
        definition: Some(format!("{} holds its members.", name)),
        provenance: Some(derived(members)),
        parent: parent.map(String::from),
        ..Default::default()
    }
}

fn move_to(id: &str, parent: &str) -> Op {
    Op::UpdateEntity {
        id: id.into(),
        name: None,
        definition: None,
        add_aliases: Vec::new(),
        remove_aliases: Vec::new(),
        clear_parent: false,
        not_about_add: Vec::new(),
        add_mention: None,
        stereotype: None,
        parent: Some(parent.into()),
        set_attributes: None,
        add_attributes: Vec::new(),
        provenance: None,
    }
}

fn diag(rule: &str, subjects: Vec<&str>) -> Diagnostic {
    Diagnostic {
        rule: rule.into(),
        severity: "error".into(),
        subjects: subjects.into_iter().map(String::from).collect(),
        message: "conflict".into(),
        reasoning: None,
        lifecycle: "open".into(),
        triage: None,
        prompt: None,
        answer: None,
        created: None,
        updated: None,
    }
}

fn seeded_req(doc: &str, statement: &str) -> Requirement {
    Requirement {
        statement: statement.into(),
        entities: vec!["ent:cart".into()],
        source: Some(mention(doc, "/t", "The Cart holds items.")),
        ..Default::default()
    }
}

fn req_at(doc: &str, sec: &str, quote: &str) -> Requirement {
    Requirement {
        statement: format!("The A {}", quote.trim_end_matches('.')),
        entities: vec!["ent:a".into()],
        source: Some(mention(doc, sec, quote)),
        ..Default::default()
    }
}

const CART: &str = "The cart holds items a customer intends to buy. Items stay until checkout. A cart may hold up to fifty items at once.";

const ORDERS: &str = "Orders are placed from a cart. An order records the address and the total. Payment is taken at placement.";
