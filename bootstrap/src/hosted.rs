//! Filesystem-free native compiler embedding. See docs/compiler/hosted-runtime.md.
use crate::session::{Trace, TraceLevel};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub type Snapshot = BTreeMap<String, Vec<u8>>;
pub type ModelTransport = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;
pub use crate::session::TraceSink;
pub type EventSink = Arc<dyn Fn(&Value) + Send + Sync>;

/// Text projections for authored revisions and delivery, not a verification verdict.
#[derive(Debug, serde::Serialize)]
pub struct Projection {
    pub source: BTreeMap<String, String>,
    pub files: BTreeMap<String, String>,
    pub criteria: BTreeMap<String, Value>,
    pub graph_generation: u64,
}

/// The host must enforce a deadline and cancellation within its model callback.
#[derive(Clone, Default)]
pub struct Execution {
    pub model: String,
    pub transport: Option<ModelTransport>,
    pub trace: Option<TraceSink>,
    pub events: Option<EventSink>,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}
impl Reply {
    fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            content_type: "application/json".into(),
            body: body.to_string().into_bytes(),
        }
    }
}

pub struct Workspace {
    files: Snapshot,
}
impl Workspace {
    pub fn new(files: Snapshot) -> Result<Self, String> {
        for name in files.keys() {
            if name.is_empty()
                || name.contains(['\\', '\0', ':'])
                || name
                    .split('/')
                    .any(|c| c.is_empty() || c == "." || c == "..")
            {
                return Err(format!("invalid workspace path: {name}"));
            }
            // A flat snapshot cannot represent a file and a directory at one path.
            let mut parent = std::path::Path::new(name).parent();
            while let Some(p) = parent {
                if p.to_str().is_some_and(|s| files.contains_key(s)) {
                    return Err(format!("file conflicts with directory: {}", p.display()));
                }
                parent = p.parent();
            }
        }
        Ok(Self { files })
    }

    pub fn snapshot(&self) -> Snapshot {
        self.files.clone()
    }

    /// Preserve authored membership explicitly. Merely writing a generated file
    /// does not make it source or a delivery candidate.
    pub fn projection(&self, authored_paths: &[String]) -> Result<Projection, String> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _fs = crate::vfs::Scope::enter(&self.files);
            project_projection(&self.files, authored_paths)
        }))
        .unwrap_or_else(|_| Err("hosted projection panicked; workspace was unchanged".into()))
    }

    /// Synchronous. The caller owns the project lease and atomic persistence.
    /// Ordinary compiler failures retain committed state. Unwinding rolls back.
    pub fn request(
        &mut self,
        method: &str,
        path: &str,
        body: &str,
        execution: &Execution,
    ) -> Result<Reply, String> {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let fs = crate::vfs::Scope::enter(&self.files);
            let _state = crate::scoped_state::Scope::enter();
            let _execution = ExecutionScope::enter(execution.clone());
            let conversations = crate::conversation::Scope::enter()?;
            let reply = dispatch(method, path, body)?;
            if path.contains("/chat/") && method != "GET" {
                conversations.save()?;
            }
            Ok::<_, String>((reply, fs.snapshot()))
        }));
        match result {
            Ok(Ok((reply, files))) => {
                self.files = files;
                Ok(reply)
            }
            Ok(Err(error)) => Err(error),
            Err(_) => Err("hosted compiler panicked; request snapshot was rolled back".into()),
        }
    }
}

