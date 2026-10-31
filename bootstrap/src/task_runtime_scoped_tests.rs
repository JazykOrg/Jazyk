use super::*;
use crate::hosted::{Execution, RuntimeTransport, Snapshot};
use std::sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, Arc, Mutex};

const LOGO: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><path fill=\"green\"/></svg>\r\n";
const CONFIG: &str = "version=1\n[[sets]]\nid='ds_main'\nname='Site'\ndocs=['docs/**']\nsource=['deliverable/**']\n[[tasks]]\nid='copy'\nname='Copy supplied logo'\ndescription='Retain the supplied asset'\ncommand='cp logo.svg deliverable/logo.svg'\nproducts=['deliverable/logo.svg']\n[[tasks]]\nid='check'\nname='Check site'\ndescription='Check the completed site'\ncommand='test -f deliverable/logo.svg'\n";

struct Fixture {
    files: Snapshot,
    runtime: RuntimeTransport,
    ready: Arc<AtomicBool>,
    reads: Arc<AtomicUsize>,
    submitted: Arc<Mutex<BTreeMap<String, Value>>>,
    override_result: Arc<Mutex<Value>>,
}

impl Fixture {
    fn new() -> Self {
        let files = BTreeMap::from([
            (".jazyk/jazyk.toml".into(), CONFIG.as_bytes().to_vec()),
            ("docs/main.md".into(), b"# Site\n\nUse the supplied logo.\n".to_vec()),
            ("logo.svg".into(), LOGO.to_vec()),
            ("deliverable/index.html".into(), b"<img src='logo.svg'>\n".to_vec()),
        ]);
        let ready = Arc::new(AtomicBool::new(false));
        let reads = Arc::new(AtomicUsize::new(0));
        let submitted = Arc::new(Mutex::new(BTreeMap::<String, Value>::new()));
        let override_result = Arc::new(Mutex::new(Value::Null));
        let (ready_runtime, reads_runtime, submissions, overrides) =
            (ready.clone(), reads.clone(), submitted.clone(), override_result.clone());
        let runtime = Arc::new(move |request: &Value| {
            let id = request["run_id"].as_str().unwrap_or_default();
            match request["operation"].as_str().unwrap() {
                "task_context" => Ok(json!({"available":true})),
                "run_task" => {
                    if request["task"]["id"] == "check" {
                        assert_eq!(STANDARD.decode(request["files"]["deliverable/logo.svg"].as_str().expect("next task sees imported logo")).unwrap(), LOGO);
                    }
                    let id = request["operation_key"].as_str().unwrap();
                    let mut submissions = submissions.lock().unwrap();
                    if let Some(prior) = submissions.get(id) { assert_eq!(prior, request); }
                    submissions.insert(id.into(), request.clone());
                    Ok(json!({"id":id,"terminated":ready_runtime.load(Ordering::SeqCst),"task":request["task"]}))
                }
                "wait_run" | "inspect_run" => {
                    let overrides = overrides.lock().unwrap();
                    let state = overrides["run_state"].as_str().or_else(|| overrides["status"].as_str()).unwrap_or("succeeded");
                    let result = json!({"status":state,"complete":overrides["complete"].as_bool().unwrap_or(true),"import_pending":true});
                    Ok(json!({"id":id,"terminated":ready_runtime.load(Ordering::SeqCst),
                        "state":state,"summary":result,"result":result}))
                }
                "result" => {
                    assert!(ready_runtime.load(Ordering::SeqCst), "never fetch an unfinished result");
                    reads_runtime.fetch_add(1, Ordering::SeqCst);
                    let submission = submissions.lock().unwrap()[id].clone();
                    let copy = submission["task"]["id"] == "copy";
                    let mut result = json!({"status":"succeeded","complete":true,"exit_code":0,
                        "log":"isolated task finished", "files":{},"reports":{},"cache":{},"deleted":[],"products":[],
                        "input_files":submission["files"]});
                    if copy {
                        result["files"]["deliverable/logo.svg"] = json!(STANDARD.encode(LOGO));
                        result["products"] = json!(["deliverable/logo.svg"]);
                    }
                    if let Some(fields) = overrides.lock().unwrap().as_object() {
                        for (key, value) in fields { result[key] = value.clone(); }
                    }
                    Ok(result)
                }
                operation => panic!("Unexpected operation {operation}"),
            }
        });
        Self { files, runtime, ready, reads, submitted, override_result }
    }
    fn execution(&self) -> Execution { Execution { runtime:Some(self.runtime.clone()), ..Default::default() } }
    fn submit(&self, id: &str, key: &str) -> Value {
        let root = Path::new("/project");
        let context = call(root, "task_context", &json!({})).unwrap();
        call(root, "run_task", &json!({"task_id":id,"expected_revision":context["revision"],"operation_key":key})).unwrap()
    }
}

