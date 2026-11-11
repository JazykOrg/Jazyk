// GC gating and escalation.
use super::*;

#[test]
fn a_gc_goal_waits_for_the_compile_goal_in_its_cone() {
    let mut s = settled_store();
    // Order is over its requirement limit (bumped down by a record), and a section
    // anchoring one of its requirements is dirty.
    record(
        &mut s,
        9,
        store::CHANGE_THRESHOLD_CROSSED,
        "ent:order",
        "limits",
        json!({"limit": "requirements-per-entity", "count": 54, "soft": 50, "hard": 80, "level": "soft", "goal": "abstract-entity"}),
    );
    record(
        &mut s,
        10,
        store::CHANGE_SECTION_DIRTY,
        "shop.md#/shop/orders",
        "section",
        json!({"added": 1}),
    );
    let b = derive(&s);
    let gc = b
        .goal("g:abstract-entity:ent:order")
        .expect("gc goal derived");
    assert!(!gc.mandatory);
    assert_eq!(gc.class, "gc");
    let blockers = b.cone_blockers(&gc.id);
    assert!(
        blockers.contains(&"g:reconcile-section:shop.md#/shop/orders".to_string()),
        "{:?}",
        blockers
    );
    assert!(!b.is_ready(&gc.id));
    // Cover the section again at a later generation: the cone quiets and the GC
    // goal is ready.
    s.docs.get_mut("shop.md").unwrap().coverage.insert(
        "/shop/orders".into(),
        Coverage {
            state: "covered".into(),
            note: None,
            claimed_by: Some("g11".into()),
        },
    );
    let b = derive(&s);
    assert!(b.cone_blockers("g:abstract-entity:ent:order").is_empty());
    assert!(
        b.is_ready("g:abstract-entity:ent:order"),
        "{:?}",
        b.readiness.get("g:abstract-entity:ent:order")
    );
    assert!(
        b.lapsed.iter().any(|id| id.starts_with("c10-")),
        "the dirty record lapsed: {:?}",
        b.lapsed
    );
    assert_eq!(b.batches.len(), 1);
    assert_eq!(b.batches[0].id, "b3-1");
    assert_eq!(b.batches[0].class, Class::Gc);
}

// Mandatory GC goals run before optional ones within a burst: the batch holding
// the goal past its hard limit ranks ahead of the batch holding the one over its
// soft limit, whatever their localities sort like.
// Mirrors docs/compiler/reconciler.md#gc-gating.
#[test]
fn mandatory_gc_batches_rank_before_optional_ones() {
    let mut s = settled_store();
    // Optional: the entity's locality sorts first by name.
    record(
        &mut s,
        9,
        store::CHANGE_THRESHOLD_CROSSED,
        "ent:order",
        "limits",
        json!({"limit": "requirements-per-entity", "count": 54, "soft": 50, "hard": 80, "level": "soft", "goal": "abstract-entity"}),
    );
    // Mandatory: a curated view past its hard limit.
    s.graph.views.insert(
        "view:class/zoo".into(),
        View {
            kind: "class".into(),
            title: "Zoo".into(),
            members: vec!["ent:order".into()],
            ..Default::default()
        },
    );
    record(
        &mut s,
        10,
        store::CHANGE_THRESHOLD_CROSSED,
        "view:class/zoo",
        "limits",
        json!({"limit": "members-per-structural-view", "count": 31, "soft": 20, "hard": 30, "level": "hard", "goal": "split-view"}),
    );
    let b = derive(&s);
    let optional = "g:abstract-entity:ent:order";
    let mandatory = "g:split-view:view:class/zoo";
    assert!(!b.goal(optional).unwrap().mandatory);
    assert!(b.goal(mandatory).unwrap().mandatory);
    assert!(b.is_ready(optional), "{:?}", b.readiness.get(optional));
    assert!(b.is_ready(mandatory), "{:?}", b.readiness.get(mandatory));
    let at = |goal: &str| {
        b.batches
            .iter()
            .position(|batch| batch.goals.iter().any(|g| g == goal))
            .expect("batched")
    };
    assert!(
        at(mandatory) < at(optional),
        "mandatory first: {:?}",
        b.batches.iter().map(|x| &x.goals).collect::<Vec<_>>()
    );
    assert!(b.batches.iter().all(|x| x.class == Class::Gc));
}

