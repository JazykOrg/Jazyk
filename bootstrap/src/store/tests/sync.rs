// Document sync, alignment proposals, and stale anchors.
use super::*;

#[test]
fn sync_docs_journals_an_edit_entry_with_section_records() {
    let mut s = Store {
        out: own_dir("edit"),
        ..Default::default()
    };
    let v1 = "# T\nintro\n\n## Alpha\nalpha body\n\n## Beta\nbeta body\n";
    let mut parsed = BTreeMap::new();
    parsed.insert(
        "t.md".to_string(),
        (hash_hex(v1), crate::md::parse_sections(v1)),
    );
    s.sync_docs(&parsed);
    assert_eq!(s.status.generation, 1);
    let entry: JournalEntry = yaml_to(&crate::layout::journal_dir(&s.out).join("g1.yaml")).unwrap();
    assert_eq!(entry.kind, "edit");
    assert_eq!(entry.dirtied.len(), 3);
    assert!(entry
        .mutations
        .iter()
        .all(|m| m["op"] == CHANGE_SECTION_DIRTY));
    assert!(s.status.has_change(CHANGE_SECTION_DIRTY, "t.md#/t/alpha"));
    let rec = s
        .status
        .changes
        .iter()
        .find(|c| c.subject == "t.md#/t/alpha")
        .unwrap();
    assert_eq!(rec.generation, 1);
    assert_eq!(rec.via, "section");
    assert_eq!(rec.detail["section"], "/t/alpha");
    // Nothing changed: no entry, no generation.
    s.sync_docs(&parsed);
    assert_eq!(s.status.generation, 1);
    // Beta removed, Alpha edited: the entry lists both, the removed record is a trail
    // the sweep clears, and Alpha's stale anchor is recorded on its section.
    s.graph.requirements.insert(
        "req:t-1".into(),
        Requirement {
            statement: "alpha body".into(),
            entities: vec![],
            source: Some(mention("t.md", "/t/alpha", "alpha body")),
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:t-2".into(),
        Requirement {
            statement: "beta body".into(),
            entities: vec![],
            source: Some(mention("t.md", "/t/beta", "beta body")),
            ..Default::default()
        },
    );
    let v2 = "# T\nintro\n\n## Alpha\nalpha CHANGED\n";
    let mut parsed2 = BTreeMap::new();
    parsed2.insert(
        "t.md".to_string(),
        (hash_hex(v2), crate::md::parse_sections(v2)),
    );
    let d2 = s.sync_docs(&parsed2);
    assert!(d2[0].stale_anchors.contains(&"req:t-1".to_string()) || !s.status.alignment.is_empty());
    let edit_gen = s
        .status
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_SECTION_REMOVED)
        .map(|c| c.generation)
        .expect("section-removed");
    let entry: JournalEntry =
        yaml_to(&crate::layout::journal_dir(&s.out).join(format!("g{}.yaml", edit_gen))).unwrap();
    assert_eq!(entry.kind, "edit");
    assert!(entry.dirtied.contains(&"t.md#/t/beta".to_string()));
    assert!(entry
        .mutations
        .iter()
        .any(|m| m["op"] == CHANGE_SECTION_REMOVED && m["section"] == "/t/beta"));
    assert!(entry
        .mutations
        .iter()
        .any(|m| m["op"] == CHANGE_SECTION_DIRTY && m["section"] == "/t/alpha"));
    assert!(
        s.status.has_change(CHANGE_ANCHOR_STALE, "t.md#/t/beta")
            || s.status.has_change(CHANGE_ALIGNMENT_PENDING, "t.md"),
        "{:?}",
        s.status.changes
    );
    // The sweep deletes the homeless requirement and clears the trail.
    s.gc();
    assert!(!s.graph.requirements.contains_key("req:t-2") || !s.status.alignment.is_empty());
    assert!(!s.status.has_change(CHANGE_SECTION_REMOVED, "t.md#/t/beta"));
}

