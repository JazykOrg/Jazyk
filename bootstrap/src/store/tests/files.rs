// Node files, saves, loads, store versions, and the generation.
use super::*;

// A save rewrites only the files whose bytes changed: a second save with nothing
// new replaces no file, and a commit leaves untouched shards in place while
// status.yaml, the commit point, is rewritten. A rewrite renames a new file into
// place, so an unchanged inode means no write.
// Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
#[cfg(unix)]
#[test]
fn save_leaves_files_with_unchanged_bytes_in_place() {
    use std::os::unix::fs::MetadataExt;
    let out = own_dir("unchanged-save");
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    let r = s.apply(vec![create("ent:a", "A")], &session());
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    let inodes = |dir: &Path| -> BTreeMap<PathBuf, u64> {
        let mut all = BTreeMap::new();
        for sub in ["graph/entities", "graph/diagnostics", "docs", ""] {
            let Ok(rd) = std::fs::read_dir(dir.join(sub)) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("yaml") {
                    all.insert(p.clone(), std::fs::metadata(&p).unwrap().ino());
                }
            }
        }
        all
    };
    let before = inodes(&out);
    assert!(before.contains_key(&out.join("status.yaml")));
    assert!(before.contains_key(&out.join("graph/entities/ent~a.yaml")));
    let again = Store::load(&out);
    again.save();
    assert_eq!(inodes(&out), before, "a save with no change writes nothing");

    let mut s = Store::load(&out);
    let r = s.apply(vec![create("ent:b", "B")], &session());
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    let after = inodes(&out);
    assert!(
        after.contains_key(&out.join("graph/entities/ent~b.yaml")),
        "the new node is written"
    );
    // The records live one per file: status.yaml stays, the commit point moves.
    assert_eq!(
        after[&out.join("status.yaml")],
        before[&out.join("status.yaml")]
    );
    assert_ne!(after[&out.join(BUILD_FILE)], before[&out.join(BUILD_FILE)]);
    let a = out.join("graph/entities/ent~a.yaml");
    assert_eq!(after[&a], before[&a], "an unchanged node file stays");
    assert_eq!(Store::load(&out).status.generation, 2);
}

// A save serializes only the nodes whose value moved since this process last
// wrote or verified their files; a file another writer changed is compared and
// rewritten as before. Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
#[cfg(unix)]
#[test]
fn a_save_serializes_only_nodes_whose_value_moved() {
    let out = own_dir("serialized-save");
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    let ops: Vec<Op> = (0..20)
        .map(|i| create(&format!("ent:n{}", i), &format!("N{}", i)))
        .collect();
    let r = s.apply(ops, &session());
    assert_eq!(r.applied, 20, "{:?}", r.skipped);
    let serialized = |s: &Store| {
        let before = SERIALIZED.with(|n| n.get());
        s.save();
        SERIALIZED.with(|n| n.get()) - before
    };
    let again = Store::load(&out);
    assert_eq!(serialized(&again), 0, "nothing moved, nothing serializes");

    let mut s = Store::load(&out);
    s.graph.entities.get_mut("ent:n4").unwrap().definition = Some("moved".into());
    assert_eq!(serialized(&s), 1, "one node moved");

    let shard = out.join("graph/entities/ent~n5.yaml");
    let canonical = std::fs::read_to_string(&shard).unwrap();
    std::fs::write(&shard, format!("{}# a hand edit\n", canonical)).unwrap();
    let again = Store::load(&out);
    assert_eq!(serialized(&again), 1, "the edited file is compared again");
    assert_eq!(std::fs::read_to_string(&shard).unwrap(), canonical);
    crate::vfs::remove_dir_all(&out).ok();
}

// Inside a read scope the whole-graph lookups answer from indices, and answer
// what the scans answer: redirects followed, each requirement once, by id.
// Mirrors docs/compiler/graph.md#derived-data.
#[test]
fn a_read_scope_answers_what_the_scans_answer() {
    let mut s = Store::default();
    for (id, parent, scope) in [
        ("ent:a", None, "public"),
        ("ent:b", Some("ent:a"), "public"),
        ("ent:c", Some("ent:a"), "public"),
        ("ent:d", None, "internal"),
        ("ent:e", None, "public"),
    ] {
        let mut e = entity(id);
        e.parent = parent.map(String::from);
        e.scope = scope.into();
        s.graph.entities.insert(id.into(), e);
    }
    s.graph.redirects.insert("ent:old".into(), "ent:b".into());
    for (rid, ents) in [
        ("req:1", vec!["ent:a", "ent:b"]),
        ("req:2", vec!["ent:old", "ent:b"]),
        ("req:3", vec!["ent:c", "ent:old", "ent:a"]),
        ("req:4", vec!["ent:gone"]),
    ] {
        s.graph.requirements.insert(
            rid.into(),
            Requirement {
                statement: rid.into(),
                entities: ents.into_iter().map(String::from).collect(),
                ..Default::default()
            },
        );
    }
    let ids = [
        "ent:a",
        "ent:b",
        "ent:c",
        "ent:d",
        "ent:e",
        "ent:old",
        "ent:gone",
        "scope:public",
        "scope:internal",
        "scope:none",
    ];
    let answers = |s: &Store| -> Vec<(Vec<String>, Vec<String>)> {
        ids.iter()
            .map(|id| {
                (
                    s.requirements_referencing(id),
                    crate::board::level_members(s, id),
                )
            })
            .collect()
    };
    let scanned = answers(&s);
    assert_eq!(scanned[1].0, vec!["req:1", "req:2", "req:3"]);
    let scope = ReadScope::enter(&s);
    assert!(ScopeIndex::of(&s).is_some());
    assert_eq!(answers(&s), scanned);
    let other = s.clone();
    assert!(ScopeIndex::of(&other).is_none(), "another store scans");
    drop(scope);
    assert!(ScopeIndex::of(&s).is_none());
}

