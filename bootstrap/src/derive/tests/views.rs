use super::*;

// Mirrors docs/compiler/model/view.md#level-views: a node with one child derives
// its level view when the child carries attributes, and stays a leaf otherwise.
#[test]
fn a_one_child_level_derives_a_class_view_when_the_child_carries_attributes() {
    let mut s = Store::default();
    let ent = |name: &str, parent: Option<&str>, attrs: &[&str]| Entity {
        name: name.into(),
        parent: parent.map(String::from),
        attributes: attrs.iter().map(|a| attr(a, None, None)).collect(),
        ..Default::default()
    };
    s.graph
        .entities
        .insert("ent:backend".into(), ent("Backend", None, &[]));
    s.graph.entities.insert(
        "ent:users".into(),
        ent("User Management", Some("ent:backend"), &[]),
    );
    s.graph.entities.insert(
        "ent:inventory".into(),
        ent("Inventory", Some("ent:backend"), &[]),
    );
    s.graph.entities.insert(
        "ent:account".into(),
        ent("User Account", Some("ent:users"), &["id", "role"]),
    );
    s.graph.entities.insert(
        "ent:product".into(),
        ent("Product", Some("ent:inventory"), &[]),
    );
    let ids: Vec<String> = default_view_rules(&s, &Indices::build(&s))
        .into_iter()
        .map(|v| v.id)
        .collect();
    assert!(ids.contains(&"view:class/users".to_string()), "{:?}", ids);
    assert!(!ids.iter().any(|i| i.ends_with("/inventory")), "{:?}", ids);
    assert_eq!(
        level_view_id(&s, "ent:users").as_deref(),
        Some("view:class/users")
    );
    assert_eq!(level_view_id(&s, "ent:inventory"), None);
}

// Mirrors docs/compiler/model/view.md#level-views: a flow drawn only among
// groupings derives no view, while the groupings' own levels keep theirs.
#[test]
fn a_flow_among_groupings_alone_derives_no_view() {
    let mut s = Store::default();
    let grouping = |name: &str, from: &[&str]| Entity {
        name: name.into(),
        provenance: Some(Provenance::Derived {
            from: from.iter().map(|x| x.to_string()).collect(),
            reasoning: "a level".into(),
        }),
        ..Default::default()
    };
    let stated = |name: &str, parent: &str| Entity {
        name: name.into(),
        parent: Some(parent.into()),
        mentions: vec![src("plot.md", "/plot", name)],
        ..Default::default()
    };
    s.graph.entities.insert(
        "ent:cast".into(),
        grouping("Cast", &["ent:ines", "ent:theo"]),
    );
    s.graph.entities.insert(
        "ent:settings".into(),
        grouping("Settings", &["ent:shop", "ent:quay"]),
    );
    s.graph
        .entities
        .insert("ent:ines".into(), stated("Ines", "ent:cast"));
    s.graph
        .entities
        .insert("ent:theo".into(), stated("Theo", "ent:cast"));
    s.graph
        .entities
        .insert("ent:shop".into(), stated("Shop", "ent:settings"));
    s.graph
        .entities
        .insert("ent:quay".into(), stated("Quay", "ent:settings"));
    for (i, (a, b)) in [("ent:ines", "ent:shop"), ("ent:theo", "ent:quay")]
        .iter()
        .enumerate()
    {
        s.graph.requirements.insert(
            format!("req:plot-{}", i + 1),
            Requirement {
                statement: format!("step {}", i),
                entities: vec![a.to_string(), b.to_string()],
                edges: vec![edge(a, b, "dependency", None)],
                facets: vec![behavior()],
                source: Some(src("plot.md", "/plot", "step")),
                ..Default::default()
            },
        );
    }
    let ids: Vec<String> = default_view_rules(&s, &Indices::build(&s))
        .into_iter()
        .map(|v| v.id)
        .collect();
    assert!(!ids.contains(&"view:usecase/plot".to_string()), "{:?}", ids);
    assert!(
        !ids.contains(&"view:sequence/plot".to_string()),
        "{:?}",
        ids
    );
    assert!(
        ids.contains(&"view:usecase/cast-plot".to_string()),
        "{:?}",
        ids
    );
}

