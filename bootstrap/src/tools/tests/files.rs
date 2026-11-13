use super::*;

// write_text_file keeps the protected-path fence when no workspace exists.
// Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
#[test]
fn writes_refuse_protected_paths_without_a_workspace() {
    let base = std::env::temp_dir().join(format!(
        "jazyk-protected-{}-{}",
        crate::sys::process_id(),
        crate::verify::now_iso().replace([':', '.'], "-")
    ));
    crate::vfs::create_dir_all(&base).unwrap();
    let mut t = session();
    t.gen.deliverable = base.clone();
    for path in [
        ".env",
        ".git/config",
        ".jazyk/status.yaml",
        "keys/server.pem",
        "a/.ssh/id_rsa",
    ] {
        let err = t
            .dispatch("write_text_file", &json!({"path": path, "content": "x"}))
            .unwrap_err();
        assert_eq!(err.rule, "protected-path", "{path}: {}", err.message);
        assert!(!base.join(path).vexists(), "{path}");
    }
    t.dispatch(
        "write_text_file",
        &json!({"path": "src/ok.txt", "content": "fine"}),
    )
    .unwrap();
    assert!(base.join("src/ok.txt").vexists());
    crate::vfs::remove_dir_all(&base).ok();
}

// list_files walks a directory reached again through a link once, and stops at
// the listing cap with truncated set.
// Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
#[cfg(unix)]
#[test]
fn list_files_survives_symlink_loops_and_stops_at_the_cap() {
    let base = std::env::temp_dir().join(format!(
        "jazyk-listing-{}-{}",
        crate::sys::process_id(),
        crate::verify::now_iso().replace([':', '.'], "-")
    ));
    let deliverable = base.join("deliverable");
    crate::vfs::create_dir_all(deliverable.join("a")).unwrap();
    crate::vfs::write(deliverable.join("a").join("one.txt"), "1\n").unwrap();
    std::os::unix::fs::symlink(&deliverable, deliverable.join("a").join("loop")).unwrap();
    let mut t = session();
    t.gen.deliverable = deliverable.clone();
    let v = t.dispatch("list_files", &json!({})).unwrap();
    assert_eq!(v["files"], json!(["a/one.txt"]), "{}", v);
    assert!(v.get("truncated").is_none(), "{}", v);
    crate::vfs::create_dir_all(deliverable.join("b")).unwrap();
    for i in 0..LIST_FILES_LIMIT + 5 {
        crate::vfs::write(deliverable.join("b").join(format!("f{:04}.txt", i)), "x").unwrap();
    }
    let v = t.dispatch("list_files", &json!({})).unwrap();
    assert_eq!(
        v["files"].as_array().unwrap().len(),
        LIST_FILES_LIMIT,
        "{}",
        v["truncated"]
    );
    assert_eq!(v["truncated"], true);
    crate::vfs::remove_dir_all(&base).ok();
}

