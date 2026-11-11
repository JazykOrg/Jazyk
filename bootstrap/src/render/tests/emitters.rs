use super::*;
use crate::render::renderer::{hosted_svg, in_process_svg, strip_element_links};
use crate::render::scene::structure;
use crate::render::text::{alias_of, bracketed, inert, quoted};

#[test]
fn part_names_and_stereotypes_cannot_break_the_component_syntax() {
    assert_eq!(bracketed("Queue [legacy]"), "[Queue (legacy)]");
    assert_eq!(bracketed("Cache]"), "[Cache)]");
    assert_eq!(quoted("say \"hi\""), "\"say 'hi'\"");
}

// Mirrors docs/compiler/diagrams.md#level-views: a level's sequence view draws
// each message endpoint lifted to its nearest ancestor among the level's
// members, so a flow among leaves draws between their groupings; the frame
// itself, when a flow starts there, draws as itself.
#[test]
fn a_level_sequence_view_lifts_endpoints_to_the_level_members() {
    let mut s = Store::default();
    let ent = |name: &str, parent: Option<&str>| Entity {
        name: name.into(),
        parent: parent.map(String::from),
        ..Default::default()
    };
    s.graph
        .entities
        .insert("ent:checkout".into(), ent("Checkout", None));
    s.graph
        .entities
        .insert("ent:cart".into(), ent("Cart", Some("ent:checkout")));
    s.graph
        .entities
        .insert("ent:funds".into(), ent("Funds", Some("ent:checkout")));
    s.graph
        .entities
        .insert("ent:gift-card".into(), ent("Gift Card", Some("ent:funds")));
    s.graph.entities.insert(
        "ent:loyalty-point".into(),
        ent("Loyalty Point", Some("ent:funds")),
    );
    let req = |text: &str, a: &str, b: &str| Requirement {
        statement: text.into(),
        entities: vec![a.into(), b.into()],
        edges: vec![ReqEdge {
            a: a.into(),
            b: b.into(),
            rel_type: Some("dependency".into()),
            cardinality: None,
        }],
        facets: vec![Facet {
            facet: "behavior".into(),
            reasoning: String::new(),
            measure: None,
        }],
        source: Some(SourceRef {
            doc: "checkout.md".into(),
            section: "/checkout".into(),
            quote: text.into(),
        }),
        ..Default::default()
    };
    s.graph.requirements.insert(
        "req:c-1".into(),
        req("A Gift Card pays for a Cart.", "ent:gift-card", "ent:cart"),
    );
    s.graph.requirements.insert(
        "req:c-2".into(),
        req(
            "A Loyalty Point discounts a Cart.",
            "ent:loyalty-point",
            "ent:cart",
        ),
    );
    s.graph.requirements.insert(
        "req:c-3".into(),
        req(
            "The Checkout charges a Gift Card.",
            "ent:checkout",
            "ent:gift-card",
        ),
    );
    // Internal to the funds: the funds level's flow, never a self-message above.
    s.graph.requirements.insert(
        "req:c-4".into(),
        req(
            "A Gift Card earns a Loyalty Point.",
            "ent:gift-card",
            "ent:loyalty-point",
        ),
    );
    // The first edge collapses inside the funds; the message follows the next
    // edge, whose lifted ends differ.
    let mut c6 = req(
        "A Gift Card earns a Loyalty Point toward a Cart.",
        "ent:gift-card",
        "ent:loyalty-point",
    );
    c6.entities.push("ent:cart".into());
    c6.edges.push(ReqEdge {
        a: "ent:loyalty-point".into(),
        b: "ent:cart".into(),
        rel_type: Some("association".into()),
        cardinality: None,
    });
    s.graph.requirements.insert("req:c-6".into(), c6);
    let mut batch = RecordBatch::new(1);
    recompute(&mut s, "g1", &mut batch);
    let id = "view:sequence/checkout-checkout";
    assert!(
        !s.graph.views[id].members.iter().any(|m| m == "req:c-4"),
        "{:?}",
        s.graph.views[id].members
    );
    assert!(
        s.graph.views["view:sequence/funds-checkout"]
            .members
            .iter()
            .any(|m| m == "req:c-4"),
        "{:?}",
        s.graph.views.keys().collect::<Vec<_>>()
    );
    let view = s
        .graph
        .views
        .get(id)
        .unwrap_or_else(|| panic!("views: {:?}", s.graph.views.keys().collect::<Vec<_>>()));
    let puml = emit(&s, id, view);
    assert!(puml.contains("participant \"Funds\""), "{}", puml);
    assert!(puml.contains("participant \"Cart\""), "{}", puml);
    assert!(!puml.contains("participant \"Gift Card\""), "{}", puml);
    assert!(
        puml.contains("participant \"Checkout\""),
        "the frame draws as itself: {}",
        puml
    );
    assert!(
        puml.contains("funds -> cart : A Gift Card earns a Loyalty Point toward a Cart. (req:c-6)"),
        "{}",
        puml
    );
    // The limit counts what the level draws: three lifted participants, not the
    // four leaves beneath them.
    assert_eq!(crate::derive::flow_view_participant_count(&s, id, view), 3);
    // The drill-down participants agree with the drawing.
    let drawn = crate::derive::children_of_view(&s, id);
    assert!(drawn.iter().any(|(m, _)| m == "ent:funds"), "{:?}", drawn);
}