#[test]
fn the_hard_threshold_escalates_to_mandatory() {
    let mut s = settled_store();
    record(
        &mut s,
        9,
        store::CHANGE_THRESHOLD_CROSSED,
        "view:class/public",
        "limits",
        json!({"limit": "members-per-structural-view", "count": 23, "soft": 20, "hard": 30, "level": "soft", "goal": "split-view"}),
    );
    s.graph.views.insert(
        "view:class/public".into(),
        View {
            kind: "class".into(),
            title: "Public".into(),
            ..Default::default()
        },
    );
    let b = derive(&s);
    let g = b.goal("g:split-view:view:class/public").unwrap();
    assert!(!g.mandatory);
    let v = b.verdict();
    assert!(v.converged());
    assert_eq!(v.optional, 1);
    record(
        &mut s,
        12,
        store::CHANGE_THRESHOLD_CROSSED,
        "view:class/public",
        "limits",
        json!({"limit": "members-per-structural-view", "count": 31, "soft": 20, "hard": 30, "level": "hard", "goal": "split-view"}),
    );
    let b = derive(&s);
    let g = b.goal("g:split-view:view:class/public").unwrap();
    assert!(g.mandatory, "{:?}", g.change);
    let v = b.verdict();
    assert_eq!(v.state, "incomplete");
    assert_eq!((v.open, v.failed, v.optional), (1, 0, 0));
}

// A split-view goal on a level's view yields to the fan-out goal on that level:
// blocked naming it while the fan-out is open or parked, back under the cone rule
// once it is gone; the fan-out never waits for the view. Mirrors
// docs/compiler/reconciler.md#gc-gating.
#[test]
fn a_level_views_split_yields_to_the_levels_fan_out() {
    let mut s = settled_store();
    let scope = format!("{}public", SCOPE_TARGET_PREFIX);
    let lv = crate::derive::level_view_id(&s, &scope).expect("the top level has a view");
    if !s.graph.views.contains_key(&lv) {
        let kind = lv
            .trim_start_matches("view:")
            .split('/')
            .next()
            .unwrap()
            .to_string();
        s.graph.views.insert(
            lv.clone(),
            View {
                kind,
                title: "Public".into(),
                ..Default::default()
            },
        );
    }
    record(
        &mut s,
        9,
        store::CHANGE_THRESHOLD_CROSSED,
        &lv,
        "limits",
        json!({"limit": "members-per-structural-view", "count": 23, "soft": 20, "hard": 30, "level": "soft", "goal": "split-view"}),
    );
    record(
        &mut s,
        9,
        store::CHANGE_THRESHOLD_CROSSED,
        &scope,
        "limits",
        json!({"limit": "children-per-entity", "count": 11, "soft": 9, "hard": 15, "level": "soft", "goal": "abstract-entity"}),
    );
    let b = derive(&s);
    let split = format!("g:split-view:{}", lv);
    let fan_out = format!("g:abstract-entity:{}", scope);
    assert!(b.goal(&fan_out).is_some(), "fan-out goal derived");
    assert_eq!(b.view_level(&lv), Some(scope.as_str()));
    assert!(!b.is_ready(&split));
    let reason = b.readiness[&split].reason().unwrap_or_default().to_string();
    assert!(
        reason.contains(&format!("fan-out first: {}", fan_out)),
        "{}",
        reason
    );
    let theirs = b.readiness[&fan_out]
        .reason()
        .unwrap_or_default()
        .to_string();
    assert!(!theirs.contains("split-view"), "{}", theirs);
    // The fan-out resolved (its record cleared): the split falls back to the cone.
    s.status
        .clear_changes(&[store::CHANGE_THRESHOLD_CROSSED], &scope);
    let b = derive(&s);
    assert!(b.goal(&fan_out).is_none());
    let reason = b.readiness[&split].reason().unwrap_or_default().to_string();
    assert!(!reason.contains("fan-out first"), "{}", reason);
}

