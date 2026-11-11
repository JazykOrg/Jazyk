use super::*;

// A heading quoted as body text is refused with the reason, not the generic
// absence line (docs/compiler/graph.md#validation-gates).
#[test]
fn heading_quoted_as_body_is_named() {
    let mut t = session();
    let err = t
        .dispatch(
            "upsert_requirement",
            &json!({
                "facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}],
                "statement": "The Cart exists.",
                "entities": ["ent:customer"],
                "section": "/shop/cart",
                "quote": "## Cart",
            }),
        )
        .unwrap_err();
    assert_eq!(err.rule, "quote-not-found");
    assert!(err.message.contains("heading"), "{}", err.message);
}

#[test]
fn quote_must_locate() {
    let mut t = session();
    let err = t
        .dispatch("upsert_entity", &json!({"name": "Shopping Cart", "mention": {"section": "/shop/cart", "quote": "this text is not there"}}))
        .unwrap_err();
    assert_eq!(err.rule, "quote-not-found");
}

#[test]
fn requirement_needs_a_statement_and_known_entities() {
    let mut t = session();
    let err = t
        .dispatch("upsert_requirement", &json!({"facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}], "statement": "   ", "entities": ["ent:customer"], "section": "/shop/cart", "quote": "holds items"}))
        .unwrap_err();
    assert_eq!(err.rule, "bad-args");
    let err2 = t
        .dispatch("upsert_requirement", &json!({"facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}], "statement": "The Cart holds items.", "entities": ["ent:cart"], "section": "/shop/cart", "quote": "holds items"}))
        .unwrap_err();
    assert_eq!(err2.rule, "unknown-id");
    assert!(
        err2.message.contains("upsert_entity"),
        "repair hint: {}",
        err2.message
    );
    // Free-form statements land: no shape gate stands between prose and the graph.
    let ok = t
        .dispatch("upsert_requirement", &json!({"facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}], "statement": "The cart is nice.", "entities": ["ent:customer"], "section": "/shop/cart", "quote": "holds items"}))
        .unwrap();
    assert_eq!(ok["created"], true);
}

// Mirrors docs/compiler/model/criterion.md and tools.md#write-tools: a criterion
// enters on its requirement, folds on the same statement, refuses a restatement,
// and covers the section its quote sits in.
#[test]
fn criteria_enter_through_upsert_criterion_and_cover_their_section() {
    let mut t = session();
    let rid = t
        .dispatch(
            "upsert_requirement",
            &json!({
"facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}],"statement": "The Shopping Cart holds the items a Customer intends to buy.",
                    "entities": ["ent:customer"], "section": "/shop/cart",
                    "quote": "The Shopping Cart holds items a Customer intends to buy."}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let v = t
        .dispatch(
            "upsert_criterion",
            &json!({"requirement": rid, "statement": "A Customer who adds two items sees a Cart holding two items.",
                    "kind": "positive", "section": "/shop", "quote": "intro text"}),
        )
        .unwrap();
    let c1 = v["id"].as_str().unwrap().to_string();
    assert!(c1.starts_with("crit:shop-"), "{}", v);
    assert_eq!(v["created"], true);
    let again = t
        .dispatch(
            "upsert_criterion",
            &json!({"requirement": rid, "statement": "A Customer who adds two items sees a Cart holding two items",
                    "kind": "positive", "section": "/shop", "quote": "intro text"}),
        )
        .unwrap();
    assert_eq!(again["id"], c1.as_str(), "{}", again);
    assert_eq!(again["updated"], true);
    let err = t
        .dispatch(
            "upsert_criterion",
            &json!({"requirement": rid, "statement": "The Shopping Cart holds the items a Customer intends to buy",
                    "kind": "positive", "section": "/shop", "quote": "intro text"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "criterion-restates", "{}", err.message);
    let err = t
        .dispatch(
            "upsert_criterion",
            &json!({"requirement": rid, "statement": "An empty Cart shows no items.", "kind": "boundary",
                    "provenance": {"derived": {"from": [rid]}}}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-provenance", "{}", err.message);
    let derived = t
        .dispatch(
            "upsert_criterion",
            &json!({"requirement": rid, "statement": "An empty Cart shows no items.", "kind": "boundary",
                    "provenance": {"derived": {"from": [rid], "reasoning": "the lower edge of holding items"}}}),
        )
        .unwrap();
    let c2 = derived["id"].as_str().unwrap();
    assert!(c2.starts_with("crit:shop-") && c2 != c1, "{}", derived);
    assert!(t
        .dispatch("upsert_criterion", &json!({"requirement": "req:nope-1", "statement": "x y z", "kind": "positive", "section": "/shop", "quote": "intro text"}))
        .is_err());
    // The intro section holds only the criterion's quote: covered by it.
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop", "state": "covered", "note": "crit:shop-1 carries the example."}),
    )
    .unwrap();
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop/cart", "state": "covered", "note": "the cart rule."}),
    )
    .unwrap();
    let v = t
        .dispatch(
            "mark_goal_done",
            &json!({"goal": "g:reconcile-section:shop.md#/shop", "justification": "The example is the criterion."}),
        )
        .unwrap();
    assert_eq!(v["marked"], "g:reconcile-section:shop.md#/shop", "{}", v);
}

// Mirrors docs/compiler/goals/declare-edges.md#gate: an edge staged without a type
// and then staged again with one is judged as the changeset leaves it, so the
// repaired call clears the refusal (a local model once spent a whole session on a
// superseded op it could not unstage).
#[test]
fn a_repaired_edge_clears_the_untyped_refusal() {
    let mut s = Store::default();
    s.graph
        .entities
        .insert("ent:operator".into(), plain("Operator"));
    s.graph
        .entities
        .insert("ent:admin-cli".into(), plain("Admin CLI"));
    s.graph.requirements.insert(
        "req:admin-3".into(),
        Requirement {
            statement: "The Admin CLI lets an Operator run Orderly.".into(),
            entities: vec!["ent:operator".into(), "ent:admin-cli".into()],
            ..Default::default()
        },
    );
    let goals = one_goal("declare-edges", "req:admin-3", Value::Null);
    let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &goals), 64, 24_000);
    let mark = |t: &mut ToolSession| {
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:declare-edges:req:admin-3", "justification": "The Operator uses the Admin CLI.", "evidence": "declared"}),
        )
    };
    t.dispatch(
        "update_requirement",
        &json!({"id": "req:admin-3", "edges": [{"a": "ent:operator", "b": "ent:admin-cli", "cardinality": "1"}]}),
    )
    .unwrap();
    assert_eq!(mark(&mut t).unwrap_err().rule, "untyped-edge");
    t.dispatch(
        "update_requirement",
        &json!({"id": "req:admin-3", "edges": [{"a": "ent:operator", "b": "ent:admin-cli", "cardinality": "1", "type": "dependency"}]}),
    )
    .unwrap();
    assert!(mark(&mut t).is_ok(), "the repaired edge stands");
    assert!(t
        .dispatch("done", &json!({"summary": "Declared the edge."}))
        .is_ok());
}

#[test]
fn update_requirement_takes_an_empty_statement_as_unchanged_and_rejects_a_bad_cardinality() {
    let mut t = session();
    t.dispatch(
        "upsert_entity",
        &json!({"name": "Shopping Cart", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}}),
    )
    .unwrap();
    let r = t
        .dispatch(
            "upsert_requirement",
            &json!({"facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}], "statement": "The Shopping Cart holds items.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "holds items a Customer intends to buy"}),
        )
        .unwrap();
    let rid = r["id"].as_str().unwrap().to_string();
    // An empty statement counts as absent: the call lands, statement unchanged.
    let v = t
        .dispatch(
            "update_requirement",
            &json!({"id": rid, "statement": "", "entities": ["ent:shopping-cart"]}),
        )
        .unwrap();
    assert_eq!(v["id"], rid);
    let err = t
        .dispatch(
            "update_requirement",
            &json!({"id": rid, "entities": ["ent:shopping-cart", "ent:customer"], "edges": [{"a": "ent:shopping-cart", "b": "ent:customer", "type": "composition", "cardinality": "many"}]}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-cardinality");
    assert!(err.message.contains("1..*"), "{}", err.message);
}

#[test]
fn edges_must_be_subset_of_entities() {
    let mut t = session();
    t.dispatch("upsert_entity", &json!({"name": "Shopping Cart", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}})).unwrap();
    let err = t
        .dispatch(
            "upsert_requirement",
            &json!({"facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}], "statement": "The Shopping Cart shall exist.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "holds items", "edges": [{"a": "ent:shopping-cart", "b": "ent:customer"}]}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-edge");
}

#[test]
fn escaped_quote_locates_and_stores_source_form() {
    let mut t = session();
    let v = t
        .dispatch(
            "upsert_requirement",
            &json!({"facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}], "statement": "The Customer shall intend to buy.", "entities": ["ent:customer"], "section": "/shop/cart", "quote": "a Customer intends to buy\\."}),
        )
        .unwrap();
    assert_eq!(v["created"], true);
    match t
        .staged
        .iter()
        .find(|o| matches!(o, Op::CreateRequirement { .. }))
        .unwrap()
    {
        Op::CreateRequirement { requirement, .. } => {
            assert_eq!(
                requirement.source.as_ref().unwrap().quote,
                "a Customer intends to buy."
            )
        }
        _ => unreachable!(),
    }
}

#[test]
fn repeated_upsert_resolves_to_one_staged_statement() {
    let mut t = session();
    t.dispatch(
        "upsert_entity",
        &json!({"name": "Shopping Cart", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}}),
    )
    .unwrap();
    let args = json!({"facets": [{"facet": "constraint", "reasoning": "an invariant the cart holds"}], "statement": "The Shopping Cart shall hold items.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "holds items a Customer intends to buy"});
    let v1 = t.dispatch("upsert_requirement", &args).unwrap();
    assert_eq!(v1["created"], true);
    // The identical call again is idempotent within the turn: same id, one staged op.
    let v2 = t.dispatch("upsert_requirement", &args).unwrap();
    assert_eq!(v2["updated"], true);
    assert_eq!(v1["id"], v2["id"]);
    assert_eq!(
        t.staged
            .iter()
            .filter(|o| matches!(o, Op::CreateRequirement { .. }))
            .count(),
        1
    );
}

#[test]
fn update_requirement_reanchors_with_section_and_quote() {
    let mut t = session_with_stale_anchor();
    // Re-anchoring needs the pair together, and the quote must locate.
    let err = t
        .dispatch(
            "update_requirement",
            &json!({"id": "req:shop-1", "section": "/shop/cart"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "bad-argument");
    let err2 = t
        .dispatch(
            "update_requirement",
            &json!({"id": "req:shop-1", "section": "/shop/cart", "quote": "not in the document"}),
        )
        .unwrap_err();
    assert_eq!(err2.rule, "quote-not-found");
    t.dispatch(
        "update_requirement",
        &json!({"id": "req:shop-1", "statement": "The Shopping Cart shall keep items a Customer intends to buy.", "section": "/shop/cart", "quote": "The Shopping Cart keeps items a Customer intends to buy."}),
    )
    .unwrap();
    t.dispatch(
        "set_coverage",
        &json!({"section": "/shop/cart", "state": "covered"}),
    )
    .unwrap();
    t.dispatch(
        "mark_goal_done",
        &json!({"goal": "g:reconcile-section:shop.md#/shop/cart", "justification": "Revised and re-anchored."}),
    )
    .unwrap();
    t.dispatch("done", &json!({"summary": "revised and re-anchored"}))
        .unwrap();
    assert!(t.done.is_some());
}

// A WorkItem built from a GC goal carries the kind's name as its task; the
// toolset is the kind's slice, never the read-only fallback (the snippet and
// the MCP case path build their tool list from it).
// The justification counter: document names, ids, and abbreviations running
// into lowercase never end a sentence; an opener after the period does.
#[test]
fn sentence_count_ignores_dots_in_names_and_abbreviations() {
    let n = ToolSession::sentence_count;
    assert_eq!(n("The definition fits customer.md verbatim."), 1);
    assert_eq!(
        n("Order is a separate concept (the area vs. its member), so it stands."),
        1
    );
    assert_eq!(n("It matches, e.g. the cart line. Nothing changed."), 2);
    assert_eq!(n("One. Two. Three."), 3);
    assert_eq!(n("Is it done? Yes! Done"), 3);
    assert_eq!(n("Ends with an id like ent:order. (Then a bracket.)"), 2);
    assert_eq!(n(""), 0);
}

// A requirement quotes one sentence; a fact line of short fragments and a numbered
// item stand. Mirrors docs/compiler/graph.md#validation-gates.
#[test]
fn a_requirement_quote_spanning_two_sentences_is_refused() {
    let mut s = Store::default();
    let text = "# Warehouse\n\n## Frontend\n\nThe frontend is a web application built using React. It communicates with the\nbackend over a REST API.\n\n## People\n\nDot Feeney. Age 58. Occupation: ferry office manager.\n\n1. The employee submits the claim to their manager.\n";
    s.docs.insert(
        "w.md".into(),
        DocRecord {
            content_hash: hash_hex(text),
            sections: crate::md::parse_sections(text),
            coverage: BTreeMap::new(),
        },
    );
    s.graph
        .entities
        .insert("ent:frontend".into(), plain("Frontend"));
    let mut t = ToolSession::new(
        s,
        reconcile_scope("w.md", &["/warehouse/frontend", "/warehouse/people"], &[]),
        64,
        24_000,
    );
    let req = |q: &str, sec: &str| {
        json!({"statement": "The Frontend is a React web application.", "entities": ["ent:frontend"],
        "section": format!("w.md#{}", sec), "quote": q, "facets": [{"facet": "constraint", "reasoning": "its form"}]})
    };
    let err = t
        .dispatch("upsert_requirement", &req("The frontend is a web application built using React. It communicates with the backend over a REST API.", "/warehouse/frontend"))
        .unwrap_err();
    assert_eq!(err.rule, "quote-multi-sentence", "{}", err.message);
    assert!(t
        .dispatch(
            "upsert_requirement",
            &req(
                "The frontend is a web application built using React.",
                "/warehouse/frontend"
            )
        )
        .is_ok());
    assert!(t
        .dispatch(
            "upsert_requirement",
            &req(
                "Dot Feeney. Age 58. Occupation: ferry office manager.",
                "/warehouse/people"
            )
        )
        .is_ok());
    assert!(t
        .dispatch(
            "upsert_requirement",
            &req(
                "1. The employee submits the claim to their manager.",
                "/warehouse/people"
            )
        )
        .is_ok());
}

#[test]
fn displayed_page_copy_uses_one_source_sentence_per_requirement() {
    let mut s = Store::default();
    let first = "A compiler that turns documentation → requirements → acceptance criteria → architecture → design → source code → tests.";
    let second = "Every requirement has a test.";
    let text = format!("# Landing Page\n\n## Hero\n\n> {first} {second}\n");
    s.docs.insert(
        "landing.md".into(),
        DocRecord {
            content_hash: hash_hex(&text),
            sections: crate::md::parse_sections(&text),
            coverage: BTreeMap::new(),
        },
    );
    s.graph
        .entities
        .insert("ent:landing".into(), plain("Landing Page"));
    let mut t = ToolSession::new(
        s,
        reconcile_scope("landing.md", &["/landing-page/hero"], &[]),
        64,
        24_000,
    );
    let req = |quote: &str| {
        json!({
            "statement": format!("The Landing Page hero displays '{quote}'"),
            "entities": ["ent:landing"], "section": "landing.md#/landing-page/hero", "quote": quote,
            "facets": [{"facet":"constraint","reasoning":"the required display text"}],
        })
    };
    assert_eq!(
        t.dispatch("upsert_requirement", &req(&format!("{first} {second}")))
            .unwrap_err()
            .rule,
        "quote-multi-sentence"
    );
    assert!(t.dispatch("upsert_requirement", &req(first)).is_ok());
    assert!(t.dispatch("upsert_requirement", &req(second)).is_ok());
}