#[test]
fn terminal_tools_publish_exact_products_inside_the_current_build_lease_once() {
    let fixture = Fixture::new();
    let fs = crate::vfs::Scope::enter(&fixture.files);
    let _state = crate::scoped_state::Scope::enter();
    let _execution = crate::hosted::test_execution_scope(fixture.execution());
    let _recovery = crate::session_recovery::Scope::enter(None);
    let root = Path::new("/project");
    let project = crate::document_sets::Workspace::load(root).unwrap().resolve(Some("ds_main")).unwrap();
    let lease = crate::control::begin_internal_build(&project, &project.out, "generate").unwrap();
    let host_file_before = std::fs::read(root.join("deliverable/logo.svg")).ok();
    fixture.submit("copy", "copy-run");
    assert!(!crate::vfs::exists(&root.join("deliverable/logo.svg")));
    call(root, "wait_run", &json!({"run_id":"copy-run"})).unwrap();
    assert_eq!(fixture.reads.load(Ordering::SeqCst), 0);
    fixture.ready.store(true, Ordering::SeqCst);
    let terminal = call(root, "wait_run", &json!({"run_id":"copy-run"})).unwrap();
    assert_eq!(terminal["result"]["import_pending"], false);
    assert_eq!(terminal["result"]["import"]["products"], json!(["deliverable/logo.svg"]));
    assert_eq!(crate::vfs::read(root.join("deliverable/logo.svg")).unwrap(), LOGO);
    assert_eq!(crate::vfs::read(root.join("logo.svg")).unwrap(), LOGO);
    assert!(crate::control::build_lease(&project.out).is_some_and(|held| held.worker == lease.token()));
    assert_eq!(std::fs::read(root.join("deliverable/logo.svg")).ok(), host_file_before);
    let status = terminal.to_string();
    assert!(!status.contains(&STANDARD.encode(LOGO)));
    assert!(worker_response("wait_run", terminal.clone()).get("summary").is_none());
    fixture.submit("check", "check-run");
    // Later product edits survive repeated status polls and restored snapshots.
    crate::vfs::write(root.join("deliverable/logo.svg"), b"newer product edit").unwrap();
    let saved = fs.snapshot();
    let _restored = crate::vfs::Scope::enter(&saved);
    let repeated = call(root, "inspect_run", &json!({"run_id":"copy-run"})).unwrap();
    assert_eq!(repeated["result"]["import"], terminal["result"]["import"]);
    assert_eq!(crate::vfs::read(root.join("deliverable/logo.svg")).unwrap(), b"newer product edit");
    assert_eq!(fixture.reads.load(Ordering::SeqCst), 2, "one result per owned run, no reread on polls");
    assert_eq!(fixture.submitted.lock().unwrap().len(), 2);
}

#[test]
fn newer_inputs_and_unowned_history_cannot_be_imported_by_a_worker() {
    let fixture = Fixture::new();
    let _fs = crate::vfs::Scope::enter(&fixture.files);
    let _execution = crate::hosted::test_execution_scope(fixture.execution());
    let _recovery = crate::session_recovery::Scope::enter(None);
    let root = Path::new("/project");
    fixture.submit("copy", "stale-run");
    fixture.ready.store(true, Ordering::SeqCst);
    crate::vfs::write(root.join("logo.svg"), b"newer input asset").unwrap();
    let status = call(root, "inspect_run", &json!({"run_id":"stale-run"})).unwrap();
    assert_eq!(status["result"]["import"]["applied"], false);
    assert!(!crate::vfs::exists(&root.join("deliverable/logo.svg")));
    call(root, "inspect_run", &json!({"run_id":"unowned-history"})).unwrap();
    assert_eq!(fixture.reads.load(Ordering::SeqCst), 1, "historical reads have no submission authority");
    {
        let _other_worker = crate::session_recovery::Scope::enter(None);
        call(root, "inspect_run", &json!({"run_id":"stale-run"})).unwrap();
        assert_eq!(fixture.reads.load(Ordering::SeqCst), 1, "another worker cannot import a prior worker's submission");
    }
    crate::vfs::write(root.join("logo.svg"), LOGO).unwrap();
    call(root, "wait_run", &json!({"run_id":"stale-run"})).unwrap();
    assert!(!crate::vfs::exists(&root.join("deliverable/logo.svg")), "stale outcomes stay stale on later polls");
}

