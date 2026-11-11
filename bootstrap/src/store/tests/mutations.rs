// Requirement, criterion, and view mutations and the change records they write.
use super::*;

// Mirrors docs/compiler/model/criterion.md: a criterion keys on its requirement plus
// statement, a restatement is refused, a statement change on the requirement marks
// its derived criteria revised, and a deleted requirement takes its criteria along.
#[test]
fn criteria_key_on_their_requirement_and_follow_it() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(
        &mut s,
        "t.md",
        "# T\nThe sort utility outputs its lines in ascending order.\n\nGiven the lines 321, 654, 453 the utility outputs 321, 453, 654.\n",
    );
    let quoted = |st: &str| Criterion {
        requirement: "req:t-1".into(),
        statement: st.into(),
        kind: "positive".into(),
        source: Some(mention("t.md", "/t", "Given the lines 321, 654, 453")),
        ..Default::default()
    };
    let ops = vec![
        create("prov:1", "Sort"),
        Op::CreateRequirement {
            id: "req:t-1".into(),
            requirement: Requirement {
                statement: "The sort utility outputs its lines in ascending order.".into(),
                entities: vec!["prov:1".into()],
                source: Some(mention(
                    "t.md",
                    "/t",
                    "outputs its lines in ascending order",
                )),
                ..Default::default()
            },
        },
        Op::CreateCriterion {
            id: "crit:new-1".into(),
            criterion: quoted("Given the lines 321, 654, 453, the utility outputs 321, 453, 654."),
        },
        Op::CreateCriterion {
            id: "crit:new-2".into(),
            criterion: Criterion {
                requirement: "req:t-1".into(),
                statement: "Given one line, the utility outputs that line unchanged.".into(),
                kind: "boundary".into(),
                provenance: Some(Provenance::Derived {
                    from: vec!["req:t-1".into()],
                    reasoning: "the edge of the ordering rule".into(),
                }),
                ..Default::default()
            },
        },
        Op::CreateCriterion {
            id: "crit:new-3".into(),
            criterion: quoted("The sort utility outputs its lines in ascending order"),
        },
        Op::CreateCriterion {
            id: "crit:new-4".into(),
            criterion: quoted("Given the lines 321, 654, 453, the utility outputs 321, 453, 654"),
        },
    ];
    let r = s.apply(ops, &wi().commit(3, Some(100)));
    assert!(
        r.skipped.iter().any(|x| x.contains("criterion-restates")),
        "{:?}",
        r.skipped
    );
    let ids: Vec<&String> = s.graph.criteria.keys().collect();
    assert_eq!(ids.len(), 2, "{:?}", ids);
    assert!(ids.iter().all(|id| id.starts_with("crit:t-")), "{:?}", ids);
    // The quoted criterion and the derived one, by provenance, not by mint order.
    let quoted_id = s
        .graph
        .criteria
        .iter()
        .find(|(_, c)| c.provenance.is_none())
        .map(|(id, _)| id.clone())
        .unwrap();
    let derived_id = s
        .graph
        .criteria
        .iter()
        .find(|(_, c)| c.provenance.is_some())
        .map(|(id, _)| id.clone())
        .unwrap();
    assert_eq!(s.criteria_of("req:t-1").len(), 2);
    assert!(s.status.has_change(CHANGE_CRIT_CREATED, &quoted_id));
    assert!(s.status.has_change(CHANGE_CRIT_CREATED, &derived_id));
    // The requirement's statement changes: the derived criterion is revised, the
    // quoted one keeps its own quote.
    s.apply(
        vec![Op::UpdateRequirement {
            id: "req:t-1".into(),
            statement: Some(
                "The sort utility outputs its lines in ascending order by default.".into(),
            ),
            entities: None,
            edges: None,
            transition: None,
            facets: None,
            source: None,
            provenance: None,
        }],
        &wi().commit(4, Some(100)),
    );
    assert!(s.status.has_change(CHANGE_CRIT_REVISED, &derived_id));
    assert!(!s.status.has_change(CHANGE_CRIT_REVISED, &quoted_id));
    // The requirement goes: its criteria go with it.
    s.apply(
        vec![Op::DeleteRequirement {
            id: "req:t-1".into(),
            reason: "gone".into(),
        }],
        &wi().commit(5, Some(100)),
    );
    assert!(s.graph.criteria.is_empty());
    assert!(s.status.has_change(CHANGE_CRIT_DELETED, &quoted_id));
    assert!(s.status.has_change(CHANGE_CRIT_DELETED, &derived_id));
}

