use super::inspect::{
    board_from_disk, preview_text, render_sets_overview, render_status, ripple_text_for,
    status_report,
};
use super::*;
use crate::store::Store;

// The aggregate status reads as one line per set, never as JSON.
// Mirrors docs/frontends/cli.md#jazyk-status.
#[test]
fn status_without_a_set_is_one_line_per_set() {
    let summary = serde_json::json!({"sets": [
        {"name": "Documents", "state": "clean", "generation": 32, "verdict": {"state": ""},
         "counts": {"open": 0, "ready": 0, "blocked": 33, "parked": 0, "failed": 0}, "warnings": []},
        {"name": "Orders", "state": "dirty", "generation": 4, "verdict": {"state": "converged"},
         "counts": {"open": 2, "ready": 1}, "warnings": ["No files match x/**"]}
    ]});
    assert_eq!(
        render_sets_overview(&summary),
        "Documents: clean, generation 32, verdict none; 0 open, 0 ready, 33 blocked, 0 parked, 0 failed\n\
         Orders: dirty, generation 4, verdict converged; 2 open, 1 ready, 0 blocked, 0 parked, 0 failed\n  \
         warning: No files match x/**\n"
    );
    assert!(render_sets_overview(&serde_json::json!({"sets": []})).contains("jazyk sets add"));
}

// A one-document project in a temp dir: the smallest board with a ready batch.
fn temp_project(name: &str) -> (Project, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("jazyk-cli-{}-{}", name, crate::sys::process_id()));
    crate::vfs::remove_dir_all(&dir).ok();
    crate::vfs::create_dir_all(dir.join("docs")).unwrap();
    crate::vfs::write(
        { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
        "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
    )
    .unwrap();
    crate::vfs::write(dir.join("docs/a.md"), "# A\n\nThe body.\n").unwrap();
    let project = Project::load(&dir);
    let out = project.out.clone();
    crate::vfs::create_dir_all(&out).unwrap();
    crate::control::Control {
        compile: "auto".into(),
        ..Default::default()
    }
    .save(&out);
    (project, out)
}

// The previewed PROTOCOL line names the batch the board minted (b<generation>-<n>,
// numbered from 1), the same id `begin_goals` accepts: a preview formatting its
// own id sent a real session's first call into `no-ready-batch`. The batch id and
// a document path resolve to the same prompt. Mirrors docs/frontends/cli.md#jazyk-preview.
#[test]
fn preview_names_the_boards_own_batch() {
    let (proj, out) = temp_project("preview");
    let (store, control, board) = board_from_disk(&proj, &out);
    let batch = board.batches.first().expect("a ready batch").clone();
    assert!(
        !batch.id.ends_with("-0"),
        "batches number from 1: {}",
        batch.id
    );

    let lint = crate::project::Linting::default();
    let text = preview_text(&store, &control, &board, "", &lint).unwrap();
    let proto = text
        .lines()
        .find(|l| l.starts_with("PROTOCOL"))
        .expect("a protocol line");
    assert!(proto.contains(&batch.id), "{}", proto);
    assert!(
        !proto.contains(&format!("b{}-0", store.status.generation)),
        "{}",
        proto
    );
    // The mode is the control plane's, never a constant.
    assert!(text.contains("auto mode"), "{}", text);

    assert_eq!(
        preview_text(&store, &control, &board, &batch.id, &lint).unwrap(),
        text
    );
    let by_doc = preview_text(&store, &control, &board, "docs/a.md", &lint).unwrap();
    assert!(by_doc.contains(&batch.id), "{}", by_doc);
    assert!(preview_text(&store, &control, &board, "b0-99", &lint).is_err());
    assert!(preview_text(&store, &control, &board, "ent:ghost", &lint).is_err());
    crate::vfs::remove_dir_all(proj.root).ok();
}

// `jazyk status` closes with what to do next: on a fresh project the first build,
// and the ready count with the command that runs it; the JSON carries the same
// lines. Mirrors docs/frontends/cli.md#jazyk-status.
#[test]
fn status_says_what_to_do_next() {
    let (proj, out) = temp_project("status");
    let (store, _control, board) = board_from_disk(&proj, &out);
    let report = status_report(&proj, &store, &board);
    let next: Vec<&str> = report["next"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        next.iter()
            .any(|n| n.contains("`jazyk compile` runs the first build")),
        "{:?}",
        next
    );
    assert!(
        next.iter()
            .any(|n| n.contains("ready: `jazyk compile` runs them")),
        "{:?}",
        next
    );
    let text = render_status(&report);
    assert!(text.starts_with("store: version"), "{}", text);
    assert!(text.contains("\nnext: "), "{}", text);
    assert!(text.contains("shape: "), "{}", text);
    crate::vfs::remove_dir_all(proj.root).ok();
}

