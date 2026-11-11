use super::*;

const BUILT: &str = "# Shop\n\nThe Cart shall hold items. Orders ship daily.\n\n## Checkout\n\nA Checkout Service takes payment.\n";

// A built shop document: two entities, one requirement with a criterion quoted in
// the same document, an open question naming the requirement, and coverage.
fn editor_fixture(name: &str) -> (std::path::PathBuf, Lsp) {
    use crate::model::{Coverage, Criterion, DocRecord};
    let tmp = std::env::temp_dir().join(format!("jazyk-lsp-{}-{}", name, crate::sys::process_id()));
    crate::vfs::remove_dir_all(&tmp).ok();
    let out = tmp.join(".jazyk");
    crate::vfs::create_dir_all(&out).unwrap();
    crate::vfs::write(tmp.join("shop.md"), BUILT).unwrap();
    let mut store = Store {
        out: out.clone(),
        ..Default::default()
    };
    let mut coverage = std::collections::BTreeMap::new();
    coverage.insert(
        "/shop".to_string(),
        Coverage {
            state: "covered".into(),
            note: None,
            claimed_by: None,
        },
    );
    coverage.insert(
        "/shop/checkout".to_string(),
        Coverage {
            state: "non-normative".into(),
            note: None,
            claimed_by: None,
        },
    );
    store.docs.insert(
        "shop.md".into(),
        DocRecord {
            content_hash: crate::model::hash_hex(BUILT),
            sections: md::parse_sections(BUILT),
            coverage,
        },
    );
    let mention = |quote: &str, section: &str| SourceRef {
        doc: "shop.md".into(),
        section: section.into(),
        quote: quote.into(),
    };
    store.graph.entities.insert(
        "ent:cart".into(),
        Entity {
            name: "Cart".into(),
            mentions: vec![mention("The Cart shall hold items.", "/shop")],
            ..Default::default()
        },
    );
    store.graph.entities.insert(
        "ent:checkout-service".into(),
        Entity {
            name: "Checkout Service".into(),
            aliases: vec!["checkout".into()],
            mentions: vec![mention(
                "A Checkout Service takes payment.",
                "/shop/checkout",
            )],
            ..Default::default()
        },
    );
    store.graph.requirements.insert(
        "req:hold".into(),
        Requirement {
            statement: "The Cart shall hold items.".into(),
            entities: vec!["ent:cart".into()],
            source: Some(mention("The Cart shall hold items.", "/shop")),
            ..Default::default()
        },
    );
    store.graph.criteria.insert(
        "crit:hold-1".into(),
        Criterion {
            requirement: "req:hold".into(),
            statement: "Orders ship daily.".into(),
            source: Some(mention("Orders ship daily.", "/shop")),
            ..Default::default()
        },
    );
    let lsp = lsp_at(tmp.clone(), out, store);
    (tmp, lsp)
}

fn open(lsp: &mut Lsp, text: &str) -> String {
    let uri = lsp.doc_to_uri("shop.md");
    let mut sink: Vec<u8> = Vec::new();
    lsp.handle(
        json!({"method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri,
            "languageId": "markdown", "version": 1, "text": text}}}),
        &mut sink,
    );
    uri
}

fn ask(lsp: &mut Store, rid: &str) {
    lsp.graph.diagnostics.insert(
        "diag:ask".into(),
        crate::model::Diagnostic {
            goal: None,
            rule: "ambiguity".into(),
            severity: "warning".into(),
            subjects: vec![rid.into()],
            message: "which cart?".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: Some(crate::model::DiagnosticPrompt {
                question: "Which cart?".into(),
                options: vec![],
                freeform: true,
            }),
            answer: None,
            created: None,
            updated: None,
        },
    );
}