// A commit writes the change records its mutations caused, each with its cause;
// resolving the goals clears them by id. Mirrors docs/compiler/graph.md#change-records.
#[test]
fn commit_writes_change_records_and_clear_changes_removes_them() {
    let mut s = Store {
        out: own_dir("records"),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nThe Cart holds items.\n");
    let ops = vec![
        create("ent:cart", "Cart"),
        Op::CreateRequirement {
            id: "req:t-1".into(),
            requirement: Requirement {
                statement: "The Cart holds items.".into(),
                entities: vec!["ent:cart".into()],
                source: Some(mention("t.md", "/t", "The Cart holds items.")),
                ..Default::default()
            },
        },
    ];
    let report = s.apply(ops, &session());
    assert_eq!(report.generation, 1);
    assert_eq!(report.changes.len(), 2, "{:?}", report.changes);
    let entity = report
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_ENTITY)
        .unwrap();
    assert_eq!(
        (
            entity.subject.as_str(),
            entity.mutation,
            entity.via.as_str()
        ),
        ("ent:cart", 1, "fields")
    );
    let created = report
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_REQ_CREATED)
        .unwrap();
    assert_eq!(
        (
            created.subject.as_str(),
            created.mutation,
            created.via.as_str()
        ),
        ("req:t-1", 2, "section")
    );
    assert!(report.changes.iter().all(|c| c.id.starts_with("c1-")));
    assert_ne!(report.changes[0].id, report.changes[1].id);
    // The journal entry names the session's goal and the store version is stamped.
    let entry: JournalEntry = yaml_to(&crate::layout::journal_dir(&s.out).join("g1.yaml")).unwrap();
    assert_eq!(entry.kind, "session");
    assert_eq!(entry.batch, vec!["g:reconcile-section:t.md".to_string()]);
    assert!(entry.opened_goals.is_empty());
    assert_eq!(Store::load(&s.out).status.version, STORE_VERSION);
    // Resolved goals ride on the entry; opened goals land after the board re-derives.
    let mut commit = wi().commit(2, Some(10));
    commit.resolved.push(Resolved {
        goal: "g:reconcile-section:t.md".into(),
        justification: "covered".into(),
        evidence: serde_json::Value::Null,
    });
    let report = s.apply(vec![], &commit);
    s.record_opened_goals(
        report.generation,
        vec![OpenedGoal {
            goal: "g:review-entity:ent:cart".into(),
            cause: Cause {
                generation: 1,
                mutation: 1,
                via: "fields".into(),
            },
        }],
    );
    let entry: JournalEntry = yaml_to(&crate::layout::journal_dir(&s.out).join("g2.yaml")).unwrap();
    assert_eq!(entry.resolved_goals.len(), 1);
    assert_eq!(entry.opened_goals[0].goal, "g:review-entity:ent:cart");
    assert_eq!(entry.rounds, 2);
    // The created record fed no pair (the requirement has no neighbor), so the
    // sweep behind the second commit cleared it; the entity record stood until
    // cleared by id, persisted. Mirrors docs/compiler/reconciler.md#pairs.
    assert!(!s.status.has_change(CHANGE_REQ_CREATED, "req:t-1"));
    let ids = s.status.change_ids(&[CHANGE_ENTITY], "ent:cart");
    assert_eq!(ids.len(), 1);
    s.clear_changes(&ids);
    assert!(!s.status.has_change(CHANGE_ENTITY, "ent:cart"));
    // The section was never covered, so a load finds it dirty again; the cleared
    // record stays cleared.
    let back = Store::load(&s.out);
    assert!(
        back.status
            .changes
            .iter()
            .all(|c| c.kind == CHANGE_SECTION_DIRTY),
        "{:?}",
        back.status.changes
    );
}