#[test]
fn class_emitter_matches_the_showcase() {
    let s = showcase();
    let puml = emit_kind(
        &s,
        "class",
        &[
            "ent:customer",
            "ent:shopping-cart",
            "ent:order",
            "ent:order-item",
        ],
    );
    // The showcase block, plus the cart's two attributes the showcase graph states
    // and the block leaves out by hand.
    let expected = "@startuml\nclass Customer <<actor>> {\n  tier : string\n}\nclass \"Shopping Cart\" as Cart {\n  items\n  currency\n}\nclass Order {\n  total\n  currency\n}\nclass \"Order Item\" as Item\nCustomer -- Cart\nCart *-- \"1..*\" Item\n@enduml\n";
    assert_eq!(canon(&puml), canon(expected), "{}", puml);
    assert!(!puml.contains("title "));
}

// Graph text never reaches a renderer as a preprocessor directive, a builtin, or
// image markup (docs/compiler/diagrams.md#the-emitters).
#[test]
fn graph_text_cannot_inject_preprocessor_directives() {
    let mut s = showcase();
    let attr = |name: &str, ty: Option<&str>| Attribute {
        name: name.into(),
        r#type: ty.map(String::from),
        value: None,
        provenance: Provenance::Decree {
            author: "owner".into(),
            at: "g1".into(),
            note: None,
        },
    };
    let cart = s.graph.entities.get_mut("ent:shopping-cart").unwrap();
    cart.attributes.push(attr("!include /etc/hosts", None));
    cart.attributes.push(attr("@enduml", None));
    cart.attributes
        .push(attr("home", Some("%getenv(\"HOME\")")));
    cart.attributes.push(attr("<img:/etc/hosts>", None));
    // A line that opens with a non-ASCII letter once panicked the crate's
    // preprocessor; the pure stages draw it.
    cart.attributes.push(attr("číslo", None));
    cart.name = "!includeurl http://example.invalid/x".into();
    let puml = emit_kind(&s, "class", &["ent:customer", "ent:shopping-cart"]);
    let body: Vec<&str> = puml.lines().skip(1).collect();
    assert_eq!(body.last(), Some(&"@enduml"), "{}", puml);
    for line in &body[..body.len() - 1] {
        let t = line.trim_start();
        assert!(!t.starts_with('!') && !t.starts_with('@'), "{}", puml);
    }
    assert!(!puml.to_ascii_lowercase().contains("<img"), "{}", puml);
    assert!(!puml.contains("%getenv"), "{}", puml);
    assert!(puml.contains("include /etc/hosts"), "{}", puml);
    // The in-process renderer draws it, and the hosted path accepts it too.
    render_svg(&puml).unwrap();
    assert!(
        hosted_svg(&strip_element_links(&puml).0).is_ok(),
        "{}",
        puml
    );
    // A timing view's time lines keep their `@`.
    assert_eq!(inert("@0"), "@0");
    assert_eq!(inert("@.5"), "@.5");
    assert_eq!(inert("  !include x"), "  \u{200B}!include x");
}