#[test]
fn submission_and_import_receipts_resume_with_the_execution_checkpoint() {
    let fixture = Fixture::new();
    let saved = Arc::new(Mutex::new(None::<(Value, Snapshot)>));
    let checkpoint = saved.clone();
    let sink: crate::hosted::CheckpointSink = Arc::new(move |mut capsule, files| {
        let checkpoint = checkpoint.clone();
        Box::pin(async move {
            let next = capsule["sequence"].as_u64().unwrap() + 1;
            capsule["sequence"] = json!(next);
            *checkpoint.lock().unwrap() = Some((capsule, files));
            Ok(next)
        })
    });
    {
        let _fs = crate::vfs::Scope::enter(&fixture.files);
        let _state = crate::scoped_state::Scope::enter();
        let _execution = crate::hosted::test_execution_scope(Execution { checkpoint:Some(sink.clone()), ..fixture.execution() });
        let _recovery = crate::session_recovery::Scope::enter(None);
        fixture.submit("copy", "original-run");
        crate::session_recovery::checkpoint_parent().unwrap();
    }
    let (capsule, files) = saved.lock().unwrap().clone().unwrap();
    crate::session_recovery::validate(&capsule).unwrap();
    assert_eq!(capsule["parent"]["task-submission:/project:original-run"]["task"]["id"], "copy");
    assert!(!files.keys().any(|path| path.contains("submissions")), "receipts belong to this execution, not project files");
    assert!(!files.contains_key("deliverable/logo.svg"));
    fixture.ready.store(true, Ordering::SeqCst);
    {
        let _fs = crate::vfs::Scope::enter(&files);
        let _state = crate::scoped_state::Scope::enter();
        let _execution = crate::hosted::test_execution_scope(Execution { resume:Some(capsule.clone()), checkpoint:Some(sink), ..fixture.execution() });
        let _recovery = crate::session_recovery::Scope::enter(Some(&capsule));
        call(Path::new("/project"), "wait_run", &json!({"run_id":"original-run"})).unwrap();
        assert_eq!(crate::vfs::read("/project/deliverable/logo.svg").unwrap(), LOGO);
        crate::vfs::write("/project/deliverable/logo.svg", b"later edit").unwrap();
        crate::session_recovery::checkpoint_parent().unwrap();
    }
    let (capsule, files) = saved.lock().unwrap().clone().unwrap();
    let _fs = crate::vfs::Scope::enter(&files);
    let _execution = crate::hosted::test_execution_scope(Execution { resume:Some(capsule.clone()), ..fixture.execution() });
    let _recovery = crate::session_recovery::Scope::enter(Some(&capsule));
    call(Path::new("/project"), "inspect_run", &json!({"run_id":"original-run"})).unwrap();
    assert_eq!(crate::vfs::read("/project/deliverable/logo.svg").unwrap(), b"later edit");
    assert_eq!(fixture.reads.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.submitted.lock().unwrap().len(), 1);
}

