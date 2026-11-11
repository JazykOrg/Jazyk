use super::*;
use crate::store::{self, RecordBatch};

mod batching;
mod cones;
mod derivation;
mod gc;

pub(crate) fn project_for(store: &Store) -> Project {
    Project {
        out: store.out.clone(),
        ..Default::default()
    }
}

pub(crate) fn record(
    store: &mut Store,
    generation: u64,
    kind: &str,
    subject: &str,
    via: &str,
    detail: Value,
) {
    let mut batch = RecordBatch::new(generation);
    batch.push(0, kind, subject, via, detail);
    for r in batch.records() {
        store.status.record_change(r.clone());
    }
}

// Compile released, generation manual: the graph goals run, the ledger goals wait.
pub(crate) fn control_auto() -> Control {
    Control {
        compile: "auto".into(),
        generate: "manual".into(),
        worker: "any".into(),
        released: Default::default(),
    }
}

fn derive(store: &Store) -> Board {
    Board::derive(store, &project_for(store), &control_auto())
}

// The showcase with every section covered and a generation behind it: the store a
// converged build leaves. The document carries every requirement's quote so no
// anchor reads stale, and the derived data (relationships, machines, default
// views) stands as a commit leaves it.
pub(crate) fn settled_store() -> Store {
    let mut s = crate::derive::tests::showcase_store();
    let text = "# Shop\n\nThe shop.\nThe order service provides the checkout API.\nThe inventory service provides the stock API.\nA shopping cart holds one or more order items.\nThe shop shall confirm checkout within 2 seconds.\nThe shop is deployed in the EU region.\n\n## Checkout\n\nThe customer submits the shopping cart through the checkout API.\nWhen checkout succeeds, the order service reserves stock through the stock API.\n\n## Orders\n\nAn order carries a total and a currency.\nWhen payment succeeds, the order becomes paid.\nIf payment is declined, then the order is held for review.\n\n## Examples\n\nAna, a gold-tier customer, keeps 3 items in her cart, priced in EUR.\n";
    s.docs.insert(
        "shop.md".into(),
        DocRecord {
            content_hash: hash_hex(text),
            sections: crate::md::parse_sections(text),
            coverage: BTreeMap::new(),
        },
    );
    let mut batch = RecordBatch::new(1);
    crate::derive::recompute(&mut s, "g1", &mut batch);
    s.status.generation = 3;
    let docs: Vec<String> = s.docs.keys().cloned().collect();
    for d in docs {
        let rec = s.docs.get_mut(&d).unwrap();
        let refs: Vec<String> = rec.sections.keys().cloned().collect();
        for r in refs {
            rec.coverage.insert(
                r,
                Coverage {
                    state: "covered".into(),
                    note: None,
                    claimed_by: Some("g1".into()),
                },
            );
        }
    }
    s.status.changes.clear();
    s
}

fn unstable_ruling(subject: &str) -> crate::model::Diagnostic {
    crate::model::Diagnostic {
        goal: None,
        rule: "unstable-derivation".into(),
        severity: "warning".into(),
        subjects: vec![subject.into()],
        message: "flips".into(),
        reasoning: None,
        lifecycle: "open".into(),
        triage: None,
        prompt: None,
        answer: None,
        created: None,
        updated: None,
    }
}