#[test]
fn an_unparseable_node_file_makes_the_store_read_only() {
    let out = own_dir("unreadable");
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    let r = s.apply(vec![create("ent:a", "A")], &session());
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    let shard = out.join("graph/entities/ent~a.yaml");
    let broken = "name: [this is not an entity\n";
    crate::vfs::write(&shard, broken).unwrap();
    let mut again = Store::load(&out);
    assert!(
        again.load_errors.iter().any(|e| e.contains("ent~a.yaml")),
        "{:?}",
        again.load_errors
    );
    let r = again.apply(vec![create("ent:b", "B")], &session());
    assert_eq!(r.applied, 0);
    assert!(
        r.skipped[0].starts_with("store-unreadable:"),
        "{:?}",
        r.skipped
    );
    assert!(again.sweep().is_empty());
    again.save();
    assert_eq!(crate::vfs::read_to_string(&shard).unwrap(), broken);
    assert!(!out.join("journal").join("g2.yaml").vexists());
    assert_eq!(Store::load(&out).status.generation, 1);
}

#[test]
fn a_commit_against_a_moved_store_is_refused() {
    let out = own_dir("moved");
    let mut a = Store {
        out: out.clone(),
        ..Default::default()
    };
    assert_eq!(a.apply(vec![create("ent:a", "A")], &session()).applied, 1);
    let mut b = Store::load(&out);
    assert_eq!(b.apply(vec![create("ent:b", "B")], &session()).applied, 1);
    let r = a.apply(vec![create("ent:c", "C")], &session());
    assert_eq!(r.applied, 0);
    assert!(r.skipped[0].starts_with("store-moved:"), "{:?}", r.skipped);
    let disk = Store::load(&out);
    assert_eq!(disk.status.generation, 2);
    assert!(disk.graph.entities.contains_key("ent:b"));
    assert!(!disk.graph.entities.contains_key("ent:c"));
    let mut a = Store::load(&out);
    assert_eq!(a.apply(vec![create("ent:c", "C")], &session()).applied, 1);
    assert_eq!(Store::load(&out).status.generation, 3);
}

#[test]
fn writes_land_whole_and_leave_no_temporary_files() {
    let out = own_dir("atomic");
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    assert_eq!(s.apply(vec![create("ent:a", "A")], &session()).applied, 1);
    fn walk(dir: &Path, into: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, into);
            } else {
                into.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(&out, &mut files);
    let leftover: Vec<_> = files
        .iter()
        .filter(|p| p.to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftover.is_empty(), "{:?}", leftover);
    assert!(out.join("status.yaml").vexists());
    assert!(out.join("graph/entities/ent~a.yaml").vexists());
    assert!(out.join("journal").join("g1.yaml").vexists());
}

// File ids: `:` as `~`, `/` as `+`, everything outside the plain set as `%XX`, and
// only the canonical spelling decodes. Mirrors docs/compiler/graph.md#node-files.
#[test]
fn file_ids_round_trip_and_never_fold_under_case() {
    for (id, name) in [
        ("ent:order", "ent~order"),
        ("view:class/shop", "view~class+shop"),
        ("ent:čaj", "ent~%C4%8Daj"),
        ("rel:order~payment", "rel~order%7Epayment"),
        ("ent:Order", "ent~%4Frder"),
        (".hidden", "%2Ehidden"),
        ("req:a.b_c-1", "req~a.b_c-1"),
        ("x+y%z", "x%2By%25z"),
    ] {
        assert_eq!(file_id(id), name, "{id}");
        assert_eq!(id_from_file(name).as_deref(), Some(id), "{name}");
    }
    for bad in [
        "", "ent~%61", "ENT~a", "ent~%4", "ent~%zz", "ent~%4f", "a b", ".x",
    ] {
        assert_eq!(id_from_file(bad), None, "{bad}");
    }
    let names: BTreeSet<String> = [
        "ent:ab", "ent:Ab", "ent:AB", "ent:%41b", "ent:a~b", "ent:a:b",
    ]
    .iter()
    .map(|id| file_id(id).to_lowercase())
    .collect();
    assert_eq!(names.len(), 6, "{names:?}");
}

