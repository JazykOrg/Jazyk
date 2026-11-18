use super::*;
use serde_json::json;

fn update(id: &str, patch: serde_json::Value) -> Op {
    let mut value = json!({"op":"update_entity","id":id});
    value
        .as_object_mut()
        .unwrap()
        .extend(patch.as_object().unwrap().clone());
    serde_json::from_value(value).unwrap()
}
#[test]
fn stale_changesets_merge_fields_and_overwrites_keep_prior_attribution() {
    let out = own_dir("open-work-fields");
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    s.apply(
        vec![Op::CreateEntity {
            id: "ent:order".into(),
            entity: entity("Order"),
        }],
        &Commit::store("edit"),
    );
    let base = s.clone();
    let mut stale = s.clone();
    let commit = |name: &str| Commit {
        base: Some(Box::new(base.clone())),
        actor: Some(json!({"workerId":name})),
        ..Commit::session(Vec::new(), 1, None)
    };
    let a = s.apply(
        vec![update("ent:order", json!({"definition":"First field"}))],
        &commit("one"),
    );
    assert!(a.skipped.is_empty(), "{:?}", a.skipped);
    let b = stale.apply(
        vec![update("ent:order", json!({"name":"Purchase"}))],
        &commit("two"),
    );
    assert!(b.skipped.is_empty(), "{:?}", b.skipped);
    let current = Store::load(&out);
    assert_eq!(current.graph.entities["ent:order"].name, "Purchase");
    assert_eq!(
        current.graph.entities["ent:order"].definition.as_deref(),
        Some("First field")
    );
    let entry = crate::goals::read_journal_entry(
        &crate::layout::journal_file(&out, b.generation),
        b.generation,
    )
    .unwrap();
    assert!(entry.overwrites.is_empty());
    let c = stale.apply(
        vec![update("ent:order", json!({"definition":"Last writer"}))],
        &commit("three"),
    );
    assert!(c.skipped.is_empty(), "{:?}", c.skipped);
    let entry = crate::goals::read_journal_entry(
        &crate::layout::journal_file(&out, c.generation),
        c.generation,
    )
    .unwrap();
    let overwrite = entry
        .overwrites
        .iter()
        .find(|v| v["field"] == "definition")
        .unwrap();
    assert_eq!(overwrite["priorValue"], "First field");
    assert_eq!(overwrite["priorGeneration"], a.generation);
    assert_eq!(overwrite["priorAuthor"]["workerId"], "one");
    assert_eq!(
        Store::load(&out).graph.entities["ent:order"]
            .definition
            .as_deref(),
        Some("Last writer")
    );
    crate::vfs::remove_dir_all(out).unwrap();
}

#[test]
fn pending_graph_redo_recovers_partial_shards_and_journal_receipt() {
    let out = own_dir("open-work-redo");
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    let report = s.apply(
        vec![Op::CreateEntity {
            id: "ent:order".into(),
            entity: entity("Order"),
        }],
        &Commit {
            work_id: Some("w-recovery".into()),
            ..Commit::session(Vec::new(), 1, None)
        },
    );
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let entry = crate::goals::read_journal_entry(
        &crate::layout::journal_file(&out, report.generation),
        report.generation,
    )
    .unwrap();
    s.prepare_redo(&entry).unwrap();
    let node = out.join("graph/entities/ent~order.yaml");
    crate::vfs::remove_file(node).unwrap();
    let restored = Store::load(&out);
    assert!(
        restored.load_errors.is_empty(),
        "{:?}",
        restored.load_errors
    );
    assert!(restored.graph.entities.contains_key("ent:order"));
    assert!(!crate::vfs::exists(&crate::store::recovery::redo_path(
        &out
    )));
    let entry = crate::goals::read_journal_entry(
        &crate::layout::journal_file(&out, report.generation),
        report.generation,
    )
    .unwrap();
    assert_eq!(entry.work_id.as_deref(), Some("w-recovery"));
    crate::vfs::remove_dir_all(out).unwrap();
}

#[test]
fn disjoint_alias_additions_do_not_report_overwriting_the_other_writer() {
    let out = own_dir("open-work-aliases");
    let mut seed = Store {
        out: out.clone(),
        ..Default::default()
    };
    seed.apply(
        vec![Op::CreateEntity {
            id: "ent:order".into(),
            entity: entity("Order"),
        }],
        &Commit::store("edit"),
    );
    let base = seed.clone();
    let mut other = seed.clone();
    let commit = Commit {
        base: Some(Box::new(base)),
        ..Commit::session(Vec::new(), 1, None)
    };
    seed.apply(
        vec![update("ent:order", json!({"add_aliases":["Purchase"]}))],
        &commit,
    );
    let result = other.apply(
        vec![update("ent:order", json!({"add_aliases":["Request"]}))],
        &commit,
    );
    assert!(result.skipped.is_empty(), "{:?}", result.skipped);
    let entry = crate::goals::read_journal_entry(
        &crate::layout::journal_file(&out, result.generation),
        result.generation,
    )
    .unwrap();
    assert!(entry.overwrites.is_empty(), "{:?}", entry.overwrites);
    let aliases = &other.graph.entities["ent:order"].aliases;
    assert!(aliases.contains(&"Purchase".into()) && aliases.contains(&"Request".into()));
    crate::vfs::remove_dir_all(out).unwrap();
}