#[test]
fn same_sentence_subsumed_statement_refreshes_in_place() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(
        &mut s,
        "t.md",
        "# T\n- `createUser` - creates a new user account\n",
    );
    let base = Requirement {
        statement: "The user management system creates a new user account.".into(),
        entities: vec!["ent:um".into()],
        source: Some(mention(
            "t.md",
            "/t",
            "- `createUser` - creates a new user account",
        )),
        ..Default::default()
    };
    s.apply(
        vec![
            create("ent:um", "User Management"),
            Op::CreateRequirement {
                id: "req:t-1".into(),
                requirement: base.clone(),
            },
        ],
        &session(),
    );
    // A resumed build rewords the same sentence's statement; one fact, one node.
    let reworded = Requirement {
        statement: "The user management system creates a new user account using createUser.".into(),
        ..base
    };
    let r = s.apply(
        vec![Op::CreateRequirement {
            id: "req:t-2".into(),
            requirement: reworded,
        }],
        &session(),
    );
    assert_eq!(s.graph.requirements.len(), 1);
    assert!(s.graph.requirements["req:t-1"]
        .statement
        .contains("using createUser"));
    assert!(r.changed_requirements.contains("req:t-1"));
    assert!(s.status.has_change(CHANGE_REQ_REVISED, "req:t-1"));
    // Distinct atomic facts from one sentence stay separate.
    assert!(!statement_subsumes(
        "The gateway is a REST service.",
        "The gateway is built with Go."
    ));
    // A second fact that repeats the first's words and adds a clause is not a
    // rewording: the deadline and the cancellation it triggers are two statements.
    assert!(!statement_rewords(
        "An Order is paid within 21 days of placement.",
        "When an Order is not paid within 21 days of placement, Orderly cancels it."
    ));
    assert!(!statement_rewords(
        "The Shop sends a confirmation email.",
        "The Shop does not send a confirmation email."
    ));
    assert!(statement_rewords(
        "The Admin CLI creates a user.",
        "The Admin CLI creates a user using createUser."
    ));
    // An obligation modal and a verb ending restate the same fact; a local model
    // minted "the system empties the Cart" beside "shall empty" off one quote.
    assert!(statement_rewords(
        "When the customer checks out, the system shall empty the Cart.",
        "When the customer checks out, the system empties the Cart."
    ));
    // A permission is not the obligation.
    assert!(!statement_rewords(
        "The Customer must cancel the Order.",
        "The Customer can cancel the Order."
    ));
}

#[test]
fn requirement_remaps_provisional_ids_and_derives_edges() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(
        &mut s,
        "t.md",
        "# T\nWhen checkout completes, the system empties the Cart of Products.\n",
    );
    let ops = vec![
        create("prov:1", "Cart"),
        create("prov:2", "Product"),
        Op::CreateRequirement {
            id: "req:t-1".into(),
            requirement: Requirement {
                statement: "When checkout completes, the system empties the Cart.".into(),
                entities: vec!["prov:1".into(), "prov:2".into()],
                edges: vec![ReqEdge {
                    a: "prov:1".into(),
                    b: "prov:2".into(),
                    rel_type: Some("composition".into()),
                    cardinality: Some("1..*".into()),
                }],
                source: Some(mention("t.md", "/t", "the system empties the Cart")),
                ..Default::default()
            },
        },
    ];
    let r = s.apply(ops, &wi().commit(3, Some(100)));
    assert_eq!(r.applied, 3);
    let req = &s.graph.requirements["req:t-1"];
    assert_eq!(
        req.entities,
        vec!["ent:cart".to_string(), "ent:product".to_string()]
    );
    assert_eq!(s.graph.relationships.len(), 1);
    let rel = s.graph.relationships.values().next().unwrap();
    assert_eq!(rel.strongest(), "composition");
    assert_eq!(rel.contributions[0].cardinality.as_deref(), Some("1..*"));
    // A two-entity requirement with edges owes no declare-edges; one without does.
    assert!(!s.status.has_change(CHANGE_EDGES_MISSING, "req:t-1"));
    s.apply(
        vec![Op::UpdateRequirement {
            id: "req:t-1".into(),
            statement: None,
            entities: None,
            edges: Some(vec![]),
            transition: None,
            facets: None,
            source: None,
            provenance: None,
        }],
        &session(),
    );
    assert!(s.status.has_change(CHANGE_EDGES_MISSING, "req:t-1"));
    assert!(s.graph.relationships.is_empty());
}