// Every kind lands one file per node and loads back whole.
#[test]
fn node_files_round_trip_every_kind() {
    let out = own_dir("node-round-trip");
    let mut s = every_kind_graph();
    s.out = out.clone();
    s.save();
    let files = tree_bytes(&out);
    for name in [
        "graph/entities/ent~order.yaml",
        "graph/entities/ent~%C4%8Daj.yaml",
        "graph/requirements/req~orders-1.yaml",
        "graph/criteria/crit~orders-1.yaml",
        "graph/views/view~class+shop.yaml",
        "graph/diagnostics/diag~lint-1.yaml",
        "graph/redirects/ent~purchase.yaml",
        "graph/relationships/rel~order%7Epayment.yaml",
        "graph/state-machines/sm~order.yaml",
        "status.yaml",
    ] {
        assert!(
            files.contains_key(name),
            "{name} missing from {:?}",
            files.keys()
        );
    }
    assert_eq!(
        files["graph/redirects/ent~purchase.yaml"],
        b"id: ent:purchase\nto: ent:order\n"
    );
    assert!(files.values().all(|b| b.ends_with(b"\n")));
    // Every node file starts with its id and holds only the node's own facts: no
    // generation markers, no mention a requirement adds by reference.
    let order = String::from_utf8(files["graph/entities/ent~order.yaml"].clone()).unwrap();
    assert!(order.starts_with("id: ent:order\n"), "{order}");
    assert!(
        !order.contains("mentions") && !order.contains("created"),
        "{order}"
    );
    let req = String::from_utf8(files["graph/requirements/req~orders-1.yaml"].clone()).unwrap();
    assert!(
        req.starts_with("id: req:orders-1\n") && !req.contains("updated:"),
        "{req}"
    );
    assert!(files.contains_key("markers.yaml"));
    let back = Store::load(&out);
    assert!(back.load_errors.is_empty(), "{:?}", back.load_errors);
    let json = |g: &Graph| serde_json::to_value(g).unwrap();
    assert_eq!(json(&back.graph), json(&s.graph));
}

// The same graph always writes the same bytes, wherever and however often it saves.
#[test]
fn two_saves_of_one_graph_write_identical_bytes() {
    let one = own_dir("deterministic-one");
    let two = own_dir("deterministic-two");
    let mut s = every_kind_graph();
    s.out = one.clone();
    s.save();
    let first = tree_bytes(&one);
    s.save();
    assert_eq!(tree_bytes(&one), first);
    let mut again = Store::load(&one);
    again.out = two.clone();
    again.save();
    assert_eq!(tree_bytes(&two), first);
}

// A node that leaves the graph takes its file along; the others stay.
#[test]
fn a_removed_node_deletes_its_file() {
    let out = own_dir("node-delete");
    let mut s = every_kind_graph();
    s.out = out.clone();
    s.save();
    s.graph.entities.remove("ent:čaj");
    s.graph.redirects.clear();
    s.graph.diagnostics.clear();
    s.save();
    let files = tree_bytes(&out);
    assert!(!files.contains_key("graph/entities/ent~%C4%8Daj.yaml"));
    assert!(!files.keys().any(|k| k.starts_with("graph/redirects/")));
    assert!(!files.keys().any(|k| k.starts_with("graph/diagnostics/")));
    assert!(files.contains_key("graph/entities/ent~order.yaml"));
    let back = Store::load(&out);
    assert!(!back.graph.entities.contains_key("ent:čaj"));
    assert!(back.graph.redirects.is_empty());
}

// A file name that spells no id is unreadable, never dropped.
#[test]
fn a_node_file_name_that_does_not_decode_makes_the_store_read_only() {
    let out = own_dir("node-bad-name");
    let mut s = every_kind_graph();
    s.out = out.clone();
    s.save();
    crate::vfs::write(out.join("graph/entities/Order.yaml"), "name: Order\n").unwrap();
    let back = Store::load(&out);
    assert!(
        back.load_errors.iter().any(|e| e.contains("Order.yaml")),
        "{:?}",
        back.load_errors
    );
    back.save();
    assert!(out.join("graph/entities/Order.yaml").vexists());
}

