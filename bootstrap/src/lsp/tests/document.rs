use super::*;

#[test]
fn section_findings_anchor_on_heading_or_first_sentence() {
    assert_eq!(
        section_lead("## Payments\n\nA payment is captured.\n").as_deref(),
        Some("## Payments")
    );
    assert_eq!(
        section_lead("\nThe shop sells books online. A customer fills a cart.\n").as_deref(),
        Some("The shop sells books online.")
    );
    assert_eq!(
        section_lead("Orders ship\nin one day. Then more.\n\nNext.").as_deref(),
        Some("Orders ship in one day.")
    );
    assert_eq!(
        section_lead("A list of things\n").as_deref(),
        Some("A list of things")
    );
    assert_eq!(
        section_lead("```plantuml\n@startuml\n```").as_deref(),
        Some("```plantuml")
    );
    let text = "# Shop\n\nIntro.\n\n## Payments\n\nA payment is captured.\n";
    let sec: crate::model::Section = serde_json::from_value(serde_json::json!({
        "title": "Payments", "kind": "heading", "order": 0,
        "raw": "## Payments\n\nA payment is captured.\n", "hash": "x"
    }))
    .unwrap();
    assert_eq!(section_anchor(text, &sec), Some((4, 0, 4, 11)));
}

#[test]
fn occurrences_report_columns_on_the_original_text() {
    // Columns are UTF-16 units: an emoji counts two.
    assert_eq!(super::occurrences("😀 order", "order"), vec![(0, 3, 5)]);
    assert_eq!(
        super::occurrences("a\n𝔸 Order 😀x", "order"),
        vec![(1, 3, 5)]
    );
    let hits = super::occurrences("İstanbul order Order", "order");
    assert_eq!(hits, vec![(0, 9, 5), (0, 15, 5)]);
    let hits = super::occurrences("orders order", "order");
    assert_eq!(hits, vec![(0, 7, 5)]);
    assert!(super::occurrences("İİİ", "x").is_empty());
}

#[cfg(windows)]
#[test]
fn file_urls_find_documents_under_a_verbatim_windows_root() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let lsp = lsp_at(root.clone(), root.join(".jazyk"), Store::default());
    let uri = super::path_to_uri(&root.join("docs/main.md"));
    assert_eq!(lsp.uri_to_doc(&uri), Some("docs/main.md".into()));
}

#[test]
fn file_urls_round_trip_native_paths_with_unicode_and_delimiters() {
    for name in ["a b.md", "%aé.md", "café.md", "%.md", "%2.md", "a#b.md"] {
        let path = std::env::temp_dir().join(name);
        let uri = super::path_to_uri(&path);
        assert_eq!(super::uri_to_path(&uri), Some(path), "{uri}");
    }
    assert!(super::uri_to_path("https://example.org/a.md").is_none());
}

#[cfg(all(feature = "native-documents", not(target_arch = "wasm32")))]
#[test]
fn docx_warning_publishes_against_the_native_projection() {
    let root = std::env::temp_dir().join(format!("jazyk-lsp-docx-{}", crate::sys::process_id()));
    crate::vfs::create_dir_all(&root).unwrap();
    crate::vfs::write(
        root.join("contract.docx"),
        include_bytes!("../../../tests/fixtures/document-proof/contract.docx"),
    )
    .unwrap();
    let project = crate::project::Project::load(&root);
    let mut lsp = Lsp::new(
        root.clone(),
        root.join(".jazyk"),
        GenSettings::resolve(&project),
    );
    let mut bytes = Vec::new();
    lsp.publish(&mut bytes, "contract.docx");
    let msg = read_message(&mut std::io::BufReader::new(bytes.as_slice())).unwrap();
    assert_eq!(msg["method"], "textDocument/publishDiagnostics");
    let warning = msg["params"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "broken-link")
        .unwrap();
    let text = lsp.doc_text("contract.docx");
    let line = text
        .lines()
        .nth(warning["range"]["start"]["line"].as_u64().unwrap() as usize)
        .unwrap();
    assert_eq!(line, "Reference: [contract](missing.md)");
    assert_eq!(warning["range"]["start"]["character"], 11);
    let mut response = Vec::new();
    lsp.handle(json!({"jsonrpc":"2.0", "id":1, "method":"jazyk/document", "params":{"uri":lsp.doc_to_uri("contract.docx")}}), &mut response);
    let msg = read_message(&mut std::io::BufReader::new(response.as_slice())).unwrap();
    assert_eq!(msg["result"]["format"], "docx");
    assert_eq!(msg["result"]["projection"]["text"], text);
    assert!(!msg["result"]["diagnostics"][0]["locations"]
        .as_array()
        .unwrap()
        .is_empty());
    crate::vfs::remove_dir_all(&root).unwrap();
}