#[test]
fn sync_docs_dirty_moved_removed() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    let v1 = "# T\nintro\n\n## Group\ngroup body\n\n### Alpha\nalpha body\n\n## Beta\nbeta body\n";
    let mut parsed = BTreeMap::new();
    parsed.insert(
        "t.md".to_string(),
        (hash_hex(v1), crate::md::parse_sections(v1)),
    );
    let d1 = s.sync_docs(&parsed);
    assert_eq!(d1.len(), 1);
    assert_eq!(d1[0].dirty_sections.len(), 4);

    // Anchor nodes in Alpha (which will move) and Beta (which will change).
    s.graph.entities.insert(
        "ent:a".into(),
        Entity {
            name: "A".into(),
            mentions: vec![mention("t.md", "/t/group/alpha", "alpha body")],
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:t-1".into(),
        Requirement {
            statement: "The A alphas.".into(),
            entities: vec!["ent:a".into()],
            source: Some(mention("t.md", "/t/group/alpha", "alpha body")),
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:t-2".into(),
        Requirement {
            statement: "The B betas.".into(),
            entities: vec!["ent:a".into()],
            source: Some(mention("t.md", "/t/beta", "beta body")),
            ..Default::default()
        },
    );
    s.docs.get_mut("t.md").unwrap().coverage.insert(
        "/t/group/alpha".into(),
        Coverage {
            state: "covered".into(),
            note: None,
            claimed_by: None,
        },
    );

    // Rename the Group heading (Alpha moves under the new reference, its raw unchanged)
    // and edit Beta so the anchored quote no longer locates.
    let v2 = "# T\nintro\n\n## Bunch\ngroup body\n\n### Alpha\nalpha body\n\n## Beta\nbeta CHANGED body\n";
    let mut parsed2 = BTreeMap::new();
    parsed2.insert(
        "t.md".to_string(),
        (hash_hex(v2), crate::md::parse_sections(v2)),
    );
    let d2 = s.sync_docs(&parsed2);
    assert_eq!(d2.len(), 1);
    // Bunch is a changed section, Beta is a changed section; the moved Alpha is not dirty.
    assert_eq!(
        d2[0].dirty_sections,
        vec!["/t/beta".to_string(), "/t/bunch".to_string()]
    );
    // Beta's quote no longer locates -> a proposal for the align session (the section
    // was edited in place, so it has a candidate); Alpha's references were rewritten.
    assert!(!d2[0].stale_anchors.contains(&"req:t-2".to_string()));
    assert!(s
        .status
        .alignment
        .iter()
        .any(|b| b.doc == "t.md" && b.proposals.iter().any(|p| p.anchor == "req:t-2")));
    assert!(s.status.has_change(CHANGE_ALIGNMENT_PENDING, "t.md"));
    assert!(!d2[0].stale_anchors.contains(&"req:t-1".to_string()));
    assert_eq!(
        s.graph.requirements["req:t-1"]
            .source
            .as_ref()
            .unwrap()
            .section,
        "/t/bunch/alpha"
    );
    assert_eq!(
        s.graph.entities["ent:a"].mentions[0].section,
        "/t/bunch/alpha"
    );
    let rec = &s.docs["t.md"];
    assert!(rec.coverage.contains_key("/t/bunch/alpha"));
}

// A save that dirties a section writes its edit records and, in the same
// generation, the standing alignment block's record: every id stays unique.
#[test]
fn sync_docs_numbers_one_generation_without_duplicate_record_ids() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    let parse = |docs: &[(&str, &str)]| -> crate::reconcile::ParsedDocs {
        docs.iter()
            .map(|(d, t)| (d.to_string(), (hash_hex(t), crate::md::parse_sections(t))))
            .collect()
    };
    let t1 = "# T\n\n## Beta\nbeta body\n";
    let u1 = "# U\nunder body\n";
    s.sync_docs(&parse(&[("t.md", t1), ("u.md", u1)]));
    s.graph.requirements.insert(
        "req:t-1".into(),
        Requirement {
            statement: "The B betas.".into(),
            source: Some(mention("t.md", "/t/beta", "beta body")),
            ..Default::default()
        },
    );
    // Beta is edited in place: its anchor becomes a pending proposal.
    let t2 = "# T\n\n## Beta\nbeta CHANGED body\n";
    s.sync_docs(&parse(&[("t.md", t2), ("u.md", u1)]));
    assert!(s.status.has_change(CHANGE_ALIGNMENT_PENDING, "t.md"));
    // Another document's edit opens no proposal: its section records and the
    // standing alignment record share one generation.
    let u2 = "# U\nunder body, revised\n";
    s.sync_docs(&parse(&[("t.md", t2), ("u.md", u2)]));
    let generation = s.status.generation;
    let here: Vec<&ChangeRecord> = s
        .status
        .changes
        .iter()
        .filter(|c| c.generation == generation)
        .collect();
    assert!(here.iter().any(|c| c.kind == CHANGE_SECTION_DIRTY));
    assert!(here.iter().any(|c| c.kind == CHANGE_ALIGNMENT_PENDING));
    let ids: BTreeSet<&str> = s.status.changes.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), s.status.changes.len(), "{:?}", s.status.changes);
}