// A fan-out goal on the scope root is GC work: it waits while a compile goal is
// open anywhere in the scope, and its locality is the level's members.
// Mirrors docs/compiler/reconciler.md#fan-out and #gc-gating.
#[test]
fn a_fan_out_goal_on_the_root_waits_for_the_compile_goal_in_its_scope() {
    let mut s = settled_store();
    let goal = Goal {
        id: "g:abstract-entity:scope:public".into(),
        kind: "abstract-entity".into(),
        class: "gc".into(),
        mandatory: false,
        target: "scope:public".into(),
        unit: "entity".into(),
        change: json!({"fan_out": 4, "limit": {"soft": 9, "hard": 15}, "candidates": []}),
        cause: None,
        state: GoalState::Open,
        hints: vec!["load scope:public".into()],
    };
    s.status.parked.push(goal.clone().into());
    record(
        &mut s,
        10,
        store::CHANGE_SECTION_DIRTY,
        "shop.md#/shop/orders",
        "section",
        json!({"added": 1}),
    );
    let b = derive(&s);
    let g = b
        .goal(&goal.id)
        .expect("the parked root goal survives derivation");
    assert_eq!(g.state, GoalState::Parked);
    let blockers = b.cone_blockers(&goal.id);
    assert!(
        blockers.contains(&"g:reconcile-section:shop.md#/shop/orders".to_string()),
        "{:?}",
        blockers
    );
    assert!(!b.is_ready(&goal.id), "{:?}", b.readiness.get(&goal.id));
    // The level's members join the locality; the root itself is no key.
    let keys = locality_keys(&s, g);
    assert!(keys.contains(&"ent ent:shop".to_string()), "{:?}", keys);
    assert!(keys.contains(&"ent ent:customer".to_string()));
    assert!(!keys.iter().any(|k| k.contains("scope:")), "{:?}", keys);
    assert!(estimate(&s, g) > 1_500);
    // Cover the section again: the scope quiets and the root goal is ready.
    s.docs.get_mut("shop.md").unwrap().coverage.insert(
        "/shop/orders".into(),
        Coverage {
            state: "covered".into(),
            note: None,
            claimed_by: Some("g11".into()),
        },
    );
    let b = derive(&s);
    assert!(b.cone_blockers(&goal.id).is_empty());
    assert!(b.is_ready(&goal.id), "{:?}", b.readiness.get(&goal.id));
    assert_eq!(b.batches.len(), 1);
    assert_eq!(b.batches[0].class, Class::Gc);
    assert_eq!(b.batches[0].goals, vec![goal.id.clone()]);
    // A scope with no entities is no target: the parked entry drops.
    let mut empty = settled_store();
    let mut stray = goal.clone();
    stray.id = "g:abstract-entity:scope:nope".into();
    stray.target = "scope:nope".into();
    empty.status.parked.push(stray.clone().into());
    let b = derive(&empty);
    assert!(b.goal(&stray.id).is_none());
    assert!(b.dropped_parked.contains(&stray.id));
    // A parked ledger goal the ledger no longer derives (its row verified since)
    // has nothing to resume: it drops even though its target stands.
    let mut verified = settled_store();
    let req = verified
        .graph
        .requirements
        .keys()
        .next()
        .cloned()
        .expect("a requirement");
    let mut done = goal.clone();
    done.id = format!("g:verify:{}", req);
    done.kind = "verify".into();
    done.target = req;
    verified.status.parked.push(done.clone().into());
    let b = derive(&verified);
    assert!(b.goal(&done.id).is_none(), "{:?}", b.goal(&done.id));
    assert!(b.dropped_parked.contains(&done.id));
    // A failed ledger goal the ledger no longer derives drops the same way.
    verified.status.parked.clear();
    verified.status.failed.push(crate::model::FailedGoal {
        goal: done.clone(),
        reason: "the test cannot run".into(),
    });
    let b = derive(&verified);
    assert!(b.goal(&done.id).is_none(), "{:?}", b.goal(&done.id));
    assert!(
        b.dropped_failed.contains(&done.id),
        "{:?}",
        b.dropped_failed
    );
    // An entry parked by flip detection resumes blocked on the ruling, whatever
    // state it was written with, so no batch takes it.
    let mut ruled = settled_store();
    let ent = ruled
        .graph
        .entities
        .keys()
        .next()
        .cloned()
        .expect("an entity");
    let mut flip = goal.clone();
    flip.id = format!("g:conform-instance:{}", ent);
    flip.kind = "conform-instance".into();
    flip.target = ent.clone();
    flip.change = json!({"reparent": "ent:row-of-shops", "between": ["ent:a", "ent:b"]});
    flip.cause = Some(crate::model::Cause {
        generation: 95,
        mutation: 0,
        via: "flip-detection".into(),
    });
    ruled.status.parked.push(flip.clone().into());
    ruled.graph.diagnostics.insert(
        "diag:unstable-derivation-1".into(),
        unstable_ruling("ent:row-of-shops"),
    );
    let b = derive(&ruled);
    let g = b.goal(&flip.id).expect("the flip entry stands");
    assert!(
        matches!(&g.state, GoalState::Blocked { on } if on.contains("ent:row-of-shops")),
        "{:?}",
        g.state
    );
    assert!(!b.is_ready(&flip.id));
    assert!(b.batches.iter().all(|bt| !bt.goals.contains(&flip.id)));
    // Answering the ruling clears the entry.
    ruled
        .graph
        .diagnostics
        .get_mut("diag:unstable-derivation-1")
        .unwrap()
        .answer = Some(crate::model::DiagnosticAnswer {
        choice: Some(0),
        text: "keep".into(),
        status: "applied".into(),
        error: None,
    });
    let b = derive(&ruled);
    assert!(
        b.dropped_parked.contains(&flip.id),
        "{:?}",
        b.dropped_parked
    );
}