// The file tools' sandbox holds after symlinks resolve: a link inside the
// deliverable pointing outside is refused like `..`, and is not listed.
// Mirrors docs/compiler/tools.md#generation-tools.
#[test]
fn deliverable_paths_resolve_symlinks_before_trusting_them() {
    let base = std::env::temp_dir().join(format!(
        "jazyk-sandbox-{}-{}",
        crate::sys::process_id(),
        crate::verify::now_iso().replace([':', '.'], "-")
    ));
    let deliverable = base.join("deliverable");
    let outside = base.join("outside");
    crate::vfs::create_dir_all(deliverable.join("src")).unwrap();
    crate::vfs::create_dir_all(&outside).unwrap();
    crate::vfs::write(deliverable.join("src").join("ok.txt"), "fine\n").unwrap();
    crate::vfs::write(outside.join("secret.txt"), "no\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, deliverable.join("link")).unwrap();
    let mut t = session();
    t.gen.deliverable = deliverable.clone();
    let v = t
        .dispatch("read_text_file", &json!({"path": "src/ok.txt"}))
        .unwrap();
    assert!(
        v["content"].as_str().unwrap_or_default().contains("fine"),
        "{}",
        v
    );
    #[cfg(unix)]
    {
        let err = t
            .dispatch("read_text_file", &json!({"path": "link/secret.txt"}))
            .unwrap_err();
        assert_eq!(err.rule, "bad-path");
        assert!(err.message.contains("symlink"), "{}", err.message);
        let err = t
            .dispatch(
                "write_text_file",
                &json!({"path": "link/new.txt", "content": "x"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "bad-path");
        assert!(!outside.join("new.txt").vexists());
        let list = t.dispatch("list_files", &json!({})).unwrap();
        let files: Vec<String> = list["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap().to_string())
            .collect();
        assert!(files.contains(&"src/ok.txt".to_string()), "{:?}", files);
        assert!(!files.iter().any(|f| f.starts_with("link")), "{:?}", files);
    }
    let err = t
        .dispatch("read_text_file", &json!({"path": "../outside/secret.txt"}))
        .unwrap_err();
    assert_eq!(err.rule, "bad-path");
    crate::vfs::remove_dir_all(&base).ok();
}

#[test]
fn guessed_file_reads_keep_protected_paths_private_under_broad_source() {
    let root = std::env::temp_dir().join(format!(
        "jazyk-read-fence-{}-{}",
        crate::sys::process_id(),
        crate::verify::now_iso().replace([':', '.'], "-")
    ));
    let mut t = session();
    t.gen.deliverable = root.clone();
    t.gen.configured = true;
    t.gen.code = vec!["**".into()];
    for path in [
        ".env",
        ".jazyk/jazyk.toml",
        ".git/config",
        ".aws/credentials",
        ".ssh/id_ed25519",
        "src/secret.pem",
        "src/private.key",
        "src/local.env",
        ".hidden/state",
    ] {
        let full = root.join(path);
        crate::vfs::create_dir_all(full.parent().unwrap()).unwrap();
        crate::vfs::write(&full, "private fixture").unwrap();
        let error = t
            .dispatch("read_text_file", &json!({"path":path}))
            .unwrap_err();
        assert_eq!(error.rule, "protected-path", "{path}");
    }
    for path in [
        "docs/notes.md",
        "target/build-evidence.json",
        ".github/workflows/deploy.yml",
    ] {
        let full = root.join(path);
        crate::vfs::create_dir_all(full.parent().unwrap()).unwrap();
        crate::vfs::write(&full, "ordinary evidence").unwrap();
        let result = t.dispatch("read_text_file", &json!({"path":path})).unwrap();
        assert_eq!(result["content"], "ordinary evidence", "{path}");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join(".env"), root.join("environment.txt")).unwrap();
        std::os::unix::fs::symlink(root.join(".git"), root.join("repository")).unwrap();
        for path in ["environment.txt", "repository/config"] {
            let error = t
                .dispatch("read_text_file", &json!({"path":path}))
                .unwrap_err();
            assert_eq!(error.rule, "protected-path", "{path}");
        }
    }
    crate::vfs::remove_dir_all(root).ok();
}

#[cfg(unix)]
#[test]
fn public_source_alias_cannot_overwrite_or_read_private_environment() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".jazyk")).unwrap();
    std::fs::create_dir_all(root.path().join("deliverable")).unwrap();
    std::fs::write(root.path().join(crate::document_sets::CONFIG), "version = 1\n[[sets]]\nid = 'ds_a'\nname = 'A'\ndocs = ['docs/**']\nsource = ['deliverable/**']\n").unwrap();
    let private = root.path().join("deliverable/.env");
    std::fs::write(&private, "PRIVATE_ENVIRONMENT").unwrap();
    std::os::unix::fs::symlink(&private, root.path().join("deliverable/public.svg")).unwrap();
    let project = crate::document_sets::Workspace::load(root.path())
        .unwrap()
        .resolve(Some("ds_a"))
        .unwrap();
    let mut tools = ToolSession::new(
        Store::load(&project.out),
        WorkScope::serving("generate"),
        20,
        8000,
    );
    tools.gen = crate::gen::GenSettings::resolve(&project);
    let error = tools
        .dispatch(
            "write_text_file",
            &json!({"path":"deliverable/public.svg", "content":"changed"}),
        )
        .unwrap_err();
    assert_eq!(error.rule, "source-selection");
    let error = tools
        .dispatch("read_text_file", &json!({"path":"deliverable/public.svg"}))
        .unwrap_err();
    assert_eq!(error.rule, "protected-path");
    let error = tools.dispatch("write_text_file", &json!({"path":"deliverable/public.svg", "old_text":"PRIVATE_ENVIRONMENT", "content":"changed", "expectedHash":crate::task_runtime::digest(b"PRIVATE_ENVIRONMENT")})).unwrap_err();
    assert_eq!(error.rule, "source-selection");
    assert_eq!(
        std::fs::read_to_string(&private).unwrap(),
        "PRIVATE_ENVIRONMENT"
    );
}

