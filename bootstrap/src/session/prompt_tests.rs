use super::prompt::{agent_contract, resolves, AGENT_CONTRACT, FEEDBACK_NOTE};
use super::*;
use crate::model::*;
use std::collections::BTreeMap;

fn fixture() -> Store {
    let mut s = Store::default();
    let text = "# Shop\nintro\n\n## Cart\nThe Shopping Cart holds items.\n";
    s.docs.insert(
        "shop.md".into(),
        DocRecord {
            content_hash: hash_hex(text),
            sections: crate::md::parse_sections(text),
            coverage: BTreeMap::new(),
        },
    );
    s.graph.entities.insert(
        "ent:shopping-cart".into(),
        Entity {
            name: "Shopping Cart".into(),
            definition: Some("holds items".into()),
            mentions: vec![SourceRef {
                doc: "shop.md".into(),
                section: "/shop/cart".into(),
                quote: "The Shopping Cart holds items.".into(),
            }],
            ..Default::default()
        },
    );
    s
}

fn goal() -> Goal {
    Goal {
        id: "g:reconcile-section:shop.md#/shop/cart".into(),
        kind: "reconcile-section".into(),
        class: "compile".into(),
        mandatory: true,
        target: "shop.md#/shop/cart".into(),
        unit: "section".into(),
        change: serde_json::json!({"unprocessed": true}),
        cause: Some(Cause {
            generation: 3,
            mutation: 0,
            via: "section".into(),
        }),
        state: GoalState::Open,
        hints: vec!["load shop.md#/shop/cart".into(), "skill extraction".into()],
    }
}

#[test]
fn alignment_evidence_is_in_the_opening_prompt_and_reloadable() {
    let mut store = fixture();
    store.status.alignment.push(DocAlignment {
        doc: "shop.md".into(),
        changes: vec![SectionOp {
            op: "moved".into(),
            from: vec!["shop.md#/shop/old-cart".into()],
            to: vec!["shop.md#/shop/cart".into()],
            similarity: Some(0.9),
        }],
        proposals: vec![AnchorProposal {
            anchor: "ent:cart".into(),
            from: "shop.md#/shop/old-cart".into(),
            quote: "The Shopping Cart holds items.".into(),
            excerpt: "Previous cart context.".into(),
            candidates: vec![AnchorCandidate {
                section: "shop.md#/shop/cart".into(),
                similarity: 0.9,
                quote_locates: true,
                nearest: None,
                excerpt: "The Shopping Cart holds items.".into(),
            }],
        }],
    });
    let mut goal = goal();
    goal.id = "g:place-anchors:shop.md".into();
    goal.kind = "place-anchors".into();
    goal.target = "shop.md".into();
    let (mut loaded, _) = initial_loaded(&store, std::slice::from_ref(&goal));
    assert!(loaded.contains("alignment:shop.md"));
    let prompt = preview(&store, &[goal]);
    for evidence in [
        "Previous cart context.",
        "old-cart",
        "quoteLocates",
        "0.9",
        "The Shopping Cart holds items.",
    ] {
        assert!(prompt.contains(evidence), "Missing {evidence}: {prompt}");
    }
    loaded.unload("alignment:shop.md");
    let reloaded = loaded.load(&store, "alignment:shop.md", 1).unwrap();
    assert!(reloaded.contains("Previous cart context."));

    let mut tight = LoadedSet::new(500);
    let first = tight.load(&store, "alignment:shop.md", 1).unwrap();
    let handle = tight.items[0].handles[0].handle.clone();
    assert!(first.contains(&handle));
    let rest = tight.expand(&store, &handle).unwrap();
    assert!(rest.contains("Expansion of alignment:shop.md (body)"));
    assert!(tight.load(&store, "alignment:missing.md", 1).is_err());
}

