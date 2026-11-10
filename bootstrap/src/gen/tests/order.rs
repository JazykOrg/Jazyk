use super::*;

// Part before whole: a parent's children generate first, and a system's direct
// child roots its subtree as one group.
// Mirrors docs/consumers/gen.md#grouping-by-component.
#[test]
fn children_generate_before_their_parent_and_groups_derive_from_containment() {
    let mut s = Store::default();
    for (id, parent) in [
        ("ent:sys", None),
        ("ent:svc", Some("ent:sys")),
        ("ent:svc-part", Some("ent:svc")),
        ("ent:other", Some("ent:sys")),
    ] {
        s.graph.entities.insert(
            id.into(),
            Entity {
                name: id.into(),
                parent: parent.map(String::from),
                ..Default::default()
            },
        );
    }
    // The system is the containment root; its direct children root the groups.
    assert!(is_system(&s, "ent:sys"));
    assert!(!is_system(&s, "ent:svc"));
    assert_eq!(group_root(&s, "ent:svc-part"), "ent:svc");
    assert_eq!(group_root(&s, "ent:svc"), "ent:svc");
    assert_eq!(group_root(&s, "ent:sys"), "ent:sys");
    assert_eq!(
        group_members(&s, "ent:svc"),
        vec!["ent:svc".to_string(), "ent:svc-part".to_string()]
    );
    // Order: every child before its parent, the system last.
    let order = generation_order(
        &s,
        &[
            "ent:sys".into(),
            "ent:svc".into(),
            "ent:svc-part".into(),
            "ent:other".into(),
        ],
    );
    let pos = |id: &str| order.iter().position(|x| x == id).unwrap();
    assert!(pos("ent:svc-part") < pos("ent:svc"), "{:?}", order);
    assert!(pos("ent:svc") < pos("ent:sys"), "{:?}", order);
    assert!(pos("ent:other") < pos("ent:sys"), "{:?}", order);
    // run_all batches by these groups: svc-part rides with its root svc, the
    // system and the childless direct child are groups of their own.
    let groups = groups_in_order(&s, &order);
    let of = |root: &str| {
        groups
            .iter()
            .find(|(r, _)| r == root)
            .map(|(_, m)| m.clone())
            .unwrap_or_default()
    };
    assert!(of("ent:svc").contains(&"ent:svc".to_string()));
    assert!(of("ent:svc").contains(&"ent:svc-part".to_string()));
    assert_eq!(of("ent:sys"), vec!["ent:sys".to_string()]);
    assert_eq!(of("ent:other"), vec!["ent:other".to_string()]);
}

// The leaf-first order is the one the per-step minimum over pending prerequisites
// gives: ties by id, a cycle broken at the fewest pending, a duplicated target
// emitted twice, an edge or child outside the targets ignored.
// Mirrors docs/consumers/gen.md#order-from-relationships.
#[test]
fn generation_order_matches_the_stepwise_minimum() {
    let mut s = Store::default();
    for (id, parent) in [
        ("ent:a", None),
        ("ent:b", Some("ent:a")),
        ("ent:c", Some("ent:a")),
        ("ent:d", Some("ent:c")),
        ("ent:e", None),
        ("ent:f", Some("ent:outside")),
        ("ent:g", Some("ent:g")),
    ] {
        s.graph.entities.insert(
            id.into(),
            Entity {
                name: id.into(),
                parent: parent.map(String::from),
                ..Default::default()
            },
        );
    }
    let edge = |a: &str, b: &str| Contribution {
        a: a.into(),
        b: b.into(),
        r#type: "dependency".into(),
        cardinality: None,
        requirements: vec![],
    };
    for (key, contributions) in [
        (
            "rel:a~e",
            vec![edge("ent:e", "ent:a"), edge("ent:a", "ent:e")],
        ),
        (
            "rel:b~d",
            vec![edge("ent:b", "ent:d"), edge("ent:b", "ent:d")],
        ),
        (
            "rel:d~f",
            vec![edge("ent:d", "ent:f"), edge("ent:f", "ent:x")],
        ),
        ("rel:g", vec![edge("ent:g", "ent:g")]),
    ] {
        s.graph.relationships.insert(
            key.into(),
            Relationship {
                members: vec![],
                contributions,
            },
        );
    }
    let stepwise = |targets: &[String]| -> Vec<String> {
        let set: std::collections::BTreeSet<&str> = targets.iter().map(|t| t.as_str()).collect();
        let prereqs = |id: &str, emitted: &std::collections::BTreeSet<String>| {
            let mut n = 0;
            for rel in s.graph.relationships.values() {
                for c in &rel.contributions {
                    if c.a == id
                        && c.b != id
                        && set.contains(c.b.as_str())
                        && !emitted.contains(&c.b)
                    {
                        n += 1;
                    }
                }
            }
            for (cid, e) in &s.graph.entities {
                if e.parent.as_deref() == Some(id)
                    && set.contains(cid.as_str())
                    && !emitted.contains(cid)
                {
                    n += 1;
                }
            }
            n
        };
        let mut remaining = targets.to_vec();
        let mut emitted = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        while !remaining.is_empty() {
            let (i, _) = remaining
                .iter()
                .enumerate()
                .min_by_key(|(_, id)| (prereqs(id, &emitted), (*id).clone()))
                .unwrap();
            let id = remaining.remove(i);
            emitted.insert(id.clone());
            out.push(id);
        }
        out
    };
    let all: Vec<String> = [
        "ent:g", "ent:f", "ent:e", "ent:d", "ent:c", "ent:b", "ent:a",
    ]
    .iter()
    .map(|t| t.to_string())
    .collect();
    for targets in [
        all.clone(),
        vec!["ent:a".into(), "ent:d".into(), "ent:b".into()],
        vec![
            "ent:c".into(),
            "ent:c".into(),
            "ent:a".into(),
            "ent:d".into(),
        ],
        vec!["ent:e".into(), "ent:a".into()],
        vec![],
    ] {
        assert_eq!(
            generation_order(&s, &targets),
            stepwise(&targets),
            "{:?}",
            targets
        );
    }
}
