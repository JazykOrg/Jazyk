use super::tests::own_dir;
use super::*;
use crate::model::*;
use crate::store::Store;
use serde_json::Value;
use std::collections::BTreeMap;

// The split-view claim holds only when the parent view itself is under the soft
// value after the staged work. Mirrors docs/compiler/goals/split-view.md#gate.
#[test]
fn a_split_view_claim_needs_the_parent_under_its_limit() {
    let cases = parse_cases();
    let case = cases.iter().find(|c| c.name == "split-view").unwrap();
    let tmp = own_dir("split-gate");
    let (store, goal) = seed_derived(case, &tmp).unwrap();
    let v = crate::goals::split_view_goal_gate(&store, &[], &goal.target).expect("over before");
    assert_eq!(v.rule, "over-limit", "{}", v.message);
    let members = store.graph.views[&goal.target].members.clone();
    // Collapsing requirements hides nothing in a flow view.
    let collapse = crate::store::Op::UpdateView {
        id: goal.target.clone(),
        title: None,
        members: None,
        add_members: Vec::new(),
        remove_members: Vec::new(),
        query: None,
        collapse: Some(vec![members[0].clone(), members[6].clone()]),
        exclude: Vec::new(),
        reasoning: None,
    };
    assert!(crate::goals::split_view_goal_gate(&store, &[collapse], &goal.target).is_some());
    // One member per phase brings the parent under.
    let coarse = crate::store::Op::UpdateView {
        id: goal.target.clone(),
        title: None,
        members: Some(vec![members[0].clone(), members[6].clone()]),
        add_members: Vec::new(),
        remove_members: Vec::new(),
        query: None,
        collapse: None,
        exclude: Vec::new(),
        reasoning: None,
    };
    assert!(crate::goals::split_view_goal_gate(&store, &[coarse], &goal.target).is_none());
    crate::vfs::remove_dir_all(&tmp).ok();
}

// requirementExists without an entity matches on the statement alone.
// Mirrors docs/benchmark/cases.md#checks.
#[test]
fn requirement_exists_takes_an_optional_entity() {
    let mut s = Store::default();
    s.graph.requirements.insert(
        "req:d-1".into(),
        crate::model::Requirement {
            statement: "The dedupe utility trims whitespace from the current line.".into(),
            ..Default::default()
        },
    );
    assert!(eval_check(
        "requirementExists",
        &serde_json::json!({"statementPattern": "trim"}),
        &s,
        0
    )
    .is_none());
    assert!(eval_check(
        "requirementExists",
        &serde_json::json!({"statementPattern": "sort"}),
        &s,
        0
    )
    .is_some());
    assert!(eval_check(
        "requirementExists",
        &serde_json::json!({"statementPattern": "trim", "entity": "Dedupe"}),
        &s,
        0
    )
    .is_some());
}

// facetCount counts the requirements carrying a facet, optionally only those
// whose statement matches. Mirrors docs/benchmark/cases.md#checks.
#[test]
fn facet_count_reads_the_facets_of_matching_statements() {
    let mut s = Store::default();
    let req = |statement: &str, facets: &[&str]| crate::model::Requirement {
        statement: statement.into(),
        facets: facets
            .iter()
            .map(|f| Facet {
                facet: f.to_string(),
                reasoning: "judged".into(),
                measure: None,
            })
            .collect(),
        ..Default::default()
    };
    s.graph.requirements.insert(
        "req:p-1".into(),
        req("Dot tells Ines who Theo works for.", &["behavior"]),
    );
    s.graph.requirements.insert(
        "req:p-2".into(),
        req("Ines asks Dot for the harbor ledger.", &["constraint"]),
    );
    s.graph
        .requirements
        .insert("req:p-3".into(), req("Ines Marlow is 31.", &["constraint"]));
    let ok = |a: Value| eval_check("facetCount", &a, &s, 0);
    assert_eq!(
        ok(serde_json::json!({"facet": "behavior", "min": 1, "max": 1})),
        None
    );
    assert_eq!(
        ok(serde_json::json!({"facet": "constraint", "min": 2})),
        None
    );
    assert_eq!(
        ok(serde_json::json!({"facet": "behavior", "statementPattern": "tells", "min": 1})),
        None
    );
    // A scene labeled constraint fails the scene's behavior check and its max 0.
    assert!(
        ok(serde_json::json!({"facet": "behavior", "statementPattern": "ledger", "min": 1}))
            .is_some()
    );
    assert!(ok(
        serde_json::json!({"facet": "constraint", "statementPattern": "ledger|tells", "max": 0})
    )
    .is_some());
    assert_eq!(
        ok(serde_json::json!({"facet": "constraint", "statementPattern": "tells", "max": 0})),
        None
    );
    assert!(ok(serde_json::json!({"facet": "behavior", "statementPattern": "(bad"})).is_some());
}

