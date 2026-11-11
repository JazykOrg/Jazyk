// Dual writes, answers carrying edits, and the ratification paths.
use super::*;

// A dual write lands the prose and the graph as one changeset, absorbs its own
// hashes so the document is not dirty against the graph, re-anchors the quoted
// facts in the section, and puts the file back when the commit skips.
// Mirrors docs/compiler/compilation.md#edit-paths.
// An answer whose option carried a suggested edit is a human edit too: the commit
// lands as `answer` with the prose edit and writes no `entity-changed`.
// Mirrors docs/compiler/compilation.md#edit-paths.
#[test]
fn an_answer_carrying_an_edit_writes_no_entity_changed() {
    let root = own_dir("answer-edit");
    let text = "# Pay\n\nAn Order is paid within 30 days.\n";
    crate::vfs::create_dir_all(root.join("docs")).unwrap();
    crate::vfs::write(root.join("docs/pay.md"), text).unwrap();
    let mut s = Store {
        out: root.join(".jazyk"),
        ..Default::default()
    };
    let mut parsed = BTreeMap::new();
    parsed.insert(
        "docs/pay.md".to_string(),
        (hash_hex(text), crate::md::parse_sections(text)),
    );
    s.sync_docs(&parsed);
    s.apply(
        vec![
            create("ent:order", "Order"),
            Op::CreateRequirement {
                id: "req:pay-1".into(),
                requirement: Requirement {
                    statement: "An Order is paid within 30 days.".into(),
                    entities: vec!["ent:order".into()],
                    source: Some(mention(
                        "docs/pay.md",
                        "/pay",
                        "An Order is paid within 30 days.",
                    )),
                    ..Default::default()
                },
            },
        ],
        &session(),
    );
    let raw = s.docs["docs/pay.md"].sections["/pay"].raw.clone();
    let edit = ProseEdit::locate(
        "docs/pay.md",
        "/pay",
        Some(&raw),
        text,
        "within 30 days",
        "within 21 days",
    )
    .unwrap();
    let setup: Vec<String> = s.status.changes.iter().map(|c| c.id.clone()).collect();
    s.clear_changes(&setup);
    let report = s
        .dual_write(
            &root,
            &edit,
            vec![Op::UpdateRequirement {
                id: "req:pay-1".into(),
                statement: Some("An Order is paid within 21 days.".into()),
                entities: None,
                edges: None,
                transition: None,
                facets: None,
                source: Some(mention(
                    "docs/pay.md",
                    "/pay",
                    "An Order is paid within 21 days.",
                )),
                provenance: None,
            }],
            &Commit::store("answer"),
            None,
        )
        .unwrap();
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert!(
        !s.status
            .changes
            .iter()
            .any(|c| c.kind == CHANGE_ENTITY && c.subject == "ent:order"),
        "{:?}",
        s.status.changes
    );
    assert!(
        s.status
            .changes
            .iter()
            .any(|c| c.kind == CHANGE_REQ_REVISED && c.subject == "req:pay-1"),
        "{:?}",
        s.status.changes
    );
}

