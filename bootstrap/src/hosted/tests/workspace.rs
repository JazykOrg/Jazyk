// Snapshots, projections, routes, and builds over a hosted workspace.
use super::*;

// A project carrying only a root jazyk.toml from before document sets has no set:
// its files stay browsable, a document read and a job each answer 409
// no-document-set, and nothing fails the request.
// Mirrors docs/compiler/document-sets.md#discovery-and-opening.
#[test]
fn a_project_without_document_sets_answers_no_document_set_instead_of_failing() {
    let mut files = Snapshot::new();
    files.insert(
        "jazyk.toml".into(),
        b"[docs]\nglob = [\"docs/**/*.md\"]\n\n[roots]\nfiles = [\"docs/main.md\"]\n".to_vec(),
    );
    files.insert(
        "docs/main.md".into(),
        b"# Main\n\nThe system shall respond.\n".to_vec(),
    );
    let mut w = Workspace::new(files).unwrap();
    let e = Execution::default();
    let doc = w
        .request_test("GET", "/api/docs/content?path=docs/main.md", "", &e)
        .unwrap();
    assert_eq!(doc.status, 409, "{}", String::from_utf8_lossy(&doc.body));
    assert!(String::from_utf8_lossy(&doc.body).contains("no-document-set"));
    let job = w
        .request_test("POST", "/api/jobs", r#"{"kind":"audit"}"#, &e)
        .unwrap();
    assert_eq!(job.status, 409, "{}", String::from_utf8_lossy(&job.body));
    assert!(String::from_utf8_lossy(&job.body).contains("Add one under Document sets"));
    let text = w
        .request_test("GET", "/api/project/text?path=docs/main.md", "", &e)
        .unwrap();
    assert_eq!(text.status, 200);
    assert!(String::from_utf8_lossy(&text.body).contains("The system shall respond."));
    // The old paths come back as the first set's suggestion, and nothing is written.
    let sets = w.request_test("GET", "/api/sets", "", &e).unwrap();
    let v: Value = serde_json::from_slice(&sets.body).unwrap();
    assert_eq!(v["suggested"]["docs"], json!(["docs/**/*.md"]), "{}", v);
    assert_eq!(v["suggested"]["roots"], json!(["docs/main.md"]), "{}", v);
    assert!(!w.snapshot().contains_key(".jazyk/jazyk.toml"));
}

#[test]
fn hosted_ledger_runner_sends_the_shared_batch_contract() {
    use crate::runner::SessionRunner;
    let w = workspace("Alpha");
    let _fs = crate::vfs::Scope::enter(&w.snapshot());
    let _state = crate::scoped_state::Scope::enter();
    let observed = Arc::new(AtomicBool::new(false));
    let saw = observed.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancelled = cancel.clone();
    let e = Execution {
        model: "ledger-fixture".into(),
        cancel,
        transport_async: Some(Arc::new(move |body| {
            let saw = saw.clone();
            let cancelled = cancelled.clone();
            Box::pin(async move {
                let request: Value = serde_json::from_str(&body).unwrap();
                let user = request["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|m| m["role"] == "user")
                    .map(|m| m["content"].as_str().unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(user.contains("g:generate:ent:alpha"));
                assert!(user.contains("begin_generation"));
                assert!(user.contains("record_generation"));
                saw.store(true, Ordering::SeqCst);
                cancelled.store(true, Ordering::SeqCst);
                Ok(model_reply("Stopped after inspecting the prompt."))
            })
        })),
        ..Default::default()
    };
    let _execution = ExecutionScope::enter(e);
    let p = crate::project::Project::load(&root());
    let runner = crate::inprocess::InProcessRunner::start(&p, &llm(), &p.out).unwrap();
    runner.run_item(
        &crate::runner::BatchRun::single(crate::model::Goal {
            id: "g:generate:ent:alpha".into(),
            kind: "generate".into(),
            target: "ent:alpha".into(),
            ..Default::default()
        }),
        &trace(),
    );
    assert!(observed.load(Ordering::SeqCst));
}

#[test]
fn projection_preserves_authored_membership_and_records_only_ledger_output() {
    let mut w = workspace("Alpha");
    for (path, text) in [

        (
            ".jazyk/jazyk.toml",
            "version = 1\ngen = {}\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"spec/docs/**/*.md\"]\nroots = []\nsource = [\"spec/product/**\"]\n",
        ),
        ("spec/docs/main.md", "# Main\nAuthored prose.\n"),
        ("spec/docs/new.md", "# New\nNewly authored prose.\n"),
        ("notes.txt", "An existing human file, edited."),
        (".github/workflows/test.yml", "name: tests"),
        (".git/config", "private git metadata"),
        ("spec/.jazyk.bak/status.yaml", "private backup"),
        ("spec/product/main.py", "print('accepted')"),
        ("spec/product/package.json", "{}"),
        ("spec/product/test.py", "assert True"),
        ("spec/product/test_extra.py", "assert 1 == 1"),
        ("spec/product/draft.py", "unrecorded output"),
        ("spec/product/build.zip", "built output"),
        (".jazyk/ds_main/gen/judge.md", "LLM judgment criteria"),
        (".jazyk/.gitignore", crate::document_sets::IGNORE),
        (".jazyk/ds_main/journal/g1.yaml", "build: g1\n"),
        (".jazyk/ds_main/diagrams/class/alpha.svg", "<svg/>"),
        (".jazyk/ds_main/graph/relationships/rel~a%7Eb.yaml", "members: []\n"),
        (".jazyk/ds_main/trace/run.jsonl", "{}"),
        (".jazyk/ds_main/control.yaml", "compile: auto\n"),
    ] {
        w.files.insert(path.into(), text.as_bytes().to_vec());
    }
    w.files
        .insert("spec/product/logo.jpg".into(), vec![0xff, 0xd8, 0xff]);
    {
        let fs = crate::vfs::Scope::enter(&w.files);
        let project = crate::project::Project::load(&root());
        let mut store = crate::store::Store::load(&project.out);
        store.status.generation = 42;
        store.graph.criteria.insert(
            "crit:stable".into(),
            crate::model::Criterion {
                requirement: "req:stable".into(),
                statement: "One returns one.".into(),
                ..Default::default()
            },
        );
        store.save_for_test();
        let programmatic = |artifact| json!({"kind":"programmatic", "label":"test", "artifact":artifact, "name":"test", "run":"python test.py"});
        let ledger: crate::gen::Ledger = serde_json::from_value(json!({
            "support":["spec/product/package.json"],
            "entities":{"alpha":{"factHash":"recorded", "requirements":[], "files":["spec/product/main.py","spec/product/logo.jpg","spec/product/build.zip","spec/product/missing.py"]}},
            "build":{"run":"python build.py","produces":["spec/product/build.zip"]},
            "requirements":{"req:stable":{
                "entity":"ent:alpha","files":["spec/product/main.py"],
                "test": programmatic("spec/product/test.py"),
                "tests": {
                    "crit:extra":{"test":programmatic("spec/product/test_extra.py"),"criterion_hash":"hash"},
                    "crit:llm":{"test":{"kind":"llm","label":"judge","artifact":"judge.md","name":"judge","run":""},"criterion_hash":"hash"}
                },
                "hashes":{"requirement":"","test":"","files":""}
            }}
        })).unwrap();
        ledger.save(&project.out).unwrap();
        w.files = fs.snapshot();
    }
    let before = w.snapshot();
    let authored = [
        "notes.txt",
        "deleted.txt",
        ".github/workflows/test.yml",
        ".git/config",
        "spec/.jazyk/status.yaml",
        "spec/.jazyk.bak/status.yaml",
        "spec/product/main.py",
        "spec/product/build.zip",
    ]
    .map(String::from);
    let projection = w.projection(&authored).unwrap();
    assert_eq!(projection.graph_generation, 42);
    assert_eq!(
        projection.criteria["crit:ds_main::stable"]["statement"],
        "One returns one."
    );
    assert_eq!(
        projection
            .source
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            ".github/workflows/test.yml",
            ".jazyk/jazyk.toml",
            "notes.txt",
            "spec/docs/main.md",
            "spec/docs/new.md",
        ]
    );
    for path in ["main.py", "package.json", "test.py", "test_extra.py"] {
        assert!(projection
            .files
            .contains_key(&format!("spec/product/{path}")));
    }
    // The set's committed compiler state rides with the delivery; local state does
    // not (docs/compiler/hosted-runtime.md#source-and-delivery-projections).
    let state: Vec<&str> = projection
        .files
        .keys()
        .map(String::as_str)
        .filter(|p| p.starts_with(".jazyk/") && *p != ".jazyk/jazyk.toml")
        .collect();
    assert_eq!(
        state,
        vec![
            ".jazyk/.gitignore",
            ".jazyk/ds_main/gen/entities/alpha.yaml",
            ".jazyk/ds_main/gen/judge.md",
            ".jazyk/ds_main/gen/ledger.yaml",
            ".jazyk/ds_main/gen/requirements/req~stable.yaml",
            ".jazyk/ds_main/graph/criteria/crit~stable.yaml",
            ".jazyk/ds_main/status.yaml",
        ]
    );
    assert_eq!(
        projection.files.len(),
        projection.source.len() + 4 + state.len()
    );
    assert!(!projection
        .source
        .keys()
        .any(|p| p.starts_with(".jazyk/ds_main/")));
    assert_eq!(
        projection.source["notes.txt"],
        "An existing human file, edited."
    );
    assert!(!projection.files.contains_key("spec/product/logo.jpg"));
    assert_eq!(w.snapshot(), before);
    assert!(!crate::vfs::is_scoped());
    let other = workspace("Beta").projection(&[]).unwrap();
    assert_eq!(other.graph_generation, 0);
    assert!(other.criteria.is_empty());
}

#[test]
fn projection_keeps_binary_in_snapshot_and_rejects_invalid_membership() {
    let mut w = workspace("Alpha");
    w.files.insert("photo.png".into(), vec![0, 255]);
    let before = w.snapshot();
    let projection = w.projection(&["photo.png".into()]).unwrap();
    assert!(!projection.source.contains_key("photo.png"));
    assert!(!projection.files.contains_key("photo.png"));
    for path in ["../escape", "/absolute", "docs//main.md", "a\\b", "a:b"] {
        assert!(w
            .projection(&[path.into()])
            .unwrap_err()
            .contains("invalid authored"));
    }
    assert!(!w.projection(&[]).unwrap().files.contains_key("photo.png"));
    assert_eq!(w.snapshot(), before);
    assert!(!crate::vfs::is_scoped());
}

#[test]
fn concurrent_real_builds_keep_projects_models_and_noop_isolated() {
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = ["Alpha", "Beta"]
        .into_iter()
        .map(|name| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut w = workspace(name);
                let calls = Arc::new(AtomicUsize::new(0));
                let e = navigation_model(name, calls.clone(), Some(barrier));
                let first = value(&mut w, "POST", "/api/jobs", json!({"kind":"compile"}), &e);
                assert_eq!(
                    first["report"]["failed"].as_u64().unwrap_or(0),
                    0,
                    "{first}"
                );
                let before = calls.load(Ordering::SeqCst);
                assert!(before > 0);
                let graph = value(&mut w, "GET", "/api/graph", Value::Null, &e);
                assert!(graph["generation"].as_u64().unwrap() > 0, "{first}");
                let next = value(&mut w, "POST", "/api/compile", json!({}), &e);
                assert_eq!(
                    calls.load(Ordering::SeqCst),
                    before,
                    "no-op called model: {next}"
                );
                let restored = w.snapshot();
                assert!(String::from_utf8_lossy(&restored["docs/main.md"]).contains(name));
                assert!(!restored.keys().any(|k| k.starts_with('/')));
                assert!(!crate::vfs::is_scoped());
                restored
            })
        })
        .collect();
    let trees: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_ne!(trees[0]["docs/main.md"], trees[1]["docs/main.md"]);
}