fn project_projection(files: &Snapshot, authored: &[String]) -> Result<Projection, String> {
    let w = crate::document_sets::Workspace::load(&root()).map_err(|e| e.1)?;
    let mut combined = Projection {
        source: Default::default(),
        files: Default::default(),
        criteria: Default::default(),
        graph_generation: 0,
    };
    let contexts: Vec<_> = if w.sets.is_empty() {
        vec![w.files_project()]
    } else {
        w.sets
            .iter()
            .map(|s| w.resolve(Some(&s.id)).unwrap())
            .collect()
    };
    for p in contexts {
        let projected = set_projection(files, authored, &p)?;
        combined.source.extend(projected.source);
        combined.files.extend(projected.files);
        for (id, v) in projected.criteria {
            let id = p
                .set_id
                .as_deref()
                .map(|set| crate::webapi::sets::qualify(&id, set))
                .unwrap_or(id);
            combined.criteria.insert(
                id,
                p.set_id
                    .as_deref()
                    .map(|set| crate::webapi::sets::qualify_value(v.clone(), set))
                    .unwrap_or(v),
            );
        }
        combined.graph_generation += projected.graph_generation;
    }
    Ok(combined)
}
fn set_projection(
    files: &Snapshot,
    authored: &[String],
    p: &crate::project::Project,
) -> Result<Projection, String> {
    use crate::vfs::VPathExt;
    let store = crate::store::Store::load(&p.out);
    let gs = crate::gen::GenSettings::resolve(p);
    let ledger = crate::gen::Ledger::load(&p.out);
    // Resolve through the VFS so redirected projects and dot components use the
    // same paths as compilation. No absent or out-of-workspace file can enter.
    let relative = |path: &Path| -> Option<String> {
        if !path.vis_file() {
            return None;
        }
        let path = crate::vfs::canonicalize(path).ok()?;
        Some(path.strip_prefix(root()).ok()?.to_str()?.to_owned())
    };
    let internal = |name: &str| {
        !matches!(name, crate::document_sets::CONFIG | ".jazyk/.gitignore")
            && (name
                .split('/')
                .any(|c| c == ".git" || c.starts_with(".jazyk") || c.starts_with("jazyk-out"))
                || root().join(name).starts_with(&p.out))
    };
    let mut owned = BTreeSet::new();
    let mut accepted = BTreeSet::new();
    for path in ledger
        .support
        .iter()
        .chain(ledger.entities.values().flat_map(|e| e.files.iter()))
        .chain(ledger.requirements.values().flat_map(|r| r.files.iter()))
    {
        if let Some(name) = relative(&gs.deliverable.join(path)) {
            owned.insert(name.clone());
            accepted.insert(name);
        }
    }
    for row in ledger.requirements.values() {
        for test in std::iter::once(&row.test).chain(row.tests.values().map(|t| &t.test)) {
            if let Some(name) = relative(&crate::gen::artifact_path(&p.out, &gs, test)) {
                owned.insert(name.clone());
                if test.kind == "programmatic" {
                    accepted.insert(name);
                }
            }
        }
    }
    for path in crate::gen::build_outputs(&ledger) {
        if let Some(name) = relative(&gs.deliverable.join(path)) {
            owned.insert(name.clone());
            accepted.remove(&name);
        }
    }
    let mut source_paths = BTreeSet::new();
    for name in authored {
        if name.is_empty()
            || name.contains(['\\', '\0', ':'])
            || name
                .split('/')
                .any(|c| c.is_empty() || c == "." || c == "..")
        {
            return Err(format!("invalid authored path: {name}"));
        }
        if files.contains_key(name) {
            source_paths.insert(name.clone());
        }
    }
    for path in p.doc_files().into_iter().chain([
        root().join(crate::document_sets::CONFIG),
        p.root.join(crate::document_sets::CONFIG),
    ]) {
        if let Some(name) = relative(&path) {
            source_paths.insert(name);
        }
    }
    source_paths.retain(|name| !internal(name) && !owned.contains(name));
    let read_text = |names: BTreeSet<String>| -> Result<BTreeMap<String, String>, String> {
        names
            .into_iter()
            .filter(|name| !internal(name))
            .map(|name| {
                let bytes = files
                    .get(&name)
                    .ok_or_else(|| format!("missing projected file: {name}"))?;
                let content = std::str::from_utf8(bytes)
                    .map_err(|_| format!("projected file is not UTF-8 text: {name}"))?;
                Ok((name, content.to_owned()))
            })
            .collect()
    };
    let source = read_text(source_paths)?;
    let mut candidate = source.clone();
    candidate.extend(read_text(accepted)?);
    let criteria = store
        .graph
        .criteria
        .iter()
        .map(|(id, criterion)| {
            serde_json::to_value(criterion)
                .map(|value| (id.clone(), value))
                .map_err(|e| e.to_string())
        })
        .collect::<Result<_, _>>()?;
    Ok(Projection {
        source,
        files: candidate,
        criteria,
        graph_generation: store.status.generation,
    })
}

