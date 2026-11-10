use super::*;

// Every staging gate names its rule and the repair.
// Mirrors docs/compiler/graph.md#validation-gates.
#[test]
fn empty_optional_arguments_count_as_absent() {
    // gpt-class models fill every schema field; "" / [] / {} must read as
    // omitted (docs/compiler/tools.md#validation-and-errors). This is the
    // exact call shape a gpt-5.5 run staged, bounced by bad-provenance.
    let mut t = session();
    let v = t
        .dispatch(
            "upsert_entity",
            &json!({
                "name": "Gadget", "definition": "A gadget.", "aliases": [],
                "scope": "", "stereotype": "device", "parent": "",
                "attributes": [],
                "mention": {"section": "/shop/cart", "quote": "A Gadget and a Line sit beside"},
                "provenance": {"derived": {"from": [], "reasoning": ""}},
                "note": ""
            }),
        )
        .unwrap();
    assert_eq!(v["created"], true);
    let v = t
        .dispatch(
            "upsert_requirement",
            &json!({
                "statement": "The gadget holds items.",
                "entities": ["ent:gadget"],
                "section": "/shop/cart", "quote": "holds items",
                "edges": [], "facets": [],
                "provenance": {"derived": {"from": [], "reasoning": ""}}
            }),
        )
        .unwrap_err();
    // An empty list is absent, and an absent facet is refused: the two doctrines
    // meet here (docs/compiler/model/requirement.md#facets).
    assert_eq!(v.rule, "facets-missing");
    let v = t
        .dispatch(
            "upsert_requirement",
            &json!({
                "statement": "The gadget holds items.",
                "entities": ["ent:gadget"],
                "section": "/shop/cart", "quote": "holds items",
                "edges": [],
                "facets": [{"facet": "constraint", "reasoning": "an invariant the gadget holds"}],
                "provenance": {"derived": {"from": [], "reasoning": ""}}
            }),
        )
        .unwrap();
    assert_eq!(v["created"], true);
    // The doctrine covers structured optionals too: an all-empty transition
    // object and all-empty items inside lists are filled-in blanks, not data.
    // This is the exact shape a gpt-5.5 run staged, bounced by unknown-id on
    // the empty transition subject.
    let v = t
        .dispatch(
            "upsert_requirement",
            &json!({
                "statement": "A customer intends to buy the items.",
                "entities": ["ent:customer", "ent:gadget"],
                "section": "/shop/cart", "quote": "intends to buy",
                "edges": [{}],
                "facets": [{}, {"facet": "behavior", "reasoning": "the customer acts"}],
                "transition": {"subject": "", "from": "", "to": "", "trigger": "", "guard": ""}
            }),
        )
        .unwrap();
    assert_eq!(v["created"], true);
    let spun = t
        .staged
        .iter()
        .find_map(|op| match op {
            Op::CreateRequirement { requirement, .. }
                if requirement.statement == "A customer intends to buy the items." =>
            {
                Some(requirement)
            }
            _ => None,
        })
        .expect("the requirement staged");
    assert!(spun.transition.is_none());
    assert!(spun.edges.is_empty());
    // The blank facet row dropped; the real one stands, as the mandatory facet
    // requires (docs/compiler/model/requirement.md#facets).
    assert_eq!(spun.facets.len(), 1);
    assert_eq!(spun.facets[0].facet, "behavior");
    // Hollow items inside otherwise-real arguments drop the same way: ""
    // aliases, blank attribute rows, a hollow per-attribute provenance
    // (falls back to the call's quote), "" entity ids, and empty edge
    // type/cardinality strings (an untyped edge, no cardinality).
    let v = t
        .dispatch(
            "upsert_entity",
            &json!({
                "name": "Sprocket", "definition": "A sprocket.",
                "aliases": [""],
                "attributes": [{}, {"name": "teeth", "type": "int",
                                    "provenance": {"section": "", "quote": ""}}],
                "mention": {"section": "/shop/cart", "quote": "beside a Sprocket"}
            }),
        )
        .unwrap();
    assert_eq!(v["created"], true);
    let sprocket = t
        .staged_entities
        .get("ent:sprocket")
        .expect("staged entity");
    assert!(sprocket.aliases.is_empty());
    assert_eq!(sprocket.attributes.len(), 1);
    let v = t
        .dispatch(
            "upsert_requirement",
            &json!({
                "facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}],
                "statement": "The sprocket gadget pair holds.",
                "entities": ["ent:sprocket", "ent:gadget", ""],
                "section": "/shop/cart", "quote": "a Customer intends",
                "edges": [{"a": "ent:sprocket", "b": "ent:gadget",
                           "type": "", "cardinality": ""}]
            }),
        )
        .unwrap();
    assert_eq!(v["created"], true);
    let pair = t
        .staged
        .iter()
        .find_map(|op| match op {
            Op::CreateRequirement { requirement, .. }
                if requirement.statement == "The sprocket gadget pair holds." =>
            {
                Some(requirement)
            }
            _ => None,
        })
        .expect("the pair requirement staged");
    assert_eq!(pair.entities.len(), 2);
    assert_eq!(pair.edges.len(), 1);
    // A hollow prompt object on a diagnostic was filled in, not asked.
    t.dispatch(
        "report_diagnostic",
        &json!({"rule": "lint", "severity": "warning",
                "subjects": ["ent:customer"],
                "message": "Sprocket is undefined in the glossary.",
                "prompt": {"question": "", "options": [], "freeform": false}}),
    )
    .unwrap();
    // All-empty on both sides still reads as no provenance at all.
    let err = t
        .dispatch(
            "upsert_entity",
            &json!({"name": "Widget", "mention": {},
                    "provenance": {"derived": {"from": [], "reasoning": ""}}}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "provenance-required");
}

// A fact sent with its quote and a derivation keeps the quote and says so; a
// transition naming no state is absent; an exclusive pair names the case it met;
// filler variants of one call are one call to the repeat guard.
// Mirrors docs/compiler/tools.md#validation-and-errors.
#[test]
fn a_quote_wins_over_a_derivation_and_refusals_name_their_case() {
    let mut t = session();
    let v = t
        .dispatch(
            "upsert_entity",
            &json!({
                "name": "Gadget", "definition": "A gadget.",
                "mention": {"section": "/shop/cart", "quote": "A Gadget and a Line sit beside"},
                "provenance": {"derived": {"from": ["ent:line"], "reasoning": "the cart names it"}}
            }),
        )
        .unwrap();
    assert_eq!(v["created"], true);
    assert!(
        v["note"]
            .as_str()
            .unwrap_or_default()
            .contains("provenance dropped"),
        "{}",
        v
    );
    let v = t
        .dispatch(
            "upsert_requirement",
            &json!({
                "statement": "The gadget holds items.",
                "entities": ["ent:gadget"],
                "section": "/shop/cart", "quote": "holds items",
                "facets": [{"facet": "constraint", "reasoning": "an invariant the gadget holds"}],
                "transition": {"subject": "ent:gadget", "from": "", "to": ""},
                "provenance": {"derived": {"from": ["ent:gadget"], "reasoning": "a reading"}}
            }),
        )
        .unwrap();
    assert_eq!(v["created"], true, "{}", v);
    assert!(
        v["note"]
            .as_str()
            .unwrap_or_default()
            .contains("section and quote"),
        "{}",
        v
    );
    let both = normalize_args(&json!({"prompt": {"question": "Which?", "options": [
        {"label": "A", "answer": "a", "edit": {"doc": "d.md", "section": "/s", "old_text": "x", "new_text": "y"}}]}}));
    let err = t.parse_prompt(&both["prompt"]).unwrap_err();
    assert_eq!(err.rule, "bad-prompt");
    assert!(
        err.message.contains("both edit and answer"),
        "{}",
        err.message
    );
    let neither = normalize_args(
        &json!({"prompt": {"question": "Which?", "options": [{"label": "A", "answer": ""}]}}),
    );
    let err = t.parse_prompt(&neither["prompt"]).unwrap_err();
    assert!(
        err.message.contains("neither edit nor answer"),
        "{}",
        err.message
    );
    // A top-level null survives; filler inside goes.
    assert_eq!(
        normalize_args(&json!({"prompt": null, "scope": "", "tags": ["", " "]})),
        json!({"prompt": null})
    );
    // Filler variants of one search are the same call to the repeat guard.
    for filler in [
        json!({"query": "gadget"}),
        json!({"query": "gadget", "queries": []}),
        json!({"query": "gadget", "queries": [""]}),
    ] {
        let _ = t.dispatch("search", &filler);
    }
    let err = t
        .dispatch("search", &json!({"query": "gadget", "entities": []}))
        .unwrap_err();
    assert_eq!(err.rule, "repeated-call");
}

// A criterion sent with its section and quote and a derivation keeps the quote:
// the derivation goes, the note says so, and the reply carries it.
// Mirrors docs/compiler/tools.md#validation-and-errors.
#[test]
fn prefer_quote_drops_derivation_on_upsert_criterion() {
    let mut args = json!({
        "requirement": "req:shop-1", "statement": "An empty Cart shows no items.", "kind": "boundary",
        "section": "/shop", "quote": "intro text",
        "provenance": {"derived": {"from": ["req:shop-1"], "reasoning": "the lower edge"}}
    });
    let note = prefer_quote("upsert_criterion", &mut args).unwrap();
    assert!(note.contains("section and quote"), "{}", note);
    assert!(args.get("provenance").is_none(), "{}", args);
    assert_eq!(args["quote"], "intro text");
    // Through dispatch the note rides on the reply and the criterion is quoted.
    let mut t = session();
    let rid = t
        .dispatch(
            "upsert_requirement",
            &json!({
                "facets": [{"facet": "constraint", "reasoning": "a stated fact the fixture carries"}],
                "statement": "The Shopping Cart holds the items a Customer intends to buy.",
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
                    "kind": "positive", "section": "/shop", "quote": "intro text",
                    "provenance": {"derived": {"from": [rid], "reasoning": "a reading"}}}),
        )
        .unwrap();
    assert_eq!(v["created"], true, "{}", v);
    assert!(
        v["note"]
            .as_str()
            .unwrap_or_default()
            .contains("provenance dropped"),
        "{}",
        v
    );
}

// The quote wins only when it is a whole quote: a requirement or criterion needs
// both `section` and `quote`, an entity needs its `mention`, and a call that
// carries no derivation or is no upsert has nothing to drop.
#[test]
fn prefer_quote_keeps_derivation_when_quote_lacks_section() {
    let derived = json!({"derived": {"from": ["ent:gadget"], "reasoning": "a reading"}});
    let mut args = json!({"statement": "The gadget holds items.", "quote": "holds items", "provenance": derived});
    assert_eq!(prefer_quote("upsert_requirement", &mut args), None);
    assert_eq!(args["provenance"], derived);
    let mut args = json!({"statement": "The gadget holds items.", "section": "/shop/cart", "provenance": derived});
    assert_eq!(prefer_quote("upsert_criterion", &mut args), None);
    assert_eq!(args["provenance"], derived);
    let mut args = json!({"name": "Gadget", "provenance": derived});
    assert_eq!(prefer_quote("upsert_entity", &mut args), None);
    assert_eq!(args["provenance"], derived);
    let mut args =
        json!({"name": "Gadget", "mention": {"section": "/shop/cart", "quote": "A Gadget"}});
    assert_eq!(prefer_quote("upsert_entity", &mut args), None);
    let mut args = json!({"id": "ent:gadget", "mention": {"section": "/shop/cart", "quote": "A Gadget"}, "provenance": derived});
    assert_eq!(prefer_quote("update_entity", &mut args), None);
    assert_eq!(args["provenance"], derived);
}

// Numbers and booleans always say something: `false` and `0` survive
// normalization where an empty string or list goes.
#[test]
fn hollow_keeps_false_and_zero() {
    assert_eq!(hollow(&json!(false)), Some(json!(false)));
    assert_eq!(hollow(&json!(0)), Some(json!(0)));
    assert_eq!(hollow(&json!(0.0)), Some(json!(0.0)));
    assert_eq!(hollow(&json!("")), None);
    assert_eq!(hollow(&json!("  ")), None);
    assert_eq!(hollow(&json!([])), None);
    assert_eq!(hollow(&json!({})), None);
    assert_eq!(hollow(&json!(null)), None);
    assert_eq!(
        hollow(&json!({"a": false, "b": 0, "c": "", "d": [false, ""], "e": {"f": ""}})),
        Some(json!({"a": false, "b": 0, "d": [false]}))
    );
    assert_eq!(
        normalize_args(&json!({"default": false, "count": 0, "note": "", "flags": [0, ""]})),
        json!({"default": false, "count": 0, "flags": [0]})
    );
}

// A transition naming one state is a transition: only one with neither `from`
// nor `to` is a filled-in blank.
#[test]
fn transition_with_only_from_survives() {
    let v =
        normalize_args(&json!({"transition": {"subject": "ent:order", "from": "open", "to": ""}}));
    assert_eq!(
        v["transition"],
        json!({"subject": "ent:order", "from": "open"})
    );
    let v =
        normalize_args(&json!({"transition": {"subject": "ent:order", "from": "", "to": "paid"}}));
    assert_eq!(
        v["transition"],
        json!({"subject": "ent:order", "to": "paid"})
    );
    let v = normalize_args(&json!({"transition": {"subject": "ent:order", "from": "", "to": " "}}));
    assert!(v.get("transition").is_none(), "{}", v);
    let v = normalize_args(&json!({"transition": {"subject": "", "from": "", "to": ""}}));
    assert!(v.get("transition").is_none(), "{}", v);
}

// Normalization reads objects; anything else passes through untouched.
#[test]
fn normalize_args_passes_non_object_through() {
    assert_eq!(normalize_args(&json!("x")), json!("x"));
    assert_eq!(normalize_args(&json!("")), json!(""));
    assert_eq!(normalize_args(&json!(null)), json!(null));
    assert_eq!(normalize_args(&json!(7)), json!(7));
    assert_eq!(normalize_args(&json!(["", "a"])), json!(["", "a"]));
    assert_eq!(normalize_args(&json!({})), json!({}));
}