// A moved-and-edited section yields proposals, not stale anchors; the block persists
// in status.yaml and the anchor survives the sweep until the session decides.
#[test]
fn sync_docs_proposes_for_a_moved_and_edited_section_and_gc_waits() {
    let mut s = Store {
        out: own_dir("proposal"),
        ..Default::default()
    };
    let v1 = format!("# T\nintro\n\n## Cart\n{}\n\n## Orders\n{}\n", CART, ORDERS);
    let mut parsed = BTreeMap::new();
    parsed.insert(
        "t.md".to_string(),
        (hash_hex(&v1), crate::md::parse_sections(&v1)),
    );
    s.sync_docs(&parsed);
    s.graph.entities.insert(
        "ent:a".into(),
        Entity {
            name: "A".into(),
            mentions: vec![mention("t.md", "/t/cart", "Items stay until checkout.")],
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:t-1".into(),
        req_at("t.md", "/t/cart", "Items stay until checkout."),
    );
    s.graph.requirements.insert(
        "req:t-2".into(),
        req_at(
            "t.md",
            "/t/cart",
            "A cart may hold up to fifty items at once.",
        ),
    );

    let v2 = format!(
        "# T\nintro\n\n## Orders\n{}\n\n## Basket\n{}\n",
        ORDERS,
        CART.replace("fifty", "sixty")
    );
    let mut parsed2 = BTreeMap::new();
    parsed2.insert(
        "t.md".to_string(),
        (hash_hex(&v2), crate::md::parse_sections(&v2)),
    );
    let d2 = s.sync_docs(&parsed2);
    assert_eq!(d2.len(), 1);
    assert_eq!(d2[0].dirty_sections, vec!["/t/basket".to_string()]);
    assert!(d2[0].stale_anchors.is_empty(), "{:?}", d2[0].stale_anchors);
    let block = s
        .status
        .alignment
        .iter()
        .find(|b| b.doc == "t.md")
        .expect("alignment block");
    // ent:a's mention coincides with req:t-1's source: derived, it follows the
    // requirement and is no proposal of its own.
    let anchors: BTreeSet<&str> = block.proposals.iter().map(|p| p.anchor.as_str()).collect();
    assert_eq!(anchors, ["req:t-1", "req:t-2"].into_iter().collect());
    assert!(block
        .changes
        .iter()
        .any(|c| c.op == "moved" && c.to == vec!["t.md#/t/basket"]));
    // The align entry carries the proposals.
    let entry: JournalEntry =
        yaml_to(&crate::layout::journal_dir(&s.out).join(format!("g{}.yaml", s.status.generation)))
            .unwrap();
    assert_eq!(entry.kind, "align");
    assert!(entry
        .mutations
        .iter()
        .any(|m| m["op"] == "propose" && m["anchor"] == "req:t-2"));
    // The persisted form round-trips.
    let reloaded = Store::load(&s.out);
    assert_eq!(reloaded.status.alignment, s.status.alignment);
    // Anchors still point at the vanished section, and the sweep leaves them alone.
    assert_eq!(
        s.graph.requirements["req:t-1"]
            .source
            .as_ref()
            .unwrap()
            .section,
        "/t/cart"
    );
    s.gc();
    assert!(s.graph.requirements.contains_key("req:t-1"));
    assert_eq!(s.graph.entities["ent:a"].mentions.len(), 1);
}

// place_anchor moves the source; reevaluate lists it as a stale anchor on the
// target document; a commit naming it pays the debt. orphan_anchor leaves the
// anchor where it was, and the decided proposals clear the block.
#[test]
fn place_and_orphan_anchor_apply_and_clear_the_block() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    let v1 = format!("# T\nintro\n\n## Cart\n{}\n", CART);
    let mut parsed = BTreeMap::new();
    parsed.insert(
        "t.md".to_string(),
        (hash_hex(&v1), crate::md::parse_sections(&v1)),
    );
    s.sync_docs(&parsed);
    s.graph.entities.insert(
        "ent:a".into(),
        Entity {
            name: "A".into(),
            mentions: vec![mention("t.md", "/t/cart", "Items stay until checkout.")],
            ..Default::default()
        },
    );
    s.graph.requirements.insert(
        "req:t-1".into(),
        req_at("t.md", "/t/cart", "Items stay until checkout."),
    );
    s.graph.requirements.insert(
        "req:t-2".into(),
        req_at(
            "t.md",
            "/t/cart",
            "A cart may hold up to fifty items at once.",
        ),
    );
    s.graph.requirements.insert(
        "req:t-3".into(),
        req_at(
            "t.md",
            "/t/cart",
            "The cart holds items a customer intends to buy.",
        ),
    );
    let v2 = format!(
        "# T\nintro\n\n## Basket\n{}\n",
        CART.replace("fifty", "sixty")
    );
    let mut parsed2 = BTreeMap::new();
    parsed2.insert(
        "t.md".to_string(),
        (hash_hex(&v2), crate::md::parse_sections(&v2)),
    );
    s.sync_docs(&parsed2);
    assert_eq!(s.status.alignment.len(), 1);
    assert!(s.status.has_change(CHANGE_ALIGNMENT_PENDING, "t.md"));

    let align_item = WorkItem {
        task: "align-doc".into(),
        target: "t.md".into(),
        dirty_sections: vec![],
        stale_anchors: vec![],
        proposals: vec!["req:t-1".into(), "req:t-2".into(), "req:t-3".into()],
    };
    let from = |q: &str| mention("t.md", "/t/cart", q);
    let report = s.apply(
        vec![
            Op::PlaceAnchor {
                id: "req:t-1".into(),
                from: from("Items stay until checkout."),
                to: mention("t.md", "/t/basket", "Items stay until checkout."),
                reevaluate: false,
            },
            Op::PlaceAnchor {
                id: "req:t-2".into(),
                from: from("A cart may hold up to fifty items at once."),
                to: mention(
                    "t.md",
                    "/t/basket",
                    "A cart may hold up to sixty items at once.",
                ),
                reevaluate: true,
            },
            Op::OrphanAnchor {
                id: "req:t-3".into(),
                from: from("The cart holds items a customer intends to buy."),
            },
        ],
        &align_item.commit(1, None),
    );
    assert_eq!(report.applied, 3, "{:?}", report.skipped);
    assert!(s.status.alignment.is_empty());
    assert!(!s.status.has_change(CHANGE_ALIGNMENT_PENDING, "t.md"));
    assert_eq!(
        s.graph.requirements["req:t-1"]
            .source
            .as_ref()
            .unwrap()
            .section,
        "/t/basket"
    );
    assert_eq!(
        s.graph.requirements["req:t-2"]
            .source
            .as_ref()
            .unwrap()
            .quote,
        "A cart may hold up to sixty items at once."
    );
    // The mention derived from req:t-1's source followed the requirement.
    assert_eq!(s.graph.entities["ent:a"].mentions[0].section, "/t/basket");
    // The orphaned anchor's section is gone and no proposal names it any more:
    // the sweep behind the commit deletes the requirement, journaled, and the
    // report says so. Mirrors docs/compiler/graph.md#the-sweep.
    assert!(!s.graph.requirements.contains_key("req:t-3"));
    assert!(
        report.swept.iter().any(|a| a.contains("req:t-3")),
        "{:?}",
        report.swept
    );
    assert!(s.status.has_change(CHANGE_REQ_DELETED, "req:t-3"));
    assert_eq!(s.status.reevaluate, vec!["req:t-2".to_string()]);
    // A substantive quote change is a revision owed a pair judgment: the commit
    // records it, and the sweep behind it clears the record at once because
    // req:t-2 has no candidate pair (docs/compiler/reconciler.md#pairs). The
    // flagged anchor is recorded stale on its new section.
    assert!(report.changed_requirements.contains("req:t-2"));
    assert!(report
        .changes
        .iter()
        .any(|c| c.kind == CHANGE_REQ_REVISED && c.subject == "req:t-2"));
    assert!(!s.status.has_change(CHANGE_REQ_REVISED, "req:t-2"));
    let stale = s
        .status
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_ANCHOR_STALE && c.subject == "t.md#/t/basket")
        .expect("anchor-stale");
    assert_eq!(stale.detail["anchors"], serde_json::json!(["req:t-2"]));

    // The next sync lists the flagged anchor as stale on its document even though
    // its quote locates; the orphan is gone, so it is nobody's stale anchor.
    let d3 = s.sync_docs(&parsed2);
    let t = d3.iter().find(|d| d.doc == "t.md").expect("item");
    assert!(t.stale_anchors.contains(&"req:t-2".to_string()));
    assert!(!t.stale_anchors.contains(&"req:t-3".to_string()));
    assert!(!t.stale_anchors.contains(&"req:t-1".to_string()));
    assert!(t.dirty_sections.contains(&"/t/basket".to_string()));

    // A commit that leaves the flagged anchor alone leaves the flag; resolving
    // the section's reconcile goal clears it.
    s.apply(vec![], &session());
    assert_eq!(s.status.reevaluate, vec!["req:t-2".to_string()]);
    let mut commit = session();
    commit.resolved.push(Resolved {
        goal: "g:reconcile-section:t.md#/t/basket".into(),
        justification: "re-evaluated".into(),
        evidence: serde_json::Value::Null,
    });
    s.apply(vec![], &commit);
    assert!(s.status.reevaluate.is_empty());
}