thread_local! { static EXECUTION: RefCell<Option<Execution>> = const { RefCell::new(None) }; }
struct ExecutionScope(Option<Execution>);
impl ExecutionScope {
    fn enter(execution: Execution) -> Self {
        Self(EXECUTION.with(|e| e.replace(Some(execution))))
    }
}
impl Drop for ExecutionScope {
    fn drop(&mut self) {
        EXECUTION.with(|e| e.replace(self.0.take()));
    }
}
fn execution() -> Execution {
    EXECUTION.with(|e| e.borrow().clone().expect("hosted execution context"))
}

pub fn root() -> PathBuf {
    PathBuf::from("/project")
}
pub fn model_name() -> String {
    execution().model
}
pub fn cancelled() -> bool {
    EXECUTION.with(|e| {
        e.borrow()
            .as_ref()
            .is_some_and(|e| e.cancel.load(Ordering::Relaxed))
    })
}
pub fn trace() -> Trace {
    let e = execution();
    Trace::to_sink(
        TraceLevel::Normal,
        e.trace.unwrap_or_else(|| Arc::new(|_| {})),
        e.cancel,
    )
}
pub fn llm() -> crate::llm::Llm {
    crate::llm::Llm {
        base_url: "host-injected".into(),
        model: model_name(),
        api_key: String::new(),
        temperature: Some(0.0),
        trace: Some(trace()),
    }
}
pub fn emit_event(kind: &str, mut body: Value) {
    body["type"] = json!(kind);
    if let Some(sink) = execution().events {
        sink(&body);
    }
}
pub(crate) fn transport(body: &str) -> Result<String, String> {
    let e = execution();
    if e.cancel.load(Ordering::Relaxed) {
        return Err("execution cancelled".into());
    }
    let transport = e
        .transport
        .ok_or("no model transport supplied for this workspace")?;
    let reply = transport(body)?;
    if e.cancel.load(Ordering::Relaxed) {
        return Err("execution cancelled".into());
    }
    Ok(reply)
}