// The initially loaded set obeys the high-water ceiling on the normal path.
// Mirrors docs/compiler/context.md#policy.
// Mirrors docs/compiler/sessions.md#the-prompt: the project block carries the
// lint rules with their severities, and nothing when the project declares none.
#[test]
fn project_block_carries_the_lint_rules() {
    let s = Store::default();
    let bare = ProjectBlock::compute(&s, &[], "auto").render();
    assert!(!bare.contains("- lint:"), "{}", bare);
    let lint = crate::project::Linting {
        warnings: vec!["An em dash appears in prose.".into()],
        errors: vec!["A TODO marker appears in prose.".into()],
    };
    let with = ProjectBlock::compute(&s, &[], "auto")
        .with_lint(&lint)
        .render();
    assert!(
        with.contains(
            "- lint: [1] warning: An em dash appears in prose.; [2] error: A TODO marker appears in prose.\n"
        ),
        "{}",
        with
    );
    assert!(preview_with(&s, &[], &lint).contains("- lint: [1] warning: An em dash"));
}

// A board's goals give the same counts as a fresh derivation.
#[test]
fn project_block_counts_from_a_board_without_deriving_again() {
    let s = fixture();
    let gs = crate::gen::GenSettings::from_out(&s.out);
    let derived: Vec<Goal> = goals::REGISTRY
        .iter()
        .flat_map(|k| k.derive_goals(&s, &gs))
        .collect();
    assert!(!derived.is_empty(), "the fixture opens goals");
    let batch = &derived[..1];
    let fresh = ProjectBlock::compute(&s, batch, "auto");
    let counted = ProjectBlock::from_goals(&s, &derived, batch, "auto");
    assert_eq!(fresh.render(), counted.render());
    assert_eq!(
        counted.elsewhere + counted.here + counted.blocked,
        derived.len()
    );
}

#[test]
fn initial_loaded_stays_under_the_high_water_mark() {
    let s = fixture();
    let g = goal();
    let (loaded, _) = initial_loaded(&s, std::slice::from_ref(&g));
    assert!(
        loaded.used() <= loaded.high_water,
        "{} > {}",
        loaded.used(),
        loaded.high_water
    );
}

// A criteria batch listing more requirements than the budget holds opens within
// it: past the budget an item is named on the Not loaded line, never stubbed.
// Mirrors docs/compiler/context.md#the-loaded-set.
#[test]
fn initial_loaded_defers_what_the_budget_cannot_hold() {
    let mut s = fixture();
    let long = "Ines Marlow keeps the lantern lit for the harbor crew until dawn on the night of the storm, and the whole quay sees it from the water. ".repeat(12);
    let mut ids = Vec::new();
    for i in 0..160 {
        let id = format!("req:big-{}", i);
        s.graph.requirements.insert(
            id.clone(),
            crate::model::Requirement {
                statement: format!("{} ({})", long, i),
                entities: vec!["ent:order".into()],
                ..Default::default()
            },
        );
        ids.push(id);
    }
    let g = Goal {
        id: "g:derive-criteria:ent:order".into(),
        kind: "derive-criteria".into(),
        class: "compile".into(),
        mandatory: true,
        target: "ent:order".into(),
        unit: "entity".into(),
        change: serde_json::json!({"none": ids, "revised": []}),
        cause: None,
        state: GoalState::Open,
        hints: vec![],
    };
    let (loaded, _) = initial_loaded(&s, std::slice::from_ref(&g));
    assert!(
        loaded.used() <= loaded.budget,
        "{} > {}",
        loaded.used(),
        loaded.budget
    );
    assert!(!loaded.deferred.is_empty());
    assert!(
        loaded
            .render_condensed(0)
            .contains("Not loaded (over the budget"),
        "{}",
        loaded.render_condensed(0)
    );
}

// A pack line naming `scope:<scope>` resolves and loads the scope's top level as
// stubs, full because it is the goal's own target.
// Mirrors docs/compiler/concepts/levels.md#the-scope-root.
#[test]
fn initial_loaded_packs_a_scope_root() {
    let s = fixture();
    assert!(resolves(&s, "scope:public"));
    assert!(!resolves(&s, "scope:nope"));
    let g = Goal {
        id: "g:abstract-entity:scope:public".into(),
        kind: "abstract-entity".into(),
        class: "gc".into(),
        mandatory: false,
        target: "scope:public".into(),
        unit: "entity".into(),
        change: serde_json::json!({"fan_out": 1, "limit": {"soft": 9, "hard": 15}, "candidates": []}),
        cause: None,
        state: GoalState::Open,
        hints: vec!["load scope:public".into(), "skill abstraction".into()],
    };
    let (loaded, _) = initial_loaded(&s, std::slice::from_ref(&g));
    assert!(
        loaded.contains("scope:public"),
        "{:?}",
        loaded.render_condensed(0)
    );
    let status = loaded.render_condensed(0);
    assert!(
        status.contains("top level: 1 entities as stubs"),
        "{}",
        status
    );
    // The prompt's loaded block names the root's line, pinned as the goal's target.
    let prompt = preview(&s, std::slice::from_ref(&g));
    assert!(
        prompt.contains("- scope:public   top level: 1 entities as stubs"),
        "{}",
        prompt
    );
    assert!(
        !prompt.contains("Consider unloading: scope:public"),
        "{}",
        prompt
    );
}