// Exact moves are journaled, and a heading level change is an exact move: nothing
// dirty beyond the new heading, coverage carried.
#[test]
fn exact_moves_are_journaled_and_survive_a_level_change() {
    let mut s = Store {
        out: own_dir("align-journal"),
        ..Default::default()
    };
    let v1 = format!("# T\nintro\n\n## Cart\n{}\n", CART);
    let mut parsed = BTreeMap::new();
    parsed.insert(
        "t.md".to_string(),
        (hash_hex(&v1), crate::md::parse_sections(&v1)),
    );
    s.sync_docs(&parsed);
    s.graph.entities.insert("ent:a".into(), entity("A"));
    s.graph.requirements.insert(
        "req:t-1".into(),
        req_at("t.md", "/t/cart", "Items stay until checkout."),
    );
    s.docs.get_mut("t.md").unwrap().coverage.insert(
        "/t/cart".into(),
        Coverage {
            state: "covered".into(),
            note: None,
            claimed_by: None,
        },
    );
    let gen_before = s.status.generation;
    let v2 = format!("# T\nintro\n\n## Group\n\n### Cart\n{}\n", CART);
    let mut parsed2 = BTreeMap::new();
    parsed2.insert(
        "t.md".to_string(),
        (hash_hex(&v2), crate::md::parse_sections(&v2)),
    );
    let d2 = s.sync_docs(&parsed2);
    assert_eq!(d2[0].dirty_sections, vec!["/t/group".to_string()]);
    assert!(d2[0].stale_anchors.is_empty());
    assert_eq!(
        s.graph.requirements["req:t-1"]
            .source
            .as_ref()
            .unwrap()
            .section,
        "/t/group/cart"
    );
    assert!(s.docs["t.md"].coverage.contains_key("/t/group/cart"));
    // The edit entry, then the align entry.
    assert_eq!(s.status.generation, gen_before + 2);
    let edit: JournalEntry =
        yaml_to(&crate::layout::journal_dir(&s.out).join(format!("g{}.yaml", gen_before + 1)))
            .unwrap();
    assert_eq!(edit.kind, "edit");
    assert_eq!(edit.dirtied, vec!["t.md#/t/group".to_string()]);
    let entry: JournalEntry =
        yaml_to(&crate::layout::journal_dir(&s.out).join(format!("g{}.yaml", gen_before + 2)))
            .unwrap();
    assert_eq!(entry.kind, "align");
    assert!(entry.batch.is_empty());
    assert_eq!(entry.mutations[0]["from"], "t.md#/t/cart");
}

