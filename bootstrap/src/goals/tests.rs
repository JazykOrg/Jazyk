use super::*;
use crate::vfs::VPathExt;

mod abstract_entity;
mod conform_instance;
mod dedupe_candidates;
mod derive_criteria;
mod reconcile_section;
mod rejudge_pair;
mod retrace;
mod review_entity;

pub(crate) use rejudge_pair::fanned_store;

fn op(v: Value) -> Op {
    serde_json::from_value(v).expect("an op")
}

fn scope(kind: &str, target: &str, change: Value) -> crate::tools::GoalScope {
    crate::tools::GoalScope::from_goal(&Goal {
        id: goal_id(kind, target),
        kind: kind.into(),
        class: "compile".into(),
        mandatory: true,
        target: target.into(),
        change,
        ..Default::default()
    })
}

fn goal_gate_of(
    store: &Store,
    staged: &[Op],
    goal: &crate::tools::GoalScope,
    batch: &[crate::tools::GoalScope],
) -> Option<Violation> {
    let gen = GenSettings::resolve(&crate::project::Project::default());
    let cx = GateContext {
        store,
        staged,
        gen: &gen,
        goal,
        batch,
    };
    kind(&goal.kind).unwrap().goal_gate(&cx)
}

#[test]
fn registry_covers_seventeen_kinds_with_pages_and_prompts() {
    assert_eq!(REGISTRY.len(), 17);
    let mut names: BTreeSet<&str> = BTreeSet::new();
    for k in REGISTRY.iter() {
        assert!(names.insert(k.kind()), "duplicate kind {}", k.kind());
        let page = format!(
            "{}/../docs/compiler/goals/{}.md",
            env!("CARGO_MANIFEST_DIR"),
            k.kind()
        );
        assert!(
            std::path::Path::new(&page).vexists(),
            "{} has no page",
            k.kind()
        );
        if blocked_on_human(k.kind()) {
            assert!(k.prompt().is_empty());
            assert!(k.toolset().is_empty());
        } else {
            assert!(
                !k.prompt().trim().is_empty(),
                "{} has no contract paragraph",
                k.kind()
            );
            // bind and derive-criteria derive from state (the ledger, the criteria),
            // not from a record kind of their own.
            assert!(
                !record_kinds(k.kind()).is_empty()
                    || matches!(k.kind(), "bind" | "derive-criteria")
            );
        }
        match k.class() {
            Class::Compile => assert!(tier(k.kind()).is_some(), "{} has no tier", k.kind()),
            Class::Gc => assert!(tier(k.kind()).is_none()),
        }
    }
    assert_eq!(
        parse_goal_id("g:retrace:view:usecase/holds"),
        Some(("retrace", "view:usecase/holds"))
    );
    assert_eq!(pair_target("req:b-1", "req:a-1"), "req:a-1~req:b-1");
}

// ---- the fan-out variant ----

fn gen_settings() -> GenSettings {
    GenSettings {
        configured: true,
        deliverable: std::path::PathBuf::from("/nonexistent"),
        worker: "agentic".into(),
        code: Vec::new(),
    }
}

fn child(name: &str, parent: Option<&str>, doc: &str) -> Entity {
    Entity {
        name: name.into(),
        parent: parent.map(String::from),
        stereotype: Some("component".into()),
        mentions: vec![SourceRef {
            doc: doc.into(),
            section: "/x".into(),
            quote: name.into(),
        }],
        ..Default::default()
    }
}

fn link(s: &mut Store, n: usize, a: &str, b: &str) {
    s.graph.requirements.insert(
        format!("req:x-{}", n),
        Requirement {
            statement: format!("{} calls {}.", a, b),
            entities: vec![a.into(), b.into()],
            source: Some(SourceRef {
                doc: "api.md".into(),
                section: "/x".into(),
                quote: "calls".into(),
            }),
            ..Default::default()
        },
    );
}

// One record per subject and limit, as the limit counts write them at commit.
fn threshold_record(s: &mut Store, subject: &str, limit: &str, count: u64) {
    let (soft, hard) = limits::threshold(limit, None).unwrap();
    s.status.changes.retain(|c| {
        !(c.kind == store::CHANGE_THRESHOLD_CROSSED
            && c.subject == subject
            && c.detail["limit"] == limit)
    });
    let n = s.status.changes.len();
    s.status.changes.push(
        ChangeRecord::new(7, n, 0, store::CHANGE_THRESHOLD_CROSSED, subject, "limits").with_detail(
            json!({
                "limit": limit, "count": count, "soft": soft, "hard": hard,
                "level": if count > hard { "hard" } else { "soft" },
                "goal": "abstract-entity",
            }),
        ),
    );
}

