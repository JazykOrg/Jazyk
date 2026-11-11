// The sweep, gc, the settle passes, check diagnostics, and record pruning.
use super::*;

// The sweep's dissolve rule: a derived grouping with fewer than two children
// dissolves, its children reparent to its parent, a tombstone redirect to the
// parent stays, and the gc entry carries the dissolve mutation. A grouping with
// two children and a stated entity with one child are untouched.
// Mirrors docs/compiler/graph.md#the-sweep.
#[test]
fn sweep_dissolves_an_under_membered_grouping_with_a_redirect() {
    let mut s = Store {
        out: own_dir("dissolve"),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nthe backend\n");
    let ents = [
        ("ent:infra", grouping("Infra", &["ent:backend"], None)),
        (
            "ent:backend",
            stated("Backend", "the backend", Some("ent:infra")),
        ),
        (
            "ent:storage",
            grouping("Storage", &["ent:cache"], Some("ent:backend")),
        ),
        (
            "ent:cache",
            stated("Cache", "the backend", Some("ent:storage")),
        ),
        (
            "ent:messaging",
            grouping("Messaging", &["ent:queue", "ent:db"], Some("ent:backend")),
        ),
        (
            "ent:queue",
            stated("Queue", "the backend", Some("ent:messaging")),
        ),
        ("ent:db", stated("Db", "the backend", Some("ent:messaging"))),
        (
            "ent:shell",
            stated("Shell", "the backend", Some("ent:backend")),
        ),
        ("ent:core", stated("Core", "the backend", Some("ent:shell"))),
    ];
    for (id, e) in ents {
        s.graph.entities.insert(id.into(), e);
    }
    let actions = s.gc();
    assert!(
        actions
            .iter()
            .any(|a| a.starts_with("dissolved ent:storage")),
        "{:?}",
        actions
    );
    assert!(
        actions.iter().any(|a| a.starts_with("dissolved ent:infra")),
        "{:?}",
        actions
    );
    assert!(!s.graph.entities.contains_key("ent:storage"));
    assert!(!s.graph.entities.contains_key("ent:infra"));
    assert_eq!(
        s.graph.entities["ent:cache"].parent.as_deref(),
        Some("ent:backend")
    );
    assert_eq!(s.graph.entities["ent:backend"].parent, None);
    // The tombstone redirects to the parent, so the old id resolves there; a
    // top-level grouping leaves a dead tombstone.
    assert_eq!(s.graph.redirects["ent:storage"], "ent:backend");
    assert_eq!(s.resolve_id("ent:storage"), "ent:backend");
    assert_eq!(s.graph.redirects["ent:infra"], "");
    assert!(s.graph.entities.contains_key("ent:messaging"));
    assert_eq!(
        s.graph.entities["ent:queue"].parent.as_deref(),
        Some("ent:messaging")
    );
    assert!(s.graph.entities.contains_key("ent:shell"));
    assert_eq!(
        s.graph.entities["ent:core"].parent.as_deref(),
        Some("ent:shell")
    );
    assert!(s.status.has_change(CHANGE_ENTITY_DELETED, "ent:storage"));
    // The move is the parents' business: the backend gained the cache; the
    // cache itself is not reviewed for a move alone.
    assert!(s.status.has_change(CHANGE_ENTITY, "ent:backend"));
    assert!(!s.status.has_change(CHANGE_ENTITY, "ent:cache"));
    assert!(!s.status.has_change(CHANGE_REPARENT_FLIP, "ent:cache"));
    let entry = journal_entry(&s, s.status.generation);
    assert_eq!(entry.kind, "gc");
    let dissolve = entry
        .mutations
        .iter()
        .find(|m| m["op"] == "dissolve_entity" && m["id"] == "ent:storage")
        .expect("dissolve mutation");
    assert_eq!(dissolve["parent"], "ent:backend");
    assert_eq!(dissolve["children"], serde_json::json!(["ent:cache"]));
    assert_eq!(
        s.journaled_parent_moves(),
        vec![
            ParentMove {
                generation: entry.generation,
                child: "ent:backend".into(),
                from: Some("ent:infra".into()),
                to: None,
            },
            ParentMove {
                generation: entry.generation,
                child: "ent:cache".into(),
                from: Some("ent:storage".into()),
                to: Some("ent:backend".into()),
            },
        ]
    );
    // A second sweep is a no-op.
    assert!(s.gc().is_empty());
    // The tool path: a stated entity is refused; a grouping dissolves the same way.
    let r = s.apply(
        vec![Op::DissolveEntity {
            id: "ent:shell".into(),
            reason: "test".into(),
            parent: None,
            children: Vec::new(),
        }],
        &session(),
    );
    assert!(
        r.skipped.iter().any(|x| x.contains("stated-entity")),
        "{:?}",
        r.skipped
    );
    assert!(s.graph.entities.contains_key("ent:shell"));
    let r = s.apply(
        vec![Op::DissolveEntity {
            id: "ent:messaging".into(),
            reason: "the two belong beside the cache".into(),
            parent: None,
            children: Vec::new(),
        }],
        &session(),
    );
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    assert_eq!(s.resolve_id("ent:messaging"), "ent:backend");
    assert_eq!(
        s.graph.entities["ent:queue"].parent.as_deref(),
        Some("ent:backend")
    );
    assert!(s.status.has_change(CHANGE_ENTITY_DELETED, "ent:messaging"));
    let entry = journal_entry(&s, r.generation);
    assert_eq!(entry.mutations[0]["op"], "dissolve_entity");
    assert_eq!(
        entry.mutations[0]["children"],
        serde_json::json!(["ent:db", "ent:queue"])
    );
}

#[test]
fn gc_removes_unanchored_and_spares_derived() {
    let mut s = Store {
        out: own_dir("gc"),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nbody\n");
    s.graph.requirements.insert(
        "req:gone-1".into(),
        Requirement {
            statement: "The X ys.".into(),
            entities: vec!["ent:x".into()],
            source: Some(mention("gone.md", "/gone", "x")),
            ..Default::default()
        },
    );
    s.graph.entities.insert(
        "ent:x".into(),
        Entity {
            name: "X".into(),
            mentions: vec![mention("gone.md", "/gone", "x")],
            ..Default::default()
        },
    );
    s.graph.entities.insert(
        "ent:invented".into(),
        Entity {
            name: "Invented".into(),
            provenance: Some(derived(&["ent:x"])),
            ..Default::default()
        },
    );
    // A derived node holding requirements is a caps-variant sub-entity, never a
    // grouping: the dissolve rule leaves it to judgment.
    s.graph.requirements.insert(
        "req:t-1".into(),
        Requirement {
            statement: "The invented thing holds.".into(),
            entities: vec!["ent:invented".into()],
            source: Some(mention("t.md", "/t", "body")),
            ..Default::default()
        },
    );
    s.graph.views.insert(
        "view:class/kept".into(),
        View {
            kind: "class".into(),
            title: "Kept".into(),
            members: vec!["ent:x".into()],
            provenance: Some(derived(&["ent:x"])),
            ..Default::default()
        },
    );
    let actions = s.gc();
    assert!(actions.len() >= 2, "{:?}", actions);
    assert!(!s.graph.requirements.contains_key("req:gone-1"));
    assert!(s.graph.requirements.contains_key("req:t-1"));
    assert!(!s.graph.entities.contains_key("ent:x"));
    assert_eq!(s.graph.redirects["ent:x"], "");
    // Derived nodes are judgment's, not the sweep's; the ones citing the dead node
    // and the curated view listing it owe a retrace.
    assert!(s.graph.entities.contains_key("ent:invented"));
    let node = s
        .status
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_NODE_DELETED && c.subject == "ent:invented")
        .expect("node-deleted");
    assert_eq!(node.via, "from");
    assert!(s
        .status
        .has_change(CHANGE_VIEW_MEMBER_GONE, "view:class/kept"));
    assert!(s.status.has_change(CHANGE_ENTITY_DELETED, "ent:x"));
    let entry: JournalEntry =
        yaml_to(&crate::layout::journal_dir(&s.out).join(format!("g{}.yaml", s.status.generation)))
            .unwrap();
    assert_eq!(entry.kind, "gc");
    // A second sweep is a no-op: the standing record is not rewritten.
    let gen = s.status.generation;
    assert!(s.gc().is_empty());
    assert_eq!(s.status.generation, gen);
}

// The sweep runs at every commit, not only inside a build: a decree committed
// through `apply` on a store holding an unanchored requirement lands its own
// entry, then the sweep lands a `gc` entry behind it and the report says so.
// Mirrors docs/compiler/graph.md#changesets and #the-sweep.
#[test]
fn every_commit_runs_the_sweep_behind_its_own_entry() {
    let mut s = Store {
        out: own_dir("sweep-at-commit"),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nbody\n");
    s.graph.requirements.insert(
        "req:gone-1".into(),
        Requirement {
            statement: "The X ys.".into(),
            entities: vec!["ent:x".into()],
            source: Some(mention("gone.md", "/gone", "x")),
            ..Default::default()
        },
    );
    s.graph.entities.insert(
        "ent:x".into(),
        Entity {
            name: "X".into(),
            mentions: vec![mention("gone.md", "/gone", "x")],
            ..Default::default()
        },
    );
    let report = s.apply(vec![create("ent:fresh", "Fresh")], &Commit::store("decree"));
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert!(
        report.swept.iter().any(|a| a.contains("req:gone-1")),
        "the sweep ran behind the decree: {:?}",
        report.swept
    );
    assert!(!s.graph.requirements.contains_key("req:gone-1"));
    assert!(!s.graph.entities.contains_key("ent:x"));
    assert!(s.graph.entities.contains_key("ent:fresh"));
    // The decree's own entry, then the sweep's, each a generation.
    let decree = journal_entry(&s, report.generation);
    assert_eq!(decree.kind, "decree");
    let gc = journal_entry(&s, report.generation + 1);
    assert_eq!(gc.kind, "gc");
    assert_eq!(s.status.generation, report.generation + 1);
    // A clean store commits without a sweep entry.
    let report = s.apply(vec![create("ent:other", "Other")], &Commit::store("decree"));
    assert!(report.swept.is_empty(), "{:?}", report.swept);
    assert_eq!(s.status.generation, report.generation);
}

#[test]
fn check_diags_reconcile_not_regenerate() {
    let mut s = Store {
        out: own_dir("checks"),
        ..Default::default()
    };
    s.reconcile_check_diags(vec![(
        "uncovered-section".into(),
        vec!["t.md#/t".into()],
        "warning".into(),
        "section /t is unprocessed".into(),
        None,
    )]);
    assert_eq!(s.graph.diagnostics.len(), 1);
    assert_eq!(s.status.generation, 1);
    let entry: JournalEntry = yaml_to(&crate::layout::journal_dir(&s.out).join("g1.yaml")).unwrap();
    assert_eq!(entry.kind, "checks");
    assert_eq!(entry.mutations[0]["op"], "report_diagnostic");
    let id = s.graph.diagnostics.keys().next().unwrap().clone();
    // Same finding again: same id, no duplicate, no generation.
    s.reconcile_check_diags(vec![(
        "uncovered-section".into(),
        vec!["t.md#/t".into()],
        "warning".into(),
        "section /t is unprocessed".into(),
        None,
    )]);
    assert_eq!(s.graph.diagnostics.len(), 1);
    assert!(s.graph.diagnostics.contains_key(&id));
    assert_eq!(s.status.generation, 1);
    // A prompt landing on a finding opens an answer.
    s.reconcile_check_diags(vec![(
        "uncovered-section".into(),
        vec!["t.md#/t".into()],
        "warning".into(),
        "section /t is unprocessed".into(),
        Some(crate::model::DiagnosticPrompt {
            question: "skip it?".into(),
            options: Vec::new(),
            freeform: true,
        }),
    )]);
    assert!(s.status.has_change(CHANGE_PROMPT_UNANSWERED, &id));
    // Finding cleared: resolved, not deleted, and the answer debt goes with it.
    s.reconcile_check_diags(vec![]);
    assert_eq!(s.graph.diagnostics[&id].lifecycle, "resolved");
    assert!(!s.status.has_change(CHANGE_PROMPT_UNANSWERED, &id));
    assert_eq!(s.status.generation, 3);
}

#[test]
fn pair_diagnostic_sticky_regardless_of_subject_order() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    let d = |subjects: Vec<String>| Diagnostic {
        goal: None,
        rule: "contradiction".into(),
        severity: "warning".into(),
        subjects,
        message: "conflict".into(),
        reasoning: None,
        lifecycle: "open".into(),
        triage: None,
        prompt: None,
        answer: None,
        created: None,
        updated: None,
    };
    // The subjects exist: the sweep behind every commit settles a finding whose
    // subjects are gone.
    for id in ["req:a", "req:b"] {
        s.graph.requirements.insert(
            id.into(),
            Requirement {
                statement: format!("{} holds.", id),
                ..Default::default()
            },
        );
    }
    s.apply(
        vec![Op::ReportDiagnostic {
            id: String::new(),
            diagnostic: d(vec!["req:a".into(), "req:b".into()]),
        }],
        &session(),
    );
    // The same pair reported from the other endpoint updates the finding in place.
    s.apply(
        vec![Op::ReportDiagnostic {
            id: String::new(),
            diagnostic: d(vec!["req:b".into(), "req:a".into()]),
        }],
        &session(),
    );
    assert_eq!(s.graph.diagnostics.len(), 1);
}

#[test]
fn answered_prompt_is_never_reasked_and_resolve_marks_handled() {
    use crate::model::{DiagnosticAnswer, DiagnosticPrompt};
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    let prompt = |q: &str| DiagnosticPrompt {
        question: q.into(),
        options: Vec::new(),
        freeform: true,
    };
    let with_prompt = |q: Option<&str>| Diagnostic {
        goal: None,
        rule: "contradiction".into(),
        severity: "warning".into(),
        subjects: vec!["req:a".into(), "req:b".into()],
        message: "conflict".into(),
        reasoning: None,
        lifecycle: "open".into(),
        triage: None,
        prompt: q.map(prompt),
        answer: None,
        created: None,
        updated: None,
    };
    for id in ["req:a", "req:b"] {
        s.graph.requirements.insert(
            id.into(),
            Requirement {
                statement: format!("{} holds.", id),
                ..Default::default()
            },
        );
    }
    let r = s.apply(
        vec![Op::ReportDiagnostic {
            id: String::new(),
            diagnostic: with_prompt(Some("which?")),
        }],
        &session(),
    );
    let id = s.graph.diagnostics.keys().next().unwrap().clone();
    // A prompt landing opens an answer; its cause is the report.
    let unanswered = r
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_PROMPT_UNANSWERED)
        .unwrap();
    assert_eq!(
        (unanswered.subject.as_str(), unanswered.via.as_str()),
        (id.as_str(), "report_diagnostic")
    );
    // A promptless re-report keeps the question; a fresh one replaces it.
    s.apply(
        vec![Op::ReportDiagnostic {
            id: String::new(),
            diagnostic: with_prompt(None),
        }],
        &session(),
    );
    assert_eq!(
        s.graph.diagnostics[&id].prompt.as_ref().unwrap().question,
        "which?"
    );
    s.apply(
        vec![Op::ReportDiagnostic {
            id: String::new(),
            diagnostic: with_prompt(Some("sharper?")),
        }],
        &session(),
    );
    assert_eq!(
        s.graph.diagnostics[&id].prompt.as_ref().unwrap().question,
        "sharper?"
    );
    // Once answered, a re-report never re-asks, and the answer debt is paid.
    s.apply(
        vec![Op::AnswerDiagnostic {
            id: id.clone(),
            answer: DiagnosticAnswer {
                choice: None,
                text: "both".into(),
                status: "handling".into(),
                error: None,
            },
        }],
        &Commit::store("answer"),
    );
    assert!(!s.status.has_change(CHANGE_PROMPT_UNANSWERED, &id));
    s.apply(
        vec![Op::ReportDiagnostic {
            id: String::new(),
            diagnostic: with_prompt(Some("again?")),
        }],
        &session(),
    );
    assert_eq!(
        s.graph.diagnostics[&id].prompt.as_ref().unwrap().question,
        "sharper?"
    );
    // Resolving while handling is the handling session finishing.
    s.apply(
        vec![Op::ResolveDiagnostic {
            id: id.clone(),
            reason: "settled".into(),
        }],
        &session(),
    );
    let d = &s.graph.diagnostics[&id];
    assert_eq!(d.lifecycle, "resolved");
    assert_eq!(d.answer.as_ref().unwrap().status, "handled");
}