#[test]
fn waiting_run_task_polls_in_code_and_interrupts_with_ordinary_status_replies() {
    let fixture = Fixture::new();
    let waits = Arc::new(AtomicUsize::new(0));
    let cancels = Arc::new(AtomicUsize::new(0));
    // 1: the poll is suspended mid-call, 2: cancelled mid-call, 3: the transport fails.
    let interrupt = Arc::new(AtomicUsize::new(0));
    let (suspend, cancel) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let (inner, ready, counted, cancelled) = (fixture.runtime.clone(), fixture.ready.clone(), waits.clone(), cancels.clone());
    let (mode, suspending, cancelling) = (interrupt.clone(), suspend.clone(), cancel.clone());
    let runtime: RuntimeTransport = Arc::new(move |request: &Value| match request["operation"].as_str().unwrap() {
        "wait_run" => match mode.swap(0, Ordering::SeqCst) {
            1 => { suspending.store(true, Ordering::SeqCst); Err("execution suspended".into()) }
            2 => { cancelling.store(true, Ordering::SeqCst); Err("execution cancelled".into()) }
            3 => Err("connection reset".into()),
            _ => {
                if counted.fetch_add(1, Ordering::SeqCst) == 2 { ready.store(true, Ordering::SeqCst); }
                inner(request)
            }
        },
        "cancel_run" => {
            cancelled.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"id":request["run_id"],"terminated":false,"state":"cancelling"}))
        }
        _ => inner(request),
    });
    let execution = Execution { runtime:Some(runtime), suspend:suspend.clone(), cancel:cancel.clone(), ..Default::default() };
    let _fs = crate::vfs::Scope::enter(&fixture.files);
    let _state = crate::scoped_state::Scope::enter();
    let _execution = crate::hosted::test_execution_scope(execution);
    let _recovery = crate::session_recovery::Scope::enter(None);
    let root = Path::new("/project");
    let revision = call(root, "task_context", &json!({})).unwrap()["revision"].clone();
    let start = json!({"task_id":"copy","expected_revision":revision,"operation_key":"copy-run","wait":true});

    // A suspension request returns the current status, so the call leaves a receipt.
    suspend.store(true, Ordering::SeqCst);
    let pending = call(root, "run_task", &start).unwrap();
    assert_eq!(pending["terminated"], false);
    assert_eq!(waits.load(Ordering::SeqCst), 0);
    suspend.store(false, Ordering::SeqCst);

    // A hosted poll suspended mid-call or a failed transport also returns status.
    for (case, message) in [(1, "execution suspended"), (3, "connection reset")] {
        interrupt.store(case, Ordering::SeqCst);
        let pending = call(root, "run_task", &start).unwrap();
        assert_eq!(pending["id"], "copy-run");
        assert_eq!(pending["terminated"], false);
        assert_eq!(pending["wait_error"], message);
        suspend.store(false, Ordering::SeqCst);
    }
    assert_eq!(waits.load(Ordering::SeqCst), 0);

    // Replaying the same key waits for the same run, imports once, and returns terminal.
    let terminal = call(root, "run_task", &start).unwrap();
    assert_eq!(terminal["terminated"], true);
    assert_eq!(terminal["result"]["import"]["products"], json!(["deliverable/logo.svg"]));
    assert_eq!(crate::vfs::read(root.join("deliverable/logo.svg")).unwrap(), LOGO);
    assert_eq!(waits.load(Ordering::SeqCst), 3);
    assert_eq!(fixture.reads.load(Ordering::SeqCst), 1);
    let submitted = fixture.submitted.lock().unwrap();
    assert_eq!(submitted.len(), 1);
    assert!(submitted["copy-run"].get("wait").is_none(), "waiting is never part of the submitted request");
    drop(submitted);

    // Cancellation of the owning operation stops the run and returns its status.
    fixture.ready.store(false, Ordering::SeqCst);
    cancel.store(true, Ordering::SeqCst);
    let start = json!({"task_id":"check","expected_revision":revision,"operation_key":"check-run","wait":true});
    let stopping = call(root, "run_task", &start).unwrap();
    assert_eq!(stopping["state"], "cancelling");
    assert_eq!(cancels.load(Ordering::SeqCst), 1);

    // Cancellation that interrupts a hosted poll still stops the run.
    cancel.store(false, Ordering::SeqCst);
    interrupt.store(2, Ordering::SeqCst);
    let start = json!({"task_id":"check","expected_revision":revision,"operation_key":"check-cancelled","wait":true});
    let stopping = call(root, "run_task", &start).unwrap();
    assert_eq!(stopping["state"], "cancelling");
    assert_eq!(cancels.load(Ordering::SeqCst), 2);

    // Without wait, run_task keeps returning immediately.
    cancel.store(false, Ordering::SeqCst);
    let before = waits.load(Ordering::SeqCst);
    let start = json!({"task_id":"check","expected_revision":revision,"operation_key":"check-later"});
    assert_eq!(call(root, "run_task", &start).unwrap()["terminated"], false);
    assert_eq!(waits.load(Ordering::SeqCst), before);
}