// A fresh checkout: only the committed state is present. The derived kinds are
// recomputed at load, a missing journal reads as empty history, and only the
// document whose bytes changed outside Jazyk is dirty.
// Mirrors docs/compiler/graph.md#a-fresh-checkout.
#[test]
fn a_fresh_checkout_loads_committed_state_and_dirties_only_the_edited_document() {
    let out = own_dir("fresh-checkout");
    let mut s = every_kind_graph();
    s.out = out.clone();
    let a = "# Orders\nAn order holds a payment.\n\n## Paying\nAn order is paid.\n";
    let b = "# Payments\nA payment settles an order.\n";
    let parse = |docs: &[(&str, &str)]| -> crate::reconcile::ParsedDocs {
        docs.iter()
            .map(|(d, t)| (d.to_string(), (hash_hex(t), crate::md::parse_sections(t))))
            .collect()
    };
    s.sync_docs(&parse(&[("orders.md", a), ("payments.md", b)]));
    s.save();
    let derived = serde_json::to_value((&s.graph.relationships, &s.graph.state_machines)).unwrap();
    assert!(!s.graph.relationships.is_empty() && !s.graph.state_machines.is_empty());
    // What a clone carries: the committed paths only.
    for local in [
        "graph/default-views",
        "graph/relationships",
        "graph/state-machines",
        "journal",
        "diagrams",
        "trace",
    ] {
        crate::vfs::remove_dir_all(out.join(local)).ok();
    }
    crate::vfs::remove_file(out.join(".lock")).ok();
    crate::vfs::remove_file(out.join(BUILD_FILE)).ok();
    crate::vfs::remove_file(out.join(MARKERS_FILE)).ok();
    crate::vfs::remove_file(out.join(CAUSES_FILE)).ok();
    let committed: Vec<String> = tree_bytes(&out).into_keys().collect();
    assert!(
        committed
            .iter()
            .all(|p| crate::document_sets::committed(&format!(".jazyk/ds_main/{p}"))),
        "{committed:?}"
    );
    let mut opened = Store::load(&out);
    assert!(opened.load_errors.is_empty(), "{:?}", opened.load_errors);
    // The generation is local: a clone starts at the committed floor, and the
    // records it carries name no generation of this checkout.
    assert_eq!(opened.status.generation, generation_floor(&opened.status));
    assert!(opened.status.changes.iter().all(|c| c.generation == 0));
    assert_eq!(
        serde_json::to_value((&opened.graph.relationships, &opened.graph.state_machines)).unwrap(),
        derived
    );
    assert!(opened.journal_entries().is_empty());
    // The prose of one document changed on another branch.
    let b2 = "# Payments\nA payment settles an order within a day.\n";
    let dirty = opened.sync_docs_in_memory(&parse(&[("orders.md", a), ("payments.md", b2)]));
    assert_eq!(
        dirty.iter().map(|d| d.doc.as_str()).collect::<Vec<_>>(),
        vec!["payments.md"]
    );
    // A commit on the checkout continues the generation and writes the derived kinds.
    let mut opened = Store::load(&out);
    let generation = opened.status.generation;
    let r = opened.apply(vec![create("ent:refund", "Refund")], &session());
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    assert_eq!(opened.status.generation, generation + 1);
    assert!(out
        .join("graph/relationships/rel~order%7Epayment.yaml")
        .vexists());
    assert!(out.join("graph/state-machines/sm~order.yaml").vexists());
}

// Ids past the cap shorten to a prefix and a hash, carry the full id inside, load
// back whole, and never share a name. Mirrors docs/compiler/graph.md#node-files.
#[test]
fn long_ids_shorten_carry_their_id_and_never_collide() {
    let long = |tail: &str| format!("ent:{}{}", "č".repeat(40), tail);
    let ids: Vec<String> = (0..200).map(|n| long(&format!("-{n}"))).collect();
    let mut names = BTreeSet::new();
    for id in &ids {
        let (stem, carries) = node_stem(id);
        assert!(carries && stem.len() <= FILE_ID_MAX, "{stem}");
        assert!(stem.contains("%~") && id_from_file(&stem).is_none());
        assert!(names.insert(stem.to_lowercase()), "collision on {id}");
    }
    // An id just at the cap stays plain; one byte more shortens.
    let at = format!("ent:{}", "a".repeat(FILE_ID_MAX - 4));
    assert_eq!(node_stem(&at), (file_id(&at), false));
    let over = format!("{at}a");
    assert!(node_stem(&over).1);
    // A cut never splits an escape.
    for pad in 0..3 {
        let id = format!("ent:{}{}", "a".repeat(pad), "č".repeat(80));
        let stem = node_stem(&id).0;
        let prefix = stem.split("%~").next().unwrap();
        assert!(
            !prefix.ends_with('%') && prefix.as_bytes()[prefix.len() - 2] != b'%',
            "{stem}"
        );
    }

    let out = own_dir("long-ids");
    let mut s = every_kind_graph();
    s.out = out.clone();
    for id in &ids {
        s.graph.entities.insert(id.clone(), entity("Long"));
    }
    s.graph.redirects.insert(long("-old"), "ent:order".into());
    s.save();
    let stem = node_stem(&ids[0]).0;
    let text = crate::vfs::read_to_string(out.join(format!("graph/entities/{stem}.yaml"))).unwrap();
    assert!(text.starts_with(&format!("id: {}\n", ids[0])), "{text}");
    let back = Store::load(&out);
    assert!(back.load_errors.is_empty(), "{:?}", back.load_errors);
    let json = |g: &Graph| serde_json::to_value(g).unwrap();
    assert_eq!(json(&back.graph), json(&s.graph));
    // Removing a long node removes its file.
    let mut s = back;
    s.graph.entities.remove(&ids[0]);
    s.save();
    assert!(!out.join(format!("graph/entities/{stem}.yaml")).vexists());
    // A shortened name whose carried id spells another name is unreadable.
    crate::vfs::write(
        out.join(format!("graph/entities/{stem}.yaml")),
        format!("id: {}\nname: Wrong\n", ids[1]),
    )
    .unwrap();
    assert!(!Store::load(&out).load_errors.is_empty());
}