#[test]
fn hosted_files_shell_config_and_binary_routes_are_confined() {
    let mut w = workspace("Alpha");
    w.files.insert("picture.png".into(), vec![0, 255, 13, 10]);
    let before = w.snapshot();
    {
        let scope = crate::vfs::Scope::enter(&before);
        for path in [
            "/etc/hosts",
            "/project/../../etc/hosts",
            "../outside",
            "/project-other/a",
        ] {
            assert_eq!(
                crate::vfs::read(path).unwrap_err().kind(),
                std::io::ErrorKind::PermissionDenied
            );
            assert!(crate::vfs::write(path, b"forbidden").is_err());
        }
        assert!(crate::sys::run_shell("exit 0", &root()).is_err());
        assert!(crate::sys::env_var("HOME").is_err());
        assert!(crate::project::load_global_llm().model.is_none());
        assert_eq!(scope.snapshot(), before);
    }
    let reply = w
        .request_test(
            "GET",
            "/api/project/file?path=picture.png",
            "",
            &Execution::default(),
        )
        .unwrap();
    assert_eq!(reply.status, 200);
    assert_eq!(reply.content_type, "image/png");
    assert_eq!(reply.body, vec![0, 255, 13, 10]);
    for path in ["../a", "/abs", "a//b", "a/./b", "a\\b", "a\0b"] {
        assert!(Workspace::new(BTreeMap::from([(path.into(), vec![])])).is_err());
    }
}