// Distinct entity ids never share an alias (docs/compiler/diagrams.md#the-emitters).
#[test]
fn aliases_stay_distinct_for_distinct_ids() {
    assert_eq!(alias_of("ent:order-item"), "order_item");
    let ids = [
        "ent:a-b",
        "ent:a_b",
        "ent:a--b",
        "ent:class",
        "ent:n-class",
        "ent:2fa",
        "ent:n-2fa",
    ];
    let aliases: BTreeSet<String> = ids.iter().map(|id| alias_of(id)).collect();
    assert_eq!(aliases.len(), ids.len(), "{aliases:?}");
    for a in &aliases {
        assert!(a.chars().all(|c| c.is_alphanumeric() || c == '_'), "{a}");
        assert!(!a.starts_with(|c: char| c.is_ascii_digit()), "{a}");
    }
}

#[test]
fn object_emitter_names_types_and_values() {
    let s = showcase();
    let puml = emit_kind(&s, "object", &["ent:ana", "ent:anas-cart"]);
    let c = canon(&puml);
    assert!(
        c.contains(&"object Ana : Customer {".to_string()),
        "{}",
        puml
    );
    assert!(
        c.contains(&"object Ana's cart : Shopping Cart {".to_string()),
        "{}",
        puml
    );
    assert!(c.contains(&"tier = gold".to_string()));
    assert!(c.contains(&"items = 3".to_string()));
    assert!(c.contains(&"currency = EUR".to_string()));
    assert!(
        c.contains(&"Ana : Customer -- Ana's cart : Shopping Cart".to_string()),
        "{:?}",
        c
    );
    // No instantiation arrow.
    assert_eq!(
        c.iter()
            .filter(|l| l.contains(" -- ") || l.contains("..>"))
            .count(),
        1
    );
}

#[test]
fn package_emitter_nests_classes_and_never_emits_an_empty_body() {
    let s = showcase();
    let puml = emit_kind(
        &s,
        "package",
        &[
            "ent:order-service",
            "ent:shopping-cart",
            "ent:order",
            "ent:order-item",
            "ent:inventory-service",
        ],
    );
    // The showcase block, minus the empty body it draws for the inventory service.
    let expected = "@startuml\npackage \"Order Service\" as OS {\n  class \"Shopping Cart\"\n  class Order\n  class \"Order Item\"\n}\npackage \"Inventory Service\" as IS\nOS ..> IS\n@enduml\n";
    assert_eq!(canon(&puml), canon(expected), "{}", puml);
    assert!(!puml.contains("{\n}"), "{}", puml);
    // Two leaf packages: one-line forms and the lifted arrow only.
    let puml = emit_kind(
        &s,
        "package",
        &["ent:order-service", "ent:inventory-service"],
    );
    assert!(!puml.contains('{'), "{}", puml);
    assert_eq!(
        canon(&puml),
        vec![
            "@startuml",
            "package Order Service",
            "package Inventory Service",
            "Order Service ..> Inventory Service",
            "@enduml"
        ]
    );
}

#[test]
fn component_emitter_draws_lollipops_and_sockets() {
    let s = showcase();
    let puml = emit_kind(
        &s,
        "component",
        &[
            "ent:customer",
            "ent:order-service",
            "ent:inventory-service",
            "ent:checkout-api",
            "ent:stock-api",
        ],
    );
    // The showcase block, plus the customer's association with the shopping cart
    // lifted to the order service that hides it.
    let expected = "@startuml\nactor Customer\ncomponent \"Order Service\" as OS <<service>>\ncomponent \"Inventory Service\" as IS <<service>>\ninterface \"checkout API\" as C\ninterface \"stock API\" as S\nCustomer ..> C\nOS -- C\nCustomer -- OS\nIS -- S\nOS --( S : use\n@enduml\n";
    let mut mine = canon(&puml);
    let mut want = canon(expected);
    mine.sort();
    want.sort();
    assert_eq!(mine, want, "{}", puml);
    // Arrows come in relationship id order: the pair slugs, lexical.
    let arrows: Vec<String> = canon(&puml)
        .into_iter()
        .filter(|l| l.contains(" -- ") || l.contains("..>") || l.contains("--("))
        .collect();
    assert_eq!(
        arrows,
        vec![
            "Customer ..> checkout API",
            "Order Service -- checkout API",
            "Customer -- Order Service",
            "Inventory Service -- stock API",
            "Order Service --( stock API : use"
        ]
    );
    // The shop's derived level view is the docs' example: the two services, the
    // customer whose edges lift into the level, and the drill-down link on the
    // service that holds a level of its own (its four children); the customer's
    // association with the hidden cart joins its lifted dependency under one arrow.
    let puml = emit(
        &s,
        "view:component/shop",
        &s.graph.views["view:component/shop"],
    );
    assert_eq!(
        canon(&puml),
        vec![
            "@startuml",
            "component Inventory Service <<service>>",
            "component Order Service <<service>>",
            "actor Customer",
            "Customer -- Order Service : 2 edges",
            "Order Service ..> Inventory Service",
            "@enduml"
        ],
        "{}",
        puml
    );
    // Every element links to its card, a level below it or not.
    assert!(
        puml.contains(
            "component \"Order Service\" as order_service <<service>> [[../../docsgen/entities/order-service.md]]\n"
        ),
        "{}",
        puml
    );
    assert!(
        puml.contains("component \"Inventory Service\" as inventory_service <<service>> [[../../docsgen/entities/inventory-service.md]]\n"),
        "{}",
        puml
    );
}