// The prompt assembles in the fixed order, with the feedback note as the second
// paragraph and the protocol line naming the batch.
// Mirrors docs/compiler/sessions.md#the-prompt.
#[test]
fn prompt_assembles_in_order_with_payloads_placed() {
    let s = fixture();
    let g = goal();
    let prompt = preview(&s, std::slice::from_ref(&g));
    assert!(prompt.starts_with("You are one session of jazyk"));
    let paras: Vec<&str> = prompt.split("\n\n").collect();
    assert_eq!(paras[1], FEEDBACK_NOTE, "the note is the second paragraph");
    // The agent contract itself names the block headings mid-sentence, so the
    // blocks are located by their heading lines.
    let project = prompt.find("\n## Project\n").expect("project block");
    let goals_at = prompt.find("\n## Goals\n").expect("goals block");
    let loaded = prompt.find("\n## Loaded (").expect("loaded block");
    let proto = prompt.rfind("PROTOCOL").expect("protocol line");
    assert!(project < goals_at && goals_at < loaded && loaded < proto);
    assert!(prompt.contains("- [g:reconcile-section:shop.md#/shop/cart] mandatory"));
    assert!(prompt.contains("Change: "));
    assert!(prompt.contains(&format!("Gate: {}", gate_line("reconcile-section"))));
    assert!(prompt.contains("Hints: load shop.md#/shop/cart; skill extraction"));
    // The batch's goal kind activates its skill from the first round.
    assert!(prompt.contains("[skill: extraction (active)]"));
    // The protocol line names the batch id, never the raw placeholder.
    assert!(!prompt.contains("{target}"));
    assert!(prompt.contains("b0-0"));
}

#[test]
fn every_contract_paragraph_names_its_goal_first() {
    for k in goals::REGISTRY.iter() {
        if goals::blocked_on_human(k.kind()) {
            continue;
        }
        assert!(
            k.prompt().starts_with("This goal"),
            "{} names its goal first",
            k.kind()
        );
    }
    let s = agent_contract();
    assert!(s.contains("report_feedback"));
    let rest = AGENT_CONTRACT
        .split_once("{{{feedback}}}")
        .map(|(_, r)| r)
        .unwrap();
    assert!(s.ends_with(rest), "nothing of the contract is lost");
}

#[test]
fn declare_edges_pins_only_unchanged_edge_and_stability_rules() {
    let store = fixture();
    let mut edges = goal();
    edges.kind = "declare-edges".into();
    edges.target = "req:cart-items".into();
    let (_, skills) = initial_loaded(&store, &[edges]);
    assert!(skills.has_pinned());
    assert_eq!(
        skills
            .rendered
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["edges"]
    );
    let extraction = skill_payload("extraction").unwrap();
    let edges = skill_payload("edges").unwrap();
    let edge_rules = extraction
        .split_once("Edges. ")
        .unwrap()
        .1
        .split_once("Transitions. ")
        .unwrap()
        .0;
    let stability = extraction
        .split_once("Stability. ")
        .unwrap()
        .1
        .split_once("Rounds. ")
        .unwrap()
        .0;
    assert_eq!(
        edges,
        format!("Edges. {edge_rules}Stability. {}\n", stability.trim_end())
    );
    assert!(edges.len() < extraction.len() / 4);
    assert_eq!(
        goals::skills_for("reconcile-section", &store, ""),
        vec!["extraction"]
    );
}