#[test]
fn incomplete_model_writes_cannot_bypass_commit_gates() {
    let mut w = workspace("Alpha");
    w.files.insert(
        "docs/main.md".into(),
        b"# Alpha\n\nAlpha stores records.\n".to_vec(),
    );
    let errors = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = errors.clone();
    let e = Execution {
        model: "bad-fixture".into(),
        transport: Some(Arc::new(move |body| {
            let request: Value = serde_json::from_str(body).unwrap();
            let tool_messages: Vec<_> = request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|m| m["role"] == "tool")
                .collect();
            if !tool_messages.is_empty() {
                seen.lock()
                    .unwrap()
                    .extend(tool_messages.iter().map(|m| m.to_string()));
                return Ok(model_reply(
                    "Finished without repairing the missing coverage.",
                ));
            }
            let actions = [
                (
                    "upsert_entity",
                    json!({"name":"Alpha","mention":{"section":"docs/main.md#/alpha","quote":"Alpha stores records."}}),
                ),
                (
                    "mark_goal_done",
                    json!({"goal":"g:reconcile-section:docs/main.md#/alpha","justification":"This is supposedly complete, without extracting its required statement."}),
                ),
                (
                    "done",
                    json!({"summary":"Skip all extraction and coverage checks."}),
                ),
            ];
            Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":actions.iter().enumerate().map(|(i,(name,args))|json!({"id":format!("bad-{i}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()}}]}).to_string())
        })),
        ..Default::default()
    };
    let result = value(&mut w, "POST", "/api/compile", json!({}), &e);
    assert!(
        result["results"][0]["body"]["report"]["failed"]
            .as_u64()
            .unwrap()
            + result["results"][0]["body"]["report"]["parked"]
                .as_u64()
                .unwrap()
            > 0,
        "{result}"
    );
    let graph = value(&mut w, "GET", "/api/graph", Value::Null, &e);
    assert!(
        graph["entities"].as_object().unwrap().is_empty(),
        "ungated staged entity leaked: {graph}"
    );
    assert!(errors
        .lock()
        .unwrap()
        .iter()
        .any(|e| e.contains("coverage")));
    assert!(
        w.snapshot().contains_key(".jazyk/ds_main/status.yaml"),
        "valid failure state should remain persistable"
    );
}