// A three-generation journal (an edit, a session resolving its goal and opening
// another, a session resolving that one) renders as one indented cascade, and a
// bare `jazyk ripple` roots at the same place: the last build.
// Mirrors docs/frontends/cli.md#jazyk-ripple.
#[test]
fn ripple_renders_a_three_generation_cascade() {
    use crate::model::{Cause, JournalEntry, OpenedGoal, Resolved};
    let dir = std::env::temp_dir().join(format!("jazyk-ripple-test-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&dir).ok();
    crate::vfs::create_dir_all(dir.join("journal")).unwrap();
    let sec = "g:reconcile-section:docs/orders.md#/orders/holds";
    let pair = "g:rejudge-pair:req:orders-6~req:payment-9";
    let g1 = JournalEntry {
        build: "g1".into(),
        generation: 1,
        kind: "edit".into(),
        dirtied: vec!["docs/orders.md#/orders/holds".into()],
        opened_goals: vec![OpenedGoal {
            goal: sec.into(),
            cause: Cause {
                generation: 1,
                mutation: 0,
                via: "section-dirty".into(),
            },
        }],
        ..Default::default()
    };
    let g2 = JournalEntry {
        build: "g2".into(),
        generation: 2,
        kind: "session".into(),
        batch: vec![sec.into()],
        resolved_goals: vec![Resolved {
            goal: sec.into(),
            justification: "req:orders-6 revised (quote, statement)".into(),
            evidence: serde_json::Value::Null,
        }],
        opened_goals: vec![OpenedGoal {
            goal: pair.into(),
            cause: Cause {
                generation: 2,
                mutation: 1,
                via: "entities".into(),
            },
        }],
        tokens: Some(21_000),
        ..Default::default()
    };
    let g3 = JournalEntry {
        build: "g3".into(),
        generation: 3,
        kind: "session".into(),
        batch: vec![pair.into()],
        resolved_goals: vec![Resolved {
            goal: pair.into(),
            justification: "consistent".into(),
            evidence: serde_json::Value::Null,
        }],
        tokens: Some(8_000),
        ..Default::default()
    };
    for (n, e) in [(1, &g1), (2, &g2), (3, &g3)] {
        crate::vfs::write(
            dir.join("journal").join(format!("g{}.yaml", n)),
            serde_norway::to_string(e).unwrap(),
        )
        .unwrap();
    }
    let store = Store {
        out: dir.clone(),
        ..Default::default()
    };

    let text = ripple_text_for(&store, Some("g1"), false).unwrap();
    assert!(
        text.starts_with("edit docs/orders.md#/orders/holds (human) g1"),
        "{}",
        text
    );
    assert!(
        text.contains("└─ reconcile-section docs/orders.md#/orders/holds g2: req:orders-6 revised"),
        "{}",
        text
    );
    assert!(
        text.contains("└─ rejudge-pair req:orders-6~req:payment-9 g3: consistent"),
        "{}",
        text
    );
    assert!(text.contains("2 sessions"), "{}", text);
    assert!(text.contains("29k tokens"), "{}", text);
    assert!(text.contains("nothing parked or failed"), "{}", text);

    // No root names the last build, which is this whole cascade.
    assert_eq!(ripple_text_for(&store, None, false).unwrap(), text);
    // An unknown root is an error, not an empty tree.
    assert!(ripple_text_for(&store, Some("ent:ghost"), false).is_err());
    crate::vfs::remove_dir_all(&dir).ok();
}

// The shape line: nodes per depth, then the fan-out histogram in the bands the
// registry values set. Mirrors docs/frontends/cli.md#jazyk-status.
#[test]
fn the_shape_line_renders_depths_and_the_fan_out_histogram() {
    use crate::limits::{CHILDREN_PER_ENTITY_HARD, CHILDREN_PER_ENTITY_SOFT};
    use crate::model::Entity;
    let (soft, hard) = (
        CHILDREN_PER_ENTITY_SOFT as usize,
        CHILDREN_PER_ENTITY_HARD as usize,
    );
    let empty = Store::default();
    assert_eq!(
        shape_line(&empty),
        format!(
            "shape: 0 nodes per depth; fan-out 2-{}: 0, {}-{}: 0, over {}: 0",
            soft,
            soft + 1,
            hard,
            hard
        )
    );

    // Three roots: one holds two children, one soft plus one, one hard plus one.
    let mut s = Store::default();
    let mut add = |id: &str, parent: Option<&str>| {
        s.graph.entities.insert(
            id.to_string(),
            Entity {
                name: id.to_string(),
                parent: parent.map(String::from),
                ..Default::default()
            },
        );
    };
    for root in ["ent:a", "ent:b", "ent:c"] {
        add(root, None);
    }
    for (root, n) in [("ent:a", 2), ("ent:b", soft + 1), ("ent:c", hard + 1)] {
        for i in 0..n {
            add(&format!("{}-{}", root, i), Some(root));
        }
    }
    assert_eq!(
        shape_line(&s),
        format!(
            "shape: 3 / {} nodes per depth; fan-out 2-{}: 2, {}-{}: 1, over {}: 1",
            2 + soft + 1 + hard + 1,
            soft,
            soft + 1,
            hard,
            hard
        )
    );
    if (soft, hard) == (9, 15) {
        assert_eq!(
            shape_line(&s),
            "shape: 3 / 28 nodes per depth; fan-out 2-9: 2, 10-15: 1, over 15: 1"
        );
    }
}
