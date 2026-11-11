use super::control::spend_at;

// `jazyk spend` stamps an agent's report on a session entry that carries no figure
// and refuses one that does, saying what it carries.
// Mirrors docs/frontends/cli.md#jazyk-spend.
#[test]
fn spend_stamps_an_unmeasured_session_and_refuses_a_measured_one() {
    let out = std::env::temp_dir().join(format!("jazyk-cli-spend-{}", crate::sys::process_id()));
    crate::vfs::remove_dir_all(&out).ok();
    crate::vfs::create_dir_all(crate::layout::journal_dir(&out)).unwrap();
    for (g, tokens) in [(7u64, None), (8, Some(9800u64))] {
        let entry = crate::model::JournalEntry {
            build: format!("g{}", g),
            generation: g,
            kind: "session".into(),
            tokens,
            ..Default::default()
        };
        crate::vfs::write(
            crate::layout::journal_dir(&out).join(format!("g{}.yaml", g)),
            serde_norway::to_string(&entry).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(
        spend_at(&out, 7, 18400, Some(9100)).unwrap(),
        "g7: 18400 tokens, 9100 cached, reported by the agent"
    );
    let r = crate::costs::report_at(&out);
    assert_eq!(
        (
            r["measured"].clone(),
            r["reported"].clone(),
            r["tokens"].clone()
        ),
        (
            serde_json::json!(1),
            serde_json::json!(1),
            serde_json::json!(28200)
        )
    );
    let err = spend_at(&out, 8, 100, None).unwrap_err();
    assert!(err.contains("already carries 9800 tokens"), "{}", err);
    assert!(spend_at(&out, 7, 100, None)
        .unwrap_err()
        .contains("reported by the agent"));
    crate::vfs::remove_dir_all(&out).ok();
}
