use super::*;
use crate::model::{hash_hex, Requirement, SourceRef};
use crate::reconcile::test_support::*;
use crate::store::Store;
use serde_json::Value;
use std::collections::BTreeMap;

// A rate limit trips the breaker at once; any endpoint failure spends no goal's
// attempt; a session that spent tokens resets the streak.
// Mirrors docs/compiler/reconciler.md#escalation.
#[test]
fn a_rate_limit_trips_the_breaker_and_spends_no_attempt() {
    let b = crate::limits::ENDPOINT_BREAKER;
    assert_eq!(
        breaker_step(
            0,
            0,
            Some("http error: HTTP 429 :: {\"error\":\"Rate limit exceeded\"}")
        ),
        (b, true)
    );
    assert_eq!(
        breaker_step(0, 0, Some("You've hit your usage limit.")),
        (b, true)
    );
    assert_eq!(
        breaker_step(0, 0, Some("The input is too large.")),
        (b, true)
    );
    assert_eq!(
        breaker_step(2, 0, Some("transport: connection refused")),
        (3, true)
    );
    assert_eq!(
        breaker_step(3, 1200, Some("the session left the batch open")),
        (0, false)
    );
    assert_eq!(breaker_step(3, 0, None), (0, false));
    assert!(!rate_limited("the batch failed its gate"));
}

#[test]
fn the_verdict_displays_its_counts() {
    let mut s = crate::derive::tests::showcase_store();
    s.status.generation = 3;
    let proj = crate::board::tests::project_for(&s);
    let board = Board::derive(&s, &proj, &crate::board::tests::control_auto());
    let v = board.verdict();
    assert_eq!(v.state, "incomplete");
    assert!(
        v.open >= 4,
        "every uncovered section is an open goal: {}",
        v.open
    );
    assert_eq!(
        v.to_string(),
        format!(
            "incomplete: {} open, 0 failed, {} blocked, 0 optional advised",
            v.open, v.blocked
        )
    );
    assert!(
        v.blocked > 0,
        "the ledger goals wait on the generate release"
    );
    assert!(
        board
            .summary_line()
            .starts_with(&format!("compile: {} goals (", v.open)),
        "{}",
        board.summary_line()
    );
}