// A deleted requirement's records name its own journal mutation; each cascaded
// criterion delete names its own. Mirrors docs/compiler/graph.md#change-records.
#[test]
fn a_requirement_delete_names_its_own_mutation() {
    let mut s = Store {
        out: own_dir("delete-cascade"),
        ..Default::default()
    };
    seed_doc(
        &mut s,
        "t.md",
        "# T\nThe sort utility outputs its lines in ascending order.\n",
    );
    s.apply(
        vec![
            create("ent:sort", "Sort"),
            Op::CreateRequirement {
                id: "req:t-1".into(),
                requirement: Requirement {
                    statement: "The sort utility outputs its lines in ascending order.".into(),
                    entities: vec!["ent:sort".into()],
                    source: Some(mention(
                        "t.md",
                        "/t",
                        "outputs its lines in ascending order",
                    )),
                    ..Default::default()
                },
            },
            Op::CreateCriterion {
                id: "crit:new-1".into(),
                criterion: Criterion {
                    requirement: "req:t-1".into(),
                    statement: "Given one line, the utility outputs that line unchanged.".into(),
                    kind: "boundary".into(),
                    provenance: Some(derived(&["req:t-1"])),
                    ..Default::default()
                },
            },
        ],
        &session(),
    );
    let r = s.apply(
        vec![Op::DeleteRequirement {
            id: "req:t-1".into(),
            reason: "gone".into(),
        }],
        &session(),
    );
    let entry = journal_entry(&s, r.generation);
    let op_at = |m: usize| {
        entry.mutations[m - 1]["op"]
            .as_str()
            .unwrap_or("")
            .to_string()
    };
    let req = r
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_REQ_DELETED && c.subject == "req:t-1")
        .expect("a requirement-deleted record");
    assert_eq!(
        op_at(req.mutation),
        "delete_requirement",
        "{:?}",
        entry.mutations
    );
    let crit = r
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_CRIT_DELETED)
        .expect("a criterion-deleted record");
    assert_eq!(
        op_at(crit.mutation),
        "delete_criterion",
        "{:?}",
        entry.mutations
    );
}

// Views: upsert by kind and title, curation clears default, deletes of a member
// are allowed and noted, a default view refuses deletion, a bump raises the limit.
// `via` on a view-member-gone record names the list the dead node sat in, and a
// change on a type reaches its instances as `attributes`.
// Mirrors docs/compiler/reconciler.md#change-records.
#[test]
fn via_names_the_view_list_and_a_types_change_reaches_instances_as_attributes() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nAna is a customer. The extra thing.\n");
    let stated = |name: &str, quote: &str| Entity {
        name: name.into(),
        mentions: vec![mention("t.md", "/t", quote)],
        ..Default::default()
    };
    let r = s.apply(
        vec![
            Op::CreateEntity {
                id: "ent:customer".into(),
                entity: stated("Customer", "customer"),
            },
            Op::CreateEntity {
                id: "ent:ana".into(),
                entity: stated("Ana", "Ana"),
            },
            Op::CreateEntity {
                id: "ent:extra".into(),
                entity: stated("Extra", "extra"),
            },
            Op::CreateRequirement {
                id: "req:t-1".into(),
                requirement: Requirement {
                    statement: "Ana is a Customer.".into(),
                    entities: vec!["ent:ana".into(), "ent:customer".into()],
                    edges: vec![ReqEdge {
                        a: "ent:ana".into(),
                        b: "ent:customer".into(),
                        rel_type: Some("instantiation".into()),
                        cardinality: None,
                    }],
                    source: Some(mention("t.md", "/t", "Ana is a customer")),
                    ..Default::default()
                },
            },
            Op::CreateView {
                id: "view:class/zoo".into(),
                view: View {
                    kind: "class".into(),
                    title: "Zoo".into(),
                    members: vec!["ent:customer".into()],
                    collapse: vec!["ent:extra".into()],
                    provenance: Some(derived(&["ent:customer"])),
                    ..Default::default()
                },
            },
        ],
        &session(),
    );
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    let r = s.apply(
        vec![Op::DeleteEntity {
            id: "ent:extra".into(),
            reason: "noise".into(),
        }],
        &session(),
    );
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    let gone = r
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_VIEW_MEMBER_GONE && c.subject == "view:class/zoo")
        .expect("view-member-gone");
    assert_eq!(gone.via, "collapse");
    assert_eq!(gone.detail["gone"], serde_json::json!(["ent:extra"]));
    let r = s.apply(
        vec![Op::UpdateEntity {
            id: "ent:customer".into(),
            name: None,
            definition: Some("a person who buys".into()),
            add_aliases: Vec::new(),
            remove_aliases: Vec::new(),
            clear_parent: false,
            not_about_add: Vec::new(),
            add_mention: None,
            stereotype: None,
            parent: None,
            set_attributes: None,
            add_attributes: Vec::new(),
            provenance: None,
        }],
        &session(),
    );
    let inst = r
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_INSTANCE && c.subject == "ent:ana")
        .expect("instance-changed on the instance");
    assert_eq!(inst.via, "attributes");
    assert_eq!(inst.detail["type"], "ent:customer");
}