#[test]
fn dual_write_absorbs_its_own_hashes_and_rolls_back_a_stale_edit() {
    let root = own_dir("dual-write");
    let text = "# Pay\n\nAn Order is paid within 30 days.\n";
    crate::vfs::create_dir_all(root.join("docs")).unwrap();
    crate::vfs::write(root.join("docs/pay.md"), text).unwrap();
    let mut s = Store {
        out: root.join(".jazyk"),
        ..Default::default()
    };
    let parsed_of = |text: &str| {
        let mut m = BTreeMap::new();
        m.insert(
            "docs/pay.md".to_string(),
            (hash_hex(text), crate::md::parse_sections(text)),
        );
        m
    };
    s.sync_docs(&parsed_of(text));
    s.apply(
        vec![
            create("ent:order", "Order"),
            Op::CreateRequirement {
                id: "req:pay-1".into(),
                requirement: Requirement {
                    statement: "An Order is paid within 30 days.".into(),
                    entities: vec!["ent:order".into()],
                    source: Some(mention(
                        "docs/pay.md",
                        "/pay",
                        "An Order is paid within 30 days.",
                    )),
                    ..Default::default()
                },
            },
        ],
        &session(),
    );
    let before = s.status.generation;
    let raw = s.docs["docs/pay.md"].sections["/pay"].raw.clone();
    let edit = ProseEdit::locate(
        "docs/pay.md",
        "/pay",
        Some(&raw),
        text,
        "within 30 days",
        "within 21 days",
    )
    .unwrap();
    // The setup's own records go, so what the dual write writes stands alone.
    let setup: Vec<String> = s.status.changes.iter().map(|c| c.id.clone()).collect();
    s.clear_changes(&setup);
    let report = s
        .dual_write(
            &root,
            &edit,
            vec![Op::UpdateRequirement {
                id: "req:pay-1".into(),
                statement: Some("An Order is paid within 21 days.".into()),
                entities: None,
                edges: None,
                transition: None,
                facets: None,
                source: Some(mention(
                    "docs/pay.md",
                    "/pay",
                    "An Order is paid within 21 days.",
                )),
                provenance: None,
            }],
            &Commit::store("dual-write"),
            None,
        )
        .unwrap();
    assert_eq!(report.generation, before + 1, "one changeset");
    let on_disk = crate::vfs::read_to_string(root.join("docs/pay.md")).unwrap();
    assert!(on_disk.contains("within 21 days"), "{}", on_disk);
    assert_eq!(s.docs["docs/pay.md"].content_hash, hash_hex(&on_disk));
    let r = &s.graph.requirements["req:pay-1"];
    assert_eq!(
        r.source.as_ref().unwrap().quote,
        "An Order is paid within 21 days."
    );
    assert!(r.statement.contains("21 days"));
    // The entity mention derived from the sentence followed it.
    assert!(s.graph.entities["ent:order"]
        .mentions
        .iter()
        .any(|m| m.quote == "An Order is paid within 21 days."));
    // A human edit is the review: the mention opens no review of the Order, while
    // the revised statement keeps its pair judgment against other requirements.
    // Mirrors docs/compiler/compilation.md#edit-paths.
    assert!(
        !s.status.has_change(CHANGE_ENTITY, "ent:order"),
        "{:?}",
        s.status.changes
    );
    assert!(
        s.status.has_change(CHANGE_REQ_REVISED, "req:pay-1"),
        "{:?}",
        s.status.changes
    );
    let entry = journal_entry(&s, report.generation);
    assert_eq!(entry.kind, "dual-write");
    assert!(entry.mutations.iter().any(|m| m["op"] == "edit_doc_prose"));
    assert!(entry
        .mutations
        .iter()
        .any(|m| m["op"] == "update_requirement"));
    // No re-dirtying: a following sync reports nothing and writes no record.
    let records = s.status.changes.clone();
    let dirty = s.sync_docs(&parsed_of(&on_disk));
    assert!(dirty.is_empty(), "{:?}", dirty);
    assert_eq!(s.status.changes, records);
    // A stale edit (the old text no longer stands) skips whole; the file comes back.
    let stale = ProseEdit {
        doc: "docs/pay.md".into(),
        section: "/pay".into(),
        old_text: "within 30 days".into(),
        new_text: "within 7 days".into(),
        old_full: on_disk.clone(),
        full: on_disk.replace("21", "7"),
    };
    let err = s
        .dual_write(
            &root,
            &stale,
            vec![Op::DeleteRequirement {
                id: "req:pay-1".into(),
                reason: "x".into(),
            }],
            &Commit::store("dual-write"),
            None,
        )
        .unwrap_err();
    assert!(err.contains("edit-stale"), "{}", err);
    assert_eq!(
        crate::vfs::read_to_string(root.join("docs/pay.md")).unwrap(),
        on_disk
    );
    assert!(s.graph.requirements.contains_key("req:pay-1"));
    // A prose edit never lands alone.
    assert!(s
        .dual_write(&root, &edit, Vec::new(), &Commit::store("dual-write"), None)
        .is_err());
    // An empty old_text appends the sentence to the section's body.
    let on_disk = crate::vfs::read_to_string(root.join("docs/pay.md")).unwrap();
    let raw = s.docs["docs/pay.md"].sections["/pay"].raw.clone();
    let insert = ProseEdit::locate(
        "docs/pay.md",
        "/pay",
        Some(&raw),
        &on_disk,
        "",
        "A late Order is cancelled.",
    )
    .unwrap();
    assert!(
        insert
            .full
            .ends_with("within 21 days.\n\nA late Order is cancelled.\n"),
        "{}",
        insert.full
    );
    crate::vfs::remove_dir_all(&root).ok();
}