#[test]
fn default_views_derive_from_the_showcase_graph() {
    let mut s = showcase_store();
    let mut batch = RecordBatch::new(1);
    recompute(&mut s, "g1", &mut batch);
    let views = &s.graph.views;
    // The scope root's level view is the per-scope view: the parentless entities
    // of the public scope, component because the shop is a system.
    let root = &views["view:component/public"];
    assert_eq!(root.kind, "component");
    assert_eq!(root.title, "Public");
    assert!(root.default);
    assert_eq!(
        root.members,
        vec!["ent:shop", "ent:customer", "ent:ana", "ent:anas-cart"]
    );
    assert_eq!(root.query, None);
    assert!(!views.contains_key("view:class/public"));
    // The shop's level: its two services and the customer, whose edges lift into
    // the order service. The interfaces are descendants, not members.
    let comp = &views["view:component/shop"];
    assert_eq!(comp.title, "Shop");
    assert_eq!(
        comp.members,
        vec!["ent:inventory-service", "ent:order-service", "ent:customer"]
    );
    // State per machine, object per type.
    let state = &views["view:state/order"];
    assert_eq!(state.members, vec!["ent:order"]);
    assert_eq!(state.title, "Order");
    assert_eq!(views["view:object/customer"].members, vec!["ent:ana"]);
    assert_eq!(
        views["view:object/shopping-cart"].members,
        vec!["ent:anas-cart"]
    );
    // Root-level flow cluster: shop.md's behavior, in document order, under the
    // unprefixed ids, titled by the document.
    let uc = &views["view:usecase/shop"];
    assert_eq!(uc.kind, "use-case");
    assert_eq!(uc.title, "Shop");
    assert_eq!(
        uc.members,
        vec!["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"]
    );
    let seq = &views["view:sequence/shop"];
    assert_eq!(seq.members, vec!["req:shop-1", "req:shop-3"]);
    assert!(matches!(
        uc.provenance.as_ref(),
        Some(Provenance::Derived { reasoning, .. }) if reasoning == "default view: use-case per flow cluster of a level"
    ));
    assert!(batch.records().is_empty());

    // A curated view is left alone and a default whose rule stops holding goes.
    s.graph.views.get_mut("view:state/order").unwrap().default = false;
    s.graph.views.get_mut("view:state/order").unwrap().members = vec!["ent:shop".into()];
    s.graph.requirements.remove("req:shop-9");
    let mut batch = RecordBatch::new(2);
    recompute(&mut s, "g2", &mut batch);
    assert_eq!(s.graph.views["view:state/order"].members, vec!["ent:shop"]);
    assert!(!s.graph.views.contains_key("view:object/customer"));
    assert!(s.graph.views.contains_key("view:component/public"));
    // Excluded and collapse survive on a still-default view; the rule rewrites
    // the members without the exclusion.
    let root = s.graph.views.get_mut("view:component/public").unwrap();
    root.excluded.push(Exclusion {
        id: "ent:ana".into(),
        note: "an instance, drawn in the object view".into(),
    });
    root.collapse.push("ent:shop".into());
    let mut batch = RecordBatch::new(3);
    recompute(&mut s, "g3", &mut batch);
    let root = &s.graph.views["view:component/public"];
    assert!(root.default);
    assert_eq!(root.excluded.len(), 1);
    assert_eq!(root.collapse, vec!["ent:shop"]);
    assert!(!root.members.contains(&"ent:ana".to_string()));
    assert!(root.members.contains(&"ent:shop".to_string()));
    // A curated query view gets new matches as a query-match record.
    s.graph.views.insert(
        "view:class/services".into(),
        View {
            kind: "class".into(),
            title: "Services".into(),
            query: Some(ViewQuery {
                stereotype: Some("service".into()),
                ..Default::default()
            }),
            provenance: Some(Provenance::Decree {
                author: "owner".into(),
                at: "g3".into(),
                note: None,
            }),
            ..Default::default()
        },
    );
    let mut batch = RecordBatch::new(4);
    recompute(&mut s, "g4", &mut batch);
    assert_eq!(
        s.graph.views["view:class/services"].members,
        vec!["ent:inventory-service", "ent:order-service"]
    );
    let records = batch.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].kind, CHANGE_QUERY_MATCH);
    assert_eq!(records[0].subject, "view:class/services");
}