// A heading renamed in place is an edit: its anchors follow the new reference
// without a proposal, its coverage does not, the section is dirty, and the align
// entry records the rename. Mirrors docs/compiler/alignment.md#what-applies-and-what-is-proposed.
#[test]
fn renamed_heading_carries_anchors_and_dirties_the_section() {
    let mut s = Store {
        out: own_dir("align-rename"),
        ..Default::default()
    };
    let v1 = format!("# T\nintro\n\n## Cart\n{}\n", CART);
    let mut parsed = BTreeMap::new();
    parsed.insert(
        "t.md".to_string(),
        (hash_hex(&v1), crate::md::parse_sections(&v1)),
    );
    s.sync_docs(&parsed);
    s.graph.entities.insert("ent:a".into(), entity("A"));
    s.graph.requirements.insert(
        "req:t-1".into(),
        req_at("t.md", "/t/cart", "Items stay until checkout."),
    );
    s.docs.get_mut("t.md").unwrap().coverage.insert(
        "/t/cart".into(),
        Coverage {
            state: "covered".into(),
            note: None,
            claimed_by: None,
        },
    );
    let gen_before = s.status.generation;
    let v2 = format!("# T\nintro\n\n## Basket\n{}\n", CART);
    let mut parsed2 = BTreeMap::new();
    parsed2.insert(
        "t.md".to_string(),
        (hash_hex(&v2), crate::md::parse_sections(&v2)),
    );
    let d2 = s.sync_docs(&parsed2);
    assert_eq!(d2.len(), 1, "{:?}", d2);
    assert_eq!(d2[0].dirty_sections, vec!["/t/basket".to_string()]);
    assert!(d2[0].stale_anchors.is_empty(), "{:?}", d2[0].stale_anchors);
    assert_eq!(
        s.graph.requirements["req:t-1"]
            .source
            .as_ref()
            .unwrap()
            .section,
        "/t/basket"
    );
    assert!(!s.docs["t.md"].coverage.contains_key("/t/basket"));
    assert!(!s.docs["t.md"].coverage.contains_key("/t/cart"));
    assert!(s.status.alignment.is_empty(), "{:?}", s.status.alignment);
    // The edit entry dirties the renamed section only; the align entry records
    // the rename, no section-removed record for the old reference.
    assert_eq!(s.status.generation, gen_before + 2);
    let edit: JournalEntry =
        yaml_to(&crate::layout::journal_dir(&s.out).join(format!("g{}.yaml", gen_before + 1)))
            .unwrap();
    assert_eq!(edit.kind, "edit");
    assert_eq!(edit.dirtied, vec!["t.md#/t/basket".to_string()]);
    let entry: JournalEntry =
        yaml_to(&crate::layout::journal_dir(&s.out).join(format!("g{}.yaml", gen_before + 2)))
            .unwrap();
    assert_eq!(entry.kind, "align");
    assert_eq!(entry.mutations[0]["op"], "rename");
    assert_eq!(entry.mutations[0]["from"], "t.md#/t/cart");
    assert_eq!(entry.mutations[0]["to"], "t.md#/t/basket");
    assert!(!s
        .status
        .changes
        .iter()
        .any(|c| c.kind == CHANGE_SECTION_REMOVED));
}

