// Journal entries, spend stamps, and the bounded journal readers.
use super::*;

// A save that dirtied sections is a generation of its own with its records.
// Mirrors docs/compiler/graph.md#journal.
// The journal's `tokens` is the session's measured spend, stamped after the turn:
// the entry commits inside the serving, before the cost is known, and a zero
// would price the build at nothing (docs/compiler/graph.md#journal).
#[test]
fn measured_spend_stamps_the_entries_a_session_wrote() {
    let out = own_dir("stamp-tokens");
    crate::vfs::create_dir_all(crate::layout::journal_dir(&out)).unwrap();
    let write = |g: u64, kind: &str, tokens: Option<u64>| {
        let entry = JournalEntry {
            build: format!("g{}", g),
            generation: g,
            kind: kind.to_string(),
            tokens,
            ..Default::default()
        };
        crate::vfs::write(
            crate::layout::journal_dir(&out).join(format!("g{}.yaml", g)),
            serde_norway::to_string(&entry).unwrap(),
        )
        .unwrap();
    };
    write(1, "edit", None);
    write(2, "session", None);
    write(3, "session", None);
    Store::stamp_tokens(&out, 1, 3, 901, 601);
    let read = |g: u64| -> JournalEntry {
        serde_norway::from_str(
            &crate::vfs::read_to_string(
                crate::layout::journal_dir(&out).join(format!("g{}.yaml", g)),
            )
            .unwrap(),
        )
        .unwrap()
    };
    // The two session entries carry the whole spend between them, the cached input
    // split the same way; the edit entry, which no session wrote, stays without one.
    assert_eq!(read(2).tokens, Some(450));
    assert_eq!(read(3).tokens, Some(451));
    assert_eq!(read(2).cached_tokens, Some(300));
    assert_eq!(read(3).cached_tokens, Some(301));
    assert_eq!(read(1).tokens, None);
    // A second stamp over the same generations leaves the first one's numbers.
    Store::stamp_tokens(&out, 1, 3, 100, 0);
    assert_eq!(read(2).tokens, Some(450));
    assert_eq!(read(2).cached_tokens, Some(300));
    // A checks entry after the session's own takes no share: the session entry
    // carries the whole spend (a split once dropped half a criteria session's).
    // An endpoint that reported no cache read leaves the field absent, not zero.
    write(4, "session", None);
    write(5, "checks", None);
    Store::stamp_tokens(&out, 3, 5, 12051, 0);
    assert_eq!(read(4).tokens, Some(12051));
    assert_eq!(read(4).cached_tokens, None);
    assert!(
        !crate::vfs::read_to_string(crate::layout::journal_file(&out, 4))
            .unwrap()
            .contains("cachedTokens")
    );
    assert_eq!(read(5).tokens, None);
}