fn dispatch(method: &str, path: &str, body: &str) -> Result<Reply, String> {
    let routed =
        match crate::webapi::sets::route(path, serde_json::from_str(body).unwrap_or(Value::Null)) {
            Ok(r) => r,
            Err((code, e)) => return Ok(Reply::json(code, json!({"error":e}))),
        };
    let (route, query) = routed.path.split_once('?').unwrap_or((&routed.path, ""));
    // A configuration that does not load, or a project with no document set, is a
    // state of the project with its own status, never a server failure.
    // Mirrors docs/compiler/document-sets.md#discovery-and-opening.
    let workspace = match crate::document_sets::Workspace::load(&root()) {
        Ok(w) => w,
        Err((code, e)) => return Ok(Reply::json(code, json!({"error":e}))),
    };
    let selected = || workspace.resolve(routed.selector.as_deref());
    let route = route.strip_prefix("/api").unwrap_or(route);
    if method == "GET" && matches!(route, "/out/file" | "/project/file" | "/deliverable/raw") {
        let rel = url::form_urlencoded::parse(query.as_bytes())
            .find(|(k, _)| k == "path")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        let p =
            if matches!(route, "/project/file" | "/deliverable/raw") && routed.selector.is_none() {
                workspace.files_project()
            } else {
                match selected() {
                    Ok(p) => p,
                    Err((code, e)) => return Ok(Reply::json(code, json!({"error":e}))),
                }
            };
        let result = match route {
            "/out/file" => crate::webapi::outfiles::out_file(&p.out, &rel),
            "/project/file" => crate::webapi::projfiles::project_file(&p.root, &rel),
            _ => crate::webapi::deliverable::deliverable_raw(
                &crate::gen::GenSettings::resolve(&p),
                &rel,
            ),
        };
        return Ok(match result {
            Ok(body) => Reply {
                status: 200,
                content_type: crate::webapi::outfiles::content_type(&rel).into(),
                body,
            },
            Err((code, msg)) => Reply::json(code, json!({"error":msg})),
        });
    }
    if method == "POST" && route == "/jobs" {
        let request = routed.body.clone();
        let kind = request["kind"].as_str().unwrap_or("compile");
        if matches!(kind, "verify" | "audit" | "gen") {
            let p = match selected() {
                Ok(p) => p,
                Err((code, e)) => return Ok(Reply::json(code, json!({"error":e}))),
            };
            let store = crate::store::Store::load(&p.out);
            let gs = crate::gen::GenSettings::resolve(&p);
            let result = if kind == "gen" {
                use crate::runner::SessionRunner;
                let _build = crate::control::begin_internal_build(&p, &p.out, "generate")?;
                let runner = crate::inprocess::InProcessRunner::start(&p, &llm(), &p.out)?;
                runner.set_build_token(Some(format!("internal-{}", crate::sys::process_id())));
                let entities = request["entities"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let _ = crate::bind::run_all(&store, &runner, &gs, &entities, &trace());
                crate::gen::run_all(
                    &store,
                    &runner,
                    &gs,
                    &entities,
                    request["force"].as_bool().unwrap_or(false),
                    &trace(),
                )
            } else if kind == "audit" {
                Ok(crate::verify::audit(&store, &gs))
            } else {
                let runner = crate::inprocess::InProcessRunner::start(&p, &llm(), &p.out)?;
                let targets = request["targets"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                crate::verify::run_all(
                    &store,
                    &runner,
                    &gs,
                    &targets,
                    request["testKind"].as_str(),
                    request["force"].as_bool().unwrap_or(false),
                    &trace(),
                )
            };
            return Ok(match result {
                Ok(report) => Reply::json(
                    200,
                    json!({"id":1,"kind":kind,"state":"finished","report":report}),
                ),
                Err(error) => Reply::json(422, json!({"error":error})),
            });
        }
    }
    if method == "GET" && route == "/capabilities" {
        return Ok(Reply::json(
            200,
            json!({"protocol":crate::protocol::EDITOR_PROTOCOL_VERSION,"backend":"hosted","projects":"list","docs":"write","events":"pushed","lsp":false,"jobs":["compile","gen","verify","audit"],"agent":"embedded","model":if execution().transport.is_some(){"configured"}else{"none"},"modelName":model_name(),"rendering":"server","persistence":"memory","shell":false,"dev":false}),
        ));
    }
    let envelope: Value =
        serde_json::from_str(&crate::webapi::dispatch::request(method, path, body))
            .map_err(|e| e.to_string())?;
    Ok(Reply::json(
        envelope["status"].as_u64().unwrap_or(500) as u16,
        envelope["body"].clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

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
        files.insert("docs/main.md".into(), b"# Main\n\nThe system shall respond.\n".to_vec());
        let mut w = Workspace::new(files).unwrap();
        let e = Execution::default();
        let doc = w.request("GET", "/api/docs/content?path=docs/main.md", "", &e).unwrap();
        assert_eq!(doc.status, 409, "{}", String::from_utf8_lossy(&doc.body));
        assert!(String::from_utf8_lossy(&doc.body).contains("no-document-set"));
        let job = w.request("POST", "/api/jobs", r#"{"kind":"audit"}"#, &e).unwrap();
        assert_eq!(job.status, 409, "{}", String::from_utf8_lossy(&job.body));
        assert!(String::from_utf8_lossy(&job.body).contains("Add one under Document sets"));
        let text = w.request("GET", "/api/project/text?path=docs/main.md", "", &e).unwrap();
        assert_eq!(text.status, 200);
        assert!(String::from_utf8_lossy(&text.body).contains("The system shall respond."));
        // The old paths come back as the first set's suggestion, and nothing is written.
        let sets = w.request("GET", "/api/sets", "", &e).unwrap();
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
            transport: Some(Arc::new(move |body| {
                let request: Value = serde_json::from_str(body).unwrap();
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

    fn workspace(name: &str) -> Workspace {
        Workspace::new(BTreeMap::from([
            (
                ".jazyk/jazyk.toml".into(),
                b"version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = [\"docs/main.md\"]\nsource = []\n"
                    .to_vec(),
            ),
            (
                "docs/main.md".into(),
                format!("# {name}\n\nNavigation index.\n").into_bytes(),
            ),
        ]))
        .unwrap()
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
        ] {
            w.files.insert(path.into(), text.as_bytes().to_vec());
        }
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
            store.save();
            let programmatic = |artifact| json!({"kind":"programmatic", "label":"test", "artifact":artifact, "name":"test", "run":"python test.py"});
            let ledger: crate::gen::Ledger = serde_json::from_value(json!({
                "support":["spec/product/package.json"],
                "entities":{"alpha":{"factHash":"recorded", "requirements":[], "files":["spec/product/main.py","spec/product/build.zip","spec/product/missing.py"]}},
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
            ledger.save(&project.out);
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
        assert_eq!(projection.files.len(), projection.source.len() + 4);
        assert_eq!(
            projection.source["notes.txt"],
            "An existing human file, edited."
        );
        assert_eq!(w.snapshot(), before);
        assert!(!crate::vfs::is_scoped());
        let other = workspace("Beta").projection(&[]).unwrap();
        assert_eq!(other.graph_generation, 0);
        assert!(other.criteria.is_empty());
    }

    #[test]
    fn projection_rejects_invalid_membership_and_selected_binary_without_mutation() {
        let mut w = workspace("Alpha");
        w.files.insert("photo.png".into(), vec![0, 255]);
        let before = w.snapshot();
        assert!(w
            .projection(&["photo.png".into()])
            .unwrap_err()
            .contains("not UTF-8"));
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

    fn value(w: &mut Workspace, method: &str, path: &str, body: Value, e: &Execution) -> Value {
        let reply = w.request(method, path, &body.to_string(), e).unwrap();
        assert!(
            reply.status < 400,
            "{}",
            String::from_utf8_lossy(&reply.body)
        );
        serde_json::from_slice(&reply.body).unwrap()
    }
    fn model_reply(content: &str) -> String {
        json!({"choices":[{"message":{"role":"assistant","content":content}}],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}).to_string()
    }
    fn navigation_model(
        name: &'static str,
        calls: Arc<AtomicUsize>,
        barrier: Option<Arc<std::sync::Barrier>>,
    ) -> Execution {
        Execution {
            model: name.into(),
            transport: Some(Arc::new(move |body| {
                let request: Value = serde_json::from_str(body).unwrap();
                assert_eq!(request["model"], name);
                let n = calls.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    if let Some(b) = &barrier {
                        b.wait();
                    }
                }
                if request["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|m| m["role"] == "tool")
                {
                    return Ok(model_reply("Done."));
                }
                let section = format!("docs/main.md#/{}", name.to_lowercase());
                let goal = format!("g:reconcile-section:{section}");
                let actions = [
                    (
                        "set_coverage",
                        json!({"section":section,"state":"non-normative","note":"This navigation index contains no product obligations."}),
                    ),
                    (
                        "mark_goal_done",
                        json!({"goal":goal,"justification":"The section is a navigation index with no normative sentences."}),
                    ),
                    (
                        "done",
                        json!({"summary":"Classified the navigation index."}),
                    ),
                ];
                Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":actions.iter().enumerate().map(|(i,(name,args))|json!({"id":format!("call-{i}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()}}],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}).to_string())
            })),
            ..Default::default()
        }
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
            .request(
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
    fn chat_histories_roundtrip_and_panics_restore_all_context() {
        let mut a = workspace("Alpha");
        let mut b = workspace("Beta");
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let e = Execution {
            model: "fixture".into(),
            transport: Some(Arc::new(|_| Ok(model_reply("Hello from the fixture.")))),
            events: Some(Arc::new(move |v| sink.lock().unwrap().push(v.clone()))),
            ..Default::default()
        };
        let chat = value(&mut a, "POST", "/api/chat/sessions", json!({}), &e);
        value(
            &mut a,
            "POST",
            &format!("/api/chat/sessions/{}/prompt", chat["id"].as_str().unwrap()),
            json!({"text":"hello alpha"}),
            &e,
        );
        let mut restored = Workspace::new(a.snapshot()).unwrap();
        let list = value(&mut restored, "GET", "/api/chat/sessions", Value::Null, &e);
        assert_eq!(list["sessions"].as_array().unwrap().len(), 1);
        assert!(list.to_string().contains("hello alpha"));
        assert_eq!(
            value(&mut b, "GET", "/api/chat/sessions", Value::Null, &e)["sessions"],
            json!([])
        );
        assert!(events
            .lock()
            .unwrap()
            .iter()
            .any(|v| v["type"] == "chat.update"));
        let before = restored.snapshot();
        let panic = Execution {
            transport: Some(Arc::new(|_| {
                crate::llm::add_tokens(999);
                crate::llm::set_tools_mode(2);
                panic!("test callback panic")
            })),
            ..Default::default()
        };
        assert!(restored
            .request(
                "POST",
                "/api/chat/sessions/chat-1/prompt",
                "{\"text\":\"panic\"}",
                &panic
            )
            .unwrap_err()
            .contains("rolled back"));
        assert_eq!(restored.snapshot(), before);
        assert!(!crate::vfs::is_scoped());
        let recovery_calls = AtomicUsize::new(0);
        let fresh = Execution {
            transport: Some(Arc::new(move |_| {
                if recovery_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    assert_eq!(crate::llm::tokens_spent(), 0);
                    assert_ne!(crate::llm::tools_mode(), 2);
                }
                Ok(model_reply("Recovered."))
            })),
            ..Default::default()
        };
        value(
            &mut restored,
            "POST",
            "/api/chat/sessions/chat-1/prompt",
            json!({"text":"recover"}),
            &fresh,
        );
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
            crate::render::render_svg("@startuml\n!include /etc/hosts\nclass Alpha\n@enduml")
                .is_err()
        );
        assert!(
            crate::render::render_svg("@startuml\nclass \"<img:/etc/hosts>\"\n@enduml").is_err()
        );
        assert!(crate::render::render_png("<svg />").is_err());
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
    fn stop_discards_staged_coverage_and_late_done_without_retry_then_build_resumes() {
        let mut w = workspace("Alpha");
        let e = navigation_model("Alpha", Arc::new(AtomicUsize::new(0)), None);
        value(&mut w, "POST", "/api/jobs", json!({"kind":"compile"}), &e);
        let baseline = w.snapshot();
        w.files.insert(
            "docs/main.md".into(),
            b"# Alpha\n\nUpdated navigation index.\n".to_vec(),
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let stopping = Execution {
            cancel: cancel.clone(),
            model: "Alpha".into(),
            transport: Some(Arc::new(move |_| {
                let n = seen.fetch_add(1, Ordering::SeqCst);
                let actions = if n == 0 {
                    vec![(
                        "set_coverage",
                        json!({"section":"docs/main.md#/alpha", "state":"non-normative", "note":"Navigation."}),
                    )]
                } else {
                    flag.store(true, Ordering::SeqCst);
                    vec![
                        (
                            "mark_goal_done",
                            json!({"goal":"g:reconcile-section:docs/main.md#/alpha", "justification":"Done."}),
                        ),
                        ("done", json!({"summary":"late completion"})),
                    ]
                };
                Ok(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":actions.iter().enumerate().map(|(i,(name,args))| json!({"id":format!("call-{i}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()}}]}).to_string())
            })),
            ..Default::default()
        };
        value(
            &mut w,
            "POST",
            "/api/jobs",
            json!({"kind":"compile"}),
            &stopping,
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "stop must not retry or dispatch the late done"
        );
        for (path, bytes) in baseline.iter().filter(|(p, _)| p.contains("/journal/")) {
            assert_eq!(
                w.files.get(path),
                Some(bytes),
                "earlier commit changed: {path}"
            );
        }
        let stopped = value(
            &mut w,
            "GET",
            "/api/status",
            Value::Null,
            &Execution::default(),
        );
        assert_ne!(stopped["verdict"]["state"], "converged");
        let resumed = Arc::new(AtomicUsize::new(0));
        let e = navigation_model("Alpha", resumed.clone(), None);
        value(&mut w, "POST", "/api/jobs", json!({"kind":"compile"}), &e);
        assert!(
            resumed.load(Ordering::SeqCst) > 0,
            "staged coverage must have been discarded"
        );
        let before = resumed.load(Ordering::SeqCst);
        value(&mut w, "POST", "/api/jobs", json!({"kind":"compile"}), &e);
        assert_eq!(
            resumed.load(Ordering::SeqCst),
            before,
            "completed resume must be a no-op"
        );
    }

}