#[test]
fn views_curate_delete_and_bump() {
    let mut s = Store {
        out: own_dir("views"),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\ncart and order\n");
    let req = |id: &str, statement: &str| Op::CreateRequirement {
        id: id.into(),
        requirement: Requirement {
            statement: statement.into(),
            entities: vec!["ent:cart".into()],
            facets: vec![Facet {
                facet: "behavior".into(),
                reasoning: "a step".into(),
                measure: None,
            }],
            source: Some(mention("t.md", "/t", "cart and order")),
            ..Default::default()
        },
    };
    s.apply(
        vec![
            create("ent:cart", "Cart"),
            create("ent:order", "Order"),
            req("req:t-1", "The cart opens."),
            req("req:t-2", "The cart closes."),
        ],
        &session(),
    );
    // Defaults derived: a class view per scope and the cart's flow.
    assert!(s.graph.views["view:class/public"].default);
    assert_eq!(
        s.graph.views["view:usecase/t"].members,
        vec!["req:t-1", "req:t-2"]
    );
    let r = s.apply(
        vec![Op::DeleteView {
            id: "view:class/public".into(),
            reason: "noise".into(),
        }],
        &session(),
    );
    assert_eq!(r.applied, 0);
    assert!(r.skipped[0].contains("default view"), "{:?}", r.skipped);
    // The scope root's level view lists its members and carries no query.
    assert_eq!(
        s.graph.views["view:class/public"].members,
        vec!["ent:cart", "ent:order"]
    );
    assert!(s.graph.views["view:class/public"].query.is_none());
    // An upsert on the default's kind and title lands on it and curates it.
    let r = s.apply(
        vec![Op::CreateView {
            id: "view:class/whatever".into(),
            view: View {
                kind: "class".into(),
                title: "public".into(),
                members: vec!["ent:cart".into()],
                query: Some(ViewQuery {
                    scope: Some("public".into()),
                    ..Default::default()
                }),
                provenance: Some(derived(&["ent:cart"])),
                ..Default::default()
            },
        }],
        &session(),
    );
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    let v = &s.graph.views["view:class/public"];
    assert!(!v.default);
    // The query the session gave the curated view keeps recomputing membership:
    // the match the session left out joins as a query-match change.
    assert_eq!(v.members, vec!["ent:cart", "ent:order"]);
    assert!(s.status.has_change(CHANGE_QUERY_MATCH, "view:class/public"));
    assert!(!s.graph.views.contains_key("view:class/whatever"));
    // A curated view is never removed by the recompute, and its members survive.
    s.apply(
        vec![Op::UpdateView {
            id: "view:usecase/t".into(),
            title: Some("Cart flow".into()),
            members: None,
            add_members: Vec::new(),
            remove_members: Vec::new(),
            query: None,
            collapse: None,
            exclude: vec![Exclusion {
                id: "req:t-2".into(),
                note: "example, not flow".into(),
            }],
            reasoning: Some("trimmed".into()),
        }],
        &session(),
    );
    let v = &s.graph.views["view:usecase/t"];
    assert!(!v.default);
    assert_eq!(v.title, "Cart flow");
    assert_eq!(v.members, vec!["req:t-1"]);
    assert_eq!(v.excluded.len(), 1);
    // Deleting a member of a curated view is allowed; the view owes a retrace.
    let r = s.apply(
        vec![Op::DeleteRequirement {
            id: "req:t-1".into(),
            reason: "gone".into(),
        }],
        &session(),
    );
    assert_eq!(r.applied, 1);
    assert_eq!(s.graph.views["view:usecase/t"].members, vec!["req:t-1"]);
    let gone = s
        .status
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_VIEW_MEMBER_GONE)
        .expect("view-member-gone");
    assert_eq!(gone.subject, "view:usecase/t");
    assert_eq!(gone.via, "members");
    assert_eq!(gone.detail["gone"], serde_json::json!(["req:t-1"]));
    // A bump on a view records the limit and curates it; a limit of the wrong kind
    // is refused.
    let r = s.apply(
        vec![
            Op::BumpLimit {
                id: "view:class/public".into(),
                limit: "members-per-structural-view".into(),
                value: 25,
                provenance: Provenance::Decree {
                    author: "owner".into(),
                    at: "now".into(),
                    note: None,
                },
            },
            Op::BumpLimit {
                id: "ent:cart".into(),
                limit: "edges-per-view".into(),
                value: 25,
                provenance: Provenance::Decree {
                    author: "owner".into(),
                    at: "now".into(),
                    note: None,
                },
            },
        ],
        &Commit::store("decree"),
    );
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    assert_eq!(
        s.graph.views["view:class/public"].limits["members-per-structural-view"].value,
        25
    );
    assert!(r.skipped[0].contains("does not apply to an entity"));
    let entry: JournalEntry =
        yaml_to(&crate::layout::journal_dir(&s.out).join(format!("g{}.yaml", r.generation)))
            .unwrap();
    assert_eq!(entry.kind, "decree");
    assert_eq!(entry.mutations[0]["op"], "bump_limit");
    // Now deletable.
    let r = s.apply(
        vec![Op::DeleteView {
            id: "view:class/public".into(),
            reason: "noise".into(),
        }],
        &session(),
    );
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    // And the next commit derives it again as a default.
    s.apply(vec![], &session());
    assert!(s.graph.views["view:class/public"].default);
}