// The levels loop end to end with no LLM. A stated backend holding twelve stated
// children, each mentioned in one of three documents and tied to its document
// mates by requirements, crosses `children-per-entity` at commit; the board derives
// the optional fan-out goal whose coupling partition follows the documents; a tool
// session groups each candidate, marks the goal done behind the fan-out gate, and
// commits; the groupings land with derived provenance, the record clears, and every
// level derives its view; a later move that thins one grouping under two has the
// sweep dissolve it behind a redirect while the views recompute and the check stays
// silent. Mirrors docs/compiler/concepts/levels.md.
#[test]
fn levels_loop_groups_a_wide_level_and_dissolves_a_thin_grouping() {
    use crate::board::tests::{control_auto, project_for};
    use crate::derive::{children_of_view, level_view_id};
    use crate::limits::{CHILDREN_PER_ENTITY, CHILDREN_PER_ENTITY_HARD, CHILDREN_PER_ENTITY_SOFT};
    use crate::store::{Commit, Op, CHANGE_THRESHOLD_CROSSED};
    use crate::tools::{ToolSession, WorkScope};

    let out = std::env::temp_dir().join(format!(
        "jazyk-levels-loop-test-{}",
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&out).ok();
    crate::vfs::create_dir_all(&out).ok();
    let mut s = Store {
        out,
        ..Default::default()
    };

    // Step 1: the backend's own document and one document per area of its children,
    // every section covered as a converged build leaves it.
    let texts: [(&str, &str); 4] = [
        ("arch.md", "# Architecture\n\nThe backend serves the shop.\n"),
        (
            "orders.md",
            "# Orders\n\nThe cart holds items.\nThe pricing engine prices the cart.\nThe checkout confirms the cart.\nThe discount applies to the checkout.\nThe invoice records the checkout.\nThe cart enqueues work on the queue.\n",
        ),
        (
            "shipping.md",
            "# Shipping\n\nThe shipment travels with a carrier.\nThe tracking follows the shipment.\nThe label marks the shipment.\nThe shipment notifies through the mailer.\n",
        ),
        (
            "infra.md",
            "# Infra\n\nThe queue feeds the mailer.\nThe cache fronts the queue.\n",
        ),
    ];
    let mut section_of: BTreeMap<&str, String> = BTreeMap::new();
    for (doc, text) in texts {
        seed_doc(&mut s, doc, text);
        let rec = s.docs.get_mut(doc).unwrap();
        let refs: Vec<String> = rec.sections.keys().cloned().collect();
        section_of.insert(doc, refs[0].clone());
        for r in refs {
            rec.coverage.insert(
                r,
                Coverage {
                    state: "covered".into(),
                    note: None,
                    claimed_by: Some("g1".into()),
                },
            );
        }
    }
    let src = |doc: &str, quote: &str| SourceRef {
        doc: doc.into(),
        section: section_of[doc].clone(),
        quote: quote.into(),
    };
    let stated = |name: &str, doc: &str, quote: &str, parent: Option<&str>| Entity {
        name: name.into(),
        parent: parent.map(String::from),
        mentions: vec![src(doc, quote)],
        ..Default::default()
    };
    // (id, name, document, quote) per child: five in orders, four in shipping,
    // three in infra.
    let children: [(&str, &str, &str, &str); 12] = [
        ("ent:cart", "Cart", "orders.md", "The cart holds items"),
        (
            "ent:pricing",
            "Pricing Engine",
            "orders.md",
            "The pricing engine prices",
        ),
        (
            "ent:checkout",
            "Checkout",
            "orders.md",
            "The checkout confirms",
        ),
        (
            "ent:discount",
            "Discount",
            "orders.md",
            "The discount applies",
        ),
        ("ent:invoice", "Invoice", "orders.md", "The invoice records"),
        (
            "ent:shipment",
            "Shipment",
            "shipping.md",
            "The shipment travels",
        ),
        ("ent:carrier", "Carrier", "shipping.md", "with a carrier"),
        (
            "ent:tracking",
            "Tracking",
            "shipping.md",
            "The tracking follows",
        ),
        ("ent:label", "Label", "shipping.md", "The label marks"),
        ("ent:queue", "Queue", "infra.md", "The queue feeds"),
        ("ent:mailer", "Mailer", "infra.md", "feeds the mailer"),
        ("ent:cache", "Cache", "infra.md", "The cache fronts"),
    ];
    let mut ops = vec![Op::CreateEntity {
        id: "ent:backend".into(),
        entity: Entity {
            stereotype: Some("system".into()),
            ..stated("Backend", "arch.md", "The backend serves", None)
        },
    }];
    for (id, name, doc, quote) in children {
        ops.push(Op::CreateEntity {
            id: id.into(),
            entity: stated(name, doc, quote, Some("ent:backend")),
        });
    }
    // (id, statement, entities, document, edge) per requirement: a requirement with
    // an edge ties its pair by a requirement and a derived relationship (weight two),
    // the two cross-area requirements carry no edge (weight one), so the partition
    // follows the documents.
    let reqs: [(&str, &str, [&str; 2], &str, bool); 11] = [
        (
            "req:orders-1",
            "The pricing engine prices the cart.",
            ["ent:pricing", "ent:cart"],
            "orders.md",
            true,
        ),
        (
            "req:orders-2",
            "The checkout confirms the cart.",
            ["ent:checkout", "ent:cart"],
            "orders.md",
            true,
        ),
        (
            "req:orders-3",
            "The discount applies to the checkout.",
            ["ent:discount", "ent:checkout"],
            "orders.md",
            true,
        ),
        (
            "req:orders-4",
            "The invoice records the checkout.",
            ["ent:invoice", "ent:checkout"],
            "orders.md",
            true,
        ),
        (
            "req:orders-5",
            "The cart enqueues work on the queue.",
            ["ent:cart", "ent:queue"],
            "orders.md",
            false,
        ),
        (
            "req:shipping-1",
            "The shipment travels with a carrier.",
            ["ent:shipment", "ent:carrier"],
            "shipping.md",
            true,
        ),
        (
            "req:shipping-2",
            "The tracking follows the shipment.",
            ["ent:tracking", "ent:shipment"],
            "shipping.md",
            true,
        ),
        (
            "req:shipping-3",
            "The label marks the shipment.",
            ["ent:label", "ent:shipment"],
            "shipping.md",
            true,
        ),
        (
            "req:shipping-4",
            "The shipment notifies through the mailer.",
            ["ent:shipment", "ent:mailer"],
            "shipping.md",
            false,
        ),
        (
            "req:infra-1",
            "The queue feeds the mailer.",
            ["ent:queue", "ent:mailer"],
            "infra.md",
            true,
        ),
        (
            "req:infra-2",
            "The cache fronts the queue.",
            ["ent:cache", "ent:queue"],
            "infra.md",
            true,
        ),
    ];
    ops.push(Op::CreateRequirement {
        id: "req:arch-1".into(),
        requirement: Requirement {
            statement: "The backend serves the shop.".into(),
            entities: vec!["ent:backend".into()],
            source: Some(src("arch.md", "The backend serves the shop")),
            ..Default::default()
        },
    });
    for (id, statement, [a, b], doc, edge) in reqs {
        ops.push(Op::CreateRequirement {
            id: id.into(),
            requirement: Requirement {
                statement: statement.into(),
                entities: vec![a.into(), b.into()],
                edges: if edge {
                    vec![ReqEdge {
                        a: a.into(),
                        b: b.into(),
                        rel_type: None,
                        cardinality: None,
                    }]
                } else {
                    Vec::new()
                },
                source: Some(src(doc, statement.trim_end_matches('.'))),
                ..Default::default()
            },
        });
    }

    // Step 2: one commit lands the level and crosses the limit on the backend.
    let report = s.apply(ops, &Commit::session(Vec::new(), 1, None));
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let crossed = report
        .changes
        .iter()
        .find(|c| c.kind == CHANGE_THRESHOLD_CROSSED && c.subject == "ent:backend")
        .expect("threshold-crossed on the backend");
    assert_eq!(crossed.detail["limit"], CHILDREN_PER_ENTITY);
    assert_eq!(crossed.detail["count"], 12);
    assert_eq!(crossed.detail["soft"], CHILDREN_PER_ENTITY_SOFT);
    assert_eq!(crossed.detail["hard"], CHILDREN_PER_ENTITY_HARD);
    assert_eq!(crossed.detail["level"], "soft");
    assert!(!s
        .status
        .has_change(CHANGE_THRESHOLD_CROSSED, "scope:public"));
    // The ingest records this commit wrote open the model's ingest goals; this
    // scenario has no model, so they are cleared as a resolving session clears them,
    // leaving the fan-out record the loop stands on.
    let ingest: Vec<String> = s
        .status
        .changes
        .iter()
        .filter(|c| c.kind != CHANGE_THRESHOLD_CROSSED)
        .map(|c| c.id.clone())
        .collect();
    s.clear_changes(&ingest);

    // Step 3: the board derives the optional fan-out goal with the document-shaped
    // partition, ready because no compile goal is open in its cone.
    let board = Board::derive(&s, &project_for(&s), &control_auto());
    let open: Vec<&str> = board.open_goals().iter().map(|g| g.id.as_str()).collect();
    assert_eq!(open, vec!["g:abstract-entity:ent:backend"]);
    let goal = board.goal("g:abstract-entity:ent:backend").unwrap().clone();
    assert!(!goal.mandatory);
    assert_eq!(goal.class, "gc");
    assert!(
        board.is_ready(&goal.id),
        "blockers: {:?}",
        board.cone_blockers(&goal.id)
    );
    assert_eq!(goal.change["fan_out"], 12);
    assert_eq!(
        goal.change["limit"],
        json!({"soft": CHILDREN_PER_ENTITY_SOFT, "hard": CHILDREN_PER_ENTITY_HARD})
    );
    // A candidate never grows past half the soft threshold, so the pricing
    // entity stays out of the ordering candidate; the session adds it back,
    // a candidate adjusted with a reason.
    let cands = goal.change["candidates"].clone();
    assert_eq!(
        cands,
        json!([
            ["ent:carrier", "ent:label", "ent:shipment", "ent:tracking"],
            ["ent:cart", "ent:checkout", "ent:discount", "ent:invoice"],
            ["ent:cache", "ent:mailer", "ent:queue"],
        ])
    );
    let mut cands = cands;
    cands[1].as_array_mut().unwrap().push(json!("ent:pricing"));

    // Step 4: a session scoped to the goal groups each candidate, marks the goal
    // done, and passes the batch gates.
    let mut t = ToolSession::new(
        s.clone(),
        WorkScope::for_batch("b4-1", std::slice::from_ref(&goal)),
        64,
        24_000,
    );
    let groupings: [(&str, &str, &str, &str); 3] = [
        (
            "ent:fulfillment",
            "Fulfillment",
            "Carries a shipment to the customer and tracks it.",
            "shipping.md states every member and treats them as one area",
        ),
        (
            "ent:ordering",
            "Ordering",
            "Prices, confirms, and invoices what the cart holds.",
            "orders.md states every member and treats them as one area",
        ),
        (
            "ent:platform",
            "Platform",
            "Queues, mails, and caches the work the areas hand off.",
            "infra.md states every member and treats them as one area",
        ),
    ];
    for (i, (id, name, definition, reasoning)) in groupings.iter().enumerate() {
        let members = cands[i].clone();
        let r = t
            .dispatch(
                "group_entities",
                &json!({
                    "name": name, "definition": definition, "members": members,
                    "stereotype": "module", "reasoning": reasoning,
                }),
            )
            .unwrap();
        assert_eq!(r["id"], *id);
        assert_eq!(r["moved"], members);
    }
    t.dispatch(
        "mark_goal_done",
        &json!({
            "goal": goal.id,
            "justification": "Ordering, Fulfillment, and Platform are the three areas the documents are written around. Each candidate partition matched one document, so the domain recognizes the split.",
        }),
    )
    .unwrap();
    t.dispatch(
        "done",
        &json!({"summary": "grouped the backend's level into its three documented areas"}),
    )
    .unwrap();
    assert!(t.done.is_some());

    // Step 5: the changeset commits; the groupings stand, the record clears, and the
    // level views derive.
    let staged = std::mem::take(&mut t.staged);
    let commit = t.commit(2, None);
    assert_eq!(commit.resolved.len(), 1);
    assert_eq!(commit.resolved[0].goal, goal.id);
    let report = s.apply(staged, &commit);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    for (i, (id, _, _, reasoning)) in groupings.iter().enumerate() {
        let g = &s.graph.entities[*id];
        assert_eq!(g.parent.as_deref(), Some("ent:backend"));
        assert!(g.mentions.is_empty());
        assert_eq!(g.stereotype.as_deref(), Some("module"));
        match &g.provenance {
            Some(Provenance::Derived {
                from,
                reasoning: why,
            }) => {
                let mut from = from.clone();
                from.sort();
                assert_eq!(json!(from), cands[i]);
                assert_eq!(why, reasoning);
            }
            other => panic!("{}: expected derived provenance, got {:?}", id, other),
        }
        for m in cands[i].as_array().unwrap() {
            assert_eq!(
                s.graph.entities[m.as_str().unwrap()].parent.as_deref(),
                Some(*id),
                "{} reparented under {}",
                m,
                id
            );
        }
        let view_id = format!("view:class/{}", id.trim_start_matches("ent:"));
        assert_eq!(level_view_id(&s, id).as_deref(), Some(view_id.as_str()));
        let v = &s.graph.views[&view_id];
        assert_eq!(v.kind, "class");
        assert!(v.default);
        let mut members = v.members.clone();
        members.sort();
        assert_eq!(json!(members), cands[i]);
    }
    assert_eq!(
        levels(&s)
            .into_iter()
            .find(|(t, _)| t == "ent:backend")
            .map(|(_, c)| c.len()),
        Some(3)
    );
    assert!(!s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:backend"));
    assert_eq!(
        level_view_id(&s, "ent:backend").as_deref(),
        Some("view:component/backend")
    );
    let top = &s.graph.views["view:component/backend"];
    assert_eq!(top.kind, "component");
    assert!(top.default);
    assert_eq!(
        top.members,
        vec!["ent:fulfillment", "ent:ordering", "ent:platform"]
    );
    assert_eq!(
        children_of_view(&s, "view:component/backend"),
        vec![
            (
                "ent:fulfillment".to_string(),
                "view:class/fulfillment".to_string()
            ),
            (
                "ent:ordering".to_string(),
                "view:class/ordering".to_string()
            ),
            (
                "ent:platform".to_string(),
                "view:class/platform".to_string()
            ),
        ]
    );
    assert!(level_shape(&s).is_empty(), "{:?}", level_shape(&s));

    // Step 6: a write moves all but one member of the smallest grouping under
    // another; the sweep dissolves the thin grouping behind a redirect.
    let mut w = ToolSession::new(s.clone(), WorkScope::serving("mcp-write"), 64, 24_000);
    for id in ["ent:queue", "ent:mailer"] {
        w.dispatch(
            "update_entity",
            &json!({"id": id, "parent": "ent:fulfillment"}),
        )
        .unwrap();
    }
    let staged = std::mem::take(&mut w.staged);
    assert_eq!(staged.len(), 2);
    let report = s.apply(staged, &Commit::store("decree"));
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    // The sweep runs behind every commit as its own `gc` entry, the decree's
    // included, and the report carries what it did; a second sweep finds nothing.
    // Mirrors docs/compiler/graph.md#the-sweep.
    let actions = report.swept.clone();
    assert!(
        actions
            .iter()
            .any(|a| a
                .starts_with("dissolved ent:platform (1 child(ren) reparented to ent:backend)")),
        "{:?}",
        actions
    );
    assert!(s.gc().is_empty());
    assert!(!s.graph.entities.contains_key("ent:platform"));
    assert_eq!(s.resolve_id("ent:platform"), "ent:backend");
    assert_eq!(
        s.graph.entities["ent:cache"].parent.as_deref(),
        Some("ent:backend")
    );
    for id in ["ent:queue", "ent:mailer"] {
        assert_eq!(
            s.graph.entities[id].parent.as_deref(),
            Some("ent:fulfillment")
        );
    }
    let top = &s.graph.views["view:component/backend"];
    assert_eq!(
        top.members,
        vec!["ent:cache", "ent:fulfillment", "ent:ordering"]
    );
    assert!(!s.graph.views.contains_key("view:class/platform"));
    // Anything holding the dissolved id resolves through the redirect to the
    // parent's level.
    assert_eq!(
        level_view_id(&s, "ent:platform").as_deref(),
        Some("view:component/backend")
    );
    // The fulfillment level lists its six children and the cache as the outside
    // entity whose edge to the queue now lifts into the level.
    let mut fulfillment = s.graph.views["view:class/fulfillment"].members.clone();
    fulfillment.sort();
    assert_eq!(
        fulfillment,
        vec![
            "ent:cache",
            "ent:carrier",
            "ent:label",
            "ent:mailer",
            "ent:queue",
            "ent:shipment",
            "ent:tracking"
        ]
    );
    assert_eq!(
        children_of_view(&s, "view:component/backend"),
        vec![
            (
                "ent:fulfillment".to_string(),
                "view:class/fulfillment".to_string()
            ),
            (
                "ent:ordering".to_string(),
                "view:class/ordering".to_string()
            ),
        ]
    );
    assert!(!s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:backend"));
    assert!(level_shape(&s).is_empty(), "{:?}", level_shape(&s));

    // Step 7: the board is stable: a repeated derivation opens nothing new, and no
    // fan-out goal stands on the backend. What stands is the ripple the loop
    // itself opened: a move is the parents' business, so the backend (children
    // left it for the groupings, the cache came back) and each minted grouping
    // (a fresh entity, then a parent that gained and lost children) is an entity
    // change the review goal reads; the moved children themselves are not, one
    // review per grouping instead of one per member. A model session resolves
    // those, none is a level goal.
    let ids = |b: &Board| -> Vec<String> { b.open_goals().iter().map(|g| g.id.clone()).collect() };
    let first = ids(&Board::derive(&s, &project_for(&s), &control_auto()));
    let second = ids(&Board::derive(&s, &project_for(&s), &control_auto()));
    assert_eq!(first, second);
    assert!(
        !first.iter().any(|g| g.starts_with("g:abstract-entity:")),
        "{:?}",
        first
    );
    let _ = &children;
    assert_eq!(
        first,
        vec![
            "g:review-entity:ent:backend".to_string(),
            "g:review-entity:ent:fulfillment".to_string(),
            "g:review-entity:ent:ordering".to_string(),
        ]
    );
}

// A record the commit recomputes while its condition holds re-opens the goal a
// session just resolved: the stub session marks every batch done and changes
// nothing, so each fan-out goal comes back after the other's commit, resolved
// and re-derived with the same change. The second cycle parks it, well under
// the build cap. Mirrors docs/compiler/reconciler.md#parked-and-failed.
#[test]
fn a_goal_resolved_and_reopened_with_the_same_change_parks_at_its_second_cycle() {
    use crate::board::tests::{control_auto, project_for};
    use crate::limits::{CHILDREN_PER_ENTITY_HARD, CHILDREN_PER_ENTITY_SOFT, REOPEN_CYCLES};
    use crate::runner::{BatchRun, SessionReport, SessionRunner};
    use crate::store::{Commit, Op, CHANGE_THRESHOLD_CROSSED};
    use std::sync::{Arc, Mutex};

    // Marks every goal of its batch done and stages nothing.
    struct DoneRunner {
        out: std::path::PathBuf,
    }
    impl SessionRunner for DoneRunner {
        fn agent_name(&self) -> String {
            "done-stub".into()
        }
        fn set_build_token(&self, _token: Option<String>) {}
        fn run_item(&self, batch: &BatchRun, _trace: &Trace) -> SessionReport {
            let mut s = Store::load(&self.out);
            let mut commit = Commit::session(batch.goal_ids(), 1, Some(10));
            commit.resolved = batch
                .goals
                .iter()
                .map(|g| Resolved {
                    goal: g.id.clone(),
                    justification: "the level reads well as it stands".into(),
                    evidence: Value::Null,
                })
                .collect();
            let report = s.apply(vec![], &commit);
            assert!(report.skipped.is_empty(), "{:?}", report.skipped);
            SessionReport {
                applied: 0,
                rounds: 1,
                tokens: 10,
                cached: 0,
                failed: None,
                resolved: batch.goal_ids(),
                stopped: None,
            }
        }
        fn ask(&self, _s: &str, _u: &str, _l: &str, _st: &str) -> Result<String, String> {
            Err("stub".into())
        }
        fn ask_traced(
            &self,
            _s: &str,
            _u: &str,
            _l: &str,
            _st: &str,
            _t: Option<&Trace>,
        ) -> Result<String, String> {
            Err("stub".into())
        }
    }

    let out = std::env::temp_dir().join(format!(
        "jazyk-reopen-cycle-test-{}",
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&out).ok();
    crate::vfs::create_dir_all(&out).unwrap();
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    // Two levels over the fan-out limit: the backend past the hard threshold (a
    // mandatory goal), the frontend past the soft one (optional), so they form
    // separate GC batches and alternate. Decreed children stay out of the sweep.
    let decree = Provenance::Decree {
        author: "owner".into(),
        at: "2026-10-09".into(),
        note: None,
    };
    let mut ops = Vec::new();
    for (parent, name, count) in [
        ("ent:backend", "Backend", CHILDREN_PER_ENTITY_HARD + 1),
        ("ent:frontend", "Frontend", CHILDREN_PER_ENTITY_SOFT + 1),
    ] {
        ops.push(Op::CreateEntity {
            id: parent.into(),
            entity: Entity {
                name: name.into(),
                stereotype: Some("system".into()),
                provenance: Some(decree.clone()),
                ..Default::default()
            },
        });
        for i in 0..count {
            ops.push(Op::CreateEntity {
                id: format!("{}-part-{}", parent, i),
                entity: Entity {
                    name: format!("{} Part {}", name, i),
                    parent: Some(parent.into()),
                    provenance: Some(decree.clone()),
                    ..Default::default()
                },
            });
        }
    }
    let report = s.apply(ops, &Commit::session(Vec::new(), 1, None));
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert!(s.status.has_change(CHANGE_THRESHOLD_CROSSED, "ent:backend"));
    assert!(s
        .status
        .has_change(CHANGE_THRESHOLD_CROSSED, "ent:frontend"));
    // No model ingests here: only the fan-out records stand.
    let ingest: Vec<String> = s
        .status
        .changes
        .iter()
        .filter(|c| c.kind != CHANGE_THRESHOLD_CROSSED)
        .map(|c| c.id.clone())
        .collect();
    s.clear_changes(&ingest);

    let events: Arc<Mutex<Vec<(String, String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let trace = Trace::to_sink(
        crate::session::TraceLevel::Normal,
        Arc::new(move |ev: &TraceEvent| {
            if let TraceEvent::Goal {
                goal,
                event,
                reason,
                ..
            } = ev
            {
                sink.lock().unwrap().push((
                    goal.clone(),
                    event.clone(),
                    reason.clone().unwrap_or_default(),
                ));
            }
        }),
        Default::default(),
    );
    let proj = project_for(&s);
    let control = control_auto();
    let runner = DoneRunner { out: out.clone() };
    let parsed = BTreeMap::new();
    let run = run_sessions(
        &proj, &out, &trace, None, &runner, &mut s, &parsed, &control,
    );

    // Both goals derived; the cap would allow 3 x 2 + 8 sessions. The loop ran
    // backend, frontend, backend, frontend and stopped: the backend's goal parked
    // at its second return, the frontend's was resolved and did not come back.
    assert_eq!(run.derived, 2);
    let cap = crate::limits::BUILD_SESSION_FACTOR * 2 + crate::limits::BUILD_SESSION_FLOOR;
    assert_eq!(run.sessions, 4, "against a cap of {}", cap);
    assert!(run.sessions as usize * 2 < cap);
    // The backend's goal was resolved, came back with the same change after the
    // frontend's commit, was resolved again, came back again: parked at the
    // second cycle, the reason naming the cycle.
    let backend = "g:abstract-entity:ent:backend";
    let parked: Vec<&ParkedGoal> = run.parked.iter().filter(|g| g.goal.id == backend).collect();
    assert_eq!(parked.len(), 1, "{:?}", run.parked);
    assert!(
        run.parked.iter().all(|g| g.goal.id == backend),
        "{:?}",
        run.parked
    );
    assert!(
        parked[0]
            .reason
            .as_deref()
            .is_some_and(|r| r.starts_with("reopened-after-resolve")),
        "{:?}",
        parked[0].reason
    );
    let ev = events.lock().unwrap();
    let park = ev
        .iter()
        .find(|(g, e, _)| g == backend && e == "parked")
        .expect("a parked event for the backend's goal");
    assert!(park.2.starts_with("reopened-after-resolve"), "{}", park.2);
    assert!(
        park.2.contains(&format!("{} times", REOPEN_CYCLES)),
        "{}",
        park.2
    );
    let reopened = ev
        .iter()
        .filter(|(g, e, _)| g == backend && e == "reopened")
        .count();
    assert_eq!(reopened as u32, REOPEN_CYCLES);
    let resolved = ev
        .iter()
        .filter(|(g, e, _)| g == backend && e == "resolved")
        .count();
    assert_eq!(resolved as u32, REOPEN_CYCLES);
    drop(ev);

    // The tail persists the parked goal whole in status.yaml, like any parked
    // goal; the next derivation resumes it parked and explain shows it so.
    let _ = finalize(&mut s, &proj, run.parked, &run.costs, &trace);
    let status = Store::load(&out).status;
    let entry = status
        .parked
        .iter()
        .find(|p| p.goal.id == backend)
        .unwrap_or_else(|| panic!("{:?}", status.parked));
    assert!(
        entry
            .reason
            .as_deref()
            .is_some_and(|r| r.starts_with("reopened-after-resolve")),
        "{:?}",
        entry.reason
    );
    let board = Board::derive(&s, &proj, &control);
    let text = board.explain(&s, backend).expect("explain");
    assert!(text.contains("parked"), "{}", text);
    assert!(text.contains("parked: reopened-after-resolve"), "{}", text);
    // The whole-build report names the reason beside the parked goal.
    let report =
        render_ripple(&ripple(&s, &format!("g{}", s.status.generation), false).expect("ripple"));
    assert!(
        report.contains(&format!("parked: {} (reopened-after-resolve", backend)),
        "{}",
        report
    );
    crate::vfs::remove_dir_all(&out).ok();
}

#[test]
fn an_unfinished_goal_gets_four_fresh_sessions_before_parking() {
    use crate::board::tests::{control_auto, project_for};
    use crate::runner::{BatchRun, SessionReport, SessionRunner};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct Unfinished(AtomicUsize);
    impl SessionRunner for Unfinished {
        fn agent_name(&self) -> String {
            "unfinished-stub".into()
        }
        fn set_build_token(&self, _: Option<String>) {}
        fn run_item(&self, _: &BatchRun, _: &Trace) -> SessionReport {
            self.0.fetch_add(1, Ordering::SeqCst);
            SessionReport {
                applied: 0,
                rounds: 1,
                tokens: 10,
                cached: 0,
                failed: None,
                resolved: Vec::new(),
                stopped: None,
            }
        }
        fn ask(&self, _: &str, _: &str, _: &str, _: &str) -> Result<String, String> {
            Err("stub".into())
        }
        fn ask_traced(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: Option<&Trace>,
        ) -> Result<String, String> {
            Err("stub".into())
        }
    }
    let out = std::env::temp_dir().join(format!(
        "jazyk-attempt-tolerance-{}",
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&out).ok();
    crate::vfs::create_dir_all(&out).unwrap();
    let text = "# Main\n\nThe system accepts an order.\n";
    let parsed: ParsedDocs = [(
        "docs/main.md".into(),
        (hash_hex(text), crate::md::parse_sections(text)),
    )]
    .into();
    let mut store = Store {
        out: out.clone(),
        ..Default::default()
    };
    store.sync_docs(&parsed);
    let project = project_for(&store);
    let control = control_auto();
    let trace = Trace::to_sink(
        crate::session::TraceLevel::Normal,
        Arc::new(|_: &TraceEvent| {}),
        Default::default(),
    );
    let runner = Unfinished(AtomicUsize::new(0));
    let run = run_sessions(
        &project, &out, &trace, None, &runner, &mut store, &parsed, &control,
    );
    assert_eq!(runner.0.load(Ordering::SeqCst), 4);
    assert_eq!(run.sessions, 4);
    assert_eq!(run.parked.len(), 1);
    assert!(run.parked[0]
        .reason
        .as_deref()
        .unwrap()
        .contains("after 4 attempts"));
    crate::vfs::remove_dir_all(&out).ok();
}

// A stopped build keeps the goals it parked before the stop.
// Mirrors docs/compiler/compilation.md#a-build.
#[test]
fn a_cancelled_build_keeps_the_goals_it_parked() {
    use std::sync::Arc;
    let out = std::env::temp_dir().join(format!("jazyk-cancel-parks-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    crate::vfs::create_dir_all(&out).unwrap();
    let text = "# Main\n\nThe system accepts an order.\n";
    let parsed: ParsedDocs = [(
        "docs/main.md".into(),
        (hash_hex(text), crate::md::parse_sections(text)),
    )]
    .into();
    let mut store = Store {
        out: out.clone(),
        ..Default::default()
    };
    store.sync_docs(&parsed);
    let project = crate::board::tests::project_for(&store);
    let cancel: Arc<std::sync::atomic::AtomicBool> = Default::default();
    let trace = Trace::to_sink(
        crate::session::TraceLevel::Normal,
        Arc::new(|_: &TraceEvent| {}),
        cancel.clone(),
    );
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    let goal = Goal {
        id: "g:reconcile-section:docs/main.md#/main".into(),
        kind: "reconcile-section".into(),
        class: "compile".into(),
        mandatory: true,
        target: "docs/main.md#/main".into(),
        ..Default::default()
    };
    let parked = vec![ParkedGoal::new(
        goal.clone(),
        "left open after 4 attempts in session b1-1",
    )];
    let report = finalize(&mut store, &project, parked, &Costs::default(), &trace);
    assert!(
        report.verdict.starts_with("incomplete"),
        "{}",
        report.verdict
    );
    assert!(
        store.status.parked.iter().any(|p| p.goal.id == goal.id),
        "{:?}",
        store.status.parked
    );
    let reloaded = Store::load(&out);
    assert!(
        reloaded.status.parked.iter().any(|p| p.goal.id == goal.id),
        "the entry is on disk"
    );
    crate::vfs::remove_dir_all(&out).ok();
}

// A verdict write refused because another writer landed first is reported: the
// report carries the refusal, the verdict line names it, and the build does not
// count as converged. Mirrors docs/compiler/graph.md#concurrency.
#[test]
fn a_refused_verdict_write_is_reported() {
    use std::sync::{Arc, Mutex};
    let out = std::env::temp_dir().join(format!(
        "jazyk-refused-verdict-{}",
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&out).ok();
    crate::vfs::create_dir_all(&out).unwrap();
    let text = "# Main\n\nThe system accepts an order.\n";
    let parsed: ParsedDocs = [(
        "docs/main.md".into(),
        (hash_hex(text), crate::md::parse_sections(text)),
    )]
    .into();
    let mut store = Store {
        out: out.clone(),
        ..Default::default()
    };
    store.sync_docs(&parsed);
    store.save_for_test();
    // Another writer commits a generation this store has not seen.
    let mut other = Store::load(&out);
    other.status.generation = store.status.generation + 1;
    other.save_for_test();
    let project = crate::board::tests::project_for(&store);
    let lines: Arc<Mutex<Vec<String>>> = Default::default();
    let sink = lines.clone();
    let trace = Trace::to_sink(
        crate::session::TraceLevel::Normal,
        Arc::new(move |e: &TraceEvent| {
            sink.lock().unwrap().push(serde_json::to_string(e).unwrap())
        }),
        Default::default(),
    );
    let report = finalize(&mut store, &project, Vec::new(), &Costs::default(), &trace);
    let why = report
        .unsaved
        .clone()
        .expect("the refusal rides in the report");
    assert!(why.starts_with("store-moved"), "{}", why);
    assert!(!report.converged());
    assert!(
        report.verdict_line().contains("(not saved: store-moved"),
        "{}",
        report.verdict_line()
    );
    assert!(
        lines
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains("verdict not saved")),
        "{:?}",
        lines.lock().unwrap()
    );
    crate::vfs::remove_dir_all(&out).ok();
}

// The build cap counts the distinct goals derived so far in the build, not the
// first board alone: the first board carries one `reconcile-section` goal (a cap
// sized for only its initial goal), the sync after the first session dirties
// enough linked documents to exceed that initial allowance. Each derives a
// goal that runs once, and the expanded cap lets every session finish.
// Mirrors docs/compiler/compilation.md#convergence.
#[test]
fn the_build_cap_grows_with_the_goals_derived_during_the_build() {
    use crate::board::tests::{control_auto, project_for};
    use crate::limits::{BUILD_SESSION_FACTOR, BUILD_SESSION_FLOOR};
    use crate::runner::{BatchRun, SessionReport, SessionRunner};
    use crate::store::{Commit, Op};
    use std::sync::Arc;

    // Covers every section of its batch and marks the goals done: a section with a
    // body and no coverage would derive again as unprocessed.
    struct CoveringRunner {
        out: std::path::PathBuf,
    }
    impl SessionRunner for CoveringRunner {
        fn agent_name(&self) -> String {
            "covering-stub".into()
        }
        fn set_build_token(&self, _token: Option<String>) {}
        fn run_item(&self, batch: &BatchRun, _trace: &Trace) -> SessionReport {
            let mut s = Store::load(&self.out);
            let ops: Vec<Op> = batch
                .goals
                .iter()
                .filter_map(|g| g.target.split_once('#'))
                .map(|(doc, section)| Op::SetCoverage {
                    doc: doc.to_string(),
                    section: section.to_string(),
                    state: "covered".into(),
                    note: None,
                })
                .collect();
            let applied = ops.len();
            let mut commit = Commit::session(batch.goal_ids(), 1, Some(10));
            commit.resolved = batch
                .goals
                .iter()
                .map(|g| Resolved {
                    goal: g.id.clone(),
                    justification: "the section states nothing to extract".into(),
                    evidence: Value::Null,
                })
                .collect();
            let report = s.apply(ops, &commit);
            assert!(report.skipped.is_empty(), "{:?}", report.skipped);
            SessionReport {
                applied,
                rounds: 1,
                tokens: 10,
                cached: 0,
                failed: None,
                resolved: batch.goal_ids(),
                stopped: None,
            }
        }
        fn ask(&self, _s: &str, _u: &str, _l: &str, _st: &str) -> Result<String, String> {
            Err("stub".into())
        }
        fn ask_traced(
            &self,
            _s: &str,
            _u: &str,
            _l: &str,
            _st: &str,
            _t: Option<&Trace>,
        ) -> Result<String, String> {
            Err("stub".into())
        }
    }

    // The root links enough one-section documents to exceed its initial cap. They sit
    // one link level below it. The store is synced to the first version with the
    // twelve covered; the parsed set the build reloads after every session carries
    // a second version of each of the twelve, so the first reload dirties them:
    // goals the first board never saw.
    const LATER: usize = BUILD_SESSION_FACTOR + BUILD_SESSION_FLOOR;
    let doc_text = |i: usize, version: usize| {
        let links: Vec<String> = (1..=LATER)
            .map(|j| format!("[d{}](./d{}.md)", j, j))
            .collect();
        format!(
            "# Document {}\n\nThe document {} states its rule, version {}.{}\n",
            i,
            i,
            version,
            if i == 0 {
                format!(" See {}.", links.join(", "))
            } else {
                String::new()
            }
        )
    };
    let parsed_with = |version_of: &dyn Fn(usize) -> usize| -> ParsedDocs {
        (0..=LATER)
            .map(|i| {
                let text = doc_text(i, version_of(i));
                (
                    format!("docs/d{}.md", i),
                    (hash_hex(&text), crate::md::parse_sections(&text)),
                )
            })
            .collect()
    };
    let first = parsed_with(&|_| 1);
    let later = parsed_with(&|i| if i == 0 { 1 } else { 2 });

    let out = std::env::temp_dir().join(format!(
        "jazyk-growing-cap-test-{}",
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&out).ok();
    crate::vfs::create_dir_all(&out).unwrap();
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    s.sync_docs(&first);
    // The linked documents are covered and clean: only the root's section is
    // dirty and unprocessed, so the first board carries one goal.
    let covered: Vec<Op> = (1..=LATER)
        .map(|i| Op::SetCoverage {
            doc: format!("docs/d{}.md", i),
            section: format!("/document-{}", i),
            state: "covered".into(),
            note: None,
        })
        .collect();
    let report = s.apply(covered, &Commit::session(Vec::new(), 1, None));
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let others: Vec<String> = s
        .status
        .changes
        .iter()
        .filter(|c| !c.subject.starts_with("docs/d0.md#"))
        .map(|c| c.id.clone())
        .collect();
    s.clear_changes(&others);

    let trace = Trace::to_sink(
        crate::session::TraceLevel::Normal,
        Arc::new(|_: &TraceEvent| {}),
        Default::default(),
    );
    let mut proj = project_for(&s);
    proj.roots = vec!["docs/d0.md".into()];
    let control = control_auto();
    let first_board = Board::derive(&s, &proj, &control);
    assert_eq!(
        first_board
            .open_goals()
            .iter()
            .map(|g| g.id.clone())
            .collect::<Vec<_>>(),
        vec!["g:reconcile-section:docs/d0.md#/document-0".to_string()]
    );
    let runner = CoveringRunner { out: out.clone() };
    let run = run_sessions(&proj, &out, &trace, None, &runner, &mut s, &later, &control);

    // One goal on the first board, more distinct over the build, each run once:
    // more sessions than a cap sized for the first board allows, under the grown
    // cap, and nothing parked.
    let goals = 1 + LATER;
    let first_board_cap = BUILD_SESSION_FACTOR + BUILD_SESSION_FLOOR;
    let grown_cap = BUILD_SESSION_FACTOR * goals + BUILD_SESSION_FLOOR;
    let reasons: Vec<Option<String>> = run.parked.iter().map(|g| g.reason.clone()).collect();
    assert_eq!(run.derived, goals, "{:?}", reasons);
    assert_eq!(
        run.sessions as usize, goals,
        "against a first-board cap of {}; parked {:?}",
        first_board_cap, reasons
    );
    assert!(run.sessions as usize > first_board_cap);
    assert!((run.sessions as usize) < grown_cap);
    assert!(run.parked.is_empty(), "{:?}", reasons);
    crate::vfs::remove_dir_all(&out).ok();
}