// The interactive path over the wire: a prompted finding publishes with its
// question, its options come back as code actions, and one executeCommand
// applies the edit and resolves it. Mirrors docs/frontends/lsp.md#capabilities.
#[test]
fn prompted_diagnostic_publishes_offers_actions_and_applies() {
    use crate::model::{Diagnostic, DiagnosticPrompt, PromptOption, SuggestedEdit};
    use crate::store::Op;
    let tmp = std::env::temp_dir().join(format!("jazyk-lsp-answer-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&tmp).ok();
    let root = tmp.join("proj");
    crate::vfs::create_dir_all(root.join("docs")).unwrap();
    crate::vfs::write(
        { let path = root.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
        "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
    )
    .unwrap();
    crate::vfs::write(
        root.join("docs/pay.md"),
        "# Pay\n\nAn Order shall be paid within 30 days.\n",
    )
    .unwrap();
    let project = crate::project::Project::load(&root);
    let out = project.out.clone();
    let (parsed, _) = crate::reconcile::parse_all(&project);
    let mut s = Store::load(&out);
    s.sync_docs(&parsed);
    let sec = s.docs["docs/pay.md"]
        .sections
        .keys()
        .next()
        .unwrap()
        .clone();
    s.apply(
        vec![Op::ReportDiagnostic {
            id: String::new(),
            diagnostic: Diagnostic {
                goal: None,
                rule: "contradiction".into(),
                severity: "warning".into(),
                subjects: vec![format!("docs/pay.md#{}", sec)],
                message: "21 vs 30 days".into(),
                reasoning: None,
                lifecycle: "open".into(),
                triage: None,
                prompt: Some(DiagnosticPrompt {
                    question: "Which bound holds?".into(),
                    options: vec![
                        PromptOption {
                            label: "21 days; fix this file".into(),
                            edit: Some(SuggestedEdit {
                                doc: "docs/pay.md".into(),
                                section: sec.clone(),
                                old_text: "within 30 days".into(),
                                new_text: "within 21 days".into(),
                                facts: Vec::new(),
                            }),
                            answer: None,
                        },
                        PromptOption {
                            label: "30 days is right".into(),
                            edit: None,
                            answer: Some("keep 30".into()),
                        },
                    ],
                    freeform: true,
                }),
                answer: None,
                created: None,
                updated: None,
            },
        }],
        &crate::model::WorkItem {
            task: "seed".into(),
            target: "t".into(),
            dirty_sections: vec![],
            stale_anchors: vec![],
            proposals: Vec::new(),
        }
        .commit(0, None),
    );
    let id = s.graph.diagnostics.keys().next().unwrap().clone();
    drop(s);

    let mut lsp = Lsp::new(
        root.clone(),
        out.clone(),
        crate::gen::GenSettings::resolve(&project),
    );
    let uri = path_to_uri(&root.join("docs/pay.md"));
    let mut wire: Vec<u8> = Vec::new();
    lsp.handle(
        json!({"method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri,
            "text": crate::vfs::read_to_string(root.join("docs/pay.md")).unwrap()}}}),
        &mut wire,
    );
    let published = String::from_utf8_lossy(&wire).to_string();
    assert!(
        published.contains("Which bound holds?"),
        "question published inline: {}",
        published
    );

    let mut wire2: Vec<u8> = Vec::new();
    lsp.handle(
        json!({"id": 7, "method": "textDocument/codeAction", "params": {
            "textDocument": {"uri": uri},
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 99, "character": 0}}}}),
        &mut wire2,
    );
    let actions = String::from_utf8_lossy(&wire2).to_string();
    assert!(
        actions.contains("Apply: 21 days; fix this file"),
        "{}",
        actions
    );
    assert!(actions.contains("Answer: 30 days is right"), "{}", actions);

    let mut wire3: Vec<u8> = Vec::new();
    lsp.handle(
        json!({"id": 8, "method": "workspace/executeCommand", "params": {
            "command": "jazyk.answerDiagnostic", "arguments": [{"id": id, "option": 0}]}}),
        &mut wire3,
    );
    let applied = String::from_utf8_lossy(&wire3).to_string();
    assert!(applied.contains("applied"), "{}", applied);
    let text = crate::vfs::read_to_string(root.join("docs/pay.md")).unwrap();
    assert!(text.contains("within 21 days"), "{}", text);
    // The refresh inside the command republished without the resolved finding.
    assert!(applied.contains("publishDiagnostics"), "{}", applied);
    assert!(!applied.contains("Which bound holds?"), "{}", applied);
    crate::vfs::remove_dir_all(&tmp).ok();
}