// The 51st requirement crosses requirements-per-entity; a bump on the entity
// raises the threshold and the record clears. Mirrors docs/compiler/graph.md#limits.
#[test]
fn threshold_crossed_on_the_fifty_first_requirement_and_cleared_by_a_bump() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\nthe order\n");
    let mut ops = vec![create("ent:order", "Order")];
    for i in 0..50 {
        ops.push(Op::CreateRequirement {
            id: format!("req:t-{}", i + 1),
            requirement: Requirement {
                statement: format!("The order does thing number {}.", i),
                entities: vec!["ent:order".into()],
                source: Some(mention("t.md", "/t", "the order")),
                ..Default::default()
            },
        });
    }
    s.apply(ops, &session());
    assert!(!s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:order"));
    let r = s.apply(
        vec![Op::CreateRequirement {
            id: "req:t-51".into(),
            requirement: Requirement {
                statement: "The order does one thing more.".into(),
                entities: vec!["ent:order".into()],
                source: Some(mention("t.md", "/t", "the order")),
                ..Default::default()
            },
        }],
        &session(),
    );
    let crossed = r
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_THRESHOLD_CROSSED)
        .expect("threshold-crossed");
    assert_eq!(crossed.subject, "ent:order");
    assert_eq!(crossed.via, "limits");
    assert_eq!(crossed.detail["limit"], "requirements-per-entity");
    assert_eq!(crossed.detail["count"], 51);
    assert_eq!(crossed.detail["level"], "soft");
    assert_eq!(crossed.detail["goal"], "abstract-entity");
    let id = crossed.id.clone();
    // Another commit keeps the record (no duplicate, same id), refreshing the count.
    let r = s.apply(
        vec![Op::CreateRequirement {
            id: "req:t-52".into(),
            requirement: Requirement {
                statement: "The order ships by courier.".into(),
                entities: vec!["ent:order".into()],
                source: Some(mention("t.md", "/t", "the order")),
                ..Default::default()
            },
        }],
        &session(),
    );
    assert!(r.changes.iter().all(|c| c.kind != CHANGE_THRESHOLD_CROSSED));
    let standing: Vec<&ChangeRecord> = s
        .status
        .changes
        .iter()
        .filter(|c| c.kind == CHANGE_THRESHOLD_CROSSED)
        .collect();
    assert_eq!(standing.len(), 1);
    assert_eq!(standing[0].id, id);
    assert_eq!(standing[0].detail["count"], 52);
    // The bump raises the threshold: the crossing lapses.
    s.apply(
        vec![Op::BumpLimit {
            id: "ent:order".into(),
            limit: "requirements-per-entity".into(),
            value: 70,
            provenance: Provenance::Decree {
                author: "owner".into(),
                at: "now".into(),
                note: Some("the order is the hub".into()),
            },
        }],
        &Commit::store("decree"),
    );
    assert!(!s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:order"));
    assert_eq!(
        s.graph.entities["ent:order"].limits["requirements-per-entity"].value,
        70
    );
}