// The initialize result declares every new capability and the legend.
// Mirrors docs/frontends/lsp.md#capabilities.
#[test]
fn initialize_declares_the_editor_capabilities() {
    let (tmp, lsp) = editor_fixture("caps");
    let caps = &lsp.capabilities()["capabilities"];
    assert_eq!(caps["inlayHintProvider"]["resolveProvider"], false);
    assert_eq!(caps["documentHighlightProvider"], true);
    assert_eq!(caps["documentSymbolProvider"], true);
    assert_eq!(caps["workspaceSymbolProvider"], true);
    assert_eq!(caps["semanticTokensProvider"]["full"], true);
    assert_eq!(
        caps["semanticTokensProvider"]["legend"],
        json!({"tokenTypes": ["type"], "tokenModifiers": ["jazykEntity"]})
    );
    let commands = caps["executeCommandProvider"]["commands"].to_string();
    assert!(commands.contains("jazyk.showLastBuild"), "{}", commands);
    crate::vfs::remove_dir_all(&tmp).ok();
}

// The status word sits at the end of each located quote, inside the requested
// range only; the code lens carries the same word.
// Mirrors docs/frontends/lsp.md#capabilities.
#[test]
fn inlay_hints_and_lenses_read_the_status_word() {
    let (tmp, mut lsp) = editor_fixture("hints");
    let uri = open(&mut lsp, BUILT);
    let all = json!({"textDocument": {"uri": uri},
        "range": {"start": {"line": 0, "character": 0}, "end": {"line": 9, "character": 0}}});
    // Nothing was ever generated: no `not built` hint on every sentence.
    assert_eq!(lsp.on_inlay_hints(&all), json!([]));
    // An open question outranks every other word, and shows even then.
    ask(&mut lsp.store, "req:hold");
    assert_eq!(
        lsp.on_inlay_hints(&all),
        json!([{
            "position": {"line": 2, "character": 26},
            "label": "· question",
            "paddingLeft": true,
            "tooltip": "req:hold: missing (not-generated)",
        }])
    );
    let outside = json!({"textDocument": {"uri": uri},
        "range": {"start": {"line": 4, "character": 0}, "end": {"line": 9, "character": 0}}});
    assert_eq!(lsp.on_inlay_hints(&outside), json!([]));
    // A suppressed question no longer counts.
    lsp.store
        .graph
        .diagnostics
        .get_mut("diag:ask")
        .unwrap()
        .triage = Some("suppressed".into());
    assert_eq!(lsp.on_inlay_hints(&all), json!([]));
    lsp.store
        .graph
        .diagnostics
        .get_mut("diag:ask")
        .unwrap()
        .triage = None;
    let lenses = lsp.on_code_lens(&json!({"textDocument": {"uri": uri}}));
    assert_eq!(lenses[0]["command"]["title"], "req:hold · question");
    // A quote that no longer locates shows no hint.
    let uri = open(&mut lsp, "# Shop\n\nThe basket holds items.\n");
    let all = json!({"textDocument": {"uri": uri}});
    assert_eq!(lsp.on_inlay_hints(&all), json!([]));
    crate::vfs::remove_dir_all(&tmp).ok();
}

// Every entity mention is one token, longest name first, encoded relative.
// Mirrors docs/frontends/lsp.md#capabilities.
#[test]
fn semantic_tokens_mark_every_entity_mention() {
    let (tmp, mut lsp) = editor_fixture("tokens");
    let uri = open(&mut lsp, BUILT);
    let tokens = lsp.on_semantic_tokens(&json!({"textDocument": {"uri": uri}}));
    // `Cart` at 2:4, `Checkout` (the heading, an alias) at 4:3, `Checkout
    // Service` at 6:2 wins over the alias inside it.
    assert_eq!(
        tokens["data"],
        json!([2, 4, 4, 0, 1, 2, 3, 8, 0, 1, 2, 2, 16, 0, 1])
    );
    crate::vfs::remove_dir_all(&tmp).ok();
}