#[test]
fn cancelled_execution_never_calls_model_or_shell_and_svg_is_self_contained() {
    let mut w = workspace("Alpha");
    let e = Execution {
        transport: Some(Arc::new(|_| panic!("cancelled model called"))),
        cancel: Arc::new(AtomicBool::new(true)),
        ..Default::default()
    };
    value(&mut w, "POST", "/api/chat/sessions", json!({}), &e);
    value(
        &mut w,
        "POST",
        "/api/chat/sessions/chat-1/prompt",
        json!({"text":"cancelled"}),
        &e,
    );
    let chat = value(&mut w, "GET", "/api/chat/sessions/chat-1", Value::Null, &e);
    assert!(chat.to_string().contains("cancelled"));
    let _scope = crate::vfs::Scope::enter(&w.snapshot());
    assert!(crate::render::render_svg("@startuml\nclass Alpha\n@enduml")
        .unwrap()
        .contains("Alpha"));
    assert!(
        crate::render::render_svg("@startuml\n!include /etc/hosts\nclass Alpha\n@enduml").is_err()
    );
    assert!(crate::render::render_svg("@startuml\nclass \"<img:/etc/hosts>\"\n@enduml").is_err());
    assert!(crate::render::render_png("<svg />").is_err());
}

// A hosted workspace keeps no replay snapshots: a batch in the scope copies
// nothing, and a request drops a directory an earlier version left behind.
// Mirrors docs/compiler/hosted-runtime.md#workspace-snapshots.
#[test]
fn hosted_requests_write_no_graph_snapshots_and_drop_old_ones() {
    let mut w = workspace("Alpha");
    w.files.insert(
        ".jazyk/ds_main/graph/entities/ent~a.yaml".into(),
        b"name: A\n".to_vec(),
    );
    w.files.insert(
        ".jazyk/ds_main/status.yaml".into(),
        format!("version: {}\n", crate::store::STORE_VERSION).into_bytes(),
    );
    {
        let fs = crate::vfs::Scope::enter(&w.files);
        crate::store::snapshot_graph(&root().join(".jazyk/ds_main"), "b:one");
        assert!(!fs.snapshot().keys().any(|p| p.contains("/snapshots/")));
    }
    w.files.insert(
        ".jazyk/ds_main/snapshots/b_old/graph/entities/ent~a.yaml".into(),
        b"name: A\n".to_vec(),
    );
    w.files.insert(
        ".jazyk/ds_main/snapshots/b_old/status.yaml".into(),
        format!("version: {}\n", crate::store::STORE_VERSION).into_bytes(),
    );
    let reply = w
        .request_test("GET", "/api/capabilities", "", &Execution::default())
        .unwrap();
    assert_eq!(reply.status, 200);
    let files = w.snapshot();
    assert!(
        !files.keys().any(|p| p.contains("/snapshots/")),
        "{:?}",
        files.keys()
    );
    assert!(files.contains_key(".jazyk/ds_main/graph/entities/ent~a.yaml"));
    assert!(files.contains_key(".jazyk/ds_main/status.yaml"));
}

