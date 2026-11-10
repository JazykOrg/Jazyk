use super::*;
use crate::model::*;
use crate::runner::SessionRunner;

mod files;
mod ledger;
mod mark;
mod medium;
mod order;
mod task;
mod worker;

// A runner that stops without recording, and one that records: gen counts an
// entity as generated only when its record moved during the session, and names
// the stop otherwise. Mirrors docs/consumers/gen.md#the-ledger.
struct StubRunner {
    record: bool,
    failed: Option<String>,
    stopped: Option<String>,
    store: Store,
    gs: GenSettings,
}
impl crate::runner::SessionRunner for StubRunner {
    fn agent_name(&self) -> String {
        "stub".into()
    }
    fn set_build_token(&self, _token: Option<String>) {}
    fn run_item(
        &self,
        _batch: &crate::runner::BatchRun,
        _trace: &crate::session::Trace,
    ) -> crate::runner::SessionReport {
        if self.record {
            crate::vfs::write(self.gs.deliverable.join("cart.rs"), "fn cart() {}\n").unwrap();
            crate::vfs::create_dir_all(self.gs.deliverable.join("tests")).unwrap();
            crate::vfs::write(
                self.gs.deliverable.join("tests/cart.rs"),
                "fn cart_test() {}\n",
            )
            .unwrap();
            mark(
                &self.store,
                "ent:cart",
                None,
                &json!({"files": ["cart.rs"], "tests": [{
                    "requirement":"req:shop-1","kind":"programmatic","artifact":"tests/cart.rs",
                    "name":"cart_test","run":"cargo test cart_test"
                }]}),
                &self.gs,
            )
            .unwrap();
        }
        crate::runner::SessionReport {
            applied: 0,
            rounds: 1,
            tokens: 0,
            cached: 0,
            failed: self.failed.clone(),
            resolved: Vec::new(),
            stopped: self.stopped.clone(),
        }
    }
    fn ask(&self, _s: &str, _u: &str, _l: &str, _st: &str) -> Result<String, String> {
        Err("stub".into())
    }
    fn ask_traced(
        &self,
        _s: &str,
        _u: &str,
        _l: &str,
        _st: &str,
        _t: Option<&crate::session::Trace>,
    ) -> Result<String, String> {
        Err("stub".into())
    }
}

fn fixture(out: &std::path::Path) -> (Store, GenSettings) {
    let mut s = Store {
        out: out.to_path_buf(),
        ..Default::default()
    };
    s.graph.entities.insert(
        "ent:cart".into(),
        Entity {
            name: "Cart".into(),
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:shop-1".into(),
        Requirement {
            statement: "The Cart shall hold items.".into(),
            entities: vec!["ent:cart".into()],
            edges: vec![],
            source: Some(SourceRef {
                doc: "shop.md".into(),
                section: "/shop".into(),
                quote: "holds".into(),
            }),
            ..Default::default()
        },
    );
    let gs = GenSettings {
        configured: true,
        deliverable: out.join("product"),
        worker: "agentic".into(),
        code: Vec::new(),
    };
    (s, gs)
}