// Deleting one side of a filed contradiction settles or re-judges: a surviving
// subject gets a node-deleted record, a diagnostic with no subject left resolves.
// Mirrors docs/compiler/reconciler.md#pairs.
#[test]
fn deleting_a_subject_settles_or_rejudges_its_diagnostics() {
    let mut s = Store {
        out: own_dir("propagate"),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nThe Cart holds items.\n");
    s.graph.entities.insert(
        "ent:cart".into(),
        Entity {
            name: "Cart".into(),
            mentions: vec![mention("t.md", "/t", "The Cart holds items.")],
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:t-1".into(),
        seeded_req("t.md", "The Cart holds items."),
    );
    s.graph.requirements.insert(
        "req:t-2".into(),
        seeded_req("t.md", "The Cart stays empty."),
    );
    s.apply(
        vec![Op::ReportDiagnostic {
            id: String::new(),
            diagnostic: diag("contradiction", vec!["req:t-1", "req:t-2"]),
        }],
        &session(),
    );
    let did = s.graph.diagnostics.keys().next().unwrap().clone();
    s.status.changes.clear();

    // One subject deleted: the diagnostic stands, the survivor owes a re-judgment.
    let r = s.apply(
        vec![Op::DeleteRequirement {
            id: "req:t-1".into(),
            reason: "fact gone".into(),
        }],
        &session(),
    );
    assert_eq!(s.graph.diagnostics[&did].lifecycle, "open");
    let node = r
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_NODE_DELETED && c.subject == "req:t-2")
        .expect("node-deleted on the survivor");
    assert_eq!(node.via, "subjects");
    assert_eq!(node.mutation, 1);
    assert_eq!(node.detail["diagnostic"], did);
    assert!(s
        .status
        .changed_subjects(&REQ_REVIEW_KINDS)
        .contains(&"req:t-2".to_string()));
    assert!(s.status.has_change(CHANGE_REQ_DELETED, "req:t-1"));

    // The open diagnostic alone keeps the survivor's pair judgment due, with no
    // computed neighbor left.
    assert!(s.pair_review_neighbors("req:t-2").is_empty());
    assert!(s.pair_review_due("req:t-2"));

    // Every subject deleted: the store resolves the diagnostic itself.
    s.apply(
        vec![Op::DeleteRequirement {
            id: "req:t-2".into(),
            reason: "fact gone".into(),
        }],
        &session(),
    );
    assert_eq!(s.graph.diagnostics[&did].lifecycle, "resolved");
    assert!(!s.status.has_change(CHANGE_NODE_DELETED, "req:t-2"));
}

// A graph deleted into a stranded state heals at the deterministic tail.
#[test]
fn settle_dangling_diags_heals_a_stranded_graph() {
    let mut s = Store {
        out: own_dir("settle"),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nThe Cart holds items.\n");
    s.graph.entities.insert("ent:cart".into(), entity("Cart"));
    s.graph.requirements.insert(
        "req:t-2".into(),
        seeded_req("t.md", "The Cart stays empty."),
    );
    s.graph.diagnostics.insert(
        "diag:contradiction-1".into(),
        diag("contradiction", vec!["req:gone", "req:t-2"]),
    );
    s.graph.diagnostics.insert(
        "diag:contradiction-2".into(),
        diag("contradiction", vec!["req:gone-a", "req:gone-b"]),
    );
    assert!(s.has_dangling_diags());

    let actions = s.settle_dangling_diags();
    // All subjects gone: resolved by the store, with a journaled action.
    assert_eq!(
        s.graph.diagnostics["diag:contradiction-2"].lifecycle,
        "resolved"
    );
    assert!(
        actions.iter().any(|a| a.contains("diag:contradiction-2")),
        "{:?}",
        actions
    );
    let entry: JournalEntry =
        yaml_to(&crate::layout::journal_dir(&s.out).join(format!("g{}.yaml", s.status.generation)))
            .unwrap();
    assert_eq!(entry.kind, "settle-diagnostics");
    // A survivor remains: the diagnostic stands and the survivor owes a judgment.
    assert_eq!(
        s.graph.diagnostics["diag:contradiction-1"].lifecycle,
        "open"
    );
    assert_eq!(
        s.status.changed_subjects(&REQ_REVIEW_KINDS),
        vec!["req:t-2".to_string()]
    );
    // Idempotent: a second sweep resolves nothing new.
    assert!(s.settle_dangling_diags().is_empty());
}

// The marker diagnostics settle at the sweep once their condition clears: an
// incomplete-build marker whose goal left parked, an uncovered-section marker whose
// section is marked or gone. Mirrors docs/compiler/graph.md#the-sweep.
#[test]
fn sweep_settles_cleared_markers() {
    let mut s = Store {
        out: own_dir("markers"),
        ..Default::default()
    };
    seed_doc(
        &mut s,
        "t.md",
        "# T\n\nThe Cart holds items.\n\n## A\n\nA body.\n\n## B\n\nB body.\n\n## C\n\nC body.\n",
    );
    let refs: Vec<String> = s.docs["t.md"].sections.keys().cloned().collect();
    assert_eq!(refs.len(), 4, "{:?}", refs);
    let mark = |s: &mut Store, r: &str, state: &str| {
        s.docs.get_mut("t.md").unwrap().coverage.insert(
            r.to_string(),
            Coverage {
                state: state.into(),
                note: None,
                claimed_by: None,
            },
        );
    };
    mark(&mut s, &refs[1], "covered");
    mark(&mut s, &refs[2], "non-normative");
    let marker = |rule: &str, subject: &str| Diagnostic {
        goal: None,
        rule: rule.into(),
        severity: "warning".into(),
        subjects: vec![subject.into()],
        message: "marker".into(),
        reasoning: None,
        lifecycle: "open".into(),
        triage: None,
        prompt: None,
        answer: None,
        created: None,
        updated: None,
    };
    for (i, r) in refs.iter().enumerate() {
        s.graph.diagnostics.insert(
            format!("diag:uncovered-section-{}", i),
            marker("uncovered-section", &format!("t.md#{}", r)),
        );
    }
    s.graph.diagnostics.insert(
        "diag:uncovered-section-9".into(),
        marker("uncovered-section", "t.md#/t/gone"),
    );
    s.status.parked.push(
        Goal {
            id: "g:reconcile-section:t.md#/t/c".into(),
            kind: "reconcile-section".into(),
            target: format!("t.md#{}", refs[3]),
            ..Default::default()
        }
        .into(),
    );
    s.graph.diagnostics.insert(
        "diag:incomplete-build-1".into(),
        marker("incomplete-build", &format!("t.md#{}", refs[3])),
    );
    s.graph.diagnostics.insert(
        "diag:incomplete-build-2".into(),
        marker("incomplete-build", &format!("t.md#{}", refs[1])),
    );
    assert!(s.has_dangling_diags());

    let actions = s.settle_cleared_markers();
    let lc = |s: &Store, id: &str| s.graph.diagnostics[id].lifecycle.clone();
    // Unmarked sections keep their marker; covered, non-normative, and gone resolve.
    assert_eq!(lc(&s, "diag:uncovered-section-0"), "open");
    assert_eq!(lc(&s, "diag:uncovered-section-1"), "resolved");
    assert_eq!(lc(&s, "diag:uncovered-section-2"), "resolved");
    assert_eq!(lc(&s, "diag:uncovered-section-3"), "open");
    assert_eq!(lc(&s, "diag:uncovered-section-9"), "resolved");
    // The goal still parked keeps its marker; the resumed one's resolves.
    assert_eq!(lc(&s, "diag:incomplete-build-1"), "open");
    assert_eq!(lc(&s, "diag:incomplete-build-2"), "resolved");
    assert_eq!(actions.len(), 4, "{:?}", actions);
    let entry: JournalEntry =
        yaml_to(&crate::layout::journal_dir(&s.out).join(format!("g{}.yaml", s.status.generation)))
            .unwrap();
    assert_eq!(entry.kind, "settle-diagnostics");
    assert_eq!(entry.mutations.len(), 4);
    // Idempotent, and the goal leaving parked clears the last marker next sweep.
    assert!(s.settle_cleared_markers().is_empty());
    s.status.parked.clear();
    let actions = s.settle_cleared_markers();
    assert_eq!(actions.len(), 1, "{:?}", actions);
    assert_eq!(lc(&s, "diag:incomplete-build-1"), "resolved");
    assert!(!s.has_dangling_diags());
}

// Check reconciliation resolves only its own single-subject findings; a judged
// pair filed under a shared rule name is a session's to resolve.
#[test]
fn check_reconcile_leaves_judged_pairs_alone() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    s.graph.diagnostics.insert(
        "diag:duplicate-requirement-1".into(),
        diag("duplicate-requirement", vec!["req:a", "req:b"]),
    );
    s.reconcile_check_diags(Vec::new());
    assert_eq!(
        s.graph.diagnostics["diag:duplicate-requirement-1"].lifecycle,
        "open"
    );
}

// (b) and (d) of the pair degree cap: an accepted verdict marks the partner
// judged on both sides' records at the commit, the slot it frees opens the next
// waiting pair by score, a created record stands while its pair waits, and it
// clears at the sweep when no candidate remains.
// Mirrors docs/compiler/reconciler.md#pairs and graph.md#change-records.
#[test]
fn resolving_a_pair_marks_judged_and_opens_the_next_waiting_pair() {
    let mut s = crate::goals::tests::fanned_store();
    s.out = own_dir("paircap");
    let pair = |p: &str| format!("g:rejudge-pair:{}", crate::goals::pair_target("req:hub", p));
    let plan = crate::goals::pair_plan(&s);
    assert!(plan.waiting.contains_key(&pair("req:p08")));
    // An unrelated commit: the sweep keeps every record, the waiting pair's
    // record among them.
    s.apply(vec![], &Commit::store("checks"));
    assert!(s.status.has_change(CHANGE_REQ_CREATED, "req:p08"));
    assert!(s.status.has_change(CHANGE_REQ_CREATED, "req:p12"));
    // The session resolves the hub against req:p12 (score 4, opened first).
    let mut commit = Commit::session(vec![pair("req:p12")], 3, None);
    commit.resolved.push(Resolved {
        goal: pair("req:p12"),
        justification: "consistent: different obligations".into(),
        evidence: serde_json::json!({"verdict": "consistent"}),
    });
    let report = s.apply(vec![], &commit);
    assert_eq!(report.generation, 7);
    // req:p12's record listed the hub as its only candidate: judged, then swept.
    assert!(!s.status.has_change(CHANGE_REQ_CREATED, "req:p12"));
    // The hub has no record of its own; nothing to mark there. Every other
    // partner's record stands untouched, req:p08's among them.
    assert!(s.status.has_change(CHANGE_REQ_CREATED, "req:p08"));
    assert!(s
        .status
        .changes
        .iter()
        .filter(|c| c.kind == CHANGE_REQ_CREATED)
        .all(|c| c.judged().is_empty()));
    // The slot opens the next waiting pair by score, then target: req:p08.
    let plan = crate::goals::pair_plan(&s);
    assert!(
        plan.open.iter().any(|g| g.id == pair("req:p08")),
        "{:?}",
        plan.open.iter().map(|g| &g.id).collect::<Vec<_>>()
    );
    assert!(plan.open.iter().all(|g| g.id != pair("req:p12")));
    assert_eq!(plan.waiting.len(), 3);
    assert!(plan.waiting.contains_key(&pair("req:p09")));
    // The journal entry carries the resolution and the persisted status agrees.
    let entry: JournalEntry = yaml_to(&crate::layout::journal_dir(&s.out).join("g7.yaml")).unwrap();
    assert_eq!(entry.resolved_goals.len(), 1);
    assert!(!Store::load(&s.out)
        .status
        .has_change(CHANGE_REQ_CREATED, "req:p12"));
    // A record with two candidates keeps standing after one is judged: give the
    // hub a record, resolve it against req:p01, and the hub's record lists the
    // partner while the other candidates keep it alive.
    let mut batch = RecordBatch::new(8);
    batch.push(
        0,
        CHANGE_REQ_REVISED,
        "req:hub",
        "fields",
        serde_json::Value::Null,
    );
    for r in batch.records() {
        s.status.record_change(r.clone());
    }
    let mut commit = Commit::session(vec![pair("req:p01")], 3, None);
    commit.resolved.push(Resolved {
        goal: pair("req:p01"),
        justification: "consistent".into(),
        evidence: serde_json::json!({"verdict": "consistent"}),
    });
    s.apply(vec![], &commit);
    let hub = s
        .status
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_REQ_REVISED && c.subject == "req:hub")
        .expect("the hub's record stands on its other candidates");
    assert_eq!(hub.judged(), vec!["req:p01".to_string()]);
    assert!(!s.status.has_change(CHANGE_REQ_CREATED, "req:p01"));
    let plan = crate::goals::pair_plan(&s);
    assert!(plan.open.iter().all(|g| g.id != pair("req:p01")));
    // A failed pair marks nothing judged.
    let mut commit = Commit::session(vec![pair("req:p02")], 3, None);
    commit.resolved.clear();
    s.apply(vec![], &commit);
    assert!(s
        .status
        .changes
        .iter()
        .find(|c| c.subject == "req:p02")
        .is_some_and(|c| c.judged().is_empty()));
    crate::vfs::remove_dir_all(&s.out).ok();
}