#[test]
fn real_extraction_commits_graph_identity_and_survives_snapshot_reload() {
    let mut w = workspace("Alpha");
    w.files.insert(
        "docs/main.md".into(),
        b"# Alpha\n\nAlpha stores records.\n".to_vec(),
    );
    let called = AtomicUsize::new(0);
    let previews = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let captured = previews.clone();
    let e = Execution {
        trace: Some(Arc::new(move |event| {
            captured
                .lock()
                .unwrap()
                .push(serde_json::to_value(event).unwrap())
        })),
        model: "extract-fixture".into(),
        transport: Some(Arc::new(move |_| {
            if called.fetch_add(1, Ordering::SeqCst) > 0 {
                return Ok(model_reply(
                    "The remaining judgment is deliberately left incomplete.",
                ));
            }
            let actions = [
                (
                    "upsert_entity",
                    json!({"name":"Alpha","mention":{"section":"docs/main.md#/alpha","quote":"Alpha stores records."}}),
                ),
                (
                    "upsert_entity",
                    json!({"name":"Records","mention":{"section":"docs/main.md#/alpha","quote":"Alpha stores records."}}),
                ),
                (
                    "upsert_requirement",
                    json!({"statement":"Alpha stores records.","facets":[{"facet":"behavior","reasoning":"Alpha performs the storage action."}],"entities":["ent:alpha","ent:records"],"edges":[{"a":"ent:alpha","b":"ent:records","type":"association"}],"section":"docs/main.md#/alpha","quote":"Alpha stores records."}),
                ),
                (
                    "set_coverage",
                    json!({"section":"docs/main.md#/alpha","state":"covered","note":"Alpha stores records is recorded as a requirement."}),
                ),
                (
                    "mark_goal_done",
                    json!({"goal":"g:reconcile-section:docs/main.md#/alpha","justification":"The statement Alpha stores records is extracted with its entity and verbatim source."}),
                ),
                (
                    "done",
                    json!({"summary":"Extracted Alpha stores records with quote provenance."}),
                ),
            ];
            Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":actions.iter().enumerate().map(|(i,(name,args))|json!({"id":format!("extract-{i}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()}}]}).to_string())
        })),
        ..Default::default()
    };
    let result = value(&mut w, "POST", "/api/compile", json!({}), &e);
    let events = previews.lock().unwrap();
    let first_preview = events
        .iter()
        .position(|v| {
            v["kind"] == "graphPreview"
                && v["phase"] == "staged"
                && v["graph"]["entities"]["ent:alpha"]["name"] == "Alpha"
        })
        .expect("accepted entity is streamed before commit");
    let finished = events
        .iter()
        .position(|v| v["kind"] == "sessionDone")
        .unwrap();
    assert!(first_preview < finished);
    assert!(events
        .iter()
        .any(|v| v["kind"] == "toolResult" && v["activity"] == "Alpha created"));
    drop(events);
    let graph = value(&mut w, "GET", "/api/graph", Value::Null, &e);
    assert_eq!(
        graph["entities"]["ent:ds_main::alpha"]["name"], "Alpha",
        "{result}: {graph}"
    );
    assert_eq!(graph["requirements"].as_object().unwrap().len(), 1);
    let snapshot = w.snapshot();
    assert!(!snapshot
        .keys()
        .any(|p| p.starts_with(".jazyk/ds_main/preview/")));
    assert!(
        !snapshot.keys().any(|p| p.contains("/snapshots/")),
        "a hosted batch writes no graph snapshot"
    );
    assert!(snapshot
        .keys()
        .any(|p| p.starts_with(".jazyk/ds_main/journal/")));
    assert!(
        snapshot.keys().any(|p| p.ends_with(".svg")),
        "hosted compilation should retain diagram output"
    );
    let mut restored = Workspace::new(snapshot).unwrap();
    assert_eq!(
        value(
            &mut restored,
            "GET",
            "/api/graph",
            Value::Null,
            &Execution::default()
        ),
        graph
    );
    let mut other = workspace("Beta");
    assert!(value(
        &mut other,
        "GET",
        "/api/graph",
        Value::Null,
        &Execution::default()
    )["entities"]
        .as_object()
        .unwrap()
        .is_empty());
}