#[test]
fn composite_emitter_frames_parts() {
    let s = showcase();
    let puml = emit_kind(
        &s,
        "composite",
        &[
            "ent:order-service",
            "ent:shopping-cart",
            "ent:order",
            "ent:order-item",
        ],
    );
    let c = canon(&puml);
    assert_eq!(c[1], "component Order Service {");
    assert_eq!(c[2], "[Shopping Cart]");
    assert_eq!(c[3], "[Order]");
    assert_eq!(c[4], "[Order Item]");
    assert_eq!(c[5], "}");
    assert_eq!(c[6], "Shopping Cart *-- 1..* Order Item");
}

#[test]
fn deployment_emitter_places_artifacts_by_value() {
    let s = showcase();
    let puml = emit_kind(&s, "deployment", &["ent:shop", "ent:customer"]);
    let c = canon(&puml);
    assert_eq!(c[1], "node EU region {");
    assert_eq!(c[2], "artifact Shop");
    assert_eq!(c[3], "}");
    assert_eq!(c[4], "artifact Customer");
}

#[test]
fn use_case_emitter_links_actors() {
    let s = showcase();
    let puml = emit(
        &s,
        "view:usecase/checkout",
        &s.graph.views["view:usecase/checkout"],
    );
    let expected = "@startuml\nactor Customer\nusecase Checkout\nCustomer -- Checkout\n@enduml\n";
    assert_eq!(canon(&puml), canon(expected), "{}", puml);
    // No member names an actor: the picture says so under the bubble
    // (docs/compiler/diagrams.md#the-emitters).
    let puml = emit_kind(&s, "use-case", &["req:shop-2", "req:shop-4"]);
    assert!(
        puml.contains("note bottom of")
            && puml.contains("no actor: none of the 2 member statement(s)"),
        "{}",
        puml
    );
}

#[test]
fn activity_emitter_branches_on_shared_transitions() {
    let s = showcase();
    let puml = emit_kind(
        &s,
        "activity",
        &["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"],
    );
    assert_eq!(
        canon(&puml),
        vec![
            "@startuml",
            "start",
            ":(req:shop-1);",
            ":(req:shop-3);",
            "if (payment succeeds?) then (yes)",
            ":(req:shop-7);",
            "else (no)",
            ":(req:shop-8);",
            "endif",
            "stop",
            "@enduml"
        ],
        "{}",
        puml
    );
    assert!(puml.contains(":When payment succeeds, the order becomes paid. (req:shop-7);"));
}

