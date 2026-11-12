use super::*;

#[test]
fn an_unparseable_ledger_reads_empty_and_refuses_to_save() {
    let out =
        std::env::temp_dir().join(format!("jazyk-gen-unreadable-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    let path = Ledger::path(&out);
    let mut l = Ledger::default();
    l.support.push("package.json".to_string());
    l.save(&out).unwrap();
    assert!(Ledger::load(&out)
        .support
        .contains(&"package.json".to_string()));
    let broken = "support: [unterminated\n";
    crate::vfs::write(&path, broken).unwrap();
    let mut bad = Ledger::load(&out);
    assert!(bad
        .unreadable
        .as_deref()
        .is_some_and(|w| w.contains("ledger.yaml")));
    assert!(bad.support.is_empty());
    bad.support.push("other".to_string());
    assert!(
        bad.save(&out).is_err(),
        "an unreadable ledger refuses to save"
    );
    assert_eq!(crate::vfs::read_to_string(&path).unwrap(), broken);
    let leftover: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftover.is_empty(), "{:?}", leftover);
}

#[test]
fn written_artifact_is_implementation_and_detects_code_changes() {
    let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let out = Path::new("/project/.jazyk/ds_main");
    let (store, gs) = fixture(out);
    let mut ledger = Ledger::load(out);
    ledger.medium = Some(Medium {
        form: "POSIX shell script".into(),
        produced: "written".into(),
        toolchain: "sh".into(),
        artifact: "hello.sh".into(),
    });
    ledger.save(out).unwrap();
    crate::vfs::create_dir_all(gs.deliverable.join("tests")).unwrap();
    crate::vfs::write(gs.deliverable.join("hello.sh"), "printf 'hello\\n'\n").unwrap();
    crate::vfs::write(
        gs.deliverable.join("tests/hello.sh"),
        "# checks_hello\nsh hello.sh\n",
    )
    .unwrap();
    mark(
        &store,
        "ent:cart",
        None,
        &serde_json::json!({
            "files": ["hello.sh"],
            "tests": [{"requirement": "req:shop-1", "kind": "programmatic",
                "artifact": "tests/hello.sh", "name": "checks_hello", "run": "sh tests/hello.sh"}]
        }),
        &gs,
    )
    .unwrap();
    let ledger = Ledger::load(out);
    let row = &ledger.requirements["req:shop-1"];
    assert_eq!(row.files, vec!["hello.sh"]);
    assert_eq!(
        crate::verify::status_of(&store, "req:shop-1", row, &gs).0,
        "unverified"
    );
    crate::vfs::write(gs.deliverable.join("hello.sh"), "printf 'changed\\n'\n").unwrap();
    assert_eq!(
        crate::verify::status_of(&store, "req:shop-1", row, &gs).0,
        "stale-code"
    );
}

// Two writers at once never lose each other's change: every load, modify, and save
// runs under the ledger lock (docs/consumers/gen.md#the-ledger).
#[test]
fn concurrent_ledger_updates_keep_both_changes() {
    let out = std::env::temp_dir().join(format!("jazyk-ledger-lock-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    Ledger::default().save(&out).unwrap();
    let writers: Vec<_> = ["a.css", "b.css", "c.css"]
        .into_iter()
        .map(|path| {
            let out = out.clone();
            std::thread::spawn(move || {
                Ledger::update(&out, |ledger| {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    ledger.support.push(path.to_string());
                    Ok(())
                })
                .unwrap();
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }
    let mut support = Ledger::load(&out).support;
    support.sort();
    assert_eq!(support, vec!["a.css", "b.css", "c.css"]);
    // Nested on one thread, the lock is reentrant.
    let _outer = Ledger::lock(&out);
    Ledger::update(&out, |_| Ok(())).unwrap();
    crate::vfs::remove_dir_all(&out).ok();
}