// transitionSubject counts the transitions whose subject is the entity, at
// least one by default. Mirrors docs/benchmark/cases.md#checks.
#[test]
fn transition_subject_reads_the_subject_entity() {
    let mut s = Store::default();
    s.graph
        .entities
        .insert("ent:ines".into(), ent("Ines Marlow", None));
    s.graph
        .entities
        .insert("ent:stance".into(), ent("Ines's stance toward Theo", None));
    let step = |subject: &str, from: &str, to: &str| crate::model::Requirement {
        statement: format!("The stance moves from {} to {}.", from, to),
        transition: Some(Transition {
            subject: subject.into(),
            from: from.into(),
            to: to.into(),
            trigger: None,
            guard: None,
        }),
        ..Default::default()
    };
    s.graph.requirements.insert(
        "req:a-1".into(),
        step("ent:stance", "stranger", "acquaintance"),
    );
    s.graph.requirements.insert(
        "req:a-2".into(),
        step("ent:stance", "acquaintance", "rival"),
    );
    let ok = |a: Value| eval_check("transitionSubject", &a, &s, 0);
    // Hung on a stance entity: the character carries none.
    let why = ok(serde_json::json!({"entity": "Ines Marlow"}));
    assert!(
        why.as_deref().is_some_and(|w| w.contains("ent:stance")),
        "{:?}",
        why
    );
    assert_eq!(
        ok(serde_json::json!({"entity": "ent:stance", "min": 2, "max": 2})),
        None
    );
    assert!(ok(serde_json::json!({"entity": "Nobody"})).is_some());
    s.graph
        .requirements
        .insert("req:a-3".into(), step("ent:ines", "rival", "ally"));
    let ok = |a: Value| eval_check("transitionSubject", &a, &s, 0);
    assert_eq!(ok(serde_json::json!({"entity": "Ines Marlow"})), None);
    assert!(ok(serde_json::json!({"entity": "Ines Marlow", "min": 2})).is_some());
}

// statusIn reads the row's derived status, never the claimed verdict alone;
// testsFail reruns each recorded programmatic test. Mirrors
// docs/benchmark/cases.md#checks.
#[test]
fn bind_checks_read_the_status_and_rerun_the_tests() {
    let tmp = own_dir("bind-checks");
    let mut s = Store {
        out: tmp.join("out"),
        ..Default::default()
    };
    s.graph.requirements.insert(
        "req:b-1".into(),
        Requirement {
            statement: "The Backend lists the products.".into(),
            entities: vec!["ent:backend".into()],
            ..Default::default()
        },
    );
    let gs = crate::gen::GenSettings {
        configured: true,
        deliverable: tmp.to_path_buf(),
        worker: "agentic".into(),
        code: vec!["**".into()],
    };
    crate::vfs::create_dir_all(gs.deliverable.join("test")).unwrap();
    crate::vfs::write(gs.deliverable.join("test/b.sh"), "# req-b-1\nexit 0\n").unwrap();
    let record = |run: &str, verdict: &str, files: Vec<String>| {
        let test = crate::gen::TestRef {
            kind: "programmatic".into(),
            label: "descriptor".into(),
            artifact: "test/b.sh".into(),
            name: "req-b-1".into(),
            run: run.into(),
            cwd: ".".into(),
        };
        let mut ledger = crate::gen::Ledger::load(&s.out);
        ledger.requirements.insert(
            "req:b-1".into(),
            crate::gen::ReqRow {
                tests: BTreeMap::new(),
                entity: "ent:backend".into(),
                sites: Vec::new(),
                hashes: crate::gen::RowHashes {
                    requirement: hash_hex("The Backend lists the products."),
                    test: crate::gen::hash_file(&crate::gen::artifact_path(&s.out, &gs, &test)),
                    files: crate::gen::hash_files(&gs, &files),
                },
                files,
                test,
                verdict: verdict.into(),
                last_run: None,
                exit_code: None,
                evidence: None,
            },
        );
        ledger.save(&s.out).unwrap();
    };
    let check = |k: &str| {
        eval_workflow_check(
            k,
            &serde_json::json!({"requirement": "req:b-1", "statuses": ["unimplemented", "failing"]}),
            &s,
            &gs,
            "req:b-1",
        )
    };
    // No row yet: both fail.
    assert!(check("statusIn").is_some());
    assert!(check("testsFail").is_some());
    // The descriptor test bound as verifying: it passes on rerun and reads verified.
    record("sh test/b.sh", "pass", Vec::new());
    let why = check("statusIn");
    assert!(
        why.as_deref().is_some_and(|w| w.contains("verified")),
        "{:?}",
        why
    );
    let why = check("testsFail");
    assert!(
        why.as_deref()
            .is_some_and(|w| w.contains("passes as recorded")),
        "{:?}",
        why
    );
    // A claimed fail does not hide a test that passes on rerun.
    record("sh test/b.sh", "fail", Vec::new());
    assert_eq!(check("statusIn"), None);
    assert!(check("testsFail").is_some());
    // A test that drives what is not there fails, and the row reads unimplemented.
    record("sh test/b.sh && false", "fail", Vec::new());
    assert_eq!(check("statusIn"), None);
    assert_eq!(check("testsFail"), None);
    crate::vfs::remove_dir_all(&tmp).ok();
}