// The generation is local: a load raises build.yaml to the highest generation the
// committed status names. Mirrors docs/compiler/graph.md#the-generation.
#[test]
fn the_generation_starts_at_the_committed_floor() {
    let out = own_dir("generation-floor");
    let mut s = every_kind_graph();
    s.out = out.clone();
    s.status.generation = 12;
    s.status.changes.push(ChangeRecord::new(
        12,
        1,
        1,
        CHANGE_ENTITY,
        "ent:order",
        "name",
    ));
    s.save();
    let status = crate::vfs::read_to_string(out.join("status.yaml")).unwrap();
    for key in ["generation:", "verdict:", "costs:", "spent:"] {
        assert!(
            !status.lines().any(|l| l.starts_with(key)),
            "{key} in {status}"
        );
    }
    assert_eq!(read_generation(&out), 12);
    crate::vfs::remove_file(out.join(BUILD_FILE)).unwrap();
    assert_eq!(
        Store::load(&out).status.generation,
        12,
        "a checkout starts at the floor"
    );
    crate::vfs::write(out.join(BUILD_FILE), "generation: 3\n").unwrap();
    let mut low = Store::load(&out);
    assert_eq!(
        low.status.generation, 12,
        "a stale local counter rises to the floor"
    );
    let r = low.apply(vec![create("ent:refund", "Refund")], &session());
    assert_eq!((r.applied, r.generation), (1, 13), "{:?}", r.skipped);
    assert_eq!(
        read_generation(&out),
        low.status.generation,
        "the commit wrote the raised counter"
    );
    crate::vfs::write(out.join(BUILD_FILE), "generation: 20\n").unwrap();
    assert_eq!(Store::load(&out).status.generation, 20);
}

// Two branches from one base each commit and build. The committed status.yaml
// merges as text with no conflict, where the local build.yaml (both bumped the
// generation) would conflict. A run that changes nothing in the graph leaves the
// committed status.yaml bytes alone. Mirrors docs/compiler/graph.md#the-generation.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn parallel_branches_merge_status_without_conflict() {
    let root = own_dir("status-merge");
    let (base, a, b) = (root.join("base"), root.join("a"), root.join("b"));
    let mut s = every_kind_graph();
    s.out = base.clone();
    s.save();
    let base_status = std::fs::read(base.join("status.yaml")).unwrap();
    // A run that only spends and judges convergence.
    let mut run = Store::load(&base);
    run.status.costs.sessions = 3;
    run.status.costs.tokens = 9000;
    run.status.spent.rounds = 7;
    run.status.verdict.state = "converged".into();
    run.save_status();
    assert_eq!(
        std::fs::read(base.join("status.yaml")).unwrap(),
        base_status
    );
    copy_dir(&base, &a);
    copy_dir(&base, &b);
    // Branch a commits once and keeps its open change records.
    let mut sa = Store::load(&a);
    assert_eq!(
        sa.apply(vec![create("ent:alpha", "Alpha")], &session())
            .applied,
        1
    );
    sa.status.costs.tokens = 111;
    sa.save_status();
    // Branch b commits twice, then converges: its records clear.
    let mut sb = Store::load(&b);
    assert_eq!(
        sb.apply(vec![create("ent:beta", "Beta")], &session())
            .applied,
        1
    );
    assert_eq!(
        sb.apply(vec![create("ent:gamma", "Gamma")], &session())
            .applied,
        1
    );
    let ids: Vec<String> = sb.status.changes.iter().map(|c| c.id.clone()).collect();
    sb.clear_changes(&ids);
    sb.status.costs.tokens = 222;
    sb.save_status();
    let merge = |file: &str| {
        std::process::Command::new("git")
            .args(["merge-file", "-p"])
            .arg(a.join(file))
            .arg(base.join(file))
            .arg(b.join(file))
            .output()
    };
    let Ok(merged) = merge("status.yaml") else {
        eprintln!("no git binary; the status split was checked without a text merge");
        return;
    };
    assert!(
        merged.status.success(),
        "status.yaml conflicts:\n{}",
        String::from_utf8_lossy(&merged.stdout)
    );
    // b ended where the base was, so the merge is a's status.
    assert_eq!(merged.stdout, std::fs::read(a.join("status.yaml")).unwrap());
    assert_eq!(std::fs::read(b.join("status.yaml")).unwrap(), base_status);
    // The local half both branches bumped is exactly what would have conflicted.
    let local = merge(BUILD_FILE).unwrap();
    assert!(!local.status.success(), "build.yaml merged cleanly");
    // The merged checkout: a's tree, b's nodes, the merged status, a's local file.
    copy_dir(&b.join("graph"), &a.join("graph"));
    std::fs::write(a.join("status.yaml"), &merged.stdout).unwrap();
    let m = Store::load(&a);
    assert!(m.load_errors.is_empty(), "{:?}", m.load_errors);
    for id in ["ent:alpha", "ent:beta", "ent:gamma"] {
        assert!(m.graph.entities.contains_key(id), "{id}");
    }
    assert!(m.status.generation >= generation_floor(&m.status));
}

