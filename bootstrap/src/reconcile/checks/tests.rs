use super::*;
use crate::model::{hash_hex, Requirement, SourceRef};
use crate::project::Project;
use crate::reconcile::test_support::*;
use crate::store::Store;

#[test]
fn pinned_literals_picks_values_and_skips_words() {
    let lits = pinned_literals(
        "The gateway shall log to `/var/log/gw.log` using model `us.claude-4` while the `username` field stays unique and `--verbose` widens it.",
    );
    assert!(lits.contains(&"/var/log/gw.log".to_string()), "{:?}", lits);
    assert!(lits.contains(&"us.claude-4".to_string()), "{:?}", lits);
    assert!(lits.contains(&"--verbose".to_string()), "{:?}", lits);
    assert!(!lits.iter().any(|l| l == "username"), "{:?}", lits);
}

#[test]
fn drift_check_flags_missing_literals_with_a_question() {
    use crate::gen::{Ledger, ReqRow, RowHashes, TestRef};
    let dir = std::env::temp_dir().join(format!("jazyk-drift-test-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&dir).ok();
    crate::vfs::create_dir_all(dir.join("product")).unwrap();
    crate::vfs::write(
        { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
        "version = 1\ngen = {}\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = [\"product/**\"]\n",
    )
    .unwrap();
    crate::vfs::write(
        dir.join("product/gw.rs"),
        "fn log() { /* writes to /tmp/other.log */ }\n",
    )
    .unwrap();
    let proj = Project::load(&dir);
    let mut store = Store {
        out: proj.out.clone(),
        ..Default::default()
    };
    store.graph.requirements.insert(
        "req:gw-1".into(),
        req(
            "The gateway shall log to `/var/log/gw.log`.",
            "ent:gw",
            "docs/gw.md",
            "/gw",
            "logs",
        ),
    );
    store.graph.requirements.insert(
        "req:gw-2".into(),
        req(
            "The gateway shall keep `/tmp/other.log` rotating.",
            "ent:gw",
            "docs/gw.md",
            "/gw",
            "rotates",
        ),
    );
    let row = |_: ()| ReqRow {
        tests: std::collections::BTreeMap::new(),
        entity: "ent:gw".into(),
        files: vec!["product/gw.rs".into()],
        sites: Vec::new(),
        test: TestRef {
            kind: "programmatic".into(),
            label: "unit".into(),
            artifact: "product/gw.rs".into(),
            name: "t".into(),
            run: "true".into(),
            cwd: ".".into(),
        },
        hashes: RowHashes::default(),
        verdict: "none".into(),
        last_run: None,
        exit_code: None,
        evidence: None,
    };
    let mut ledger = Ledger::default();
    ledger.requirements.insert("req:gw-1".into(), row(()));
    ledger.requirements.insert("req:gw-2".into(), row(()));
    crate::vfs::create_dir_all(&proj.out).unwrap();
    ledger.save(&proj.out).unwrap();

    let f = drift_checks(&store, &proj);
    assert_eq!(
        f.len(),
        1,
        "{:?}",
        f.iter().map(|x| &x.1).collect::<Vec<_>>()
    );
    let (rule, subject, sev, msg, prompt) = &f[0];
    assert_eq!(rule, "pinned-fact-drift");
    assert_eq!(*subject, ["req:gw-1"]);
    assert_eq!(sev, "warning");
    assert!(msg.contains("/var/log/gw.log"), "{}", msg);
    let p = prompt.as_ref().expect("the finding carries its question");
    assert_eq!(p.options.len(), 2);
    assert!(p.freeform);
    crate::vfs::remove_dir_all(&dir).ok();
}

// Mirrors docs/compiler/compilation.md#checks: containment counts in the
// reachability walk, so a level under a grouping is as reachable as the grouping
// and a grouping is reachable through a child the roots reach.
#[test]
fn reachability_walks_containment_both_ways() {
    let mut s = Store::default();
    seed_doc(&mut s, "root.md", "# Shop\nThe Shop sells.\n");
    let ent = |name: &str, parent: Option<&str>| Entity {
        name: name.into(),
        parent: parent.map(String::from),
        ..Default::default()
    };
    let mut shop = ent("Shop", Some("ent:platform"));
    shop.mentions.push(SourceRef {
        doc: "root.md".into(),
        section: "/shop".into(),
        quote: "The Shop sells.".into(),
    });
    s.graph.entities.insert("ent:shop".into(), shop);
    s.graph
        .entities
        .insert("ent:platform".into(), ent("Platform", None));
    s.graph
        .entities
        .insert("ent:cache".into(), ent("Cache", Some("ent:platform")));
    s.graph
        .entities
        .insert("ent:island".into(), ent("Island", None));
    let proj = Project {
        roots: vec!["root.md".into()],
        ..Default::default()
    };
    let f = checks(&s, &proj);
    let hits = rules_for(&f, "unreachable-entity");
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert_eq!(hits[0].1, ["ent:island"]);
}

// Eight checks had no test and had never fired in fifteen measured iterations: a
// check nobody has seen fire and nobody tests is a check nobody knows works.
// Mirrors docs/compiler/compilation.md#checks.
// Flip detection over natural keys: an id carrying a collision suffix while a
// tombstone holds the base slug is a key deleted and recreated.
// Mirrors docs/compiler/reconciler.md#flip-detection.
#[test]
fn unstable_extraction_flags_a_recreated_natural_key() {
    let mut s = Store::default();
    seed_doc(&mut s, "shop.md", "# Shop\nThe Cart holds items.\n");
    let named = |n: &str| Entity {
        name: n.to_string(),
        ..Default::default()
    };
    s.graph.entities.insert("ent:cart-2".into(), named("Cart"));
    s.graph
        .entities
        .insert("ent:basket-2".into(), named("Basket"));
    // A tombstone is an empty redirect: the key was deleted, not merged.
    s.graph.redirects.insert("ent:cart".into(), String::new());
    s.graph
        .redirects
        .insert("ent:basket".into(), "ent:trolley".into());
    let f = checks(&s, &Project::default());
    let hits = rules_for(&f, "unstable-extraction");
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert_eq!(hits[0].1, ["ent:cart-2"]);
    assert!(hits[0].3.contains("ent:cart"), "{}", hits[0].3);
}

// `incomplete-build` names the goals a budget parked; a pair parked by flip
// detection carries its unstable-derivation finding instead.
// Mirrors docs/compiler/compilation.md#checks.
#[test]
fn incomplete_build_skips_flip_parked_goals() {
    let mut s = Store::default();
    let goal = |id: &str, via: &str| Goal {
        id: id.into(),
        kind: "review-entity".into(),
        class: "compile".into(),
        mandatory: true,
        target: id.trim_start_matches("g:review-entity:").into(),
        cause: Some(Cause {
            generation: 4,
            mutation: 0,
            via: via.into(),
        }),
        ..Default::default()
    };
    s.status.parked.push(ParkedGoal::new(
        goal("g:review-entity:ent:cart", "entities"),
        "the build cap of 64 sessions ran out",
    ));
    s.status.parked.push(ParkedGoal::new(
        goal("g:review-entity:ent:order", "flip-detection"),
        "cross-class flip: `order`",
    ));
    let f = checks(&s, &Project::default());
    let hits = rules_for(&f, "incomplete-build");
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert_eq!(hits[0].1, ["ent:cart"]);
}

// A decompiled draft nobody has edited since the machine wrote it is unratified;
// any edit clears it (docs/consumers/decompile.md#ratification).
#[test]
fn unratified_flags_an_untouched_decompiled_draft_and_clears_on_edit() {
    let dir = journal_dir();
    let mut s = Store {
        out: dir.clone(),
        ..Default::default()
    };
    let drafted = "# Cart\nThe Cart holds items.\n";
    seed_doc(&mut s, "cart.md", drafted);
    seed_doc(&mut s, "shop.md", "# Shop\nThe Shop sells.\n");
    crate::vfs::create_dir_all(dir.join("decompile")).unwrap();
    crate::vfs::write(
        dir.join("decompile/drafts.yaml"),
        format!("cart.md: {}\n", hash_hex(drafted)),
    )
    .unwrap();
    let found = checks(&s, &Project::default());
    let hits = rules_for(&found, "unratified");
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert_eq!(hits[0].1, ["cart.md"]);
    // One edit is a ratification.
    seed_doc(
        &mut s,
        "cart.md",
        "# Cart\nThe Cart holds items a Customer picked.\n",
    );
    let after = checks(&s, &Project::default());
    assert!(rules_for(&after, "unratified").is_empty());
    crate::vfs::remove_dir_all(&dir).ok();
}

#[test]
fn coverage_checks_flag_an_unprocessed_and_a_suspicious_section() {
    let mut s = Store::default();
    seed_doc(
        &mut s,
        "shop.md",
        "# Shop\n\n## Cart\nThe Cart holds items a Customer intends to buy.\n\n## Notes\nThe Cart shall keep every item until checkout.\n",
    );
    s.docs.get_mut("shop.md").unwrap().coverage.insert(
        "/shop/notes".into(),
        crate::model::Coverage {
            state: "non-normative".into(),
            note: None,
            claimed_by: None,
        },
    );
    let f = checks(&s, &Project::default());
    let uncovered = rules_for(&f, "uncovered-section");
    assert!(
        uncovered.iter().any(|h| h.1 == ["shop.md#/shop/cart"]),
        "{:?}",
        uncovered
    );
    let suspicious = rules_for(&f, "suspicious-non-normative");
    assert_eq!(suspicious.len(), 1, "{:?}", suspicious);
    assert_eq!(suspicious[0].1, ["shop.md#/shop/notes"]);
}

#[test]
fn stale_provenance_flags_a_quote_that_no_longer_locates() {
    let mut s = Store::default();
    seed_doc(&mut s, "shop.md", "# Shop\nThe Cart holds items.\n");
    s.graph.requirements.insert(
        "req:shop-1".into(),
        req(
            "The Cart holds items a Customer intends to buy.",
            "ent:cart",
            "shop.md",
            "/shop",
            "The Cart holds items a Customer intends to buy.",
        ),
    );
    s.graph.requirements.insert(
        "req:shop-2".into(),
        req(
            "The Cart holds items.",
            "ent:cart",
            "shop.md",
            "/shop",
            "The Cart holds items.",
        ),
    );
    let f = checks(&s, &Project::default());
    let hits = rules_for(&f, "stale-provenance");
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert_eq!(hits[0].1, ["req:shop-1"]);
}

#[test]
fn document_size_checks_flag_a_long_section_and_a_wide_document() {
    let mut s = Store::default();
    let long = format!(
        "# Big\n{}\n",
        "The system shall respond quickly. ".repeat(400)
    );
    seed_doc(&mut s, "big.md", &long);
    let mut wide = String::from("# Wide\n");
    for i in 0..crate::limits::MAX_DOC_SECTIONS + 2 {
        wide.push_str(&format!("\n## Section {}\nThe system shall hold.\n", i));
    }
    seed_doc(&mut s, "wide.md", &wide);
    let f = checks(&s, &Project::default());
    let big = rules_for(&f, "section-too-large");
    assert!(big.iter().any(|h| h.1 == ["big.md#/big"]), "{:?}", big);
    let doc = rules_for(&f, "doc-too-large");
    assert_eq!(doc.len(), 1, "{:?}", doc);
    assert!(doc[0].3.contains("split the document"), "{}", doc[0].3);
}

#[test]
fn unreachable_state_flags_a_state_no_path_reaches() {
    let mut s = Store::default();
    s.graph.state_machines.insert(
        "sm:order".into(),
        crate::model::StateMachine {
            subject: "ent:order".into(),
            states: vec![
                "placed".into(),
                "paid".into(),
                "archived".into(),
                "purged".into(),
            ],
            // `archived` and `purged` enter each other and nothing else enters
            // them: a cycle detached from the initial state, which is the shape
            // the check catches.
            transitions: vec![
                crate::model::StateTransition {
                    from: "archived".into(),
                    to: "purged".into(),
                    trigger: Some("retention lapses".into()),
                    guard: None,
                    requirements: vec!["req:orders-2".into()],
                },
                crate::model::StateTransition {
                    from: "purged".into(),
                    to: "archived".into(),
                    trigger: Some("restored from backup".into()),
                    guard: None,
                    requirements: vec!["req:orders-3".into()],
                },
                crate::model::StateTransition {
                    from: "placed".into(),
                    to: "paid".into(),
                    trigger: Some("payment confirmed".into()),
                    guard: None,
                    requirements: vec!["req:orders-1".into()],
                },
            ],
            ..Default::default()
        },
    );
    let f = checks(&s, &Project::default());
    let hits = rules_for(&f, "unreachable-state");
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert_eq!(hits[0].1, ["ent:order"]);
    assert!(hits[0].3.contains("archived"), "{}", hits[0].3);
}

#[test]
fn empty_file_flagged_with_zero_llm_calls() {
    let mut s = Store::default();
    seed_doc(&mut s, "empty.md", "");
    seed_doc(&mut s, "blank.md", "\n\n  \n");
    seed_doc(&mut s, "full.md", "# Full\nThe system shall respond.\n");
    let f = checks(&s, &Project::default());
    let hits = rules_for(&f, "empty-file");
    assert_eq!(hits.len(), 2, "{:?}", hits);
    assert!(hits
        .iter()
        .all(|(_, subj, sev, _, _)| sev == "warning" && *subj != ["full.md"]));
}

// A document that exists but cannot be read this time keeps its stored section
// tree: the parse carries the record forward, so alignment sees it unchanged and
// the sweep never collects its facts. Mirrors docs/compiler/parsing.md#format-handlers.
#[test]
fn an_unreadable_document_keeps_its_stored_record() {
    let dir = std::env::temp_dir().join(format!(
        "jazyk-unreadable-doc-test-{}",
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&dir).ok();
    crate::vfs::create_dir_all(dir.join("docs")).unwrap();
    crate::vfs::write(dir.join("docs/a.md"), "# A\nThe system shall respond.\n").unwrap();
    let proj = Project {
        root: dir.clone(),
        out: dir.join(".jazyk"),
        docs_glob: vec!["docs/**/*.md".to_string()],
        ..Default::default()
    };
    let mut store = Store {
        out: proj.out.clone(),
        ..Default::default()
    };
    let (parsed, _) = parse_all(&proj);
    let hash = parsed["docs/a.md"].0.clone();
    store.sync_docs(&parsed);
    assert!(crate::store::stored_doc(&proj.out, "docs/a.md").is_some());
    crate::vfs::write(dir.join("docs/a.md"), b"# A\n\xff\xfe\n").unwrap();
    let (parsed, _) = parse_all(&proj);
    assert_eq!(parsed["docs/a.md"].0, hash);
    assert!(parsed["docs/a.md"].1.contains_key("/a"));
    let mut again = Store::load(&proj.out);
    assert!(again.sync_docs(&parsed).is_empty());
    assert!(again.docs.contains_key("docs/a.md"));
    let findings = checks(&again, &proj);
    let errors = rules_for(&findings, "parse-error");
    assert_eq!(errors.len(), 1, "{:?}", errors);
    crate::vfs::write(dir.join("docs/a.md"), "# A\nThe system shall respond.\n").unwrap();
    let (parsed, _) = parse_all(&proj);
    assert!(Store::load(&proj.out).sync_docs(&parsed).is_empty());
}

// Opaque inputs compile, while known-format read failures remain actionable.
#[test]
fn opaque_inputs_compile_and_parse_errors_resolve() {
    let dir = std::env::temp_dir().join(format!("jazyk-format-test-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&dir).ok();
    crate::vfs::create_dir_all(dir.join("docs")).unwrap();
    crate::vfs::write(
        dir.join("docs/good.md"),
        "# Good\nThe system shall respond.\n",
    )
    .unwrap();
    crate::vfs::write(dir.join("docs/notes.opaque"), "plain notes\n").unwrap();
    crate::vfs::write(dir.join("docs/bad.md"), b"# Bad\n\xff\xfe\n").unwrap();
    let proj = Project {
        root: dir.clone(),
        out: dir.join(".jazyk"),
        docs_glob: vec!["docs/**/*".to_string()],
        ..Default::default()
    };
    let (parsed, _) = parse_all(&proj);
    assert_eq!(
        parsed.keys().collect::<Vec<_>>(),
        vec!["docs/good.md", "docs/notes.opaque"]
    );
    let f = checks(&Store::default(), &proj);
    let unsupported = rules_for(&f, "unsupported-format");
    assert!(unsupported.is_empty(), "{:?}", unsupported);
    assert_eq!(parsed["docs/notes.opaque"].1["/"].kind, "blob");
    let errors = rules_for(&f, "parse-error");
    assert_eq!(errors.len(), 1, "{:?}", errors);
    assert_eq!(errors[0].1, ["docs/bad.md"]);
    assert_eq!(errors[0].2, "error");
    assert!(errors[0].3.contains("not valid UTF-8"), "{}", errors[0].3);

    // Sticky and idempotent: two runs keep one diagnostic per finding; fixing the
    // file resolves it.
    let mut s = Store {
        out: proj.out.clone(),
        ..Default::default()
    };
    s.reconcile_check_diags(checks(&s, &proj));
    s.reconcile_check_diags(checks(&s, &proj));
    let open = |s: &Store, rule: &str| {
        s.graph
            .diagnostics
            .values()
            .filter(|d| d.rule == rule && d.lifecycle == "open")
            .count()
    };
    assert_eq!(open(&s, "unsupported-format"), 0);
    assert_eq!(open(&s, "parse-error"), 1);
    assert_eq!(s.graph.diagnostics.len(), 1);
    crate::vfs::write(dir.join("docs/bad.md"), "# Bad\nnow readable\n").unwrap();
    crate::vfs::remove_file(dir.join("docs/notes.opaque")).unwrap();
    s.reconcile_check_diags(checks(&s, &proj));
    assert_eq!(open(&s, "unsupported-format"), 0);
    assert_eq!(open(&s, "parse-error"), 0);
    assert!(s
        .graph
        .diagnostics
        .values()
        .all(|d| d.lifecycle == "resolved"));
    crate::vfs::remove_dir_all(&dir).ok();
}

#[test]
fn broken_links_use_current_files_and_resolve_without_model_calls() {
    let dir = std::env::temp_dir().join(format!("jazyk-broken-links-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&dir).ok();
    crate::vfs::create_dir_all(dir.join("slides")).unwrap();
    let source = "# Slides\nSee [Intro](slide-intro.md) and [again](slide-intro.md).\nSee [other](other.md).\n";
    crate::vfs::write(dir.join("slides/main.md"), source).unwrap();
    let proj = Project {
        root: dir.clone(),
        out: dir.join(".jazyk"),
        docs_glob: vec!["**/*.md".into()],
        ..Default::default()
    };
    let mut store = Store {
        out: proj.out.clone(),
        ..Default::default()
    };
    // The source was never compiled. A stale target record does not count
    // as an existing file, and two missing targets need different identities.
    seed_doc(&mut store, "slides/slide-intro.md", "# Deleted\n");
    let findings = checks(&store, &proj);
    let links = rules_for(&findings, "broken-link");
    assert_eq!(links.len(), 2, "{:?}", links);
    assert_eq!(links[0].1, ["slides/main.md", "slides/slide-intro.md"]);
    assert!(links[0].3.contains("slides/main.md:2:5"));
    assert!(links[0].3.contains("Create the document or fix the link"));
    store.reconcile_check_diags(findings);
    let id = store
        .graph
        .diagnostics
        .iter()
        .find(|(_, d)| d.rule == "broken-link" && d.subjects[1] == "slides/slide-intro.md")
        .unwrap()
        .0
        .clone();
    let generation = store.status.generation;
    store.reconcile_check_diags(checks(&store, &proj));
    assert_eq!(
        store.status.generation, generation,
        "no-op check must not journal again"
    );
    crate::vfs::write(dir.join("slides/slide-intro.md"), "# Intro\n").unwrap();
    store.reconcile_check_diags(checks(&store, &proj));
    assert_eq!(store.graph.diagnostics[&id].lifecycle, "resolved");
    assert_eq!(
        store
            .graph
            .diagnostics
            .values()
            .filter(|d| d.rule == "broken-link" && d.lifecycle == "open")
            .count(),
        1
    );
    crate::vfs::write(dir.join("slides/main.md"), "# Slides\nNo links.\n").unwrap();
    store.reconcile_check_diags(checks(&store, &proj));
    assert!(store
        .graph
        .diagnostics
        .values()
        .filter(|d| d.rule == "broken-link")
        .all(|d| d.lifecycle == "resolved"));
    crate::vfs::remove_dir_all(&dir).ok();
}

#[test]
fn one_stemmer_in_two_stages() {
    use crate::store::{content_stem, suffix_stem};
    // Mirrors docs/compiler/reconciler.md#pairs.
    for (word, suffix, content) in [
        ("files", "file", "fil"),
        ("filed", "fil", "fil"),
        ("sorting", "sort", "sort"),
        ("reverses", "reverse", "revers"),
        ("price", "price", "pric"),
        ("use", "use", "use"),
        ("is", "is", "is"),
    ] {
        assert_eq!(suffix_stem(word), suffix, "{}", word);
        assert_eq!(content_stem(word), content, "{}", word);
    }
}

#[test]
fn duplicate_requirement_keeps_decimals_apart() {
    // Split on the decimal point, `1.5` and `2.5` left only words two statements
    // share; the natural key's normalization keeps each number one word
    // (docs/compiler/compilation.md#checks).
    let mut s = Store::default();
    for (id, n, doc) in [("req:p-1", "1.5", "a.md"), ("req:p-2", "2.5", "b.md")] {
        let statement = format!("The basket shall weigh {} kilograms at most.", n);
        s.graph
            .requirements
            .insert(id.into(), req(&statement, "ent:one", doc, "/a", &statement));
    }
    let f = checks(&s, &Project::default());
    assert!(
        rules_for(&f, "duplicate-requirement").is_empty(),
        "{:?}",
        rules_for(&f, "duplicate-requirement")
    );
}

#[test]
fn duplicate_requirement_splits_warning_and_info() {
    let mut s = Store::default();
    s.graph.requirements.insert(
        "req:a-1".into(),
        req(
            "The system shall archive completed orders.",
            "ent:one",
            "a.md",
            "/a",
            "archives completed orders",
        ),
    );
    s.graph.requirements.insert(
        "req:a-2".into(),
        req(
            "The system shall archive completed orders.",
            "ent:one",
            "a.md",
            "/a",
            "archives completed orders",
        ),
    );
    s.graph.requirements.insert(
        "req:b-1".into(),
        req(
            "The store shall mint every id at creation.",
            "ent:two",
            "a.md",
            "/a",
            "mints every id at creation",
        ),
    );
    s.graph.requirements.insert(
        "req:b-2".into(),
        req(
            "The store shall mint every id at creation.",
            "ent:two",
            "b.md",
            "/b",
            "the store mints every id",
        ),
    );
    s.graph.requirements.insert(
        "req:c-1".into(),
        req(
            "The record shall have an id field.",
            "ent:three",
            "c.md",
            "/c",
            "- `id` - the identifier",
        ),
    );
    s.graph.requirements.insert(
        "req:c-2".into(),
        req(
            "The record shall have a name field.",
            "ent:three",
            "c.md",
            "/c",
            "- `name` - the display name",
        ),
    );
    let f = checks(&s, &Project::default());
    let hits = rules_for(&f, "duplicate-requirement");
    assert_eq!(hits.len(), 2, "{:?}", hits);
    assert!(hits
        .iter()
        .any(|(_, subj, sev, msg, _)| subj.iter().any(|s| s == "req:a-1")
            && sev == "warning"
            && msg.contains("keep one")));
    assert!(hits
        .iter()
        .any(|(_, subj, sev, msg, _)| subj.iter().any(|s| s == "req:b-1")
            && sev == "info"
            && msg.contains("both kept")));
    // A session's two-subject finding on the cross-document pair: the check
    // files no twin beside it (docs/compiler/compilation.md#checks).
    s.graph.diagnostics.insert(
        "diag:duplicate-requirement-9".into(),
        crate::model::Diagnostic {
            rule: "duplicate-requirement".into(),
            severity: "info".into(),
            subjects: vec!["req:b-1".into(), "req:b-2".into()],
            message: "the same fact".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        },
    );
    let f = checks(&s, &Project::default());
    let hits = rules_for(&f, "duplicate-requirement");
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert!(hits[0].1.iter().any(|s| s == "req:a-1"));
}

// Mirrors docs/compiler/compilation.md#checks: a requirement without a criterion is
// information without a deliverable and a warning with one; a restating criterion
// is a finding of its own.
#[test]
fn criterion_checks_follow_the_deliverable() {
    let mut s = Store::default();
    s.graph.entities.insert(
        "ent:one".into(),
        crate::model::Entity {
            name: "One".into(),
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:a-1".into(),
        Requirement {
            statement: "The system shall archive completed orders.".into(),
            entities: vec!["ent:one".into()],
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:a-2".into(),
        Requirement {
            statement: "The system shall keep one copy.".into(),
            entities: vec!["ent:one".into()],
            ..Default::default()
        },
    );
    s.graph.criteria.insert(
        "crit:a-1".into(),
        crate::model::Criterion {
            requirement: "req:a-2".into(),
            statement: "The system shall keep one copy".into(),
            kind: "positive".into(),
            ..Default::default()
        },
    );
    let f = checks(&s, &Project::default());
    let missing = rules_for(&f, "criterion-missing");
    assert_eq!(missing.len(), 1, "{:?}", missing);
    assert_eq!(missing[0].2, "info");
    assert!(missing[0].1.iter().any(|x| x == "req:a-1"));
    let restates = rules_for(&f, "criterion-restates");
    assert_eq!(restates.len(), 1, "{:?}", restates);
    let p = Project {
        gen_deliverable: Some("product".into()),
        ..Default::default()
    };
    let f = checks(&s, &p);
    assert_eq!(rules_for(&f, "criterion-missing")[0].2, "warning");
}

// The same situation observed two ways is flagged; a different situation is not.
// Mirrors docs/compiler/compilation.md#checks.
#[test]
fn criteria_with_one_situation_and_two_outcomes_conflict() {
    let mut s = Store::default();
    s.graph.entities.insert(
        "ent:sort".into(),
        Entity {
            name: "Sort".into(),
            ..Default::default()
        },
    );
    for rid in ["req:a-1", "req:t-1"] {
        s.graph.requirements.insert(
            rid.into(),
            Requirement {
                statement: format!("{} holds.", rid),
                entities: vec!["ent:sort".into()],
                ..Default::default()
            },
        );
    }
    let crit = |req: &str, given: &str, when: &str, then: &str| crate::model::Criterion {
        requirement: req.into(),
        statement: format!("{} {} {}", given, when, then),
        given: Some(given.into()),
        when: Some(when.into()),
        then: Some(then.into()),
        kind: "positive".into(),
        ..Default::default()
    };
    s.graph.criteria.insert(
        "crit:a-1".into(),
        crit(
            "req:a-1",
            "the lines `321`, `654`, `453`",
            "the utility sorts",
            "it prints `321`, `453`, `654`",
        ),
    );
    s.graph.criteria.insert(
        "crit:t-1".into(),
        crit(
            "req:t-1",
            "The lines `321`, `654`, `453`.",
            "The utility sorts.",
            "it prints `321`, `654`, `453`",
        ),
    );
    s.graph.criteria.insert(
        "crit:a-2".into(),
        crit(
            "req:a-1",
            "the lines `321`, `654`, `453`",
            "the utility sorts with `-r`",
            "it prints `654`, `453`, `321`",
        ),
    );
    let f = checks(&s, &Project::default());
    let conflicts = rules_for(&f, "criteria-conflict");
    assert_eq!(conflicts.len(), 1, "{:?}", conflicts);
    assert_eq!(
        conflicts[0].1,
        vec!["crit:a-1".to_string(), "crit:t-1".to_string()]
    );
    assert_eq!(conflicts[0].2, "warning");
}

#[test]
fn criteria_with_opposite_bounds_do_not_conflict() {
    let mut s = Store::default();
    s.graph.entities.insert(
        "ent:pay".into(),
        Entity {
            name: "Payment".into(),
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:p-1".into(),
        Requirement {
            statement: "Payments over 100 need a review.".into(),
            entities: vec!["ent:pay".into()],
            ..Default::default()
        },
    );
    let crit = |id: &str, given: &str, then: &str| {
        (
            id.to_string(),
            crate::model::Criterion {
                requirement: "req:p-1".into(),
                statement: format!("{} the payment is submitted {}", given, then),
                given: Some(given.into()),
                when: Some("the payment is submitted".into()),
                then: Some(then.into()),
                kind: "positive".into(),
                ..Default::default()
            },
        )
    };
    for (id, c) in [
        crit("crit:p-1", "an amount > 100", "it waits for a review"),
        crit("crit:p-2", "an amount < 100", "it goes through"),
    ] {
        s.graph.criteria.insert(id, c);
    }
    let f = checks(&s, &Project::default());
    assert!(
        rules_for(&f, "criteria-conflict").is_empty(),
        "{:?}",
        rules_for(&f, "criteria-conflict")
    );
}

#[test]
fn normative_signals_catch_prose_without_shall() {
    assert!(looks_normative(
        "The user management system handles user accounts and authentication.\n"
    ));
    assert!(looks_normative(
        "Login operation can be performed by unauthenticated.\n"
    ));
    assert!(looks_normative(
        "# Operations\n- `addProduct` - adds a new product to the inventory\n"
    ));
    assert!(looks_normative("Sections shall be covered.\n"));
}

#[test]
fn navigation_and_changelog_prose_stays_quiet() {
    assert!(!looks_normative(
        "See the [frontend documentation](./frontend.md) for more information.\n"
    ));
    assert!(!looks_normative(
        "# Changelog\n- 1.2: fixed typos in the intro\n"
    ));
    assert!(!looks_normative(
        "# Operations\n\nThe user management system supports the following operations:\n"
    ));
}

#[test]
fn justification_closure_finds_a_fact_without_provenance() {
    let mut s = crate::derive::tests::showcase_store();
    s.graph.requirements.insert(
        "req:x-1".into(),
        Requirement {
            statement: "Invented.".into(),
            entities: vec!["ent:order".into()],
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:x-2".into(),
        Requirement {
            statement: "Derived without a proposal.".into(),
            entities: vec!["ent:order".into()],
            provenance: Some(Provenance::Derived {
                from: vec!["ent:order".into()],
                reasoning: "split".into(),
            }),
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:x-3".into(),
        req(
            "Quoted in a dead section.",
            "ent:order",
            "shop.md",
            "/shop/gone",
            "gone",
        ),
    );
    let f = checks(&s, &Project::default());
    let hits = rules_for(&f, "unjustified-fact");
    let subjects: Vec<&str> = hits
        .iter()
        .flat_map(|h| h.1.iter().map(String::as_str))
        .collect();
    assert!(subjects.contains(&"req:x-1"), "{:?}", subjects);
    assert!(subjects.contains(&"req:x-2"), "{:?}", subjects);
    assert!(subjects.contains(&"req:x-3"), "{:?}", subjects);
    assert!(
        !subjects.contains(&"req:shop-1"),
        "a quoted fact in a live section is justified"
    );
    assert!(hits.iter().all(|h| h.2 == "error"));
}

#[test]
fn flow_placement_flags_an_unplaced_behavior_and_feeds_curate_view() {
    let mut s = crate::derive::tests::showcase_store();
    // A lone behavior requirement in another document: its cluster has one member.
    let text = "# Returns\n\nreturns body\n";
    seed_doc(&mut s, "returns.md", text);
    s.graph.requirements.insert(
        "req:returns-1".into(),
        Requirement {
            statement: "The customer returns an item.".into(),
            entities: vec!["ent:customer".into()],
            facets: vec![Facet {
                facet: "behavior".into(),
                reasoning: "a step".into(),
                measure: None,
            }],
            source: Some(SourceRef {
                doc: "returns.md".into(),
                section: "/returns".into(),
                quote: "returns body".into(),
            }),
            ..Default::default()
        },
    );
    let mut batch = crate::store::RecordBatch::new(1);
    crate::derive::recompute(&mut s, "g1", &mut batch);
    assert!(
        s.graph.views.values().any(|v| v.kind == "use-case"),
        "the showcase derives a flow view"
    );
    let f = checks(&s, &Project::default());
    let hits = rules_for(&f, "unplaced-behavior");
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert_eq!(hits[0].1, ["req:returns-1"]);
    let placement = flow_placement(&s, &Indices::build(&s));
    let (_, best, _, facet) = &placement[0];
    assert_eq!(facet, "behavior");
    assert!(
        best.as_deref()
            .is_some_and(|v| v.starts_with("view:usecase/") && v.ends_with("shop")),
        "{:?}",
        best
    );
}

#[test]
fn provider_check_names_missing_and_ambiguous_realizers() {
    let mut s = crate::derive::tests::showcase_store();
    let mut batch = crate::store::RecordBatch::new(1);
    crate::derive::recompute(&mut s, "g1", &mut batch);
    let f = checks(&s, &Project::default());
    assert!(
        rules_for(&f, "provider-missing").is_empty(),
        "both interfaces are realized"
    );
    // Drop the realizer of the stock API: the dependency is left without a provider.
    s.graph.requirements.remove("req:shop-4");
    crate::derive::recompute(&mut s, "g2", &mut batch);
    let f = checks(&s, &Project::default());
    let missing = rules_for(&f, "provider-missing");
    assert_eq!(missing.len(), 1, "{:?}", missing);
    assert_eq!(missing[0].1, ["ent:stock-api"]);
    // Two realizers of the checkout API.
    s.graph.requirements.insert(
        "req:shop-12".into(),
        Requirement {
            statement: "The inventory service also provides the checkout API.".into(),
            entities: vec!["ent:inventory-service".into(), "ent:checkout-api".into()],
            edges: vec![ReqEdge {
                a: "ent:inventory-service".into(),
                b: "ent:checkout-api".into(),
                rel_type: Some("realization".into()),
                cardinality: None,
            }],
            source: Some(SourceRef {
                doc: "shop.md".into(),
                section: "/shop".into(),
                quote: "The shop.".into(),
            }),
            ..Default::default()
        },
    );
    crate::derive::recompute(&mut s, "g3", &mut batch);
    let f = checks(&s, &Project::default());
    let ambiguous = rules_for(&f, "provider-ambiguous");
    assert_eq!(ambiguous.len(), 1, "{:?}", ambiguous);
    assert_eq!(ambiguous[0].1, ["ent:checkout-api"]);
    // Interface-like keys on structure, not the label: with the stereotype gone,
    // the realizations alone keep the provider check on the entity.
    s.graph
        .entities
        .get_mut("ent:checkout-api")
        .unwrap()
        .stereotype = None;
    let f = checks(&s, &Project::default());
    let ambiguous = rules_for(&f, "provider-ambiguous");
    assert_eq!(ambiguous.len(), 1, "{:?}", ambiguous);
    assert_eq!(ambiguous[0].1, ["ent:checkout-api"]);
}

#[test]
fn nondeterministic_transition_names_the_pair_as_subjects() {
    let m = StateMachine {
        subject: "ent:order".into(),
        states: vec!["placed".into(), "paid".into(), "held".into()],
        initial: Some("placed".into()),
        transitions: vec![
            StateTransition {
                from: "placed".into(),
                to: "paid".into(),
                trigger: Some("payment succeeds".into()),
                guard: None,
                requirements: vec!["req:shop-7".into()],
            },
            StateTransition {
                from: "placed".into(),
                to: "held".into(),
                trigger: Some("payment succeeds".into()),
                guard: None,
                requirements: vec!["req:shop-8".into()],
            },
        ],
    };
    let f = machine_checks(&m);
    let hits = rules_for(&f, "nondeterministic-transition");
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert_eq!(hits[0].1, ["req:shop-7", "req:shop-8"]);
}

// A merged arrow (a restated transition) reads as one arrow to every machine check:
// its requirements all become subjects of a nondeterministic pair, and dead-end and
// unhandled-event see it neither twice nor as a distinct event.
// Mirrors docs/compiler/model/state-machine.md#checks.
#[test]
fn machine_checks_read_a_merged_arrow_as_one_arrow() {
    let arrow = |from: &str, to: &str, trigger: &str, reqs: &[&str]| StateTransition {
        from: from.into(),
        to: to.into(),
        trigger: Some(trigger.into()),
        guard: None,
        requirements: reqs.iter().map(|r| r.to_string()).collect(),
    };
    let m = StateMachine {
        subject: "ent:order".into(),
        states: vec!["placed".into(), "paid".into(), "held".into()],
        initial: Some("placed".into()),
        transitions: vec![
            arrow(
                "placed",
                "paid",
                "payment succeeds",
                &["req:shop-12", "req:shop-7"],
            ),
            arrow("placed", "held", "payment declined", &["req:shop-8"]),
            arrow(
                "paid",
                "held",
                "payment declined",
                &["req:shop-13", "req:shop-9"],
            ),
        ],
    };
    let f = machine_checks(&m);
    assert!(rules_for(&f, "nondeterministic-transition").is_empty());
    let dead = rules_for(&f, "dead-end-state");
    assert_eq!(dead.len(), 1, "{:?}", dead);
    assert!(
        dead[0].3.contains("held") && !dead[0].3.contains("paid"),
        "{}",
        dead[0].3
    );
    let unhandled = rules_for(&f, "unhandled-event");
    assert_eq!(unhandled.len(), 1, "{:?}", unhandled);
    assert_eq!(
        unhandled[0].3,
        "ent:order: no transition for paid on payment succeeds"
    );
    // A second arrow out of placed on the merged arrow's trigger names every
    // contributor of both arrows.
    let mut m = m;
    m.states.push("refunded".into());
    m.transitions.push(arrow(
        "placed",
        "refunded",
        "payment succeeds",
        &["req:shop-99"],
    ));
    let f = machine_checks(&m);
    let hits = rules_for(&f, "nondeterministic-transition");
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert_eq!(hits[0].1, ["req:shop-12", "req:shop-7", "req:shop-99"]);
}

#[test]
fn conformance_check_flags_an_attribute_the_type_does_not_declare() {
    let mut s = crate::derive::tests::showcase_store();
    let mut batch = crate::store::RecordBatch::new(1);
    crate::derive::recompute(&mut s, "g1", &mut batch);
    let f = checks(&s, &Project::default());
    assert!(
        rules_for(&f, "nonconformant-instance").is_empty(),
        "{:?}",
        rules_for(&f, "nonconformant-instance")
    );
    s.graph
        .entities
        .get_mut("ent:ana")
        .unwrap()
        .attributes
        .push(Attribute {
            name: "shoe size".into(),
            r#type: None,
            value: Some("38".into()),
            provenance: Provenance::Quote(SourceRef {
                doc: "shop.md".into(),
                section: "/shop/examples".into(),
                quote: "examples".into(),
            }),
        });
    let f = checks(&s, &Project::default());
    let hits = rules_for(&f, "nonconformant-instance");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].1, ["ent:ana"]);
    assert!(
        hits[0].3.contains("shoe size") && hits[0].3.contains("ent:customer"),
        "{}",
        hits[0].3
    );
    let sm = rules_for(&f, "dead-end-state");
    assert_eq!(sm.len(), 1, "the order machine has final states: {:?}", sm);
    assert!(rules_for(&f, "quality-unmeasured").is_empty());
}

// Mirrors docs/compiler/compilation.md#checks: a well-shaped store is silent.
#[test]
fn level_shape_is_silent_on_a_well_shaped_store() {
    let s = level_store();
    let hits = level_shape(&s);
    assert!(hits.is_empty(), "{:?}", hits);
    assert_eq!(
        levels(&s),
        vec![
            (
                "scope:public".to_string(),
                vec!["ent:backend".to_string(), "ent:frontend".to_string()]
            ),
            (
                "ent:backend".to_string(),
                vec!["ent:database".to_string(), "ent:server".to_string()]
            ),
        ]
    );
    assert_eq!(
        level_view_of(&s, "scope:public").as_deref(),
        Some("view:component/public")
    );
    assert_eq!(
        level_view_of(&s, "ent:backend").as_deref(),
        Some("view:class/backend")
    );
    assert_eq!(level_view_of(&s, "ent:server"), None);
}

// A level of two or more without its structural view fires, on a node and on the
// scope root alike; a one-child node is not a level and needs no view.
#[test]
fn level_shape_fires_for_a_missing_level_view() {
    let mut s = level_store();
    s.graph.views.remove("view:class/backend");
    let hits = level_shape(&s);
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert_eq!(hits[0].1, ["ent:backend"]);
    assert!(
        hits[0].3.contains("no structural level view"),
        "{}",
        hits[0].3
    );
    assert!(hits[0].3.contains("view:class/backend"), "{}", hits[0].3);

    s.graph.views.remove("view:component/public");
    let hits = level_shape(&s);
    assert_eq!(hits.len(), 2, "{:?}", hits);
    assert!(hits.iter().any(|h| h.1 == ["scope:public"]), "{:?}", hits);

    // Down to one child, the backend is no level: nothing to show.
    s.graph.entities.remove("ent:database");
    let hits = level_shape(&s);
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert_eq!(hits[0].1, ["scope:public"]);
}

// A level over the hard fan-out fires until the mandatory goal regroups it; the
// node's own bump moves the threshold. Mirrors docs/compiler/graph.md#per-node-bumps.
#[test]
fn level_shape_fires_over_the_hard_fan_out() {
    use crate::limits::{CHILDREN_PER_ENTITY, CHILDREN_PER_ENTITY_HARD};
    let mut s = level_store();
    let n = CHILDREN_PER_ENTITY_HARD as usize + 1;
    for i in 0..n {
        s.graph.entities.insert(
            format!("ent:module-{}", i),
            Entity {
                name: format!("Module {}", i),
                parent: Some("ent:server".into()),
                mentions: vec![SourceRef {
                    doc: "arch.md".into(),
                    section: "/arch".into(),
                    quote: format!("Module {}", i),
                }],
                ..Default::default()
            },
        );
    }
    s.graph.views.insert(
        "view:class/server".into(),
        View {
            kind: "class".into(),
            title: "Server".into(),
            members: (0..n).map(|i| format!("ent:module-{}", i)).collect(),
            default: true,
            ..Default::default()
        },
    );
    assert_eq!(
        fan_out_thresholds(&s, "ent:server"),
        (
            crate::limits::CHILDREN_PER_ENTITY_SOFT,
            CHILDREN_PER_ENTITY_HARD
        )
    );
    let hits = level_shape(&s);
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert_eq!(hits[0].1, ["ent:server"]);
    assert!(
        hits[0]
            .3
            .contains(&format!("over the hard {} threshold", CHILDREN_PER_ENTITY)),
        "{}",
        hits[0].3
    );
    // The shape counts it in the over-hard band.
    let shape = shape(&s);
    assert_eq!(shape.per_depth, vec![2, 2, n]);
    assert_eq!(shape.bands, [2, 0, 1]);

    // A bump on the node lifts hard by the registry's distance: silent again.
    s.graph
        .entities
        .get_mut("ent:server")
        .unwrap()
        .limits
        .insert(
            CHILDREN_PER_ENTITY.to_string(),
            LimitBump {
                value: CHILDREN_PER_ENTITY_HARD,
            },
        );
    assert!(fan_out_thresholds(&s, "ent:server").1 >= n as u64);
    assert!(level_shape(&s).is_empty());
}

// A derived grouping under two children fires (the sweep's dissolve did not
// settle the store); a stated entity holding one child is a grouping in role only
// and never fires. Mirrors docs/compiler/concepts/levels.md#groupings.
#[test]
fn level_shape_fires_for_an_under_membered_grouping() {
    let mut s = level_store();
    let grouping = |name: &str, from: &[&str]| Entity {
        name: name.into(),
        parent: Some("ent:backend".into()),
        definition: Some(format!("{} holds the persistence layer.", name)),
        provenance: Some(Provenance::Derived {
            from: from.iter().map(|f| f.to_string()).collect(),
            reasoning: "the members share one responsibility".into(),
        }),
        ..Default::default()
    };
    s.graph
        .entities
        .insert("ent:storage".into(), grouping("Storage", &["ent:database"]));
    s.graph.entities.get_mut("ent:database").unwrap().parent = Some("ent:storage".into());
    s.graph.views.get_mut("view:class/backend").unwrap().members =
        vec!["ent:server".into(), "ent:storage".into()];
    assert!(is_derived_grouping(&s, "ent:storage"));
    assert!(!is_derived_grouping(&s, "ent:backend"));
    let hits = level_shape(&s);
    assert_eq!(hits.len(), 1, "{:?}", hits);
    assert_eq!(hits[0].1, ["ent:storage"]);
    assert!(hits[0].3.contains("1 child(ren)"), "{}", hits[0].3);

    // A childless grouping fires too; a stated one-child entity does not.
    s.graph
        .entities
        .insert("ent:empty".into(), grouping("Empty", &[]));
    let hits = level_shape(&s);
    assert_eq!(hits.len(), 2, "{:?}", hits);
    assert!(hits.iter().any(|h| h.1 == ["ent:empty"]), "{:?}", hits);
    assert!(hits.iter().all(|h| h.1 != ["ent:server"]), "{:?}", hits);

    // With a second member the grouping is a level again, and its view answers.
    s.graph.entities.remove("ent:empty");
    s.graph.entities.get_mut("ent:server").unwrap().parent = Some("ent:storage".into());
    s.graph.entities.get_mut("ent:storage").unwrap().parent = None;
    s.graph.views.insert(
        "view:class/storage".into(),
        View {
            kind: "class".into(),
            title: "Storage".into(),
            members: vec!["ent:database".into(), "ent:server".into()],
            default: true,
            ..Default::default()
        },
    );
    s.graph.views.remove("view:class/backend");
    let hits = level_shape(&s);
    assert!(hits.is_empty(), "{:?}", hits);
}