// Mirrors docs/compiler/model/view.md#level-views: one structural view per node
// with two or more children, none for a one-child node, the kind by the
// stereotypes present.
#[test]
fn level_views_derive_per_node_with_two_or_more_children() {
    let mut s = showcase_store();
    let mut batch = RecordBatch::new(1);
    recompute(&mut s, "g1", &mut batch);
    // The order service holds four children (three first named in the root
    // section, tied by id, then the order) and the customer's edges lift in; its
    // interface child makes the view a component view.
    let os = &s.graph.views["view:component/order-service"];
    assert_eq!(os.title, "Order Service");
    assert!(os.default);
    assert_eq!(
        os.members,
        vec![
            "ent:checkout-api",
            "ent:order-item",
            "ent:shopping-cart",
            "ent:order",
            "ent:customer"
        ]
    );
    assert!(matches!(
        os.provenance.as_ref(),
        Some(Provenance::Derived { from, reasoning })
            if from[0] == "ent:order-service" && reasoning == "default view: level view of a node"
    ));
    assert_eq!(
        level_view_id(&s, "ent:order-service").as_deref(),
        Some("view:component/order-service")
    );
    // One child: no level view, and no id.
    assert!(!s
        .graph
        .views
        .contains_key("view:component/inventory-service"));
    assert!(!s.graph.views.contains_key("view:class/inventory-service"));
    assert_eq!(level_view_id(&s, "ent:inventory-service"), None);
    assert_eq!(level_view_id(&s, "ent:order"), None);
    // Two plain children under a plain node: a class view.
    for id in ["ent:line-a", "ent:line-b"] {
        s.graph.entities.insert(
            id.into(),
            Entity {
                name: id.into(),
                parent: Some("ent:order".into()),
                ..Default::default()
            },
        );
    }
    let mut batch = RecordBatch::new(2);
    recompute(&mut s, "g2", &mut batch);
    assert_eq!(
        level_view_id(&s, "ent:order").as_deref(),
        Some("view:class/order")
    );
    assert_eq!(
        s.graph.views["view:class/order"].members,
        vec!["ent:line-a", "ent:line-b"]
    );
    // A node that drops below two children loses its view at the same commit.
    s.graph.entities.remove("ent:line-b");
    let mut batch = RecordBatch::new(3);
    recompute(&mut s, "g3", &mut batch);
    assert!(!s.graph.views.contains_key("view:class/order"));
    assert_eq!(level_view_id(&s, "ent:order"), None);
}

// Mirrors docs/compiler/model/view.md#level-views: a grouping of components is a
// component level, so the top diagram stays a component view after the
// components move under plain groupings.
#[test]
fn a_level_over_groupings_of_components_is_a_component_view() {
    let mut s = Store::default();
    let ent = |name: &str, parent: Option<&str>, stereotype: Option<&str>| Entity {
        name: name.into(),
        parent: parent.map(String::from),
        stereotype: stereotype.map(String::from),
        ..Default::default()
    };
    s.graph
        .entities
        .insert("ent:ledger".into(), ent("Ledger", None, Some("backend")));
    s.graph
        .entities
        .insert("ent:platform".into(), ent("Platform", None, None));
    s.graph
        .entities
        .insert("ent:orders".into(), ent("Orders", None, None));
    s.graph.entities.insert(
        "ent:queue".into(),
        ent("Queue", Some("ent:platform"), Some("component")),
    );
    s.graph.entities.insert(
        "ent:cache".into(),
        ent("Cache", Some("ent:platform"), Some("component")),
    );
    s.graph
        .entities
        .insert("ent:order".into(), ent("Order", Some("ent:orders"), None));
    s.graph.entities.insert(
        "ent:invoice".into(),
        ent("Invoice", Some("ent:orders"), None),
    );
    let mut batch = RecordBatch::new(1);
    recompute(&mut s, "g1", &mut batch);
    assert_eq!(
        level_view_id(&s, "scope:public").as_deref(),
        Some("view:component/public"),
        "components two levels down keep the root a component level"
    );
    assert_eq!(
        level_view_id(&s, "ent:platform").as_deref(),
        Some("view:component/platform")
    );
    assert_eq!(
        level_view_id(&s, "ent:orders").as_deref(),
        Some("view:class/orders"),
        "a grouping of plain entities is a class level"
    );
    // A whole-part statement from the ledger to a grandchild never makes the
    // ledger a peer of the orders level: the frame is not a member.
    s.graph.entities.get_mut("ent:orders").unwrap().parent = Some("ent:ledger".into());
    s.graph.entities.get_mut("ent:platform").unwrap().parent = Some("ent:ledger".into());
    s.graph.requirements.insert(
        "req:x-1".into(),
        Requirement {
            statement: "The Ledger covers the Order.".into(),
            entities: vec!["ent:ledger".into(), "ent:order".into()],
            edges: vec![edge("ent:ledger", "ent:order", "composition", None)],
            ..Default::default()
        },
    );
    let mut batch = RecordBatch::new(2);
    recompute(&mut s, "g2", &mut batch);
    assert_eq!(
        level_view_members(&s, "ent:orders"),
        vec!["ent:invoice", "ent:order"],
        "an ancestor with a whole-part edge is the frame, never a peer"
    );
}