#[test]
fn state_emitter_matches_the_showcase() {
    let s = showcase();
    let puml = emit(&s, "view:state/order", &s.graph.views["view:state/order"]);
    assert_eq!(
        puml,
        "@startuml\n[*] --> placed\nplaced --> paid : payment succeeds\nplaced --> held : payment declined\n@enduml\n"
    );
    // A subject without a machine draws as one state; spaced names get aliases.
    let puml = emit_kind(&s, "state", &["ent:customer"]);
    assert!(puml.contains("state \"Customer\" as customer"));
    let mut s = s;
    s.graph
        .requirements
        .get_mut("req:shop-8")
        .unwrap()
        .transition
        .as_mut()
        .unwrap()
        .to = "held for review".into();
    recompute(&mut s, "g2", &mut RecordBatch::new(2));
    let puml = emit(&s, "view:state/order", &s.graph.views["view:state/order"]);
    assert!(
        puml.contains("state \"held for review\" as held_for_review"),
        "{}",
        puml
    );
    assert!(puml.contains("placed --> held_for_review : payment declined"));
}

#[test]
fn sequence_emitter_resolves_interfaces_to_providers() {
    let s = showcase();
    let puml = emit(
        &s,
        "view:sequence/checkout",
        &s.graph.views["view:sequence/checkout"],
    );
    let expected = "@startuml\nactor Customer\nCustomer -> \"Order Service\" : submit cart (req:shop-1)\n\"Order Service\" -> \"Inventory Service\" : reserve stock (req:shop-3)\n@enduml\n";
    assert_eq!(
        lines_with(&puml, "->"),
        lines_with(expected, "->"),
        "{}",
        puml
    );
    assert_eq!(lines_with(&puml, "actor "), vec!["actor Customer"]);
    assert!(puml.contains("participant \"Order Service\" as order_service"));
    // A member with no edge is a self-message on its first entity.
    let puml = emit_kind(&s, "sequence", &["req:shop-1", "req:shop-5"]);
    assert!(
        lines_with(&puml, "->").contains(&"Order -> Order : (req:shop-5)".to_string()),
        "{}",
        puml
    );
}

#[test]
fn communication_emitter_numbers_messages() {
    let s = showcase();
    let puml = emit_kind(&s, "communication", &["req:shop-1", "req:shop-3"]);
    let expected = "@startuml\nactor Customer\nrectangle \"Order Service\" as OS\nrectangle \"Inventory Service\" as IS\nCustomer -> OS : 1. submit cart\nOS -> IS : 2. reserve stock\n@enduml\n";
    assert_eq!(canon(&puml), canon(expected), "{}", puml);
}

#[test]
fn timing_emitter_marks_the_measure() {
    let s = showcase();
    let puml = emit_kind(&s, "timing", &["ent:order", "req:shop-10"]);
    let expected = "@startuml\ntitle checkout confirms within 2s (req:shop-10)\nrobust \"Order\" as O\n@0\nO is placed\n@2\nO is paid\n@enduml\n";
    let c = canon(&puml);
    let want = canon(expected);
    assert_eq!(c[2..], want[2..], "{}", puml);
    assert!(
        c[1].starts_with("title The shop shall confirm checkout within 2 seconds. (req:shop-10)"),
        "{}",
        puml
    );
    // No measure: the lane and nothing else.
    let puml = emit_kind(&s, "timing", &["ent:order"]);
    assert_eq!(canon(&puml), vec!["@startuml", "robust Order", "@enduml"]);
}

#[test]
fn overview_emitter_references_sequence_views() {
    let s = showcase();
    let puml = emit_kind(
        &s,
        "overview",
        &["req:shop-1", "req:shop-3", "req:shop-7", "req:shop-8"],
    );
    assert_eq!(
        canon(&puml),
        vec![
            "@startuml",
            "start",
            ":ref: view:sequence/checkout;",
            ":(req:shop-7);",
            ":(req:shop-8);",
            "stop",
            "@enduml"
        ],
        "{}",
        puml
    );
}

// An association whose ids sort reversed keeps its cardinality at its `b` end
// (docs/compiler/diagrams.md#the-emitters).
#[test]
fn reversed_association_keeps_its_cardinality() {
    let mut s = tree_store();
    s.graph.requirements.insert(
        "req:t-1".into(),
        Requirement {
            statement: "B holds many A".into(),
            entities: vec!["ent:b".into(), "ent:a".into()],
            edges: vec![ReqEdge {
                a: "ent:b".into(),
                b: "ent:a".into(),
                rel_type: Some("association".into()),
                cardinality: Some("0..*".into()),
            }],
            source: Some(SourceRef {
                doc: "t.md".into(),
                section: "/t".into(),
                quote: "q".into(),
            }),
            ..Default::default()
        },
    );
    let mut batch = RecordBatch::new(1);
    recompute(&mut s, "g1", &mut batch);
    let members = vec!["ent:a".to_string(), "ent:b".to_string()];
    let st = structure(&s, &members, &BTreeSet::new());
    assert_eq!(st.arrows.len(), 1);
    let arrow = &st.arrows[0];
    assert_eq!((arrow.a.as_str(), arrow.b.as_str()), ("ent:b", "ent:a"));
    assert_eq!(arrow.cardinality.as_deref(), Some("0..*"));
}

