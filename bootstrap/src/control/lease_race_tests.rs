// Claimants racing for one lease: create-new semantics pick exactly one winner of a
// free slot, and exactly one claimant takes over an expired lease; every loser is
// told who holds it. Mirrors docs/compiler/control-plane.md#workers-and-leases.
use super::*;

fn fresh_out(tag: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let out = std::env::temp_dir().join(format!(
        "jazyk-lease-race-{}-{}-{}",
        tag,
        crate::sys::process_id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    crate::vfs::remove_dir_all(&out).ok();
    crate::vfs::create_dir_all(leases_dir(&out)).unwrap();
    out
}

const CLAIMANTS: usize = 8;
const ROUNDS: usize = 40;

// Every claimant released at once on one task; (worker, result) per claimant.
fn race(out: &Path, task: &str) -> Vec<(String, Result<(), String>)> {
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(CLAIMANTS));
    let claimants: Vec<_> = (0..CLAIMANTS)
        .map(|n| {
            let out = out.to_path_buf();
            let task = task.to_string();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let worker = format!("w{n}");
                barrier.wait();
                let r = claim(&out, &task, &worker);
                (worker, r)
            })
        })
        .collect();
    claimants.into_iter().map(|c| c.join().unwrap()).collect()
}

fn check_one_winner(out: &Path, task: &str, results: &[(String, Result<(), String>)]) {
    let winners: Vec<&String> = results
        .iter()
        .filter(|(_, r)| r.is_ok())
        .map(|(w, _)| w)
        .collect();
    assert_eq!(winners.len(), 1, "{results:?}");
    let holder = read_lease(&lease_file(out, task)).map(|l| l.worker);
    assert_eq!(holder.as_ref(), Some(winners[0]), "{results:?}");
    for (_, r) in results {
        if let Err(who) = r {
            assert!(
                who == winners[0] || who == "another worker",
                "a loser was told {who}, the holder is {}",
                winners[0]
            );
        }
    }
    // Nothing a claimant moved aside or wrote half is left in the directory.
    let names: Vec<String> = crate::vfs::read_dir(leases_dir(out))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
}

#[test]
fn claimants_racing_for_a_free_slot_get_exactly_one_winner() {
    let out = fresh_out("free");
    for round in 0..ROUNDS {
        let task = format!("g:reconcile-section:docs/a.md#/r{round}");
        let results = race(&out, &task);
        check_one_winner(&out, &task, &results);
        release_lease(&out, &task);
    }
    crate::vfs::remove_dir_all(&out).ok();
}

#[test]
fn claimants_racing_to_take_over_an_expired_lease_get_exactly_one_winner() {
    let out = fresh_out("expired");
    for round in 0..ROUNDS {
        let task = format!("batch-{round}");
        let expired = Lease {
            task: task.clone(),
            worker: "dead".into(),
            claim_id: "dead-id".into(),
            claimed_at: 1,
            expires_at: 1,
            goals: Vec::new(),
        };
        crate::vfs::write(
            lease_file(&out, &task),
            serde_norway::to_string(&expired).unwrap(),
        )
        .unwrap();
        let results = race(&out, &task);
        check_one_winner(&out, &task, &results);
        release_lease(&out, &task);
    }
    crate::vfs::remove_dir_all(&out).ok();
}