#[test]
fn internal_task_execution_fetches_and_imports_one_retained_result() {
    let fixture = Fixture::new();
    let _fs = crate::vfs::Scope::enter(&fixture.files);
    let _state = crate::scoped_state::Scope::enter();
    let _execution = crate::hosted::test_execution_scope(fixture.execution());
    let _recovery = crate::session_recovery::Scope::enter(None);
    fixture.ready.store(true, Ordering::SeqCst);
    let (_, result) = execute_with_key(Path::new("/project"), "copy", "generation-copy").unwrap();
    assert_eq!(result["status"], "succeeded");
    assert_eq!(fixture.reads.load(Ordering::SeqCst), 1);
    assert_eq!(crate::vfs::read("/project/deliverable/logo.svg").unwrap(), LOGO);
    execute_with_key(Path::new("/project"), "copy", "generation-copy").unwrap();
    assert_eq!(fixture.reads.load(Ordering::SeqCst), 1, "the saved internal result also replays without a fetch");
}

#[test]
fn terminal_result_validation_keeps_products_private_until_complete_and_matching() {
    for (override_result, error) in [
        (json!({"status":"failed"}), false),
        (json!({"complete":false}), false),
        (json!({"input_files":{}}), true),
        (json!({"status":"succeeded","run_state":"failed"}), true),
        (json!({"files":{".jazyk/stolen":STANDARD.encode("bad")},"products":[".jazyk/stolen"]}), true),
    ] {
        let fixture = Fixture::new();
        let _fs = crate::vfs::Scope::enter(&fixture.files);
        let _execution = crate::hosted::test_execution_scope(fixture.execution());
        let _recovery = crate::session_recovery::Scope::enter(None);
        fixture.submit("copy", "candidate-run");
        *fixture.override_result.lock().unwrap() = override_result;
        fixture.ready.store(true, Ordering::SeqCst);
        let imported = call(Path::new("/project"), "wait_run", &json!({"run_id":"candidate-run"}));
        assert_eq!(imported.is_err(), error, "{imported:?}");
        assert!(!crate::vfs::exists(Path::new("/project/deliverable/logo.svg")));
        assert!(!crate::vfs::exists(Path::new("/project/.jazyk/stolen")));
        assert_eq!(crate::vfs::read("/project/logo.svg").unwrap(), LOGO);
    }
}

#[test]
fn products_respect_documents_and_nested_projects_but_keep_existing_public_artifacts() {
    for path in ["docs/main.md", "logo.svg", "nested/result.pdf", "target/book.jar", "dist/slides.pdf", ".cache/book.pdf"] {
        let fixture = Fixture::new();
        let _fs = crate::vfs::Scope::enter(&fixture.files);
        let root = Path::new("/project");
        crate::vfs::create_dir_all(root.join("nested/.jazyk")).unwrap();
        crate::vfs::write(root.join("nested/.jazyk/jazyk.toml"), "version=1\nsets=[]\n").unwrap();
        let mut task = definitions(CONFIG).unwrap().remove(0);
        task.products = vec![path.into()];
        let config = edit(&CONFIG.replace("docs=['docs/**']", "docs=['docs/**','logo.svg']"), &[task.clone()]).unwrap();
        crate::vfs::write(root.join(".jazyk/jazyk.toml"), config).unwrap();
        if matches!(path, "target/book.jar" | "dist/slides.pdf" | ".cache/book.pdf") {
            crate::vfs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            crate::vfs::write(root.join(path), b"previous generated artifact").unwrap();
        }
        let input = snapshot(root).unwrap();
        let result = validate_result(&task, &input, json!({"status":"succeeded","complete":true,
            "log":"done","files":{path:STANDARD.encode(b"generated artifact")},"reports":{},"cache":{},
            "deleted":[],"products":[path]})).unwrap();
        let before = crate::vfs::read(root.join(path)).ok();
        let imported = apply_result(root, &task, &input, &result, "artifact-run").unwrap();
        if matches!(path, "target/book.jar" | "dist/slides.pdf" | ".cache/book.pdf") {
            assert!(input.contains_key(path), "previous product is frozen input");
            assert_eq!(imported["applied"], true);
            assert_eq!(crate::vfs::read(root.join(path)).unwrap(), b"generated artifact");
            let next_input = snapshot(root).unwrap();
            let mut next_result = result.clone();
            next_result["files"][path] = json!(STANDARD.encode("next generated artifact"));
            assert_eq!(apply_result(root, &task, &next_input, &next_result, "next-artifact-run").unwrap()["applied"], true);
            assert_eq!(crate::vfs::read(root.join(path)).unwrap(), b"next generated artifact");
        } else {
            assert_eq!(imported["applied"], false, "{path}: {imported}");
            assert_eq!(crate::vfs::read(root.join(path)).ok(), before);
        }
    }
}

