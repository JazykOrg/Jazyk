use super::*;

#[test]
fn lifting_and_collapse_aggregate_hidden_edges() {
    let mut s = tree_store();
    add_req(&mut s, "req:t-1", "ent:a1", "ent:b1", "dependency");
    recompute(&mut s, "g1", &mut RecordBatch::new(1));
    let system = view("class", "System", &["ent:a", "ent:b"]);
    let puml = emit(&s, "view:class/system", &system);
    assert_eq!(
        canon(&puml),
        vec!["@startuml", "class A", "class B", "A ..> B", "@enduml"],
        "{}",
        puml
    );
    // A second type on the same lifted pair: one arrow, the stronger type, a count.
    add_req(&mut s, "req:t-2", "ent:a1", "ent:b1", "association");
    // An instantiation never joins; an edge inside one subtree is not drawn.
    add_req(&mut s, "req:t-3", "ent:a1", "ent:b1", "instantiation");
    add_req(&mut s, "req:t-4", "ent:a1", "ent:a2", "composition");
    recompute(&mut s, "g2", &mut RecordBatch::new(2));
    let puml = emit(&s, "view:class/system", &system);
    assert_eq!(
        canon(&puml),
        vec![
            "@startuml",
            "class A",
            "class B",
            "A -- B : 2 edges",
            "@enduml"
        ],
        "{}",
        puml
    );
    // Listing the leaves draws them and their own arrows; collapsing a hides them
    // again and links the sub-view that details a.
    let leaves = view(
        "class",
        "Leaves",
        &["ent:a", "ent:a1", "ent:a2", "ent:b", "ent:b1"],
    );
    let puml = emit(&s, "view:class/leaves", &leaves);
    let c = canon(&puml);
    assert!(c.contains(&"A1 -- B1 : 2 edges".to_string()), "{}", puml);
    assert!(c.contains(&"A1 *-- A2".to_string()), "{}", puml);
    s.graph.views.insert(
        "view:class/a-parts".into(),
        View {
            query: Some(ViewQuery {
                parent: Some("ent:a".into()),
                ..Default::default()
            }),
            ..view("class", "A parts", &[])
        },
    );
    let collapsed = View {
        collapse: vec!["ent:a".into()],
        ..leaves
    };
    let puml = emit(&s, "view:class/leaves", &collapsed);
    assert!(
        puml.contains("class \"A\" as a [[../../docsgen/entities/a.md]]"),
        "{}",
        puml
    );
    assert!(!puml.contains("A1"), "{}", puml);
    assert!(
        canon(&puml).contains(&"A -- B1 : 2 edges".to_string()),
        "{}",
        puml
    );
}

#[test]
fn over_limit_views_auto_collapse_or_mark_every_member() {
    // Twenty-one roots over a bump of ten (hard twenty): nothing to collapse.
    let mut s = Store::default();
    let ids: Vec<String> = (0..21).map(|i| format!("ent:root-{}", i)).collect();
    for id in &ids {
        s.graph.entities.insert(
            id.clone(),
            Entity {
                name: id.clone(),
                ..Default::default()
            },
        );
    }
    let members: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mut v = view("class", "Roots", &members);
    v.limits.insert(
        "members-per-structural-view".into(),
        LimitBump { value: 10 },
    );
    let puml = emit(&s, "view:class/roots", &v);
    assert!(
        puml.contains("title Roots (over limit: 21 members)"),
        "{}",
        puml
    );
    assert_eq!(puml.matches("\nclass ").count(), 21);
    // Without the bump, twenty-one is over soft only: no intervention.
    let plain = view("class", "Roots", &members);
    assert!(!emit(&s, "view:class/roots", &plain).contains("title "));

    // A root with twenty-one listed children: the largest subtree collapses.
    let mut s = Store::default();
    s.graph.entities.insert(
        "ent:r".into(),
        Entity {
            name: "R".into(),
            ..Default::default()
        },
    );
    let kids: Vec<String> = (0..21).map(|i| format!("ent:k-{}", i)).collect();
    for id in &kids {
        s.graph.entities.insert(
            id.clone(),
            Entity {
                name: id.clone(),
                parent: Some("ent:r".into()),
                ..Default::default()
            },
        );
    }
    let mut members = vec!["ent:r".to_string()];
    members.extend(kids.iter().cloned());
    let members: Vec<&str> = members.iter().map(String::as_str).collect();
    let mut v = view("class", "Tree", &members);
    v.limits.insert(
        "members-per-structural-view".into(),
        LimitBump { value: 10 },
    );
    let puml = emit(&s, "view:class/tree", &v);
    assert!(
        puml.contains("title Tree (collapsed: 1 subtrees over limit)"),
        "{}",
        puml
    );
    assert_eq!(puml.matches("\nclass ").count(), 1);
    assert!(puml.contains("class \"R\" as r"));

    // A flow view past the hard member count (twenty) renders every member, marked.
    let mut s = Store::default();
    s.graph.entities.insert(
        "ent:x".into(),
        Entity {
            name: "X".into(),
            ..Default::default()
        },
    );
    let steps: Vec<String> = (0..21).map(|i| format!("req:f-{}", i)).collect();
    for (i, id) in steps.iter().enumerate() {
        s.graph.requirements.insert(
            id.clone(),
            Requirement {
                statement: format!("Step {}.", i),
                entities: vec!["ent:x".into()],
                ..Default::default()
            },
        );
    }
    let members: Vec<&str> = steps.iter().map(String::as_str).collect();
    let v = view("activity", "Flow", &members);
    let puml = emit(&s, "view:activity/flow", &v);
    assert!(
        puml.contains("title Flow (over limit: 21 members)"),
        "{}",
        puml
    );
    assert_eq!(puml.matches("(req:f-").count(), 21);
    // Twenty is within the hard threshold: no mark.
    let v = view("activity", "Flow", &members[..20]);
    assert!(!emit(&s, "view:activity/flow", &v).contains("title "));
}