// A CRLF document locates its sections for a dual write the same way an LF one
// does, and the edit keeps CRLF. Mirrors docs/compiler/graph.md#mutations.
#[test]
fn a_crlf_document_takes_a_dual_write_in_its_section() {
    let lf =
        "# T\n\n## Alpha\nThe cart holds items.\nIt is shared.\n\n## Beta\nThe cart holds items.\n";
    let crlf = lf.replace('\n', "\r\n");
    let sections = crate::md::parse_sections(&crlf);
    let beta = sections["/t/beta"].raw.clone();
    let alpha = sections["/t/alpha"].raw.clone();
    // A replacement scoped to Beta never lands in Alpha's identical sentence.
    let edit = ProseEdit::locate(
        "t.md",
        "/t/beta",
        Some(&beta),
        &crlf,
        "The cart holds items.",
        "The cart holds goods.",
    )
    .unwrap();
    assert_eq!(
        edit.full,
        crlf.replace(
            "## Beta\r\nThe cart holds items.",
            "## Beta\r\nThe cart holds goods."
        )
    );
    // An append after a multi-line section lands right after it, in CRLF.
    let edit = ProseEdit::locate(
        "t.md",
        "/t/alpha",
        Some(&alpha),
        &crlf,
        "",
        "A cart expires.\nAfter a day.",
    )
    .unwrap();
    assert!(
        edit.full
            .contains("It is shared.\r\n\r\nA cart expires.\r\nAfter a day.\r\n\r\n## Beta"),
        "{:?}",
        edit.full
    );
    assert!(
        !edit.full.replace("\r\n", "").contains('\n'),
        "{:?}",
        edit.full
    );
}