#[test]
fn command_changes_to_existing_inputs_outside_products_remain_reviewable() {
    let fixture = Fixture::new();
    let _fs = crate::vfs::Scope::enter(&fixture.files);
    let _execution = crate::hosted::test_execution_scope(fixture.execution());
    let _recovery = crate::session_recovery::Scope::enter(None);
    fixture.submit("copy", "candidate-source-run");
    *fixture.override_result.lock().unwrap() = json!({"files":{
        "deliverable/logo.svg":STANDARD.encode(LOGO),
        "logo.svg":STANDARD.encode("rewritten input asset"),
        "deliverable/index.html":STANDARD.encode("rewritten source")}});
    fixture.ready.store(true, Ordering::SeqCst);
    let status = call(Path::new("/project"), "wait_run", &json!({"run_id":"candidate-source-run"})).unwrap();
    assert_eq!(status["result"]["import"]["evidence_withheld"], true);
    assert_eq!(crate::vfs::read("/project/logo.svg").unwrap(), LOGO);
    assert_eq!(crate::vfs::read("/project/deliverable/index.html").unwrap(), fixture.files["deliverable/index.html"]);
    assert_eq!(crate::vfs::read("/project/deliverable/logo.svg").unwrap(), LOGO);
}

#[test]
fn task_input_snapshots_exclude_nested_projects_and_private_state_on_both_filesystems() {
    fn seed_and_check(root: &Path) {
        for directory in [".jazyk/runtime", "container/nested/.jazyk", "container/.jazyk"] {
            crate::vfs::create_dir_all(root.join(directory)).unwrap();
        }
        for (path, text) in [
            (".jazyk/jazyk.toml", CONFIG),
            (".jazyk/.gitignore", "runtime/\n"),
            (".jazyk/runtime/private.json", "owner history"),
            ("container/nested/.jazyk/jazyk.toml", "version=1\nsets=[]\n[llm]\napi_key='other-project-secret'\n"),
            ("container/nested/public.txt", "another project's source"),
            ("container/.jazyk/private.yaml", "nested compiler state"),
            ("container/allowed.txt", "this project's input"),
        ] { crate::vfs::write(root.join(path), text).unwrap(); }
        let input = snapshot(root).unwrap();
        assert!(input.contains_key(".jazyk/jazyk.toml"));
        assert!(input.contains_key(".jazyk/.gitignore"));
        assert!(input.contains_key("container/allowed.txt"));
        for prefix in [".jazyk/runtime/", "container/nested/", "container/.jazyk/", "alias/", "private-alias/"] {
            assert!(!input.keys().any(|path| path.starts_with(prefix)), "{prefix}");
        }
        let task = definitions(CONFIG).unwrap().remove(0);
        let raw = json!({"status":"succeeded","complete":true,"log":"done", "files":{
            "deliverable/.jazyk/private.yaml":STANDARD.encode("forged private state")},
            "reports":{},"cache":{},"deleted":[],"products":[]});
        assert!(validate_result(&task, &input, raw).unwrap_err().contains("Protected output"));
    }
    {
        let _fs = crate::vfs::Scope::enter(&Default::default());
        seed_and_check(Path::new("/project"));
    }
    let root = std::env::temp_dir().join(format!("jazyk-task-input-boundary-{}-{}", crate::sys::process_id(), crate::sys::now_millis()));
    seed_and_check(&root);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("container/nested"), root.join("alias")).unwrap();
        std::os::unix::fs::symlink(root.join("container/.jazyk"), root.join("private-alias")).unwrap();
        seed_and_check(&root);
    }
    std::fs::remove_dir_all(root).unwrap();
}