// Inside the requirement's quote: the quote and its criterion's quote.
// Mirrors docs/frontends/lsp.md#capabilities.
#[test]
fn document_highlight_covers_the_requirement_and_its_criteria() {
    let (tmp, mut lsp) = editor_fixture("highlight");
    let uri = open(&mut lsp, BUILT);
    let at = |line: usize, character: usize| json!({"textDocument": {"uri": uri}, "position": {"line": line, "character": character}});
    assert_eq!(
        lsp.on_document_highlight(&at(2, 10)),
        json!([
            {"range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 26}}, "kind": 1},
            {"range": {"start": {"line": 2, "character": 27}, "end": {"line": 2, "character": 45}}, "kind": 1},
        ])
    );
    assert_eq!(lsp.on_document_highlight(&at(6, 3)), json!([]));
    crate::vfs::remove_dir_all(&tmp).ok();
}

// The stored tree as nested symbols with counts and read state, positioned in
// the current text. Mirrors docs/frontends/lsp.md#capabilities.
#[test]
fn document_symbols_nest_the_stored_sections() {
    let (tmp, mut lsp) = editor_fixture("symbols");
    // A line inserted above moves both sections down by two.
    let uri = open(&mut lsp, &format!("Intro.\n\n{}", BUILT));
    let symbols = lsp.on_document_symbols(&json!({"textDocument": {"uri": uri}}));
    assert_eq!(symbols.as_array().unwrap().len(), 1);
    let shop = &symbols[0];
    assert_eq!(shop["name"], "Shop");
    assert_eq!(shop["detail"], "1 requirement · read");
    assert_eq!(shop["selectionRange"]["start"]["line"], 2);
    assert_eq!(shop["range"]["end"]["line"], 8);
    let checkout = &shop["children"][0];
    assert_eq!(checkout["name"], "Checkout");
    assert_eq!(checkout["detail"], "0 requirements · not normative");
    assert_eq!(checkout["range"]["start"]["line"], 6);
    assert_eq!(
        checkout["selectionRange"]["end"],
        json!({"line": 6, "character": 11})
    );
    crate::vfs::remove_dir_all(&tmp).ok();
}

// Entities by name or alias and requirements by statement, each located.
// Mirrors docs/frontends/lsp.md#capabilities.
#[test]
fn workspace_symbols_find_entities_and_requirements() {
    let (tmp, lsp) = editor_fixture("wsym");
    let found = lsp.on_workspace_symbols(&json!({"query": "CHECKOUT"}));
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert_eq!(found[0]["name"], "Checkout Service");
    assert_eq!(found[0]["kind"], 5);
    assert_eq!(found[0]["location"]["range"]["start"]["line"], 6);
    let found = lsp.on_workspace_symbols(&json!({"query": "hold items"}));
    assert_eq!(found.as_array().unwrap().len(), 1);
    assert_eq!(found[0]["containerName"], "req:hold");
    assert_eq!(found[0]["location"]["range"]["start"]["line"], 2);
    assert_eq!(
        lsp.on_workspace_symbols(&json!({"query": ""}))
            .as_array()
            .unwrap()
            .len(),
        3
    );
    crate::vfs::remove_dir_all(&tmp).ok();
}

// An edited section lenses on its heading; the command writes the last-built
// text as a projection and asks the client to show it.
// Mirrors docs/frontends/lsp.md#capabilities and #commands.
#[test]
fn changed_sections_lens_and_show_the_last_build() {
    let (tmp, mut lsp) = editor_fixture("changed");
    let uri = open(&mut lsp, BUILT);
    let changed = |lsp: &Lsp| {
        lsp.on_code_lens(&json!({"textDocument": {"uri": uri}}))
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| l["command"]["command"] == "jazyk.showLastBuild")
            .map(|l| l["range"]["start"]["line"].as_u64().unwrap())
            .collect::<Vec<_>>()
    };
    assert!(changed(&lsp).is_empty());
    open(&mut lsp, &BUILT.replace("takes payment", "takes cards"));
    assert_eq!(changed(&lsp), [4]);
    let mut sink: Vec<u8> = Vec::new();
    lsp.handle(
        json!({"id": 9, "method": "workspace/executeCommand", "params": {
            "command": "jazyk.showLastBuild", "arguments": [{"uri": uri}]}}),
        &mut sink,
    );
    let projection = tmp.join(".jazyk/last-build/shop.md");
    assert_eq!(crate::vfs::read_to_string(&projection).unwrap(), BUILT);
    let wire = String::from_utf8(sink).unwrap();
    assert!(
        wire.contains("\"method\":\"window/showDocument\""),
        "{}",
        wire
    );
    assert!(wire.contains("last-build/shop.md"), "{}", wire);
    crate::vfs::remove_dir_all(&tmp).ok();
}
