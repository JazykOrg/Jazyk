use super::*;
use crate::reconcile::test_support::*;

#[test]
fn cross_class_flip_parks_the_pair_after_two_flips() {
    let dir = journal_dir();
    let mut s = crate::derive::tests::showcase_store();
    s.out = dir.clone();
    let create = |id: &str| json!({"CreateEntity": {"id": id, "entity": {"name": "Order pricing", "scope": "public"}}});
    write_entry(
        &dir,
        5,
        &["g:abstract-entity:ent:order"],
        vec![create("ent:order-pricing")],
        "pricing statements cohere",
    );
    write_entry(
        &dir,
        6,
        &["g:review-entity:ent:order"],
        vec![
            json!({"MergeEntities": {"keep": "ent:order", "absorb": "ent:order-pricing", "reason": "one concept"}}),
        ],
        "pricing is the order itself",
    );
    let (f, parked) = flip_detection(&s);
    assert!(f.is_empty(), "one flip is not yet oscillation: {:?}", f);
    assert!(parked.is_empty());
    write_entry(
        &dir,
        7,
        &["g:abstract-entity:ent:order"],
        vec![create("ent:order-pricing-2")],
        "pricing statements cohere again",
    );
    let (f, parked) = flip_detection(&s);
    assert_eq!(f.len(), 1, "{:?}", f);
    let (rule, subject, _, msg, prompt) = &f[0];
    assert_eq!(rule, "unstable-derivation");
    assert_eq!(*subject, ["ent:order"]);
    assert!(
        msg.contains("pricing statements cohere again")
            && msg.contains("pricing is the order itself"),
        "{}",
        msg
    );
    assert_eq!(prompt.as_ref().unwrap().options.len(), 2);
    let ids: BTreeSet<&str> = parked.iter().map(|g| g.goal.id.as_str()).collect();
    assert_eq!(
        ids,
        ["g:abstract-entity:ent:order", "g:review-entity:ent:order"]
            .into_iter()
            .collect()
    );
    // Blocked on the ruling, so no batch takes them (a parked goal resumes first).
    assert!(parked.iter().all(|g| matches!(&g.goal.state, GoalState::Blocked { on } if on.contains("unstable-derivation ruling"))), "{:?}", parked);
    assert!(
        parked.iter().all(|g| g
            .reason
            .as_deref()
            .is_some_and(|r| r.starts_with("cross-class flip"))),
        "{:?}",
        parked
    );
    crate::vfs::remove_dir_all(&dir).ok();
}

// A child grouped under a new parent by one session and moved back by the next is
// a reparent flip: the second move parks, one unstable-derivation diagnostic
// carries both justifications and asks which parent holds.
// Mirrors docs/compiler/reconciler.md#flip-detection.
#[test]
fn reparent_flip_parks_the_second_move() {
    let dir = journal_dir();
    let mut s = level_store();
    s.out = dir.clone();
    write_entry(
        &dir,
        5,
        &["g:abstract-entity:ent:backend"],
        vec![
            json!({"op": "create_entity", "id": "ent:storage",
                   "entity": {"name": "Storage", "scope": "public", "parent": "ent:backend"}}),
            json!({"op": "update_entity", "id": "ent:database",
                   "parent": "ent:storage", "prior": {"parent": "ent:backend"}}),
        ],
        "the database is the storage layer",
    );
    let (f, parked) = flip_detection(&s);
    assert!(f.is_empty(), "one move is not a flip: {:?}", f);
    assert!(parked.is_empty());
    write_entry(
        &dir,
        6,
        &["g:review-entity:ent:storage"],
        vec![json!({"op": "update_entity", "id": "ent:database",
                    "parent": "ent:backend", "prior": {"parent": "ent:storage"}})],
        "the database belongs to the backend directly",
    );
    let (f, parked) = flip_detection(&s);
    assert_eq!(f.len(), 1, "{:?}", f);
    let (rule, subject, _, msg, prompt) = &f[0];
    assert_eq!(rule, "unstable-derivation");
    assert_eq!(*subject, ["ent:database"]);
    assert!(
        msg.contains("the database is the storage layer")
            && msg.contains("the database belongs to the backend directly"),
        "{}",
        msg
    );
    let p = prompt.as_ref().unwrap();
    assert_eq!(p.options.len(), 2);
    assert!(p.freeform);
    assert!(p.question.contains("`Backend`"), "{}", p.question);
    assert_eq!(parked.len(), 1);
    assert_eq!(parked[0].goal.id, "g:review-entity:ent:storage");
    assert!(
        matches!(&parked[0].goal.state, GoalState::Blocked { on } if on.contains("ent:database")),
        "{:?}",
        parked[0].goal.state
    );
    assert_eq!(parked[0].goal.change["reparent"], "ent:database");
    assert!(
        parked[0]
            .reason
            .as_deref()
            .is_some_and(|r| r.starts_with("reparent flip")),
        "{:?}",
        parked[0].reason
    );

    // A later regroup under a re-minted id is the same flip on the same natural
    // key; the newest move is the one parked.
    write_entry(
        &dir,
        7,
        &["g:abstract-entity:ent:backend"],
        vec![
            json!({"op": "create_entity", "id": "ent:storage-2",
                   "entity": {"name": "Storage", "scope": "public", "parent": "ent:backend"}}),
            json!({"op": "update_entity", "id": "ent:database",
                   "parent": "ent:storage-2", "prior": {"parent": "ent:backend"}}),
        ],
        "regrouped",
    );
    let (f, parked) = flip_detection(&s);
    assert_eq!(f.len(), 1, "{:?}", f);
    assert!(f[0].3.contains("regrouped"), "{}", f[0].3);
    assert_eq!(parked[0].goal.id, "g:abstract-entity:ent:backend");
    crate::vfs::remove_dir_all(&dir).ok();
}