// Mirrors docs/compiler/concepts/levels.md#the-scope-root: the root form keeps the
// per-scope id, lists the parentless entities, and adds the outside entities whose
// edges lift into the level.
#[test]
fn root_level_view_keeps_the_per_scope_id_and_lists_parentless_entities_plus_interactors() {
    let mut s = showcase_store();
    let mut batch = RecordBatch::new(1);
    recompute(&mut s, "g1", &mut batch);
    assert_eq!(
        level_view_id(&s, "scope:public").as_deref(),
        Some("view:component/public")
    );
    assert_eq!(
        s.graph.views["view:component/public"].members,
        vec!["ent:shop", "ent:customer", "ent:ana", "ent:anas-cart"]
    );
    assert_eq!(
        level_view_members(&s, "scope:public"),
        vec!["ent:shop", "ent:customer", "ent:ana", "ent:anas-cart"]
    );
    // A second scope with two plain roots gets a class view under its own name; an
    // entity of the public scope depending on one of them is an interactor.
    for (id, name) in [("ent:ledger", "Ledger"), ("ent:invoice", "Invoice")] {
        s.graph.entities.insert(
            id.into(),
            Entity {
                name: name.into(),
                scope: "billing".into(),
                ..Default::default()
            },
        );
    }
    s.graph.requirements.insert(
        "req:shop-12".into(),
        Requirement {
            statement: "The order posts to the ledger.".into(),
            entities: vec!["ent:order".into(), "ent:ledger".into()],
            edges: vec![edge("ent:order", "ent:ledger", "dependency", None)],
            source: Some(src("shop.md", "/shop/orders", "posts")),
            ..Default::default()
        },
    );
    let mut batch = RecordBatch::new(2);
    recompute(&mut s, "g2", &mut batch);
    let billing = &s.graph.views["view:class/billing"];
    assert_eq!(billing.kind, "class");
    assert_eq!(billing.title, "Billing");
    assert!(billing.default);
    assert_eq!(billing.query, None);
    assert_eq!(
        billing.members,
        vec!["ent:ledger", "ent:invoice", "ent:order"]
    );
    assert!(matches!(
        billing.provenance.as_ref(),
        Some(Provenance::Derived { from, reasoning })
            if from == &billing.members && reasoning == "default view: level view of the scope root"
    ));
    // The same edge makes the ledger an interactor of the order service's level.
    assert!(s.graph.views["view:component/order-service"]
        .members
        .contains(&"ent:ledger".to_string()));
    // A scope with one parentless entity has no root view.
    s.graph.entities.remove("ent:invoice");
    let mut batch = RecordBatch::new(3);
    recompute(&mut s, "g3", &mut batch);
    assert!(!s.graph.views.contains_key("view:class/billing"));
    assert_eq!(level_view_id(&s, "scope:billing"), None);
}