// The sweep over a large store: the scorer's inputs are computed once per pass,
// so a pass over thousands of created records costs one walk of the requirements
// per record. Mirrors docs/compiler/reconciler.md#pairs.
#[test]
fn prune_records_scores_thousands_of_records_in_one_pass() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    let verbs = [
        "accepts",
        "rejects",
        "stores",
        "renders",
        "schedules",
        "archives",
    ];
    let nouns = ["order", "invoice", "ticket", "report", "session", "token"];
    for e in 0..300 {
        s.graph
            .entities
            .insert(format!("ent:e{}", e), entity(&format!("Entity {}", e)));
    }
    for r in 0..3000 {
        let a = r % 300;
        let b = (r * 7 + 1) % 300;
        s.graph.requirements.insert(
            format!("req:r{}", r),
            Requirement {
                statement: format!(
                    "Entity {} {} the {} of entity {} within {} days.",
                    a,
                    verbs[r % verbs.len()],
                    nouns[(r / 3) % nouns.len()],
                    b,
                    r % 30
                ),
                entities: vec![format!("ent:e{}", a), format!("ent:e{}", b)],
                ..Default::default()
            },
        );
        s.status.changes.push(ChangeRecord::new(
            1,
            r + 1,
            1,
            CHANGE_REQ_CREATED,
            &format!("req:r{}", r),
            "statement",
        ));
    }
    let before = s.status.changes.len();
    // One pass computes the scorer's inputs once for every record; the count is
    // the deterministic check. The wall clock only catches a pathological pass, so
    // its bound stays far above any machine under parallel test load.
    let scorings = PAIR_SCORINGS.with(|n| n.get());
    let started = std::time::Instant::now();
    s.prune_records();
    let took = started.elapsed();
    assert_eq!(
        PAIR_SCORINGS.with(|n| n.get()) - scorings,
        1,
        "prune_records over {} records scored more than once",
        before
    );
    assert!(
        took < std::time::Duration::from_secs(120),
        "prune_records over {} records took {:?}",
        before,
        took
    );
    // Every record stands or clears exactly as the per-record scorer says.
    let scoring = s.pair_scoring();
    for r in 0..10 {
        let rid = format!("req:r{}", r);
        let stands = s.status.changes.iter().any(|c| c.subject == rid);
        let rec = ChangeRecord::new(1, r + 1, 1, CHANGE_REQ_CREATED, &rid, "statement");
        let candidates = s.pair_review_candidates_with(&rec, &scoring);
        assert_eq!(stands, !candidates.is_empty(), "{}", rid);
        assert_eq!(candidates, s.pair_review_candidates(&rec), "{}", rid);
    }
    crate::vfs::remove_dir_all(&s.out).ok();
}