// fileMatches counts a pattern in one deliverable file and fileCount counts the
// files whose relative path matches, minus the excluded: the conventions a
// generated deliverable follows, graded over its files. Mirrors
// docs/benchmark/cases.md#checks.
#[test]
fn file_checks_read_the_deliverable() {
    let tmp = own_dir("file-checks");
    let s = Store {
        out: tmp.join("out"),
        ..Default::default()
    };
    let gs = crate::gen::GenSettings {
        configured: true,
        deliverable: tmp.to_path_buf(),
        worker: "agentic".into(),
        code: vec!["**".into()],
    };
    let write = |rel: &str, text: &str| {
        let p = gs.deliverable.join(rel);
        crate::vfs::create_dir_all(p.parent().unwrap()).unwrap();
        crate::vfs::write(&p, text).unwrap();
    };
    let check = |k: &str, arg: Value| eval_workflow_check(k, &arg, &s, &gs, "ent:greeter");
    let named = serde_json::json!({"path": "package.json", "pattern": r#""name"\s*:\s*"[^"]*greeter[^"]*""#, "min": 1});
    let unpinned = serde_json::json!({"path": "package.json", "pattern": r#""\s*:\s*"(latest|\*)""#, "max": 0});
    // No file yet: a missing file fails, whatever the bounds.
    let why = check("fileMatches", named.clone());
    assert!(
        why.as_deref().is_some_and(|w| w.contains("does not exist")),
        "{:?}",
        why
    );
    assert!(check("fileMatches", unpinned.clone()).is_some());
    // The erp shape: named for one part, every dependency at latest.
    write(
            "package.json",
            "{\n  \"name\": \"warehouse-erp-frontend\",\n  \"main\": \"server.js\",\n  \"dependencies\": { \"express\": \"latest\", \"pg\": \"*\" }\n}\n",
        );
    let why = check("fileMatches", named.clone());
    assert!(
        why.as_deref()
            .is_some_and(|w| w.contains("0 matches") && w.contains("at least 1")),
        "{:?}",
        why
    );
    let why = check("fileMatches", unpinned.clone());
    assert!(
        why.as_deref()
            .is_some_and(|w| w.contains("2 matches") && w.contains("at most 0")),
        "{:?}",
        why
    );
    // Pinned and named for the owner: both pass, case-insensitively.
    write(
            "package.json",
            "{\n  \"name\": \"@acme/Greeter\",\n  \"main\": \"src/index.js\",\n  \"dependencies\": { \"express\": \"^4.19.2\" }\n}\n",
        );
    assert_eq!(check("fileMatches", named), None);
    assert_eq!(check("fileMatches", unpinned), None);
    // A bad pattern fails the check, never passes it silently.
    assert!(check(
        "fileMatches",
        serde_json::json!({"path": "package.json", "pattern": "(", "max": 0})
    )
    .is_some());

    // Modules: the entry point, its test, a fixture under test/, and a second
    // entry point beside the first.
    write("src/index.js", "module.exports = {};\n");
    write("src/index.test.js", "require('node:test');\n");
    write("test/helper.js", "\n");
    write("node_modules/left/over.js", "\n");
    let modules = |max: u64| {
        serde_json::json!({
            "pathPattern": r"\.(js|mjs|cjs)$",
            "excludePattern": r"(^|/)(test|tests|__tests__|node_modules)/|\.(test|spec)\.(js|mjs|cjs)$",
            "min": 1,
            "max": max,
        })
    };
    assert_eq!(check("fileCount", modules(1)), None);
    // Every .js file counts without the exclusion, and the failure names them.
    let why = check(
        "fileCount",
        serde_json::json!({"pathPattern": r"\.js$", "max": 1}),
    );
    assert!(
        why.as_deref()
            .is_some_and(|w| w.contains("4 files") && w.contains("test/helper.js")),
        "{:?}",
        why
    );
    write("src/app.js", "module.exports = {};\n");
    let why = check("fileCount", modules(1));
    assert!(
        why.as_deref().is_some_and(|w| w.contains("2 files")
            && w.contains("src/app.js")
            && w.contains("src/index.js")),
        "{:?}",
        why
    );
    assert_eq!(check("fileCount", modules(2)), None);
    // An empty match is a count of zero, so min bites and max 0 passes.
    assert!(check(
        "fileCount",
        serde_json::json!({"pathPattern": r"\.py$", "min": 1})
    )
    .is_some());
    assert_eq!(
        check(
            "fileCount",
            serde_json::json!({"pathPattern": r"\.py$", "max": 0})
        ),
        None
    );
    assert!(check(
        "fileCount",
        serde_json::json!({"pathPattern": "(", "max": 0})
    )
    .is_some());
    crate::vfs::remove_dir_all(&tmp).ok();
}

fn ent(name: &str, parent: Option<&str>) -> Entity {
    Entity {
        name: name.into(),
        parent: parent.map(String::from),
        ..Default::default()
    }
}

fn structure_store() -> Store {
    let mut s = Store::default();
    s.graph.entities.insert(
        "ent:orders".into(),
        Entity {
            definition: Some("Holds the order area.".into()),
            provenance: Some(Provenance::Derived {
                from: vec!["ent:cart".into(), "ent:pricing".into()],
                reasoning: "the orders document".into(),
            }),
            ..ent("Orders", None)
        },
    );
    s.graph
        .entities
        .insert("ent:cart".into(), ent("Cart", Some("ent:orders")));
    s.graph
        .entities
        .insert("ent:pricing".into(), ent("Pricing", Some("ent:orders")));
    s.graph
        .entities
        .insert("ent:billing".into(), ent("Billing", None));
    s.graph
        .entities
        .insert("ent:invoice".into(), ent("Invoice", Some("ent:billing")));
    s.graph
        .entities
        .insert("ent:billing-area".into(), ent("Billing Area", None));
    s.graph.requirements.insert(
        "req:a".into(),
        Requirement {
            statement: "The orders area consists of the cart and pricing.".into(),
            entities: vec!["ent:orders".into(), "ent:cart".into(), "ent:pricing".into()],
            edges: vec![ReqEdge {
                a: "ent:orders".into(),
                b: "ent:cart".into(),
                rel_type: Some("composition".into()),
                cardinality: None,
            }],
            ..Default::default()
        },
    );
    s.graph.views.insert(
        "view:sequence/flow".into(),
        View {
            kind: "sequence".into(),
            title: "Flow".into(),
            members: vec!["req:a".into()],
            excluded: vec![
                Exclusion {
                    id: "req:b".into(),
                    note: "belongs to view:sequence/detail".into(),
                },
                Exclusion {
                    id: "req:c".into(),
                    note: "none".into(),
                },
            ],
            collapse: vec!["ent:orders".into()],
            ..Default::default()
        },
    );
    s.graph.views.insert(
        "view:sequence/detail".into(),
        View {
            kind: "sequence".into(),
            title: "Flow: Detail".into(),
            members: vec!["req:d".into(), "req:a".into()],
            ..Default::default()
        },
    );
    s
}

#[test]
fn containment_checks_read_the_tree() {
    let s = structure_store();
    let ok = |k: &str, a: Value| eval_check(k, &a, &s, 0);
    assert_eq!(
        ok(
            "childCount",
            serde_json::json!({"parent": "ent:orders", "min": 2, "max": 2})
        ),
        None
    );
    assert!(ok(
        "childCount",
        serde_json::json!({"parent": "Orders", "max": 1})
    )
    .is_some());
    assert_eq!(
        ok(
            "childCount",
            serde_json::json!({"parent": "scope:public", "max": 3})
        ),
        None
    );
    assert!(ok(
        "childCount",
        serde_json::json!({"parent": "scope:public", "max": 2})
    )
    .is_some());
    assert!(ok(
        "childCount",
        serde_json::json!({"parent": "ent:nope", "max": 2})
    )
    .is_some());
    assert_eq!(
        ok(
            "parentIs",
            serde_json::json!({"entity": "ent:cart", "parent": "Orders"})
        ),
        None
    );
    assert!(ok(
        "parentIs",
        serde_json::json!({"entity": "ent:cart", "parent": "ent:billing"})
    )
    .is_some());
    assert!(ok(
        "parentIs",
        serde_json::json!({"entity": "ent:billing", "parent": "ent:orders"})
    )
    .is_some());
    // A grouping: derived from exactly its members, holding exactly them.
    assert_eq!(
        ok(
            "groupingOf",
            serde_json::json!({"members": ["ent:cart", "ent:pricing"]})
        ),
        None
    );
    assert_eq!(
        ok(
            "groupingOf",
            serde_json::json!({"members": ["ent:cart", "ent:pricing"], "namePattern": "^orders$"})
        ),
        None
    );
    assert!(ok(
        "groupingOf",
        serde_json::json!({"members": ["ent:cart", "ent:pricing"], "namePattern": "^billing"})
    )
    .is_some());
    assert!(ok("groupingOf", serde_json::json!({"members": ["ent:cart"]})).is_some());
    // A stated parent is no grouping: no derived provenance.
    assert!(ok(
        "groupingOf",
        serde_json::json!({"members": ["ent:invoice"]})
    )
    .is_some());
    assert_eq!(
        ok(
            "entityNameCount",
            serde_json::json!({"namePattern": "billing", "max": 2})
        ),
        None
    );
    assert!(ok(
        "entityNameCount",
        serde_json::json!({"namePattern": "billing", "max": 1})
    )
    .is_some());
    assert_eq!(
        ok(
            "entityNameCount",
            serde_json::json!({"namePattern": "^billing$", "max": 1})
        ),
        None
    );
    assert_eq!(
        ok("nodeExists", serde_json::json!({"id": "ent:cart"})),
        None
    );
    assert_eq!(ok("nodeExists", serde_json::json!({"id": "req:a"})), None);
    assert_eq!(
        ok(
            "nodeExists",
            serde_json::json!({"id": "view:sequence/flow"})
        ),
        None
    );
    assert!(ok("nodeExists", serde_json::json!({"id": "ent:gone"})).is_some());
}

#[test]
fn edge_checks_read_direction_and_type() {
    let s = structure_store();
    let ok = |k: &str, a: Value| eval_check(k, &a, &s, 0);
    assert_eq!(
        ok(
            "edgeDeclared",
            serde_json::json!({"requirement": "req:a", "a": "ent:orders", "b": "ent:cart", "type": "composition"})
        ),
        None
    );
    assert_eq!(
        ok(
            "edgeDeclared",
            serde_json::json!({"requirement": "req:a", "a": "Orders", "b": "Cart"})
        ),
        None
    );
    // Direction counts, and so does the type.
    assert!(ok(
        "edgeDeclared",
        serde_json::json!({"requirement": "req:a", "a": "ent:cart", "b": "ent:orders"})
    )
    .is_some());
    assert!(ok("edgeDeclared", serde_json::json!({"requirement": "req:a", "a": "ent:orders", "b": "ent:cart", "type": "dependency"})).is_some());
    assert!(ok(
        "edgeDeclared",
        serde_json::json!({"requirement": "req:a", "a": "ent:orders", "b": "ent:pricing"})
    )
    .is_some());
    assert_eq!(
        ok(
            "edgeAbsent",
            serde_json::json!({"requirement": "req:a", "a": "ent:cart", "b": "ent:pricing"})
        ),
        None
    );
    assert!(ok(
        "edgeAbsent",
        serde_json::json!({"requirement": "req:a", "a": "ent:cart", "b": "ent:orders"})
    )
    .is_some());
    assert!(ok(
        "edgeAbsent",
        serde_json::json!({"requirement": "req:nope", "a": "ent:cart", "b": "ent:orders"})
    )
    .is_some());
}

// relationshipExists takes one `type` or any of several `types`.
// Mirrors docs/benchmark/cases.md#checks.
#[test]
fn relationship_exists_takes_any_of_several_types() {
    let mut s = structure_store();
    crate::derive::recompute_relationships(&mut s);
    let ok = |a: Value| eval_check("relationshipExists", &a, &s, 0);
    assert_eq!(ok(serde_json::json!({"a": "Orders", "b": "Cart"})), None);
    assert_eq!(
        ok(serde_json::json!({"a": "Orders", "b": "Cart", "type": "composition"})),
        None
    );
    assert_eq!(
        ok(
            serde_json::json!({"a": "Orders", "b": "Cart", "types": ["composition", "aggregation"]})
        ),
        None
    );
    assert!(ok(serde_json::json!({"a": "Orders", "b": "Cart", "type": "aggregation"})).is_some());
    assert!(ok(
        serde_json::json!({"a": "Orders", "b": "Cart", "types": ["aggregation", "dependency"]})
    )
    .is_some());
    assert!(ok(
        serde_json::json!({"a": "Orders", "b": "Pricing", "types": ["composition", "aggregation"]})
    )
    .is_some());
}

#[test]
fn view_checks_read_membership_exclusions_and_order() {
    let s = structure_store();
    let ok = |k: &str, a: Value| eval_check(k, &a, &s, 0);
    assert_eq!(
        ok("viewExists", serde_json::json!({"kind": "sequence"})),
        None
    );
    assert_eq!(
        ok(
            "viewExists",
            serde_json::json!({"kind": "sequence", "excluding": "view:sequence/flow"})
        ),
        None
    );
    assert_eq!(
        ok(
            "viewExists",
            serde_json::json!({"kind": "sequence", "titlePattern": "detail", "excluding": "view:sequence/flow"})
        ),
        None
    );
    assert!(ok("viewExists", serde_json::json!({"kind": "sequence", "titlePattern": "^flow$", "excluding": "view:sequence/flow"})).is_some());
    assert!(ok("viewExists", serde_json::json!({"kind": "class"})).is_some());
    assert_eq!(
        ok(
            "viewMember",
            serde_json::json!({"view": "view:sequence/flow", "member": "req:a"})
        ),
        None
    );
    assert!(ok(
        "viewMember",
        serde_json::json!({"view": "view:sequence/flow", "member": "req:b"})
    )
    .is_some());
    assert!(ok(
        "viewMember",
        serde_json::json!({"view": "view:none", "member": "req:a"})
    )
    .is_some());
    assert_eq!(
        ok(
            "viewExcludes",
            serde_json::json!({"view": "view:sequence/flow", "member": "req:b"})
        ),
        None
    );
    // A placeholder note is no note.
    assert!(ok(
        "viewExcludes",
        serde_json::json!({"view": "view:sequence/flow", "member": "req:c"})
    )
    .is_some());
    assert!(ok(
        "viewExcludes",
        serde_json::json!({"view": "view:sequence/flow", "member": "req:a"})
    )
    .is_some());
    assert_eq!(
        ok(
            "viewMemberOrder",
            serde_json::json!({"view": "view:sequence/detail", "before": "req:d", "after": "req:a"})
        ),
        None
    );
    assert!(ok(
        "viewMemberOrder",
        serde_json::json!({"view": "view:sequence/detail", "before": "req:a", "after": "req:d"})
    )
    .is_some());
    assert!(ok(
        "viewMemberOrder",
        serde_json::json!({"view": "view:sequence/detail", "before": "req:a", "after": "req:x"})
    )
    .is_some());
    // Accounted: a member, an excluded id with a note, a member of another
    // view, an id hidden under a collapsed entity; never a placeholder note.
    assert_eq!(
        ok(
            "membersAccounted",
            serde_json::json!({"view": "view:sequence/flow", "members": ["req:a", "req:b", "req:d", "ent:cart"]})
        ),
        None
    );
    assert!(ok(
        "membersAccounted",
        serde_json::json!({"view": "view:sequence/flow", "members": ["req:c"]})
    )
    .is_some());
    assert!(ok(
        "membersAccounted",
        serde_json::json!({"view": "view:sequence/flow", "members": ["req:zzz"]})
    )
    .is_some());
}

#[test]
fn view_within_limit_recomputes_the_commits_count() {
    let mut s = Store::default();
    for i in 0..10 {
        s.graph
            .entities
            .insert(format!("ent:p{}", i), ent(&format!("P{}", i), None));
    }
    let step = |i: usize| Requirement {
        statement: format!("P{} calls P{}.", i, i + 1),
        entities: vec![format!("ent:p{}", i), format!("ent:p{}", i + 1)],
        edges: vec![ReqEdge {
            a: format!("ent:p{}", i),
            b: format!("ent:p{}", i + 1),
            rel_type: Some("dependency".into()),
            cardinality: None,
        }],
        ..Default::default()
    };
    for i in 0..9 {
        s.graph.requirements.insert(format!("req:s{}", i), step(i));
    }
    s.graph.views.insert(
        "view:sequence/wide".into(),
        View {
            kind: "sequence".into(),
            title: "Wide".into(),
            members: (0..9).map(|i| format!("req:s{}", i)).collect(),
            ..Default::default()
        },
    );
    s.graph.views.insert(
        "view:sequence/narrow".into(),
        View {
            kind: "sequence".into(),
            title: "Narrow".into(),
            members: (0..3).map(|i| format!("req:s{}", i)).collect(),
            ..Default::default()
        },
    );
    let ok = |k: &str, a: Value| eval_check(k, &a, &s, 0);
    // Ten participants against a soft limit of eight.
    let why = ok(
        "viewWithinLimit",
        serde_json::json!({"view": "view:sequence/wide", "limit": "participants-per-sequence-view"}),
    );
    assert!(
        why.as_deref()
            .is_some_and(|w| w.contains("10 participants-per-sequence-view > 8")),
        "{:?}",
        why
    );
    assert_eq!(
        ok(
            "viewWithinLimit",
            serde_json::json!({"view": "view:sequence/narrow", "limit": "participants-per-sequence-view"})
        ),
        None
    );
    assert_eq!(
        ok(
            "viewWithinLimit",
            serde_json::json!({"view": "view:sequence/wide", "limit": "members-per-flow-view"})
        ),
        None
    );
    assert!(ok(
        "viewWithinLimit",
        serde_json::json!({"view": "view:sequence/wide", "limit": "no-such-limit"})
    )
    .is_some());
    assert!(ok(
        "viewWithinLimit",
        serde_json::json!({"view": "view:sequence/none", "limit": "edges-per-view"})
    )
    .is_some());
}

#[test]
fn diagnostic_exists_takes_every_listed_subject() {
    let mut s = Store::default();
    s.graph.diagnostics.insert(
        "diag:contradiction-1".into(),
        Diagnostic {
            rule: "contradiction".into(),
            severity: "error".into(),
            subjects: vec!["req:a".into(), "req:b".into()],
            message: "cannot both hold".into(),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt: None,
            answer: None,
            created: None,
            updated: None,
        },
    );
    let ok = |a: Value| eval_check("diagnosticExists", &a, &s, 0);
    assert_eq!(ok(serde_json::json!({"rule": "contradiction"})), None);
    assert_eq!(
        ok(serde_json::json!({"rule": "contradiction", "subject": "req:a"})),
        None
    );
    assert_eq!(
        ok(serde_json::json!({"rule": "contradiction", "subjects": ["req:a", "req:b"]})),
        None
    );
    assert!(
        ok(serde_json::json!({"rule": "contradiction", "subjects": ["req:a", "req:c"]})).is_some()
    );
    assert!(ok(
        serde_json::json!({"rule": "duplicate-requirement", "subjects": ["req:a", "req:b"]})
    )
    .is_some());
}

#[test]
fn patterns_are_regexes_matched_case_insensitively() {
    let re = |p: &str| compile(p).unwrap();
    assert!(re("empt(y|ies|ied)").is_match("the system shall EMPTY the Cart"));
    assert!(re("^--|/|\\.md").is_match("--api-key"));
    assert!(re("^--|/|\\.md").is_match("src/link.rs"));
    assert!(re("^--|/|\\.md").is_match("notes.md"));
    assert!(!re("^--|/|\\.md").is_match("Shopping Cart"));
    assert!(re("^addProduct$").is_match("addproduct"));
    // An invalid pattern is a check failure, never a silent pass.
    assert!(compile("(unclosed").is_err());
}