#[test]
fn file_reads_bound_single_utf8_lines_and_resume_at_following_lines() {
    let root = std::env::temp_dir().join(format!(
        "jazyk-read-bound-{}-{}",
        crate::sys::process_id(),
        crate::verify::now_iso().replace([':', '.'], "-")
    ));
    crate::vfs::create_dir_all(&root).unwrap();
    let mut t = session();
    t.gen.deliverable = root.clone();
    // 3-byte characters put the boundary inside a scalar value at 64 KiB.
    let line = "界".repeat(ToolSession::READ_FILE_BYTES);
    crate::vfs::write(
        root.join("minified.json"),
        format!("{line}\nfollowing line\n"),
    )
    .unwrap();
    let first = t
        .dispatch("read_text_file", &json!({"path":"minified.json"}))
        .unwrap();
    let content = first["content"].as_str().unwrap();
    assert!(content.len() <= ToolSession::READ_FILE_BYTES);
    assert!(line.starts_with(content));
    assert_eq!(first["truncated"], true);
    assert_eq!(first["truncatedLine"], 1);
    assert_eq!(first["nextLine"], 2);
    assert!(first["notice"].as_str().unwrap().contains("omitted"));
    let next = t
        .dispatch(
            "read_text_file",
            &json!({"path":"minified.json","line":first["nextLine"]}),
        )
        .unwrap();
    assert_eq!(next["content"], "following line\n");
    assert!(next["truncated"].is_null());
    crate::vfs::write(root.join("minified.json"), &line).unwrap();
    let only = t
        .dispatch("read_text_file", &json!({"path":"minified.json"}))
        .unwrap();
    assert_eq!(only["truncatedLine"], 1);
    assert!(
        only["nextLine"].is_null(),
        "a partial last line has no following line"
    );
    let regular = "a".repeat(1023) + "\n";
    crate::vfs::write(root.join("many.txt"), regular.repeat(100)).unwrap();
    let page = t
        .dispatch("read_text_file", &json!({"path":"many.txt","limit":400}))
        .unwrap();
    assert_eq!(
        page["content"].as_str().unwrap().as_bytes(),
        regular.repeat(64).as_bytes()
    );
    assert_eq!(page["nextLine"], 65);
    assert!(page["truncatedLine"].is_null());
    crate::vfs::remove_dir_all(root).ok();
}

#[test]
fn literal_file_edit_preserves_unread_content_and_returns_whole_file_hashes() {
    let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let root = std::path::PathBuf::from("/project/deliverable");
    crate::vfs::create_dir_all(&root).unwrap();
    let original = "<title>Jazyk ž</title>\r\nregex = r'\\w+\\.py'\r\n<footer>keep me</footer>\r\n";
    crate::vfs::write(root.join("index.html"), original).unwrap();
    let mut tools = session();
    tools.gen.deliverable = root.clone();
    let page = tools
        .dispatch("read_text_file", &json!({"path":"index.html","limit":1}))
        .unwrap();
    assert_eq!(page["content"], "<title>Jazyk ž</title>\r\n");
    assert_eq!(
        page["hash"],
        crate::task_runtime::digest(original.as_bytes())
    );
    assert_eq!(page["truncated"], true);
    let replacement = "<title>Jazyk ✓</title>";
    let reply = tools.dispatch("write_text_file", &json!({"path":"index.html","old_text":"<title>Jazyk ž</title>","content":replacement,"expectedHash":page["hash"]})).unwrap();
    let expected = original.replace("<title>Jazyk ž</title>", replacement);
    assert_eq!(
        crate::vfs::read_to_string(root.join("index.html")).unwrap(),
        expected
    );
    assert_eq!(reply["replaced"], 1);
    assert_eq!(reply["fileBytes"], expected.len());
    assert_eq!(
        reply["hash"],
        crate::task_runtime::digest(expected.as_bytes())
    );
    let appended = tools.dispatch("write_text_file", &json!({"path":"index.html","content":"<!-- fin -->","append":true,"expectedHash":reply["hash"]})).unwrap();
    assert_eq!(
        appended["hash"],
        crate::task_runtime::digest(format!("{expected}<!-- fin -->").as_bytes())
    );
    tools.dispatch("write_text_file", &json!({"path":"index.html","content":"","old_text":"<!-- fin -->","expectedHash":appended["hash"]})).unwrap();
    assert_eq!(
        crate::vfs::read_to_string(root.join("index.html")).unwrap(),
        expected
    );
}