// An entity no document states takes its mentions from the requirements naming it.
// The first is the most upstream one by link level from the roots, not the first by
// path: `catalog.md` sorts before `system.md`, but the root `system.md` links to it.
// Mirrors docs/compiler/graph.md#node-files.
#[test]
fn by_reference_mentions_follow_link_levels_from_the_roots() {
    let out = own_dir("mention-levels").join(".jazyk/ds_main");
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    seed_doc(
        &mut s,
        "system.md",
        "# System\nThe system serves a customer. See [the catalog](catalog.md).\n",
    );
    seed_doc(
        &mut s,
        "catalog.md",
        "# Catalog\nA customer browses the catalog.\n",
    );
    s.status.roots = vec!["system.md".into()];
    let mut customer = entity("Customer");
    customer.provenance = Some(derived(&["req:x"]));
    let req = |doc: &str, sec: &str, quote: &str| Op::CreateRequirement {
        id: String::new(),
        requirement: Requirement {
            statement: quote.into(),
            entities: vec!["ent:customer".into()],
            source: Some(mention(doc, sec, quote)),
            facets: vec![Facet {
                facet: "behavior".into(),
                reasoning: "stated".into(),
                measure: None,
            }],
            ..Default::default()
        },
    };
    // The catalog requirement lands first, so neither commit order nor path order
    // would put the root first.
    let r = s.apply(
        vec![
            Op::CreateEntity {
                id: "ent:customer".into(),
                entity: customer,
            },
            req("catalog.md", "/catalog", "A customer browses the catalog."),
        ],
        &session(),
    );
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    let r = s.apply(
        vec![req("system.md", "/system", "The system serves a customer.")],
        &session(),
    );
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    let docs = |st: &Store| -> Vec<String> {
        st.graph.entities["ent:customer"]
            .mentions
            .iter()
            .map(|m| m.doc.clone())
            .collect()
    };
    assert_eq!(docs(&s), ["system.md", "catalog.md"]);
    let loaded = Store::load(&out);
    assert!(loaded.load_errors.is_empty(), "{:?}", loaded.load_errors);
    assert_eq!(docs(&loaded), ["system.md", "catalog.md"]);
    // With no roots stamped, every document is its own level in path order.
    let mut no_roots = loaded;
    no_roots.status.roots.clear();
    let roots: Vec<String> = Vec::new();
    normalize_mentions(&mut no_roots.graph, &no_roots.docs, &roots, false);
    assert_eq!(docs(&no_roots), ["catalog.md", "system.md"]);
}

// Two branches from one base each extract a requirement about the same entity from
// different sections and commit. Git merges the whole store with no conflict: the
// ids carry no counter, the entity file holds no by-reference mention, and every
// change record is a file of its own. Mirrors docs/compiler/graph.md#node-files.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn parallel_branches_merge_the_whole_store_through_git() {
    let root = own_dir("git-merge");
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(&root)
            .output()
    };
    if git(&["init", "-q", "-b", "base"])
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("no git binary; the merge was not exercised");
        return;
    }
    let out = root.join(".jazyk/ds_main");
    let text = "# Shop\nThe shop sells tea.\n\n## Orders\nAn order holds items.\n\n## Payments\nA payment settles an order.\n";
    let mut s = Store {
        out: out.clone(),
        ..Default::default()
    };
    seed_doc(&mut s, "shop.md", text);
    let r = s.apply(vec![create("ent:order", "Order")], &session());
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    assert!(
        root.join(".jazyk/.gitignore").vexists(),
        "a save writes the ignore file"
    );
    let req = |section: &str, quote: &str| Op::CreateRequirement {
        id: String::new(),
        requirement: Requirement {
            statement: quote.into(),
            entities: vec!["ent:order".into()],
            source: Some(mention("shop.md", section, quote)),
            facets: vec![Facet {
                facet: "behavior".into(),
                reasoning: "stated".into(),
                measure: None,
            }],
            ..Default::default()
        },
    };
    let commit = |msg: &str| {
        let add = git(&["add", "-A"]).unwrap();
        assert!(
            add.status.success(),
            "{}",
            String::from_utf8_lossy(&add.stderr)
        );
        let c = git(&["commit", "-q", "-m", msg]).unwrap();
        assert!(c.status.success(), "{}", String::from_utf8_lossy(&c.stderr));
    };
    commit("base");
    assert!(git(&["checkout", "-q", "-b", "a"])
        .unwrap()
        .status
        .success());
    let mut sa = Store::load(&out);
    let r = sa.apply(
        vec![req("/shop/orders", "An order holds items.")],
        &session(),
    );
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    commit("a");
    assert!(git(&["checkout", "-q", "base"]).unwrap().status.success());
    assert!(git(&["checkout", "-q", "-b", "b"])
        .unwrap()
        .status
        .success());
    let mut sb = Store::load(&out);
    let r = sb.apply(
        vec![req("/shop/payments", "A payment settles an order.")],
        &session(),
    );
    assert_eq!(r.applied, 1, "{:?}", r.skipped);
    commit("b");
    let merged = git(&["merge", "-q", "--no-ff", "--no-edit", "a"]).unwrap();
    assert!(
        merged.status.success(),
        "the store conflicted:\n{}{}",
        String::from_utf8_lossy(&merged.stdout),
        String::from_utf8_lossy(&merged.stderr)
    );
    let m = Store::load(&out);
    assert!(m.load_errors.is_empty(), "{:?}", m.load_errors);
    assert_eq!(
        m.graph.requirements.len(),
        2,
        "{:?}",
        m.graph.requirements.keys()
    );
    let ids: BTreeSet<&String> = m.graph.requirements.keys().collect();
    assert!(
        ids.iter().any(|i| i.starts_with("req:shop-orders-")),
        "{ids:?}"
    );
    assert!(
        ids.iter().any(|i| i.starts_with("req:shop-payments-")),
        "{ids:?}"
    );
    // Both sources come back as mentions on the entity both branches named.
    assert_eq!(
        m.graph.entities["ent:order"].mentions.len(),
        2,
        "{:?}",
        m.graph.entities["ent:order"].mentions
    );
    let order = std::fs::read_to_string(out.join("graph/entities/ent~order.yaml")).unwrap();
    assert!(!order.contains("mentions"), "{order}");
    // Each branch's change records survive the merge.
    for id in &ids {
        assert!(
            m.status.has_change(CHANGE_REQ_CREATED, id),
            "{id}: {:?}",
            m.status.changes
        );
    }
    // Nothing local was committed.
    let tracked = git(&["ls-files"]).unwrap();
    let tracked = String::from_utf8_lossy(&tracked.stdout);
    for local in [
        "build.yaml",
        "markers.yaml",
        "causes.yaml",
        "journal/",
        "relationships/",
        "default-views/",
    ] {
        assert!(!tracked.contains(local), "{local} tracked:\n{tracked}");
    }
    assert!(tracked.contains(".jazyk/ds_main/changes/"), "{tracked}");
}