// A derived requirement mints under stem x, lands a provenance-pending record, and
// ratifies to a quote in place; a decree retracts. Mirrors docs/compiler/compilation.md#edit-paths.
#[test]
fn derived_and_decreed_requirements_pend_ratify_and_retract() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nThe cart is split by category.\n");
    let r = s.apply(
        vec![
            create("ent:cart", "Cart"),
            Op::CreateRequirement {
                id: "prov:new".into(),
                requirement: Requirement {
                    statement: "The cart is split by category.".into(),
                    entities: vec!["ent:cart".into()],
                    provenance: Some(derived(&["ent:cart"])),
                    ..Default::default()
                },
            },
            Op::CreateRequirement {
                id: "prov:decree".into(),
                requirement: Requirement {
                    statement: "The cart never exceeds ten categories.".into(),
                    entities: vec!["ent:cart".into()],
                    provenance: Some(Provenance::Decree {
                        author: "owner".into(),
                        at: "now".into(),
                        note: None,
                    }),
                    ..Default::default()
                },
            },
            Op::CreateRequirement {
                id: "prov:none".into(),
                requirement: Requirement {
                    statement: "Nothing backs this.".into(),
                    entities: vec!["ent:cart".into()],
                    ..Default::default()
                },
            },
            Op::ReportDiagnostic {
                id: String::new(),
                diagnostic: Diagnostic {
                    goal: None,
                    rule: "ratification-pending".into(),
                    severity: "info".into(),
                    subjects: vec!["prov:new".into()],
                    message: "the docs should say so".into(),
                    reasoning: None,
                    lifecycle: "open".into(),
                    triage: None,
                    prompt: None,
                    answer: None,
                    created: None,
                    updated: None,
                },
            },
        ],
        &session(),
    );
    assert_eq!(r.applied, 5, "{:?}", r.skipped);
    assert!(r.skipped[0].contains("no provenance"));
    // The commit filed the decreed fact's proposal, statement verbatim; the
    // staged diagnostic on the derived one already stood, so no second one landed there.
    let id_of = |s: &Store, statement: &str| {
        s.graph
            .requirements
            .iter()
            .find(|(_, r)| r.statement == statement)
            .map(|(id, _)| id.clone())
            .expect("the requirement landed")
    };
    let x1 = id_of(&s, "The cart is split by category.");
    let x2 = id_of(&s, "The cart never exceeds ten categories.");
    assert!(x1.starts_with("req:x-") && x2.starts_with("req:x-") && x1 != x2);
    let proposal_id = s
        .graph
        .diagnostics
        .iter()
        .find(|(_, d)| d.rule == "ratification-pending" && d.subjects == vec![x2.clone()])
        .map(|(id, _)| id.clone())
        .expect("the commit files the proposal for the decreed fact");
    let prompt = s.graph.diagnostics[&proposal_id].prompt.as_ref().unwrap();
    assert!(prompt
        .question
        .contains("The cart never exceeds ten categories."));
    assert_eq!(
        s.graph
            .diagnostics
            .values()
            .filter(|d| d.rule == "ratification-pending" && d.subjects == vec![x1.clone()])
            .count(),
        1
    );
    assert!(s.graph.requirements.contains_key(&x1));
    assert!(s.graph.requirements.contains_key(&x2));
    assert!(s.status.has_change(CHANGE_PROVENANCE_PENDING, &x1));
    assert!(s.status.has_change(CHANGE_PROVENANCE_PENDING, &x2));
    // The same derived statement from the same sources folds, never duplicates.
    s.apply(
        vec![Op::CreateRequirement {
            id: "prov:again".into(),
            requirement: Requirement {
                statement: "The cart is split by category".into(),
                entities: vec!["ent:cart".into()],
                provenance: Some(derived(&["ent:cart"])),
                ..Default::default()
            },
        }],
        &session(),
    );
    assert_eq!(s.graph.requirements.len(), 2);
    // The sweep never deletes derived or decreed nodes.
    s.gc();
    assert_eq!(s.graph.requirements.len(), 2);
    // Ratification flips the provenance to the quote and resolves the diagnostic.
    let setup: Vec<String> = s.status.changes.iter().map(|c| c.id.clone()).collect();
    s.clear_changes(&setup);
    let r = s.apply(
        vec![Op::RatifyProvenance {
            id: x1.clone(),
            source: mention("t.md", "/t", "The cart is split by category."),
        }],
        &Commit::store("ratify"),
    );
    assert_eq!(r.applied, 2, "{:?}", r.skipped);
    let q = &s.graph.requirements[&x1];
    assert!(q.source.is_some() && q.provenance.is_none());
    assert!(!s.status.has_change(CHANGE_PROVENANCE_PENDING, &x1));
    // An accepted proposal is a human edit: the mention the landed sentence adds
    // opens no review of the Cart. Mirrors docs/compiler/goals/ratify.md#accept.
    assert!(
        !s.status.has_change(CHANGE_ENTITY, "ent:cart"),
        "{:?}",
        s.status.changes
    );
    assert!(s
        .graph
        .diagnostics
        .values()
        .filter(|d| d.subjects.contains(&x1))
        .all(|d| d.lifecycle == "resolved"));
    assert_eq!(s.graph.diagnostics[&proposal_id].lifecycle, "open");
    assert!(s.graph.entities["ent:cart"]
        .mentions
        .iter()
        .any(|m| m.section == "/t"));
    // Retracting the decree deletes it; a quoted fact is not a decree.
    let r = s.apply(
        vec![
            Op::RetractDecree {
                id: x2.clone(),
                reason: "withdrawn".into(),
            },
            Op::RetractDecree {
                id: x1.clone(),
                reason: "withdrawn".into(),
            },
        ],
        &Commit::store("decree"),
    );
    assert_eq!(r.applied, 2, "{:?}", r.skipped);
    assert!(!s.graph.requirements.contains_key(&x2));
    assert!(s.status.has_change(CHANGE_REQ_DELETED, &x2));
    assert!(r.skipped[0].contains("not a decree"));
    assert!(s
        .graph
        .diagnostics
        .get(&proposal_id)
        .is_none_or(|d| d.lifecycle == "resolved"));
}