#[test]
fn literal_file_edits_refuse_stale_missing_or_ambiguous_context_without_writing() {
    let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let root = std::path::PathBuf::from("/project/deliverable");
    crate::vfs::create_dir_all(&root).unwrap();
    let path = root.join("page.txt");
    let mut tools = session();
    tools.gen.deliverable = root;
    crate::vfs::write(&path, "original").unwrap();
    let stale = tools
        .dispatch("read_text_file", &json!({"path":"page.txt"}))
        .unwrap()["hash"]
        .clone();
    crate::vfs::write(&path, "aaa other aaa").unwrap();
    let hash = crate::task_runtime::digest(b"aaa other aaa");
    for (args, rule) in [
        (
            json!({"old_text":"original","expectedHash":stale}),
            "file-conflict",
        ),
        // Without expectedHash the edit applies to the version this session last
        // read, which is stale here.
        (json!({"old_text":"aaa"}), "file-conflict"),
        (json!({"old_text":"aaa","expectedHash":""}), "bad-args"),
        (
            json!({"old_text":"aaa","expectedHash":hash,"append":true}),
            "invalid-edit",
        ),
        (json!({"old_text":"","expectedHash":hash}), "edit-match"),
        (
            json!({"old_text":"absent","expectedHash":hash}),
            "edit-match",
        ),
        (json!({"old_text":"aaa","expectedHash":hash}), "edit-match"),
        (json!({"old_text":"aa","expectedHash":hash}), "edit-match"),
        (json!({"expectedHash":stale}), "file-conflict"),
        (json!({"expectedHash":stale,"append":true}), "file-conflict"),
    ] {
        let mut args = args;
        args["path"] = json!("page.txt");
        args["content"] = json!("replacement");
        assert_eq!(
            tools.dispatch("write_text_file", &args).unwrap_err().rule,
            rule
        );
        assert_eq!(crate::vfs::read_to_string(&path).unwrap(), "aaa other aaa");
    }
    assert_eq!(
        crate::vfs::read_dir(path.parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn atomic_file_tool_edits_preserve_native_executable_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("check.sh");
    std::fs::write(&path, "#!/bin/sh\necho before\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o750)).unwrap();
    let mut tools = session();
    tools.gen.deliverable = root.path().to_path_buf();
    let read = tools
        .dispatch("read_text_file", &json!({"path":"check.sh"}))
        .unwrap();
    tools.dispatch("write_text_file", &json!({"path":"check.sh","old_text":"echo before","content":"echo after","expectedHash":read["hash"]})).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "#!/bin/sh\necho after\n"
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o750
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn svg_reads_copy_exact_utf8_line_endings_and_final_newlines() {
    let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let root = std::path::Path::new("/project");
    crate::vfs::create_dir_all(root.join(".jazyk")).unwrap();
    crate::vfs::create_dir_all(root.join("assets")).unwrap();
    crate::vfs::write(root.join(crate::document_sets::CONFIG), "version = 1\n[[sets]]\nid = 'ds_svg'\nname = 'SVG'\ndocs = ['docs/**']\nsource = ['product/**']\n").unwrap();
    let project = crate::document_sets::Workspace::load(root)
        .unwrap()
        .resolve(Some("ds_svg"))
        .unwrap();
    let mut tools = ToolSession::new(
        Store::load(&project.out),
        WorkScope::serving("generate"),
        100,
        24000,
    );
    tools.gen = crate::gen::GenSettings::resolve(&project);
    let lines = [
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 20\">",
        "  <title>Jazyk, žluťoučký ✓</title>",
        "  <path fill=\"#112233\" d=\"M1 1H99V19H1Z\"/>",
        "</svg>",
    ];
    for (name, ending) in [("lf", "\n"), ("crlf", "\r\n")] {
        for final_newline in [false, true] {
            let source = lines.join(ending) + if final_newline { ending } else { "" };
            let input = format!("assets/{name}-{final_newline}.svg");
            crate::vfs::write(root.join(&input), source.as_bytes()).unwrap();
            for limit in [None, Some(2)] {
                let output = format!("product/{name}-{final_newline}-{limit:?}.svg");
                let mut line = 1;
                let mut append = false;
                loop {
                    let read = tools
                        .dispatch(
                            "read_text_file",
                            &json!({"path":input,"line":line,"limit":limit}),
                        )
                        .unwrap();
                    assert_eq!(read["totalLines"], lines.len());
                    assert!(read["truncatedLine"].is_null());
                    tools
                        .dispatch(
                            "write_text_file",
                            &json!({"path":output,"content":read["content"],"append":append}),
                        )
                        .unwrap();
                    let Some(next) = read["nextLine"].as_u64() else {
                        break;
                    };
                    assert!(next > line);
                    line = next;
                    append = true;
                }
                assert_eq!(
                    crate::vfs::read(root.join(&output)).unwrap(),
                    source.as_bytes(),
                    "{output}"
                );
            }
        }
    }
}

#[test]
fn complete_file_pages_preserve_delimiters_at_line_and_byte_caps() {
    let _scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let root = std::path::Path::new("/project");
    crate::vfs::create_dir_all(root).unwrap();
    let mut tools = session();
    tools.gen.deliverable = root.into();
    // Both replies span pages. The first reaches the unchanged default line
    // count; the second reaches exactly 64 KiB including its CRLF delimiters.
    for (path, source, first_next, first_bytes) in [
        (
            "lines.txt",
            "line\r\n".repeat(401),
            401,
            "line\r\n".len() * ToolSession::READ_FILE_LINES,
        ),
        (
            "bytes.txt",
            ("x".repeat(1022) + "\r\n").repeat(100),
            65,
            ToolSession::READ_FILE_BYTES,
        ),
    ] {
        crate::vfs::write(root.join(path), source.as_bytes()).unwrap();
        let first = tools
            .dispatch("read_text_file", &json!({"path":path}))
            .unwrap();
        assert_eq!(first["nextLine"], first_next);
        assert_eq!(first["truncated"], true);
        let prefix = first["content"].as_str().unwrap();
        assert_eq!(prefix.len(), first_bytes);
        assert!(source.starts_with(prefix));
        assert!(prefix.ends_with("\r\n"));
        let last = tools
            .dispatch(
                "read_text_file",
                &json!({"path":path,"line":first["nextLine"]}),
            )
            .unwrap();
        assert!(last["nextLine"].is_null());
        assert!(last["truncated"].is_null());
        assert_eq!(
            format!("{prefix}{}", last["content"].as_str().unwrap()).as_bytes(),
            source.as_bytes()
        );
    }
}

// write_text_file shares a file across the component group and refuses it to
// another group; a path restating the deliverable's own name lands under the
// root with a note, in every file tool.
// Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
#[test]
fn file_tools_share_within_a_group_and_unroot_a_restated_deliverable() {
    let dir = std::env::temp_dir().join(format!(
        "jazyk-group-files-{}-{}",
        crate::sys::process_id(),
        crate::verify::now_iso().replace([':', '.'], "-")
    ));
    let deliverable = dir.join("handbook");
    crate::vfs::create_dir_all(&deliverable).unwrap();
    let mut s = Store {
        out: dir.join("out"),
        ..Default::default()
    };
    s.graph.entities.insert("ent:org".into(), plain("Org"));
    for (id, name, parent) in [
        ("ent:operations", "Operations", "ent:org"),
        ("ent:store-manager", "Store Manager", "ent:operations"),
        ("ent:sales", "Sales", "ent:org"),
    ] {
        let mut e = plain(name);
        e.parent = Some(parent.into());
        s.graph.entities.insert(id.into(), e);
    }
    let mut ledger = crate::gen::Ledger::load(&s.out);
    ledger.entities.insert(
        "operations".into(),
        crate::gen::EntityGen {
            fact_hash: String::new(),
            requirements: Vec::new(),
            files: vec!["departments.md".into()],
            tasks: Vec::new(),
            unattached: None,
            recorded_at: None,
            record_sequence: 0,
        },
    );
    ledger.save(&s.out).unwrap();
    let session_for = |target: &str| {
        let goals = vec![Goal {
            id: format!("g:generate:{}", target),
            kind: "generate".into(),
            mandatory: true,
            target: target.into(),
            ..Default::default()
        }];
        let mut t = ToolSession::new(s.clone(), WorkScope::for_batch("b0-1", &goals), 64, 24_000);
        t.gen.deliverable = deliverable.clone();
        t
    };
    // A member of the owner's group writes the shared page.
    let mut t = session_for("ent:store-manager");
    t.dispatch(
        "write_text_file",
        &json!({"path": "departments.md", "content": "# Departments\n"}),
    )
    .unwrap();
    let read = t
        .dispatch("read_text_file", &json!({"path":"departments.md"}))
        .unwrap();
    assert_eq!(read["owner"], "ent:operations");
    assert_eq!(read["requires_generation"], false);
    assert!(read.get("begin_generation").is_none());
    // Another group is refused, the owner named.
    let mut t = session_for("ent:sales");
    let read = t
        .dispatch("read_text_file", &json!({"path":"departments.md"}))
        .unwrap();
    assert_eq!(read["owner"], "ent:operations");
    assert_eq!(read["begin_generation"]["entity"], "ent:operations");

    let err = t
        .dispatch(
            "write_text_file",
            &json!({"path": "departments.md", "content": "x"}),
        )
        .unwrap_err();
    assert_eq!(err.rule, "file-owned");
    assert!(err.message.contains("operations"), "{}", err.message);
    let err = t.dispatch("write_text_file", &json!({"path":"departments.md", "old_text":"# Departments", "content":"# Changed", "expectedHash":read["hash"]})).unwrap_err();
    assert_eq!(err.rule, "file-owned");
    assert_eq!(
        crate::vfs::read_to_string(deliverable.join("departments.md")).unwrap(),
        "# Departments\n"
    );
    // The restated root: written, read, and listed under the root, with a note.
    let v = t
        .dispatch(
            "write_text_file",
            &json!({"path": "handbook/sales.md", "content": "# Sales\n"}),
        )
        .unwrap();
    assert_eq!(v["written"], "sales.md", "{}", v);
    assert!(
        v["note"]
            .as_str()
            .unwrap_or_default()
            .contains("`sales.md`"),
        "{}",
        v
    );
    assert!(deliverable.join("sales.md").vis_file());
    assert!(!deliverable.join("handbook").vexists());
    let v = t
        .dispatch("read_text_file", &json!({"path": "handbook/sales.md"}))
        .unwrap();
    assert!(
        v["content"]
            .as_str()
            .unwrap_or_default()
            .contains("# Sales"),
        "{}",
        v
    );
    assert!(v["note"].is_string(), "{}", v);
    let v = t
        .dispatch("list_files", &json!({"path": "handbook"}))
        .unwrap();
    assert!(
        v["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "sales.md"),
        "{}",
        v
    );
    // A deliverable that really holds `handbook/` keeps the path as given.
    crate::vfs::create_dir_all(deliverable.join("handbook")).unwrap();
    let v = t
        .dispatch(
            "write_text_file",
            &json!({"path": "handbook/inner.md", "content": "x"}),
        )
        .unwrap();
    assert_eq!(v["written"], "handbook/inner.md", "{}", v);
    assert!(v["note"].is_null(), "{}", v);
    crate::vfs::remove_dir_all(&dir).ok();
}

fn memory_project(files: &[(&str, &[u8])]) -> (crate::vfs::Scope, ToolSession, std::path::PathBuf) {
    let scope = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
    let root = std::path::PathBuf::from("/project/deliverable");
    crate::vfs::create_dir_all(&root).unwrap();
    for (path, bytes) in files {
        let path = root.join(path);
        crate::vfs::create_dir_all(path.parent().unwrap()).unwrap();
        crate::vfs::write(path, bytes).unwrap();
    }
    let mut tools = session();
    tools.gen.deliverable = root.clone();
    (scope, tools, root)
}

// search_files is the in-process grep: regex or literal, case folding, path and
// glob narrowing, context lines, per-file counts, paging, and the read fence. It
// runs over the in-memory filesystem a hosted workspace uses.
// Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
#[test]
fn search_files_greps_eligible_text_over_the_virtual_filesystem() {
    let (_scope, mut tools, _) = memory_project(&[
        ("src/main.rs", b"fn main() {\n    greet(\"World\");\n}\n\nfn greet(name: &str) {\n    println!(\"Hello {name}\");\n}\n"),
        ("src/lib/util.ts", b"export function greet() {}\n// GREET twice: greet\n"),
        ("README.md", b"Call greet(name) to say hello.\n"),
        (".env", b"GREET_TOKEN=secret\n"),
        ("logo.png", b"\x89PNG\0greet"),
    ]);
    let v = tools
        .dispatch("search_files", &json!({"pattern": "fn \\w+\\("}))
        .unwrap();
    assert_eq!(
        v["matches"],
        json!([
            {"path": "src/main.rs", "line": 1, "column": 1, "text": "fn main() {"},
            {"path": "src/main.rs", "line": 5, "column": 1, "text": "fn greet(name: &str) {"}
        ]),
        "{v}"
    );
    assert!(v.get("truncated").is_none(), "{v}");
    // The secret and the binary never answer; the binary counts as skipped.
    let v = tools
        .dispatch(
            "search_files",
            &json!({"pattern": "greet", "ignoreCase": true, "filesOnly": true}),
        )
        .unwrap();
    assert_eq!(
        v["matches"],
        json!([
            {"path": "README.md", "count": 1},
            {"path": "src/lib/util.ts", "count": 2},
            {"path": "src/main.rs", "count": 2}
        ]),
        "{v}"
    );
    assert_eq!(v["skipped"], 1, "{v}");
    let v = tools
        .dispatch(
            "search_files",
            &json!({"pattern": "greet(", "literal": true, "glob": "*.md"}),
        )
        .unwrap();
    assert_eq!(v["matches"][0]["path"], "README.md", "{v}");
    assert_eq!(v["matches"].as_array().unwrap().len(), 1, "{v}");
    let v = tools
        .dispatch(
            "search_files",
            &json!({"pattern": "World", "path": "src/main.rs", "context": 1}),
        )
        .unwrap();
    assert_eq!(
        v["matches"],
        json!([{"path": "src/main.rs", "line": 2, "column": 12, "text": "    greet(\"World\");",
                "before": ["fn main() {"], "after": ["}"]}]),
        "{v}"
    );
    let v = tools
        .dispatch(
            "search_files",
            &json!({"pattern": "greet", "glob": "src/**/*.ts"}),
        )
        .unwrap();
    assert_eq!(v["matches"].as_array().unwrap().len(), 2, "{v}");
    assert_eq!(v["files"], 1, "{v}");
    assert_eq!(
        tools
            .dispatch("search_files", &json!({"pattern": "greet("}))
            .unwrap_err()
            .rule,
        "bad-pattern"
    );
    assert_eq!(
        tools
            .dispatch("search_files", &json!({"pattern": "x", "path": "../etc"}))
            .unwrap_err()
            .rule,
        "bad-path"
    );
}

// A search past the match cap resumes from its cursor and no match is lost.
#[test]
fn search_files_resumes_past_the_match_cap() {
    let body: String = (0..SEARCH_FILES_MATCHES + 30)
        .map(|i| format!("hit {i}\n"))
        .collect();
    let (_scope, mut tools, _) =
        memory_project(&[("big.txt", body.as_bytes()), ("z.txt", b"hit z\n")]);
    let first = tools
        .dispatch("search_files", &json!({"pattern": "^hit"}))
        .unwrap();
    assert_eq!(first["scanComplete"], false, "{first}");
    assert_eq!(
        first["next"],
        json!({"path": "big.txt", "line": SEARCH_FILES_MATCHES + 1})
    );
    let second = tools
        .dispatch(
            "search_files",
            &json!({"pattern": "^hit", "cursor": first["next"]}),
        )
        .unwrap();
    assert_eq!(second["scanComplete"], true, "{second}");
    assert!(second.get("next").is_none(), "{second}");
    let matches = second["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 31);
    assert_eq!(matches[0]["line"], SEARCH_FILES_MATCHES + 1, "{second}");
    assert_eq!(matches[30]["path"], "z.txt", "{second}");
}

// A scan past the per-call byte budget stops with a cursor even when it found
// nothing yet; following the cursor reaches the match without rescanning.
#[test]
fn search_files_empty_page_past_the_scan_budget_carries_a_cursor() {
    let filler = "x\n".repeat(SEARCH_FILES_FILE_BYTES / 2 - 1);
    let mut files: Vec<(String, Vec<u8>)> = (0..12)
        .map(|i| (format!("f{i:02}.txt"), filler.clone().into_bytes()))
        .collect();
    files.push(("z.txt".into(), b"needle\n".to_vec()));
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    let (_scope, mut tools, _) = memory_project(&refs);
    let mut cursor = Value::Null;
    let mut pages = 0;
    let found = loop {
        pages += 1;
        let mut args = json!({"pattern": "needle"});
        if !cursor.is_null() {
            args["cursor"] = cursor.clone();
        }
        let v = tools.dispatch("search_files", &args).unwrap();
        if v["scanComplete"] == true {
            break v;
        }
        assert!(v["matches"].as_array().unwrap().is_empty(), "{v}");
        assert!(v["files"].as_u64().unwrap() > 0, "{v}");
        cursor = v["next"].clone();
        assert!(pages < 10, "the cursor must advance");
    };
    assert!(pages > 1, "the budget must cut the scan");
    assert_eq!(found["matches"][0]["path"], "z.txt", "{found}");
}

// list_files takes a glob: `*` stays in one directory, `**` crosses them, and a
// bare name pattern matches at any depth.
#[test]
fn list_files_filters_by_glob() {
    let (_scope, mut tools, _) = memory_project(&[
        ("src/a.rs", b"a"),
        ("src/deep/b.rs", b"b"),
        ("src/c.ts", b"c"),
        ("d.rs", b"d"),
    ]);
    let list = |tools: &mut ToolSession, pattern: &str| {
        tools
            .dispatch("list_files", &json!({"pattern": pattern}))
            .unwrap()["files"]
            .clone()
    };
    assert_eq!(
        list(&mut tools, "*.rs"),
        json!(["d.rs", "src/a.rs", "src/deep/b.rs"])
    );
    assert_eq!(list(&mut tools, "src/*.rs"), json!(["src/a.rs"]));
    assert_eq!(
        list(&mut tools, "src/**/*.rs"),
        json!(["src/a.rs", "src/deep/b.rs"])
    );
    assert_eq!(
        tools
            .dispatch("list_files", &json!({"path": "src", "pattern": "*.ts"}))
            .unwrap()["files"],
        json!(["src/c.ts"])
    );
}

// A literal edit without expectedHash applies to the version the session last read
// or wrote; an unread path is refused toward a read, a changed file is a conflict,
// replaceAll changes every occurrence, and a miss explains itself.
// Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
#[test]
fn literal_edits_use_the_remembered_version_and_explain_misses() {
    let (_scope, mut tools, root) =
        memory_project(&[("app.py", b"def f():\n    return  1\n\nx = f()\ny = f()\n")]);
    let path = root.join("app.py");
    let edit = |tools: &mut ToolSession, args: Value| {
        let mut args = args;
        args["path"] = json!("app.py");
        tools.dispatch("write_text_file", &args)
    };
    let err = edit(
        &mut tools,
        json!({"old_text": "x = f()", "content": "x = 2"}),
    )
    .unwrap_err();
    assert_eq!(err.rule, "edit-needs-read", "{}", err.message);
    assert!(err.message.contains("read_text_file"), "{}", err.message);
    tools
        .dispatch("read_text_file", &json!({"path": "app.py"}))
        .unwrap();
    let reply = edit(
        &mut tools,
        json!({"old_text": "x = f()", "content": "x = 2"}),
    )
    .unwrap();
    assert_eq!(reply["replaced"], 1);
    // The write's own hash is remembered: a second edit needs no new read.
    let err = edit(&mut tools, json!({"old_text": "f()", "content": "g()"})).unwrap_err();
    assert_eq!(err.rule, "edit-match");
    assert!(err.message.contains("2 matches"), "{}", err.message);
    let reply = edit(
        &mut tools,
        json!({"old_text": "f()", "content": "g()", "replaceAll": true}),
    )
    .unwrap();
    assert_eq!(reply["replaced"], 2);
    assert_eq!(
        crate::vfs::read_to_string(&path).unwrap(),
        "def g():\n    return  1\n\nx = 2\ny = g()\n"
    );
    let err = edit(
        &mut tools,
        json!({"old_text": "return 1", "content": "return 3"}),
    )
    .unwrap_err();
    assert_eq!(err.rule, "edit-match");
    assert!(err.message.contains("line 2"), "{}", err.message);
    crate::vfs::write(&path, "changed elsewhere\n").unwrap();
    let err = edit(&mut tools, json!({"old_text": "changed", "content": "x"})).unwrap_err();
    assert_eq!(err.rule, "file-conflict");
    assert_eq!(
        crate::vfs::read_to_string(&path).unwrap(),
        "changed elsewhere\n"
    );
}

// The per-call sessions of a serving carry the remembered file versions across
// ledger claims; the repeated-call guard starts fresh per unit.
#[test]
fn remembered_file_versions_survive_a_new_unit() {
    let (_scope, mut tools, _) = memory_project(&[("a.txt", b"one two\n")]);
    tools
        .dispatch("read_text_file", &json!({"path": "a.txt"}))
        .unwrap();
    let memory = tools.memory().for_new_unit();
    let mut next = session();
    next.gen.deliverable = tools.gen.deliverable.clone();
    next.adopt_memory(memory);
    next.dispatch(
        "write_text_file",
        &json!({"path": "a.txt", "old_text": "two", "content": "2"}),
    )
    .unwrap();
}

// The runtime's read-only classification: lookups run after a failed call in the
// same reply, writes do not. Mirrors docs/compiler/agent-runtime.md#models-and-tools.
#[test]
fn read_only_tools_exclude_every_write() {
    for name in [
        "load",
        "search",
        "read_section",
        "read_text_file",
        "list_files",
        "search_files",
        "read_document",
        "inspect_run",
    ] {
        assert!(crate::tools::read_only(name), "{name}");
    }
    for name in GRAPH_WRITE_TOOLS.into_iter().chain([
        "write_text_file",
        "write_document",
        "record_generation",
        "record_binding",
        "record_verdict",
        "run_tests",
        "run_task",
        "mark_goal_done",
        "done",
        "begin_goals",
        "edit_graph",
    ]) {
        assert!(!crate::tools::read_only(name), "{name}");
    }
}

// A match late on a long line is quoted in the excerpt, not cut away.
#[test]
fn search_files_quotes_a_late_match_on_a_long_line() {
    let line = format!("{}NEEDLE{}\n", "x".repeat(5000), "y".repeat(500));
    let (_scope, mut tools, _) = memory_project(&[("min.js", line.as_bytes())]);
    let v = tools
        .dispatch("search_files", &json!({"pattern": "NEEDLE"}))
        .unwrap();
    let text = v["matches"][0]["text"].as_str().unwrap();
    assert!(text.contains("NEEDLE"), "{text}");
    assert!(text.starts_with('…'), "{text}");
    assert_eq!(v["matches"][0]["column"], 5001);
}