// Mirrors docs/compiler/model/view.md#level-views: the flows lift into a node's
// level under prefixed ids, while the root form keeps the unprefixed ones.
#[test]
fn lifted_flow_clustering_at_a_non_root_level_uses_prefixed_ids() {
    let mut s = showcase_store();
    let mut batch = RecordBatch::new(1);
    recompute(&mut s, "g1", &mut batch);
    // The shop level: every customer flow reaches it through the services. The
    // node level's title carries the node, so it reads apart from the root's.
    let uc = &s.graph.views["view:usecase/shop-shop"];
    assert_eq!(uc.kind, "use-case");
    assert_eq!(uc.title, "Inside Shop");
    assert_eq!(
        uc.members,
        vec!["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"]
    );
    assert!(matches!(
        uc.provenance.as_ref(),
        Some(Provenance::Derived { from, .. })
            if from[0] == "ent:shop" && from[1] == "req:shop-1"
    ));
    let seq = &s.graph.views["view:sequence/shop-shop"];
    assert_eq!(seq.members, vec!["req:shop-1", "req:shop-3"]);
    assert_eq!(seq.title, "Inside Shop");
    // The order service's level: three of the four reach it through its
    // children; the stock API step is the service itself (the frame) acting on
    // an outside interface, a flow of the shop level, so it stays out here.
    assert_eq!(
        s.graph.views["view:usecase/order-service-shop"].members,
        vec!["req:shop-1", "req:shop-7", "req:shop-8"]
    );
    // The root form keeps the unprefixed ids, and each flow view knows its level.
    assert!(s.graph.views.contains_key("view:usecase/shop"));
    assert_eq!(
        flow_view_level(&s, "view:usecase/shop").as_deref(),
        Some("scope:public")
    );
    assert_eq!(
        flow_view_level(&s, "view:sequence/shop-shop").as_deref(),
        Some("ent:shop")
    );
    assert_eq!(flow_view_level(&s, "view:usecase/nope"), None);
    // A flow among the order service's children with the customer absent joins
    // the document's cluster at that level: the key is the document, never the
    // first entity that happens to lead a sentence.
    for (id, text) in [
        ("req:shop-13", "The cart totals its items."),
        ("req:shop-14", "The cart prices its items."),
    ] {
        s.graph.requirements.insert(
            id.into(),
            Requirement {
                statement: text.into(),
                entities: vec!["ent:shopping-cart".into(), "ent:order-item".into()],
                edges: vec![edge(
                    "ent:shopping-cart",
                    "ent:order-item",
                    "dependency",
                    None,
                )],
                facets: vec![behavior()],
                source: Some(src("shop.md", "/shop/orders", text)),
                ..Default::default()
            },
        );
    }
    let mut batch = RecordBatch::new(2);
    recompute(&mut s, "g2", &mut batch);
    let svc = &s.graph.views["view:usecase/order-service-shop"];
    assert!(
        svc.members.contains(&"req:shop-13".to_string()),
        "{:?}",
        svc.members
    );
    assert!(
        svc.members.contains(&"req:shop-14".to_string()),
        "{:?}",
        svc.members
    );
    assert_eq!(svc.title, "Order Service: Shop");
    assert!(!s
        .graph
        .views
        .contains_key("view:usecase/order-service-shopping-cart-shop"));
    // At the shop level both entities lift to the order service, and at the
    // root to the shop: a flow internal to one child with a level of its own is
    // that child's, never a self-message above.
    assert!(!s.graph.views["view:usecase/shop-shop"]
        .members
        .contains(&"req:shop-13".to_string()));
    assert!(!s.graph.views["view:usecase/shop"]
        .members
        .contains(&"req:shop-13".to_string()));
    assert_eq!(
        lift_into(&s, &["ent:shop".to_string()], "ent:order-item").as_deref(),
        Some("ent:shop")
    );
    assert_eq!(
        lift_into(&s, &["ent:order".to_string()], "ent:customer"),
        None
    );
}

// Mirrors docs/compiler/model/view.md#fields: children lists, per drawn entity
// with a level view, the view one level down, in member order.
#[test]
fn children_of_view_lists_the_level_views_below_a_view() {
    let mut s = showcase_store();
    let mut batch = RecordBatch::new(1);
    recompute(&mut s, "g1", &mut batch);
    assert_eq!(
        children_of_view(&s, "view:component/public"),
        vec![("ent:shop".to_string(), "view:component/shop".to_string())]
    );
    assert_eq!(
        children_of_view(&s, "view:component/shop"),
        vec![(
            "ent:order-service".to_string(),
            "view:component/order-service".to_string()
        )]
    );
    assert!(children_of_view(&s, "view:component/order-service").is_empty());
    // A flow view's participants are its level's lifted members: the shop-level
    // sequence draws the customer, the order service, and the inventory service.
    assert_eq!(
        children_of_view(&s, "view:sequence/shop-shop"),
        vec![(
            "ent:order-service".to_string(),
            "view:component/order-service".to_string()
        )]
    );
    // The root sequence draws the raw participants lifted to the roots: the shop.
    assert_eq!(
        children_of_view(&s, "view:sequence/shop"),
        vec![("ent:shop".to_string(), "view:component/shop".to_string())]
    );
    // A curated view is read the same way; an unknown view has no children.
    s.graph.views.insert(
        "view:class/mine".into(),
        View {
            kind: "class".into(),
            title: "Mine".into(),
            members: vec!["ent:order".into(), "ent:order-service".into()],
            ..Default::default()
        },
    );
    assert_eq!(
        children_of_view(&s, "view:class/mine"),
        vec![(
            "ent:order-service".to_string(),
            "view:component/order-service".to_string()
        )]
    );
    assert!(children_of_view(&s, "view:class/nope").is_empty());
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