// Mirrors docs/compiler/diagrams.md#drill-down: every rendered entity links to
// its card, a level below it or not, in its own level view too.
#[test]
fn structural_emitters_link_every_member_to_its_card() {
    let s = level_tree();
    let a_view = crate::derive::level_view_id(&s, "ent:a").expect("two children make a level");
    assert!(crate::derive::level_view_id(&s, "ent:b").is_none());
    assert_eq!(card_link("ent:a"), "[[../../docsgen/entities/a.md]]");
    for kind in ["class", "component"] {
        let v = view(kind, "System", &["ent:a", "ent:b"]);
        let puml = emit(&s, &format!("view:{}/system", kind), &v);
        assert!(
            puml.contains(&format!("{} \"A\" as a {}\n", kind, card_link("ent:a"))),
            "{}",
            puml
        );
        assert!(
            puml.contains(&format!("{} \"B\" as b {}\n", kind, card_link("ent:b"))),
            "{}",
            puml
        );
        assert_eq!(puml.matches("[[").count(), 2, "{}", puml);
        assert!(!puml.contains(".svg]]"), "{}", puml);
    }
    let own = view("class", "A", &["ent:a", "ent:a1", "ent:a2"]);
    let puml = emit(&s, &a_view, &own);
    assert!(puml.contains(&card_link("ent:a1")), "{}", puml);
    assert_eq!(puml.matches("[[").count(), 3, "{}", puml);
}

// A flow participant links to its card the same way; a message carries none.
#[test]
fn flow_participants_link_to_their_cards() {
    let mut s = tree_store();
    for id in ["ent:a", "ent:b"] {
        s.graph.entities.get_mut(id).unwrap().stereotype = Some("actor".into());
    }
    add_req(&mut s, "req:t-1", "ent:a", "ent:b1", "dependency");
    add_req(&mut s, "req:t-2", "ent:b", "ent:a1", "dependency");
    recompute(&mut s, "g1", &mut RecordBatch::new(1));
    let v = view("use-case", "Flow", &["req:t-1", "req:t-2"]);
    let puml = emit(&s, "view:usecase/flow", &v);
    assert!(
        puml.contains(&format!("actor \"A\" as a {}\n", card_link("ent:a"))),
        "{}",
        puml
    );
    assert!(
        puml.contains(&format!("actor \"B\" as b {}\n", card_link("ent:b"))),
        "{}",
        puml
    );
    // The in-process crate would split a linked actor in two: one actor per member.
    let svg = in_process_svg(&puml).unwrap();
    assert_eq!(svg.matches("class=\"entity\"").count(), 3, "{}", svg);
    assert!(
        svg.contains("xlink:href=\"../../docsgen/entities/a.md\""),
        "{}",
        svg
    );
    assert_eq!(svg.matches("<a ").count(), 2, "{}", svg);

    let seq = view("sequence", "Flow", &["req:t-1", "req:t-2"]);
    let puml = emit(&s, "view:sequence/flow", &seq);
    assert!(
        puml.contains(&format!("actor \"A\" as a {}\n", card_link("ent:a"))),
        "{}",
        puml
    );
    assert!(
        puml.contains(&format!(
            "participant \"B1\" as b1 {}\n",
            card_link("ent:b1")
        )),
        "{}",
        puml
    );
    for line in puml.lines().filter(|l| l.contains(" -> ")) {
        assert!(!line.contains("[["), "a message carries no link: {}", line);
    }
    let svg = in_process_svg(&puml).unwrap();
    assert!(
        svg.contains("xlink:href=\"../../docsgen/entities/b1.md\""),
        "{}",
        svg
    );
    assert!(!svg.contains("[["), "{}", svg);
}