// An entity mention whose quote stopped locating in a changed section is a stale
// anchor too, not only a requirement source.
// An edit that adds a line to a reconciled section: the record carries the line, the
// loaded body marks it, and a covered claim that stages nothing for it bounces until
// a staged quote or the coverage note accounts for it. The case that motivated the
// rule: a layout lead-in added above three quoted paragraphs whose sentences were
// already extracted. Mirrors docs/compiler/goals/reconcile-section.md#gate.
#[test]
fn an_added_line_rides_the_record_and_gates_the_covered_claim() {
    use crate::goals::GoalKind;
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    let parse = |text: &str| {
        let mut parsed = BTreeMap::new();
        parsed.insert(
            "t.md".to_string(),
            (hash_hex(text), crate::md::parse_sections(text)),
        );
        parsed
    };
    let v1 = "# T\nintro\n\n## Content\n\n> First. Second.\n\n> Third.\n";
    s.sync_docs(&parse(v1));
    let req = |quote: &str| Requirement {
        statement: format!("The page displays \"{}\"", quote),
        source: Some(SourceRef {
            doc: "t.md".into(),
            section: "/t/content".into(),
            quote: quote.into(),
        }),
        ..Default::default()
    };
    s.graph.requirements.insert("req:t-1".into(), req("First."));
    s.graph
        .requirements
        .insert("req:t-2".into(), req("Second."));
    s.graph.requirements.insert("req:t-3".into(), req("Third."));
    let v2 = "# T\nintro\n\n## Content\n\nWritten as separate paragraphs:\n\n> First. Second.\n\n> Third.\n";
    s.sync_docs(&parse(v2));
    let rec = s
        .status
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_SECTION_DIRTY && c.subject == "t.md#/t/content")
        .expect("a section-dirty record");
    assert_eq!(rec.detail["added"], 1, "{}", rec.detail);
    assert_eq!(rec.detail["removed"], 0, "{}", rec.detail);
    assert_eq!(
        rec.detail["addedLines"],
        serde_json::json!(["Written as separate paragraphs:"])
    );
    assert!(rec.detail["previous"].is_string(), "{}", rec.detail);

    // The loaded body marks the line from the record, though the store on disk
    // already holds the new body.
    let mut set = crate::context::LoadedSet::new(20_000);
    let text = set.load(&s, "t.md#/t/content", 1).unwrap();
    assert!(
        text.contains("+ Written as separate paragraphs:"),
        "{}",
        text
    );
    assert!(!text.contains("+ > First."), "{}", text);

    let gate = |staged: Vec<Op>| -> Vec<String> {
        crate::goals::ReconcileSection
            .gates(&s, &staged)
            .into_iter()
            .map(|v| v.rule.clone())
            .collect()
    };
    let cover = |note: &str| Op::SetCoverage {
        doc: "t.md".into(),
        section: "/t/content".into(),
        state: "covered".into(),
        note: Some(note.into()),
    };
    // The claim from the failed build: nothing staged, the old requirements cited.
    assert!(gate(vec![cover(
        "All sentences are already extracted. No new sentences."
    )])
    .contains(&"added-line-unaccounted".to_string()));
    // A requirement quoting the added line accounts for it.
    let staged = vec![
        Op::CreateRequirement {
            id: "req:t-4".into(),
            requirement: req("Written as separate paragraphs:"),
        },
        cover("req:t-4 carries the layout."),
    ];
    assert!(gate(staged).is_empty());
    // So does a note that repeats the line and says why it states nothing.
    assert!(gate(vec![cover(
        "\"Written as separate paragraphs:\" restates how the quotes already render."
    )])
    .is_empty());
}