// A version 3 store loads as it is, and its next save writes the version 4 layout
// with every id and fact kept. Mirrors docs/compiler/graph.md#store-version.
#[test]
fn a_version_3_store_upgrades_in_place_at_its_next_save() {
    let out = own_dir("v3-upgrade");
    let w = |rel: &str, text: &str| {
        let p = out.join(rel);
        crate::vfs::create_dir_all(p.parent().unwrap()).unwrap();
        crate::vfs::write(p, text).unwrap();
    };
    w("status.yaml", "version: 3\nchanges:\n- id: c4-1\n  generation: 4\n  mutation: 1\n  kind: entity-changed\n  subject: ent:order\n  via: name\nlookalikes:\n- pair: ent:order~ent:purchase\n  verdict: separate\n  score: 0.6\n  generation: 3\nroots:\n- shop.md\n");
    w("build.yaml", "generation: 4\n");
    w("graph/entities/ent~order.yaml", "name: Order\nmentions:\n- doc: shop.md\n  section: /shop\n  quote: An order holds items.\ncreated: g1\nupdated: g4\n");
    w("graph/requirements/req~shop-1.yaml", "statement: An order holds items.\nentities:\n- ent:order\nfacets:\n- facet: behavior\n  reasoning: stated\nsource:\n  doc: shop.md\n  section: /shop\n  quote: An order holds items.\ncreated: g2\nupdated: g2\n");
    w("graph/views/view~class+shop.yaml", "kind: class\ntitle: Shop\nmembers:\n- ent:order\nprovenance:\n  derived:\n    from:\n    - ent:order\n    reasoning: 'default view: x'\ndefault: true\n");
    w("graph/diagnostics/diag~lint-1.yaml", "rule: lint\nseverity: info\nsubjects:\n- req:shop-1\nmessage: m\nlifecycle: open\ncreated: g3\nupdated: g3\n");
    w("docs/shop.md.yaml", "contentHash: abc\nsections:\n  /shop:\n    title: Shop\n    kind: root\n    order: 0\n    raw: |-\n      # Shop\n      An order holds items.\n    hash: h1\n    lines:\n    - 0\n    - 2\n");
    w(
        "gen/ledger.yaml",
        "entities:\n  order:\n    factHash: f\n    requirements:\n    - req:shop-1\n",
    );
    let s = Store::load(&out);
    assert!(s.load_errors.is_empty(), "{:?}", s.load_errors);
    assert_eq!(s.graph.entities["ent:order"].updated.as_deref(), Some("g4"));
    assert_eq!(s.graph.entities["ent:order"].mentions.len(), 1);
    assert!(s.status.has_change(CHANGE_ENTITY, "ent:order"));
    assert_eq!(s.status.lookalikes.len(), 1);
    assert_eq!(s.docs["shop.md"].content_hash, "abc");
    assert_eq!(s.docs["shop.md"].sections["/shop"].lines, [0, 2]);
    let ledger = crate::gen::Ledger::load(&out);
    assert!(ledger.entities.contains_key("order"));
    let before = serde_json::to_value((&s.graph, &s.status.changes, &s.status.lookalikes)).unwrap();
    s.save();
    ledger.save(&out).unwrap();
    let status = std::fs::read_to_string(out.join("status.yaml")).unwrap();
    assert!(
        status.contains("version: 4")
            && !status.contains("changes")
            && !status.contains("lookalikes"),
        "{status}"
    );
    assert!(out.join("changes/entity-changed/ent~order.yaml").vexists());
    assert!(out
        .join("lookalikes/ent~order%7Eent~purchase.yaml")
        .vexists());
    let order = std::fs::read_to_string(out.join("graph/entities/ent~order.yaml")).unwrap();
    assert_eq!(order, "id: ent:order\nname: Order\n");
    // A default view is never committed: the recompute at load owns it.
    assert!(!out.join("graph/views/view~class+shop.yaml").vexists());
    let doc = std::fs::read_to_string(out.join("docs/shop.md.yaml")).unwrap();
    assert!(
        !doc.contains("contentHash") && !doc.contains("lines"),
        "{doc}"
    );
    assert!(std::fs::read_to_string(out.join(BUILD_FILE))
        .unwrap()
        .contains("shop.md: abc"));
    let wide = std::fs::read_to_string(out.join("gen/ledger.yaml")).unwrap();
    assert!(!wide.contains("factHash"), "{wide}");
    assert!(std::fs::read_to_string(out.join("gen/entities/order.yaml"))
        .unwrap()
        .starts_with("id: order\n"));
    // The upgraded store reads back the same graph, markers and all.
    let back = Store::load(&out);
    assert!(back.load_errors.is_empty(), "{:?}", back.load_errors);
    assert_eq!(back.status.version, STORE_VERSION);
    assert_eq!(
        serde_json::to_value((&back.graph, &back.status.changes, &back.status.lookalikes)).unwrap(),
        before
    );
    assert_eq!(back.docs["shop.md"].sections["/shop"].lines, [0, 2]);
    assert_eq!(back.docs["shop.md"].content_hash, "abc");
    assert_eq!(
        crate::gen::Ledger::load(&out).entities["order"].fact_hash,
        "f"
    );
    // A cleared record deletes its file.
    let mut s = back;
    s.status.changes.clear();
    s.save_status();
    assert!(!out.join("changes/entity-changed/ent~order.yaml").vexists());
}

