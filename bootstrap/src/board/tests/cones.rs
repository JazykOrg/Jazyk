// Cones, the referrer index, and explain.
use super::*;

// Each direction keeps its own visited set: a node one seed reaches upward still
// walks downward from the other seed. Mirrors docs/compiler/reconciler.md#cones.
#[test]
fn a_node_reached_upward_still_walks_downward_from_the_other_seed() {
    use crate::model::{Entity, Provenance, Requirement};
    let mut s = Store::default();
    s.graph.entities.insert(
        "ent:pricing".into(),
        Entity {
            name: "Pricing".into(),
            provenance: Some(Provenance::Derived {
                from: vec!["req:b".into()],
                reasoning: "split".into(),
            }),
            ..Default::default()
        },
    );
    s.graph.entities.insert(
        "ent:discount".into(),
        Entity {
            name: "Discount".into(),
            parent: Some("ent:pricing".into()),
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:a".into(),
        Requirement {
            statement: "Pricing applies to every order.".into(),
            entities: vec!["ent:pricing".into()],
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:b".into(),
        Requirement {
            statement: "Orders are priced.".into(),
            ..Default::default()
        },
    );
    let c = cone(&s, "req:a~req:b");
    assert!(c.nodes.contains("ent:pricing"));
    assert!(
        c.nodes.contains("ent:discount"),
        "req:b's downward walk reaches the child: {:?}",
        c.nodes
    );
}

#[test]
fn cones_walk_up_and_down_never_sideways() {
    let s = crate::derive::tests::showcase_store();
    let c = cone(&s, "ent:order-item");
    assert!(
        c.nodes.contains("ent:order-service"),
        "parent chain is upward"
    );
    assert!(
        c.nodes.contains("req:shop-6"),
        "the requirement naming it is downward"
    );
    assert!(
        c.sections.contains("shop.md#/shop"),
        "its anchoring section"
    );
    assert!(
        !c.nodes.contains("ent:customer"),
        "a sibling reached sideways is out: {:?}",
        c.nodes
    );
    let top = cone(&s, "ent:shop");
    assert!(
        top.nodes.contains("ent:order"),
        "the system's cone holds its descendants"
    );
    assert!(top.nodes.contains("req:shop-1"));
}

// The cone of `scope:<scope>` is the downward walk from every parentless entity of
// the scope: the whole scope, and nothing of another scope.
// Mirrors docs/compiler/reconciler.md#cones.
#[test]
fn the_cone_of_a_scope_target_is_the_whole_scope() {
    let mut s = crate::derive::tests::showcase_store();
    s.graph.entities.insert(
        "ent:ledger".into(),
        Entity {
            name: "Ledger".into(),
            scope: "finance".into(),
            ..Default::default()
        },
    );
    assert_eq!(scope_target("scope:public"), Some("public"));
    assert_eq!(scope_target("ent:shop"), None);
    assert_eq!(scope_target("scope:"), None);
    let root = scope_root(&s, "public");
    assert!(root.contains(&"ent:shop".to_string()), "{:?}", root);
    assert!(root.contains(&"ent:customer".to_string()));
    assert!(
        !root.contains(&"ent:order".to_string()),
        "a child is no root"
    );
    assert!(!root.contains(&"ent:ledger".to_string()), "another scope");
    assert_eq!(level_members(&s, "scope:public"), root);
    assert_eq!(
        level_members(&s, "ent:shop"),
        vec![
            "ent:inventory-service".to_string(),
            "ent:order-service".into()
        ]
    );
    let c = cone(&s, "scope:public");
    for id in [
        "ent:shop",
        "ent:order-service",
        "ent:order",
        "ent:customer",
        "req:shop-1",
    ] {
        assert!(
            c.nodes.contains(id),
            "{} in the scope's cone: {:?}",
            id,
            c.nodes
        );
    }
    assert!(c.sections.contains("shop.md#/shop"));
    assert!(c.docs.contains("shop.md"));
    assert!(!c.nodes.contains("ent:ledger"), "another scope stays out");
    assert!(c.holds_target("req:shop-6"));
    assert!(c.holds_target("shop.md#/shop/orders"));
    assert!(cone(&s, "scope:finance").nodes.contains("ent:ledger"));
    assert!(cone(&s, "scope:nope").nodes.is_empty());
}

#[test]
fn explain_names_the_goals_a_change_opens() {
    let s = settled_store();
    let b = derive(&s);
    let text = b.explain(&s, "ent:customer").expect("known target");
    assert!(text.contains("review-entity"), "{}", text);
    assert!(
        text.contains("conform-instance") && text.contains("ent:ana"),
        "{}",
        text
    );
    assert!(b.explain(&s, "ent:nope").is_none());
}

// The referrer index answers every node as the scan over the store does, so the
// cone over the index is the cone over the scan.
// Mirrors docs/compiler/graph.md#derived-data and docs/compiler/reconciler.md#cones.
#[test]
fn the_referrer_index_matches_the_scan() {
    let mut synthetic = crate::derive::tests::synthetic_store(300, 700);
    let mut batch = RecordBatch::new(1);
    crate::derive::recompute(&mut synthetic, "g1", &mut batch);
    for s in [settled_store(), synthetic] {
        let refs = Refs::build(&s);
        let mut ids: Vec<String> = s.graph.entities.keys().cloned().collect();
        ids.extend(s.graph.requirements.keys().cloned());
        ids.extend(s.graph.views.keys().cloned());
        for (doc, rec) in &s.docs {
            ids.push(doc.clone());
            ids.extend(rec.sections.keys().map(|r| format!("{}#{}", doc, r)));
        }
        ids.push("ent:nope".into());
        ids.push("nope.md#/nope".into());
        for id in &ids {
            let indexed: BTreeSet<String> = referrers(&s, &refs, id).into_iter().collect();
            let scanned: BTreeSet<String> = referrers_scan(&s, id).into_iter().collect();
            assert_eq!(indexed, scanned, "{}", id);
        }
        for id in &ids {
            assert_eq!(cone_with(&s, &refs, id), cone(&s, id), "{}", id);
        }
    }
}