#[test]
fn hosted_image_upload_survives_projection_and_workspace_reload() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    for selected in [false, true] {
        let mut w = workspace("Alpha");
        if selected {
            let config = String::from_utf8(w.files[".jazyk/jazyk.toml"].clone()).unwrap();
            w.files.insert(
                ".jazyk/jazyk.toml".into(),
                config.replace("docs/**/*.md", "docs/**/*").into_bytes(),
            );
        }
        let path = "docs/setup/jazyk-web-gui.jpg";
        let mut bytes = vec![0xff; 797_000];
        bytes[..3].copy_from_slice(&[0xff, 0xd8, 0xff]);
        value(
            &mut w,
            "PUT",
            &format!("/api/project/file?path={path}"),
            json!({"base64": STANDARD.encode(&bytes)}),
            &Execution::default(),
        );
        let before = w.snapshot();
        let projection = w.projection(&[path.into()]).unwrap();
        assert!(!projection.source.contains_key(path));
        assert!(!projection.files.contains_key(path));
        assert_eq!(w.snapshot(), before);
        let mut restored = Workspace::new(before).unwrap();
        let reply = restored
            .request_test(
                "GET",
                &format!("/api/project/file?path={path}"),
                "",
                &Execution::default(),
            )
            .unwrap();
        assert_eq!(reply.status, 200);
        assert_eq!(reply.body, bytes);
    }
}
