use super::*;

#[test]
fn recomputation_never_authors_diagrams() {
    let mut store = showcase_store();
    let mut batch = RecordBatch::new(1);
    recompute(&mut store, "g1", &mut batch);
    assert!(store.graph.views.is_empty());
    assert!(!store.graph.relationships.is_empty());
    assert!(!store.graph.state_machines.is_empty());
    recompute(&mut store, "g1", &mut batch);
    assert!(store.graph.views.is_empty());
}

#[test]
fn authored_query_membership_recomputes_without_creating_other_diagrams() {
    let mut s = showcase_store();
    s.graph.views.insert(
        "view:class/services".into(),
        View {
            kind: "class".into(),
            title: "Services".into(),
            query: Some(ViewQuery {
                stereotype: Some("service".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    let mut batch = RecordBatch::new(1);
    recompute(&mut s, "g1", &mut batch);
    assert_eq!(s.graph.views.len(), 1);
    assert_eq!(
        s.graph.views["view:class/services"].members,
        vec!["ent:inventory-service", "ent:order-service"]
    );
    assert_eq!(batch.records().len(), 1);
    assert_eq!(batch.records()[0].kind, CHANGE_QUERY_MATCH);
}

#[test]
fn navigation_selects_authored_diagrams_by_area() {
    let mut s = showcase_store();
    assert_eq!(level_view_id(&s, "ent:shop"), None);
    s.graph.views.insert(
        "view:component/inside-the-shop".into(),
        View {
            kind: "component".into(),
            title: "Inside the shop".into(),
            authored: BTreeMap::from([(
                "area".into(),
                serde_json::json!({"target":"ent:shop","extent":"area"}),
            )]),
            members: vec!["ent:order-service".into(), "ent:inventory-service".into()],
            ..Default::default()
        },
    );
    assert_eq!(
        level_view_id(&s, "ent:shop").as_deref(),
        Some("view:component/inside-the-shop")
    );
}

#[test]
fn threshold_crossings_count_against_bumps() {
    let mut s = showcase_store();
    let mut batch = RecordBatch::new(1);
    recompute(&mut s, "g1", &mut batch);
    assert!(threshold_crossings(&s).is_empty());
    // One child over soft crosses children-per-entity on the node.
    let (soft, hard) = limits::threshold("children-per-entity", None).unwrap();
    for i in 0..=soft {
        s.graph.entities.insert(
            format!("ent:part-{}", i),
            Entity {
                name: format!("Part {}", i),
                parent: Some("ent:order".into()),
                ..Default::default()
            },
        );
    }
    let crossings = threshold_crossings(&s);
    assert_eq!(crossings.len(), 1);
    assert_eq!(crossings[0].subject, "ent:order");
    assert_eq!(crossings[0].limit, "children-per-entity");
    assert_eq!(
        (crossings[0].count, crossings[0].soft, crossings[0].hard),
        (soft + 1, soft, hard)
    );
    let mut batch = RecordBatch::new(2);
    record_threshold_crossings(&mut s, &mut batch);
    assert_eq!(batch.records().len(), 1);
    assert_eq!(batch.records()[0].detail["level"], "soft");
    // A bump raises the threshold and the crossing lapses.
    s.graph
        .entities
        .get_mut("ent:order")
        .unwrap()
        .limits
        .insert("children-per-entity".into(), LimitBump { value: soft + 5 });
    assert!(threshold_crossings(&s).is_empty());
    // The scope root counts its parentless entities under the same row, on
    // `scope:<scope>`, past hard as a hard crossing.
    let parentless = s
        .graph
        .entities
        .values()
        .filter(|e| e.parent.is_none())
        .count() as u64;
    for i in parentless..=hard {
        s.graph.entities.insert(
            format!("ent:top-{}", i),
            Entity {
                name: format!("Top {}", i),
                ..Default::default()
            },
        );
    }
    let crossings = threshold_crossings(&s);
    assert_eq!(crossings.len(), 1);
    assert_eq!(crossings[0].subject, "scope:public");
    assert_eq!(crossings[0].limit, "children-per-entity");
    assert_eq!(crossings[0].count, hard + 1);
    let mut batch = RecordBatch::new(3);
    record_threshold_crossings(&mut s, &mut batch);
    assert_eq!(batch.records()[0].subject, "scope:public");
    assert_eq!(batch.records()[0].detail["level"], "hard");
}

// The level index holds what the per-level scans compute: the members of every
// held level and every flow view, in order.
// Mirrors docs/compiler/graph.md#derived-data.
#[test]
fn the_level_index_matches_the_per_level_scans() {
    let mut synthetic = synthetic_store(300, 700);
    let mut batch = RecordBatch::new(1);
    recompute(&mut synthetic, "g1", &mut batch);
    let mut showcase = showcase_store();
    recompute(&mut showcase, "g1", &mut batch);
    for s in &[crate::board::tests::settled_store(), showcase, synthetic] {
        let ix = Indices::build(s);
        let lv = ix.levels(s);
        let scanned: Vec<Level> = levels(s, &ix);
        assert_eq!(lv.levels, scanned);
        let held: Vec<usize> = (0..scanned.len())
            .filter(|i| level_holds(s, &scanned[*i]))
            .collect();
        assert_eq!(lv.held, held);
        assert_eq!(lv.members.len(), held.len());
        for i in held {
            let l = &scanned[i];
            assert_eq!(
                lv.members[&l.target],
                level_members_of_scan(s, l, &ix),
                "{}",
                l.target
            );
        }
        assert_eq!(lv.flow_views, level_flow_views_scan(s, &ix));
        assert!(!lv.flow_views.is_empty());
    }
}
