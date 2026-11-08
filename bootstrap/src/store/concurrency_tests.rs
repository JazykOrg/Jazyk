// Writers racing on one store: the lock serializes commits, and a writer whose
// loaded generation is no longer the one on disk is refused with `store-moved`
// instead of overwriting the winner. Mirrors docs/compiler/graph.md#concurrency.
use super::*;
use crate::model::{Attribute, Entity, Provenance, WorkItem};

fn fresh_dir(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "jazyk-store-race-{}-{}-{}",
        tag,
        crate::sys::process_id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    crate::vfs::remove_dir_all(&dir).ok();
    crate::vfs::create_dir_all(&dir).unwrap();
    dir
}

fn commit() -> Commit {
    WorkItem {
        task: "reconcile-doc".into(),
        target: "t.md".into(),
        dirty_sections: vec![],
        stale_anchors: vec![],
        proposals: Vec::new(),
    }
    .commit(1, None)
}

fn create(id: &str) -> Op {
    Op::CreateEntity {
        id: id.into(),
        entity: Entity {
            name: id.trim_start_matches("ent:").into(),
            attributes: vec![Attribute {
                name: "fixture".into(),
                r#type: None,
                value: None,
                provenance: Provenance::Decree {
                    author: "test".into(),
                    at: String::new(),
                    note: None,
                },
            }],
            ..Default::default()
        },
    }
}

const WRITERS: usize = 8;

// Writers that all loaded the same generation race to commit: exactly one lands,
// every other is refused as store-moved, and the disk holds the winner's entity
// alone at the next generation.
#[test]
fn racing_writers_from_one_generation_land_exactly_one_commit() {
    let out = fresh_dir("one-wins");
    let mut seed = Store {
        out: out.clone(),
        ..Default::default()
    };
    assert_eq!(seed.apply(vec![create("ent:seed")], &commit()).applied, 1);
    let start = Store::load(&out).status.generation;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(WRITERS));
    let writers: Vec<_> = (0..WRITERS)
        .map(|n| {
            let out = out.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut store = Store::load(&out);
                barrier.wait();
                let r = store.apply(vec![create(&format!("ent:w{n}"))], &commit());
                (n, r.applied, r.skipped)
            })
        })
        .collect();
    let results: Vec<_> = writers.into_iter().map(|w| w.join().unwrap()).collect();
    let winners: Vec<usize> = results
        .iter()
        .filter(|(_, applied, _)| *applied == 1)
        .map(|(n, _, _)| *n)
        .collect();
    assert_eq!(winners.len(), 1, "{results:?}");
    for (_, applied, skipped) in &results {
        if *applied == 0 {
            assert!(
                skipped
                    .first()
                    .is_some_and(|s| s.starts_with("store-moved:")),
                "{skipped:?}"
            );
        }
    }
    let disk = Store::load(&out);
    assert_eq!(disk.status.generation, start + 1);
    let written: Vec<&String> = disk
        .graph
        .entities
        .keys()
        .filter(|k| k.starts_with("ent:w"))
        .collect();
    assert_eq!(written, vec![&format!("ent:w{}", winners[0])]);
    assert!(disk.graph.entities.contains_key("ent:seed"));
    assert!(!out
        .join("journal")
        .join(format!("g{}.yaml", start + 2))
        .vexists());
    crate::vfs::remove_dir_all(&out).ok();
}

// Writers that reload and restage after a refusal all land in the end: one
// generation each, no entity lost, one journal entry per generation.
#[test]
fn writers_that_reload_after_store_moved_all_land_in_turn() {
    let out = fresh_dir("all-land");
    let mut seed = Store {
        out: out.clone(),
        ..Default::default()
    };
    assert_eq!(seed.apply(vec![create("ent:seed")], &commit()).applied, 1);
    let start = Store::load(&out).status.generation;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(WRITERS));
    let writers: Vec<_> = (0..WRITERS)
        .map(|n| {
            let out = out.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let mut refused = 0;
                loop {
                    let mut store = Store::load(&out);
                    let r = store.apply(vec![create(&format!("ent:w{n}"))], &commit());
                    if r.applied == 1 {
                        return refused;
                    }
                    assert!(
                        r.skipped
                            .first()
                            .is_some_and(|s| s.starts_with("store-moved:")),
                        "{:?}",
                        r.skipped
                    );
                    refused += 1;
                    assert!(refused < 10_000, "a writer never landed");
                }
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }
    let disk = Store::load(&out);
    assert_eq!(disk.status.generation, start + WRITERS as u64);
    for n in 0..WRITERS {
        assert!(
            disk.graph.entities.contains_key(&format!("ent:w{n}")),
            "ent:w{n} lost"
        );
    }
    for g in (start + 1)..=(start + WRITERS as u64) {
        assert!(
            out.join("journal").join(format!("g{g}.yaml")).vexists(),
            "no journal entry for g{g}"
        );
    }
    crate::vfs::remove_dir_all(&out).ok();
}
