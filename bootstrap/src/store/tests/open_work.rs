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