// `jazyk spend` stamps an external agent's own report onto a committed session
// entry lacking a figure, marked tokensSource: agent, and refuses an entry that
// already carries one (measured or reported), a non-session entry, a missing
// generation, and a cached figure above the bill.
// Mirrors docs/frontends/cli.md#jazyk-spend.
#[test]
fn an_agent_report_stamps_one_unmeasured_session_entry_and_refuses_the_rest() {
    let out = own_dir("stamp-reported");
    crate::vfs::create_dir_all(crate::layout::journal_dir(&out)).unwrap();
    let write = |g: u64, kind: &str, tokens: Option<u64>| {
        let entry = JournalEntry {
            build: format!("g{}", g),
            generation: g,
            kind: kind.to_string(),
            tokens,
            ..Default::default()
        };
        crate::vfs::write(
            crate::layout::journal_dir(&out).join(format!("g{}.yaml", g)),
            serde_norway::to_string(&entry).unwrap(),
        )
        .unwrap();
    };
    write(1, "edit", None);
    write(2, "session", None);
    write(3, "session", Some(9800));
    let read = |g: u64| -> JournalEntry {
        serde_norway::from_str(
            &crate::vfs::read_to_string(
                crate::layout::journal_dir(&out).join(format!("g{}.yaml", g)),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let stamped = Store::stamp_reported(&out, 2, 18400, Some(9100)).unwrap();
    assert_eq!(stamped.tokens, Some(18400));
    let e = read(2);
    assert_eq!(e.tokens, Some(18400));
    assert_eq!(e.cached_tokens, Some(9100));
    assert_eq!(e.tokens_source.as_deref(), Some("agent"));
    let text = crate::vfs::read_to_string(crate::layout::journal_file(&out, 2)).unwrap();
    assert!(
        text.contains("tokensSource: agent") && text.contains("cachedTokens: 9100"),
        "{}",
        text
    );
    // A second report on the same entry is refused and names the standing figure.
    let err = Store::stamp_reported(&out, 2, 100, None).unwrap_err();
    assert!(
        err.contains("18400 tokens") && err.contains("reported by the agent"),
        "{}",
        err
    );
    assert_eq!(read(2).tokens, Some(18400));
    // A measured entry is never overwritten.
    let err = Store::stamp_reported(&out, 3, 100, None).unwrap_err();
    assert!(
        err.contains("9800 tokens") && err.contains("measured by the harness"),
        "{}",
        err
    );
    assert_eq!(read(3).tokens_source, None);
    // Not a session, not there, not a bill.
    assert!(Store::stamp_reported(&out, 1, 100, None)
        .unwrap_err()
        .contains("not a session"));
    assert!(Store::stamp_reported(&out, 9, 100, None)
        .unwrap_err()
        .contains("no journal entry g9"));
    assert!(Store::stamp_reported(&out, 2, 0, None).is_err());
    write(4, "session", None);
    assert!(Store::stamp_reported(&out, 4, 100, Some(101))
        .unwrap_err()
        .contains("cannot exceed"));
    assert_eq!(read(4).tokens, None);
    // A zero cached figure leaves the field absent rather than writing 0.
    Store::stamp_reported(&out, 4, 100, Some(0)).unwrap();
    assert_eq!(read(4).cached_tokens, None);
}

// The bounded journal readers answer as a full scan does: the newest-first stop
// for a decree's prior and for a child's last move, the range reader for a
// session's entries. Mirrors docs/compiler/graph.md#journal.
#[test]
fn bounded_journal_lookups_agree_with_a_full_scan() {
    let mut s = Store {
        out: tmp(),
        ..Default::default()
    };
    for (id, name, parent) in [
        ("ent:a", "A", Some("ent:p1")),
        ("ent:b", "B", Some("ent:p2")),
        ("ent:p1", "P1", None),
        ("ent:p2", "P2", None),
    ] {
        s.graph.entities.insert(
            id.into(),
            Entity {
                parent: parent.map(String::from),
                ..entity(name)
            },
        );
    }
    let decree = serde_json::json!({"decree": {"author": "me", "at": ""}});
    let entries: Vec<(u64, &str, Vec<serde_json::Value>)> = vec![
        (
            1,
            "session",
            vec![
                serde_json::json!({"op": "create_entity", "id": "ent:dead", "entity": {"name": "Dead", "scope": "private"}}),
                serde_json::json!({"op": "create_entity", "id": "ent:p1", "entity": {"name": "P1", "scope": "public"}}),
            ],
        ),
        (
            2,
            "decree",
            vec![
                serde_json::json!({"op": "update_entity", "id": "ent:a", "parent": "ent:p1", "prior": {"parent": "ent:dead"}, "provenance": decree}),
            ],
        ),
        (
            3,
            "decree",
            vec![
                serde_json::json!({"op": "update_requirement", "id": "req:x", "statement": "new", "prior": {"statement": "old1", "source": {"doc": "d.md", "section": "s", "quote": "old1"}}, "provenance": decree}),
            ],
        ),
        (
            5,
            "decree",
            vec![
                serde_json::json!({"op": "update_requirement", "id": "req:x", "statement": "newer", "prior": {"statement": "old2", "source": {"doc": "d.md", "section": "s", "quote": "old2"}}, "provenance": decree}),
            ],
        ),
        (
            6,
            "session",
            vec![
                serde_json::json!({"op": "update_requirement", "id": "req:x", "statement": "newest", "prior": {"statement": "old3"}}),
            ],
        ),
        (
            7,
            "gc",
            vec![
                serde_json::json!({"op": "dissolve_entity", "id": "ent:grp", "parent": "ent:p2", "children": ["ent:b", "ent:c"]}),
            ],
        ),
        (
            8,
            "decree",
            vec![
                serde_json::json!({"op": "update_entity", "id": "ent:a", "parent": "ent:p2", "prior": {"parent": "ent:p1"}, "provenance": decree}),
            ],
        ),
        (
            9,
            "session",
            vec![
                serde_json::json!({"op": "update_entity", "id": "ent:b", "parent": "ent:p1", "prior": {"parent": "ent:p2"}}),
                serde_json::json!({"op": "update_entity", "id": "ent:b", "parent": "ent:gone", "prior": {"parent": "ent:p1"}}),
            ],
        ),
        (
            11,
            "session",
            vec![
                serde_json::json!({"op": "update_entity", "id": "ent:a", "parent": "ent:p1", "prior": {"parent": "ent:p2"}}),
            ],
        ),
    ];
    crate::vfs::create_dir_all(crate::layout::journal_dir(&s.out)).unwrap();
    for (g, kind, mutations) in &entries {
        let entry = JournalEntry {
            build: format!("g{}", g),
            generation: *g,
            kind: kind.to_string(),
            mutations: mutations.clone(),
            ..Default::default()
        };
        crate::vfs::write(s.journal_path(*g), serde_norway::to_string(&entry).unwrap()).unwrap();
    }
    // A torn entry is skipped, never fatal.
    crate::vfs::write(s.journal_path(10), "build: g10\nmutations: [unterminated\n").unwrap();

    let full = s.journal_entries();
    assert_eq!(full.len(), entries.len());
    let naive_prior = |id: &str, op: &str| -> Option<serde_json::Value> {
        full.iter().rev().find_map(|e| {
            e.mutations.iter().rev().find_map(|mv| {
                (mv["op"] == op
                    && mv["id"] == id
                    && mv["prior"].is_object()
                    && mv["provenance"]["decree"].is_object())
                .then(|| mv["prior"].clone())
            })
        })
    };
    for (id, op) in [
        ("req:x", "update_requirement"),
        ("ent:a", "update_entity"),
        ("ent:b", "update_entity"),
        ("ent:none", "update_entity"),
    ] {
        assert_eq!(s.decree_prior(id, op), naive_prior(id, op), "{} {}", id, op);
    }
    assert_eq!(
        s.decree_prior("req:x", "update_requirement").unwrap()["statement"],
        "old2"
    );
    assert_eq!(
        s.decree_prior("ent:a", "update_entity").unwrap()["parent"],
        "ent:p1"
    );
    assert_eq!(s.decree_prior("ent:b", "update_entity"), None);

    let moves = vec![
        (
            "ent:a".to_string(),
            Some("ent:p1".to_string()),
            Some("ent:p2".to_string()),
            1,
        ),
        (
            "ent:b".to_string(),
            Some("ent:gone".to_string()),
            Some("ent:dead".to_string()),
            2,
        ),
        ("ent:c".to_string(), None, Some("ent:p2".to_string()), 3),
        ("ent:never".to_string(), None, None, 4),
    ];
    let (last, names) = s.parent_history(&moves);
    let all_moves = parent_moves_in(&full);
    assert_eq!(all_moves, s.journaled_parent_moves());
    let mut naive_last: BTreeMap<String, ParentMove> = BTreeMap::new();
    for mv in &all_moves {
        if moves.iter().any(|(c, _, _, _)| *c == mv.child) {
            naive_last.insert(mv.child.clone(), mv.clone());
        }
    }
    assert_eq!(last, naive_last);
    assert_eq!(last["ent:a"].generation, 11);
    assert_eq!(last["ent:b"].to.as_deref(), Some("ent:gone"));
    assert_eq!(last["ent:c"].from.as_deref(), Some("ent:grp"));
    assert!(!last.contains_key("ent:never"));
    let all_names = created_names_in(&full);
    let mut dead: BTreeSet<String> = BTreeSet::new();
    for (_, from, to, _) in &moves {
        dead.extend(from.iter().chain(to.iter()).cloned());
    }
    for mv in last.values() {
        dead.extend(mv.from.iter().chain(mv.to.iter()).cloned());
    }
    let naive_names: BTreeMap<String, (String, String)> = all_names
        .into_iter()
        .filter(|(id, _)| dead.contains(id) && !s.graph.entities.contains_key(id))
        .collect();
    assert_eq!(names, naive_names);
    assert_eq!(
        names["ent:dead"],
        ("Dead".to_string(), "private".to_string())
    );
    assert!(
        !names.contains_key("ent:p1"),
        "a live parent needs no journal name"
    );

    let all = crate::goals::read_journal(&s.out);
    assert_eq!(all.len(), entries.len());
    for (from, to) in [(0, 20), (2, 8), (5, 6), (9, 9), (10, 11)] {
        let ranged = crate::goals::read_journal_range(&s.out, from, to);
        let naive: Vec<&JournalEntry> = all
            .iter()
            .filter(|e| e.generation > from && e.generation <= to)
            .collect();
        assert_eq!(
            ranged.iter().map(|e| e.generation).collect::<Vec<_>>(),
            naive.iter().map(|e| e.generation).collect::<Vec<_>>(),
            "({}, {}]",
            from,
            to
        );
    }
    crate::vfs::remove_dir_all(&s.out).ok();
}