// A status.yaml that does not parse is a broken file, not another version: a build
// leaves it in place and the store is read-only.
// Mirrors docs/compiler/graph.md#store-version.
#[test]
fn a_status_that_does_not_parse_is_never_archived() {
    let out = own_dir("status-torn");
    crate::vfs::create_dir_all(out.join("graph/entities")).unwrap();
    crate::vfs::write(out.join("status.yaml"), "version: [4\n").unwrap();
    crate::vfs::write(
        out.join("graph/entities/ent~order.yaml"),
        "id: ent:order\nname: Order\n",
    )
    .unwrap();
    let s = Store::open_for_build(&out);
    assert!(!s.load_errors.is_empty());
    assert!(out.join("graph/entities/ent~order.yaml").vexists());
    assert!(!out
        .with_file_name(format!(
            "{}.bak",
            out.file_name().unwrap().to_string_lossy()
        ))
        .vexists());
    s.save();
    assert_eq!(
        std::fs::read_to_string(out.join("status.yaml")).unwrap(),
        "version: [4\n"
    );
}

// Section line ranges recompute from the raw texts exactly as the parse gave them.
#[test]
fn section_lines_recompute_from_raw_texts() {
    let text =
        "Intro line.\n\n# Top\nBody.\n\n```mermaid\na\n```\n\n## Child\nMore.\n\n# Second\nEnd.\n";
    let parsed = crate::md::parse_sections(text);
    let mut rec = DocRecord {
        content_hash: String::new(),
        sections: parsed.clone(),
        coverage: BTreeMap::new(),
    };
    for s in rec.sections.values_mut() {
        s.lines = [0, 0];
    }
    fill_section_lines(&mut rec);
    for (r, s) in &parsed {
        assert_eq!(rec.sections[r].lines, s.lines, "{r}");
    }
}

// The store version: a build archives an out directory of another version to
// `<out>.bak` and starts empty; a reader treats it as empty without touching it.
// Mirrors docs/compiler/graph.md#store-version.
#[test]
fn store_version_archives_on_build_and_reads_as_empty() {
    let dir = own_dir("version");
    let mut s = Store {
        out: dir.clone(),
        ..Default::default()
    };
    seed_doc(&mut s, "t.md", "# T\ncart\n");
    s.apply(vec![create("ent:cart", "Cart")], &session());
    assert_eq!(Store::open_for_build(&dir).graph.entities.len(), 1);
    // Rewrite status.yaml without a version, as an older store would have it.
    crate::vfs::write(dir.join("status.yaml"), "generation: 7\n").unwrap();
    let read = Store::load(&dir);
    assert!(read.graph.entities.is_empty());
    assert_eq!(read.status.generation, 0);
    assert!(dir.join("graph/entities/ent~cart.yaml").vexists());
    let bak = dir.with_file_name(format!(
        "{}.bak",
        dir.file_name().unwrap().to_string_lossy()
    ));
    crate::vfs::remove_dir_all(&bak).ok();
    let fresh = Store::open_for_build(&dir);
    assert!(fresh.graph.entities.is_empty());
    assert!(!dir.join("status.yaml").vexists());
    assert!(bak.join("graph/entities/ent~cart.yaml").vexists());
    // A second archive replaces the first.
    crate::vfs::create_dir_all(&dir).unwrap();
    crate::vfs::write(dir.join("status.yaml"), "version: 1\ngeneration: 2\n").unwrap();
    crate::vfs::write(dir.join("marker"), "second").unwrap();
    let fresh = Store::open_for_build(&dir);
    assert!(fresh.graph.entities.is_empty());
    assert!(bak.join("marker").vexists());
    assert!(!bak.join("graph").vexists());
    // A fresh directory is simply empty: nothing to archive.
    crate::vfs::remove_dir_all(&dir).ok();
    assert!(Store::open_for_build(&dir).graph.entities.is_empty());
    assert!(!dir.join("status.yaml").vexists());
}