#[test]
fn dirty_section_checks_entity_mention_quotes() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    let v1 = "# T\nintro\n\n## Cart\nThe cart holds items.\nA customer owns the cart.\n";
    let mut parsed = BTreeMap::new();
    parsed.insert(
        "t.md".to_string(),
        (hash_hex(v1), crate::md::parse_sections(v1)),
    );
    s.sync_docs(&parsed);
    s.graph.entities.insert(
        "ent:a".into(),
        Entity {
            name: "A".into(),
            mentions: vec![mention("t.md", "/t/cart", "A customer owns the cart.")],
            ..Default::default()
        },
    );
    let v2 = "# T\nintro\n\n## Cart\nThe cart holds items.\nNobody owns anything here at all.\n";
    let mut parsed2 = BTreeMap::new();
    parsed2.insert(
        "t.md".to_string(),
        (hash_hex(v2), crate::md::parse_sections(v2)),
    );
    let d2 = s.sync_docs(&parsed2);
    let all: Vec<String> = d2[0].stale_anchors.clone();
    let proposed: Vec<String> = s
        .status
        .alignment
        .iter()
        .flat_map(|b| b.proposals.iter().map(|p| p.anchor.clone()))
        .collect();
    assert!(
        all.contains(&"ent:a".to_string()) || proposed.contains(&"ent:a".to_string()),
        "stale {:?} proposed {:?}",
        all,
        proposed
    );
}