#[test]
fn criterion_locations_publish_own_and_related_requirement_ranges_once() {
    let mut store = Store::default();
    store.graph.requirements.insert("req:one".into(), serde_json::from_value(json!({"statement":"Rule", "entities":[], "source":{"doc":"rules.md","section":"/rules","quote":"Orders complete."}})).unwrap());
    store.graph.criteria.insert("crit:one".into(), serde_json::from_value(json!({"requirement":"req:one", "statement":"Example", "source":{"doc":"examples.md","section":"/examples","quote":"Returns 200."}})).unwrap());
    store.graph.diagnostics.insert("diag:one".into(), serde_json::from_value(json!({"rule":"contradiction","severity":"error","subjects":["crit:one","req:one"],"message":"Conflict"})).unwrap());
    let mut lsp = lsp_at(
        std::env::temp_dir(),
        std::env::temp_dir().join("diagnostic-lsp"),
        store,
    );
    lsp.overlay
        .insert("rules.md".into(), "# Rules\n\nOrders complete.\n".into());
    lsp.overlay
        .insert("examples.md".into(), "# Examples\n\nReturns 200.\n".into());
    lsp.store.graph.diagnostics.get_mut("diag:one").unwrap().prompt = Some(serde_json::from_value(json!({
        "question":"Which is correct?", "options":[{"label":"Explain", "answer":"Explain"}], "freeform":true
    })).unwrap());
    for doc in ["rules.md", "examples.md"] {
        let diagnostics = lsp.document_diagnostics(doc);
        assert_eq!(diagnostics.len(), 1, "{doc}");
        assert_eq!(diagnostics[0]["range"]["start"]["line"], 2);
        assert_eq!(diagnostics[0]["data"]["jazykDiag"], "diag:one");
        let actions = lsp.on_code_action(&json!({"textDocument":{"uri": lsp.doc_to_uri(doc)},
            "range":{"start":{"line":2,"character":0},"end":{"line":2,"character":4}}}));
        assert_eq!(actions.as_array().unwrap().len(), 1, "{doc}");
    }
    lsp.store
        .graph
        .diagnostics
        .get_mut("diag:one")
        .unwrap()
        .triage = Some("suppressed".into());
    assert!(lsp.document_diagnostics("rules.md").is_empty());
}

// A cursor position is in UTF-16 code units, the LSP's default encoding: an
// astral character before a name counts two, so the name under the cursor is the
// one the editor shows there, not the one a character count would find.
// Mirrors docs/frontends/lsp.md#capabilities.
#[test]
fn cursor_positions_count_astral_characters_as_two_units() {
    let tmp = std::env::temp_dir().join(format!("jazyk-lsp-astral-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&tmp).ok();
    let out = tmp.join(".jazyk");
    crate::vfs::create_dir_all(&out).unwrap();
    let mut lsp = lsp_at(tmp.clone(), out.clone(), shop_at(&out));
    let mut wire: Vec<u8> = Vec::new();
    lsp.handle(
        json!({"id": 1, "method": "initialize", "params": {}}),
        &mut wire,
    );
    let uri = path_to_uri(&tmp.join("shop.md"));
    lsp.handle(
        json!({"method": "textDocument/didOpen", "params": {"textDocument": {"uri": uri,
            "text": "\u{1F600}\u{1F600} The Order Service takes.\n\u{1D538} second line Order Service\n"}}}),
        &mut wire,
    );
    let mut id = 10;
    let mut hits = |line: u64, character: u64| -> bool {
        id += 1;
        let mut wire: Vec<u8> = Vec::new();
        lsp.handle(
            json!({"id": id, "method": "textDocument/typeDefinition", "params": {
                "textDocument": {"uri": uri}, "position": {"line": line, "character": character}}}),
            &mut wire,
        );
        let reply = String::from_utf8_lossy(&wire).to_string();
        reply.contains("order-service.md")
    };
    // Line 0: two emoji (four units) and "The " put the name at units 9..22.
    assert!(!hits(0, 8), "a character count put the name at 7");
    assert!(hits(0, 9));
    assert!(hits(0, 21), "the name's last unit");
    assert!(!hits(0, 22));
    // Line 1: one astral letter (two units) puts the name at units 15..28.
    assert!(!hits(1, 14));
    assert!(hits(1, 15));
    assert!(hits(1, 27));
    crate::vfs::remove_dir_all(&tmp).ok();
}