#[test]
fn concurrent_diagram_edits_audit_authored_query_and_suppression_fields() {
    let out = own_dir("open-work-diagrams");
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    let mut ops = Vec::new();
    for i in 0..4 {
        ops.push(Op::CreateEntity {
            id: format!("ent:part-{i}"),
            entity: entity(&format!("Part {i}")),
        });
    }
    ops.push(Op::CreateView {
        id: "view:class/parts".into(),
        view: View {
            kind: "class".into(),
            title: "Parts".into(),
            members: (0..4).map(|i| format!("ent:part-{i}")).collect(),
            query: Some(ViewQuery {
                scope: Some("public".into()),
                ..Default::default()
            }),
            authored: BTreeMap::from([
                ("purpose".into(), json!("Explain the parts")),
                (
                    "area".into(),
                    json!({"target":"scope:public","extent":"selection"}),
                ),
            ]),
            provenance: Some(Provenance::Derived {
                from: vec!["ent:part-0".into()],
                reasoning: "Fixture explanation".into(),
            }),
            ..Default::default()
        },
    });
    let seeded = s.apply(ops, &Commit::store("edit"));
    assert!(seeded.skipped.is_empty(), "{:?}", seeded.skipped);
    let base = s.clone();
    let mut stale = base.clone();
    let commit = Commit {
        base: Some(Box::new(base.clone())),
        ..Commit::session(Vec::new(), 1, None)
    };
    let patch = |patch: serde_json::Value| {
        let mut op = json!({"op":"update_view","id":"view:class/parts"});
        op.as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        serde_json::from_value::<Op>(op).unwrap()
    };
    let a = s.apply(
        vec![patch(json!({"authored":{"purpose":"First purpose"}}))],
        &commit,
    );
    assert!(a.skipped.is_empty());
    let b = stale.apply(
        vec![patch(
            json!({"authored":{"description":"Independent detail"}}),
        )],
        &commit,
    );
    assert!(b.skipped.is_empty());
    let read = |generation| {
        crate::goals::read_journal_entry(&crate::layout::journal_file(&out, generation), generation)
            .unwrap()
    };
    assert!(read(b.generation).overwrites.is_empty());
    let c = stale.apply(
        vec![patch(json!({"authored":{"purpose":"Last purpose"}}))],
        &commit,
    );
    assert!(c.skipped.is_empty());
    assert!(read(c.generation)
        .overwrites
        .iter()
        .any(|f| f["field"] == "purpose" && f["priorValue"] == "First purpose"));
    let current = Store::load(&out);
    assert_eq!(
        current.graph.views["view:class/parts"].authored["description"],
        "Independent detail"
    );
    let base = current.clone();
    let mut first = current.clone();
    let mut second = current;
    let commit = Commit {
        base: Some(Box::new(base)),
        ..Commit::session(Vec::new(), 2, None)
    };
    first.apply(vec![patch(json!({"query":{"scope":"internal"}}))], &commit);
    let cleared = second.apply(vec![patch(json!({"clear_query":true}))], &commit);
    assert!(cleared.skipped.is_empty());
    assert!(Store::load(&out).graph.views["view:class/parts"]
        .query
        .is_none());
    assert!(read(cleared.generation)
        .overwrites
        .iter()
        .any(|f| f["field"] == "query.scope"));
    let mut first = Store::load(&out);
    let base = first.clone();
    let mut second = base.clone();
    let finding = crate::diagrams::findings(&base)
        .into_iter()
        .find(|f| f.family == "structure")
        .unwrap();
    let mut decision = finding.value();
    decision["ruleVersion"] = json!(1);
    decision["reason"] = json!("First human rationale");
    let commit = Commit {
        base: Some(Box::new(base)),
        ..Commit::session(Vec::new(), 3, None)
    };
    first.apply(
        vec![Op::SetDiagramDecision {
            id: finding.id.clone(),
            decision: Some(decision.clone()),
        }],
        &commit,
    );
    decision["reason"] = json!("Second human rationale");
    let changed = second.apply(
        vec![Op::SetDiagramDecision {
            id: finding.id,
            decision: Some(decision),
        }],
        &commit,
    );
    assert!(changed.skipped.is_empty(), "{:?}", changed.skipped);
    assert!(read(changed.generation)
        .overwrites
        .iter()
        .any(|f| f["field"] == "decision.diagram_coverage.reason"));
    let mut current = Store::load(&out);
    let mut session = crate::tools::ToolSession::new(
        current.clone(),
        crate::tools::WorkScope::serving("chat"),
        64,
        24000,
    );
    session.dispatch("update_view",&json!({"id":"view:class/parts","description":"Owner's chosen explanation","reasoning":"Record the owner's explicit presentation choice","decision":"Use this explanation for the parts diagram"})).unwrap();
    let result = current.apply(session.staged, &Commit::store("edit"));
    assert!(result.skipped.is_empty(), "{:?}", result.skipped);
    assert!(matches!(
        Store::load(&out).graph.views["view:class/parts"].provenance,
        Some(Provenance::Decree { .. })
    ));
    crate::vfs::remove_dir_all(out).unwrap();
}