// Retracting a derived grouping dissolves it: the children return to its parent,
// a quoted requirement that named it re-points there, the pending record clears,
// and the ratify entry journals the dissolve_entity mutation so the moves replay.
// Mirrors docs/compiler/goals/ratify.md#retract.
#[test]
fn retracting_a_derived_grouping_dissolves_it_onto_its_parent() {
    let mut s = Store {
        out: own_dir("retract-grouping"),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nThe cart holds items.\n");
    let under = |name: &str, parent: &str, provenance: Option<Provenance>| Entity {
        parent: Some(parent.into()),
        provenance,
        ..entity(name)
    };
    let r = s.apply(
        vec![
            create("ent:checkout", "Checkout"),
            Op::CreateEntity {
                id: "ent:selection".into(),
                entity: under(
                    "Selection",
                    "ent:checkout",
                    Some(derived(&["ent:cart", "ent:wishlist"])),
                ),
            },
            Op::CreateEntity {
                id: "ent:cart".into(),
                entity: under("Cart", "ent:selection", None),
            },
            Op::CreateEntity {
                id: "ent:wishlist".into(),
                entity: under("Wishlist", "ent:selection", None),
            },
            Op::CreateRequirement {
                id: "prov:q".into(),
                requirement: Requirement {
                    statement: "The cart holds items.".into(),
                    entities: vec!["ent:cart".into(), "ent:selection".into()],
                    source: Some(mention("t.md", "/t", "The cart holds items.")),
                    ..Default::default()
                },
            },
        ],
        &session(),
    );
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    assert!(s
        .status
        .has_change(CHANGE_PROVENANCE_PENDING, "ent:selection"));
    let rid = s
        .graph
        .requirements
        .iter()
        .find(|(_, q)| q.statement == "The cart holds items.")
        .map(|(id, _)| id.clone())
        .unwrap();
    let r = s.apply(
        vec![Op::RetractDecree {
            id: "ent:selection".into(),
            reason: "retracted".into(),
        }],
        &Commit::store("ratify"),
    );
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    assert!(!s.graph.entities.contains_key("ent:selection"));
    assert_eq!(s.graph.redirects["ent:selection"], "ent:checkout");
    for c in ["ent:cart", "ent:wishlist"] {
        assert_eq!(s.graph.entities[c].parent.as_deref(), Some("ent:checkout"));
    }
    let q = &s.graph.requirements[&rid];
    assert!(!q.entities.iter().any(|e| e == "ent:selection"));
    assert!(q.entities.iter().any(|e| e == "ent:checkout"));
    assert!(!s
        .status
        .has_change(CHANGE_PROVENANCE_PENDING, "ent:selection"));
    // A retract is journaled as `ratify` but is no accepted sentence: the parent
    // that took the requirement and the children is reviewed as usual.
    // Mirrors docs/compiler/goals/ratify.md#retract.
    assert!(
        s.status.has_change(CHANGE_ENTITY, "ent:checkout"),
        "{:?}",
        s.status.changes
    );
    let entry: JournalEntry = yaml_to(
        &s.out
            .join("journal")
            .join(format!("g{}.yaml", r.generation)),
    )
    .unwrap();
    assert_eq!(entry.kind, "ratify");
    assert!(entry.mutations.iter().any(|m| m["op"] == "dissolve_entity"));
    let moves = s.journaled_parent_moves();
    assert!(moves.iter().any(|mv| mv.child == "ent:wishlist"
        && mv.from.as_deref() == Some("ent:selection")
        && mv.to.as_deref() == Some("ent:checkout")));
    // A parentless entity keeps its quoted requirements: the refusal names them.
    let r = s.apply(
        vec![
            Op::CreateEntity {
                id: "ent:top".into(),
                entity: Entity {
                    provenance: Some(derived(&["ent:checkout"])),
                    ..entity("Top")
                },
            },
            Op::CreateRequirement {
                id: "prov:t".into(),
                requirement: Requirement {
                    statement: "The top holds the checkout.".into(),
                    entities: vec!["ent:top".into()],
                    provenance: Some(derived(&["ent:top"])),
                    ..Default::default()
                },
            },
        ],
        &session(),
    );
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    let r = s.apply(
        vec![Op::RetractDecree {
            id: "ent:top".into(),
            reason: "retracted".into(),
        }],
        &Commit::store("ratify"),
    );
    assert!(r.skipped[0].contains("no parent to take its requirements"));
    assert!(s.graph.entities.contains_key("ent:top"));
    // All or nothing: the refused retract takes the answer beside it down with
    // it, nothing lands, no generation is minted, and the diagnostic can still
    // be answered.
    let before = s.status.generation;
    let proposal = s
        .graph
        .diagnostics
        .iter()
        .find(|(_, d)| d.rule == "ratification-pending" && d.subjects == vec!["ent:top"])
        .map(|(id, _)| id.clone())
        .expect("the decree filed its proposal");
    let r = s.apply(
        vec![
            Op::RetractDecree {
                id: "ent:top".into(),
                reason: "retracted".into(),
            },
            Op::AnswerDiagnostic {
                id: proposal.clone(),
                answer: crate::model::DiagnosticAnswer {
                    choice: Some(1),
                    text: "retract".into(),
                    status: "applied".into(),
                    error: None,
                },
            },
        ],
        &Commit {
            all_or_nothing: true,
            ..Commit::store("ratify")
        },
    );
    assert_eq!(r.applied, 0);
    assert!(r.skipped[0].contains("no parent to take its requirements"));
    assert_eq!(s.status.generation, before);
    assert!(s.graph.diagnostics[&proposal].answer.is_none());
    assert!(!s
        .out
        .join("journal")
        .join(format!("g{}.yaml", before + 1))
        .vexists());
}