fn fan_out_record(s: &mut Store, subject: &str, count: u64) {
    threshold_record(s, subject, limits::CHILDREN_PER_ENTITY, count);
}

// ---- the caps variant ----

fn statement(s: &mut Store, id: &str, text: &str, entities: &[&str], section: &str) {
    s.graph.requirements.insert(
        id.into(),
        Requirement {
            statement: text.into(),
            entities: entities.iter().map(|e| e.to_string()).collect(),
            source: Some(SourceRef {
                doc: "svc.md".into(),
                section: section.into(),
                quote: text.into(),
            }),
            ..Default::default()
        },
    );
}

// A backend with twelve children in two coupled groups plus stragglers, and a
// public scope root with ten parentless entities.
fn levels_store() -> Store {
    let mut s = Store::default();
    s.graph
        .entities
        .insert("ent:backend".into(), child("Backend", None, "arch.md"));
    for (i, n) in ["cart", "pricing", "checkout", "discount"]
        .iter()
        .enumerate()
    {
        s.graph.entities.insert(
            format!("ent:{}", n),
            child(
                n,
                Some("ent:backend"),
                if i % 2 == 0 { "orders.md" } else { "api.md" },
            ),
        );
    }
    for n in ["shipment", "carrier", "tracking"] {
        s.graph.entities.insert(
            format!("ent:{}", n),
            child(n, Some("ent:backend"), "shipping.md"),
        );
    }
    for n in ["audit", "cache", "queue", "search", "mailer"] {
        s.graph.entities.insert(
            format!("ent:{}", n),
            child(n, Some("ent:backend"), "infra.md"),
        );
    }
    // A leaf under cart: its requirement lifts to cart.
    s.graph.entities.insert(
        "ent:cart-item".into(),
        child("cart item", Some("ent:cart"), "orders.md"),
    );
    let mut n = 0;
    let mut l = |s: &mut Store, a: &str, b: &str| {
        n += 1;
        link(s, n, a, b);
    };
    l(&mut s, "ent:cart", "ent:pricing");
    l(&mut s, "ent:cart", "ent:pricing");
    l(&mut s, "ent:pricing", "ent:discount");
    l(&mut s, "ent:cart-item", "ent:checkout");
    l(&mut s, "ent:checkout", "ent:discount");
    l(&mut s, "ent:shipment", "ent:carrier");
    l(&mut s, "ent:carrier", "ent:tracking");
    l(&mut s, "ent:shipment", "ent:tracking");
    l(&mut s, "ent:queue", "ent:mailer");
    s.graph.relationships.insert(
        "rel:ent:cart~ent:pricing".into(),
        Relationship {
            members: vec!["ent:cart".into(), "ent:pricing".into()],
            contributions: Vec::new(),
        },
    );
    // Ten parentless entities beside the backend at the scope root.
    for i in 0..9 {
        s.graph.entities.insert(
            format!("ent:top-{}", i),
            child(&format!("top {}", i), None, "arch.md"),
        );
    }
    fan_out_record(&mut s, "ent:backend", 12);
    fan_out_record(&mut s, "scope:public", 10);
    s
}

// The agent contract asks for independent calls in one reply under the native
// codec. Mirrors docs/compiler/goals/prompts/agent-contract.md.
#[test]
fn the_contract_asks_for_independent_calls_in_one_reply() {
    let contract = include_str!("../../../docs/compiler/goals/prompts/agent-contract.md");
    assert!(contract.contains("Independent calls go together in one reply"));
    let skill = include_str!("../../../docs/compiler/skills/extraction.md");
    assert!(skill.contains("one `search` with `queries`"));
}

// The extraction skill refuses a default stereotype: a concept the section only
// names and uses carries none, and `component`, `entity`, `class` are never
// defaults (the f2 root ingest labeled every domain concept «component»).
// Mirrors docs/compiler/skills/extraction.md.
#[test]
fn the_extraction_skill_refuses_default_stereotypes() {
    let skill = include_str!("../../../docs/compiler/skills/extraction.md");
    assert!(skill.contains("carries no stereotype: leave the field absent"));
    assert!(skill.contains("`component`, `entity`, and `class` are never defaults"));
}

// The abstraction skill refuses a grouping named as the plural of one member or
// with a word another level uses for stated children (the org root fan-out minted
// "Departments" beside the company that holds the departments; the near-duplicate
// gate folds no plurals). Mirrors docs/compiler/skills/abstraction.md.
#[test]
fn the_abstraction_skill_refuses_a_plural_of_a_member_as_a_grouping_name() {
    let skill = include_str!("../../../docs/compiler/skills/abstraction.md");
    assert!(skill.contains("never named as the plural of one member"));
    assert!(skill.contains("the near-duplicate gate folds no plurals"));
}
