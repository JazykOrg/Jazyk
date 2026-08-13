// The job manager: compile, generation, verification, and audit run in-process on one
// worker thread, one at a time, in submission order. Every kind contends on the store
// lock and the LLM budget, so serializing them is the point.
// Mirrors docs/frontends/gui.md#jobs.
use super::state::SharedState;
use crate::turn::{Trace, TraceEvent, TraceLevel};
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

const JOB_EVENT_RING: usize = 50_000;
const TRACE_RETENTION_DAYS: u64 = 30;

// Strings longer than this travel as a preview. The transcript on disk keeps the
// whole payload; a reader asks for the one event it wants to see in full
// (docs/frontends/gui.md#jobs).
const WIRE_STRING_CAP: usize = 2_000;

// One event as it goes to the browser: same shape, long strings cut to a preview
// with their byte count and an `elided` flag beside them.
pub(crate) fn elide(v: &Value) -> Value {
    match v {
        Value::String(s) if s.len() > WIRE_STRING_CAP => {
            json!(format!("{}… [{} chars total]", crate::llm::truncate(s, WIRE_STRING_CAP), s.len()))
        }
        Value::Array(a) => Value::Array(a.iter().map(elide).collect()),
        Value::Object(o) => {
            let mut out = serde_json::Map::new();
            let mut cut = false;
            for (k, val) in o {
                if let Value::String(s) = val {
                    if s.len() > WIRE_STRING_CAP {
                        cut = true;
                    }
                }
                let next = elide(val);
                if next != *val {
                    cut = true;
                }
                out.insert(k.clone(), next);
            }
            if cut {
                out.insert("elided".into(), json!(true));
            }
            Value::Object(out)
        }
        _ => v.clone(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JobKind {
    Compile,
    Gen { entities: Vec<String>, force: bool },
    Verify { targets: Vec<String>, test_kind: Option<String>, force: bool },
    Audit,
    // Draft docs for unclaimed code under the named scopes (empty: every scope).
    Decompile { scopes: Vec<String> },
    // Grade a model: endpoint and model override the resolved settings when given.
    Benchmark { base_url: Option<String>, model: Option<String> },
}

impl JobKind {
    fn name(&self) -> &'static str {
        match self {
            JobKind::Compile => "compile",
            JobKind::Gen { .. } => "gen",
            JobKind::Verify { .. } => "verify",
            JobKind::Audit => "audit",
            JobKind::Decompile { .. } => "decompile",
            JobKind::Benchmark { .. } => "benchmark",
        }
    }
    fn as_value(&self) -> Value {
        match self {
            JobKind::Compile => json!({ "kind": "compile" }),
            JobKind::Gen { entities, force } => json!({ "kind": "gen", "entities": entities, "force": force }),
            JobKind::Verify { targets, test_kind, force } => {
                json!({ "kind": "verify", "targets": targets, "testKind": test_kind, "force": force })
            }
            JobKind::Audit => json!({ "kind": "audit" }),
            JobKind::Decompile { scopes } => json!({ "kind": "decompile", "scopes": scopes }),
            JobKind::Benchmark { base_url, model } => {
                json!({ "kind": "benchmark", "baseUrl": base_url, "model": model })
            }
        }
    }
}

pub struct Job {
    pub id: u64,
    pub kind: JobKind,
    pub state: &'static str, // queued | running | done | failed | cancelled
    pub queued_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub result: Option<Value>,
    // The job's trace events, for a client that opens the job after it started.
    pub events: VecDeque<Value>,
    pub cancel: Arc<AtomicBool>,
    // Stem of the persisted transcript under <out>/trace/.
    pub stem: String,
}

impl Job {
    fn summary(&self) -> Value {
        json!({
            "id": self.id,
            "kind": self.kind.as_value(),
            "state": self.state,
            "queuedAt": self.queued_at,
            "startedAt": self.started_at,
            "finishedAt": self.finished_at,
            "result": self.result,
            "stem": self.stem,
        })
    }
}

struct Inner {
    jobs: BTreeMap<u64, Job>,
    queue: VecDeque<u64>,
    next_id: u64,
    running: Option<u64>,
    stop: bool,
}

pub struct JobManager {
    inner: Mutex<Inner>,
    wake: Condvar,
}

impl JobManager {
    pub fn new() -> JobManager {
        JobManager {
            inner: Mutex::new(Inner {
                jobs: BTreeMap::new(),
                queue: VecDeque::new(),
                next_id: 0,
                running: None,
                stop: false,
            }),
            wake: Condvar::new(),
        }
    }

    // Queue a job. A kind already waiting in the queue dedupes to the queued job's id.
    pub fn submit(&self, st: &SharedState, kind: JobKind) -> u64 {
        let mut inner = self.inner.lock().unwrap();
        for qid in &inner.queue {
            if inner.jobs.get(qid).map(|j| j.kind == kind).unwrap_or(false) {
                return *qid;
            }
        }
        inner.next_id += 1;
        let id = inner.next_id;
        let job = Job {
            id,
            kind,
            state: "queued",
            queued_at: crate::verify::now_iso(),
            started_at: None,
            finished_at: None,
            result: None,
            events: VecDeque::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            stem: String::new(),
        };
        let summary = job.summary();
        inner.jobs.insert(id, job);
        inner.queue.push_back(id);
        drop(inner);
        st.events.emit("job.queued", json!({ "job": summary }));
        self.wake.notify_all();
        id
    }

    pub fn list(&self) -> Vec<Value> {
        let inner = self.inner.lock().unwrap();
        inner.jobs.values().rev().map(|j| j.summary()).collect()
    }

    pub fn get(&self, id: u64) -> Option<Value> {
        let inner = self.inner.lock().unwrap();
        inner.jobs.get(&id).map(|j| {
            let mut v = j.summary();
            v["events"] = json!(j.events.iter().collect::<Vec<_>>());
            v
        })
    }

    // Best effort: a queued job cancels immediately, a running one stops at its next
    // boundary (between waves, entities, or rows). In-flight LLM calls finish.
    pub fn cancel(&self, st: &SharedState, id: u64) -> Option<Value> {
        let mut inner = self.inner.lock().unwrap();
        let job = inner.jobs.get(&id)?;
        job.cancel.store(true, Ordering::Relaxed);
        if job.state == "queued" {
            inner.queue.retain(|q| *q != id);
            let job = inner.jobs.get_mut(&id).unwrap();
            job.state = "cancelled";
            job.finished_at = Some(crate::verify::now_iso());
            let summary = job.summary();
            drop(inner);
            st.events.emit("job.finished", json!({ "jobId": id, "state": "cancelled", "result": Value::Null }));
            return Some(summary);
        }
        Some(inner.jobs.get(&id).unwrap().summary())
    }

    pub fn cancel_running(&self) {
        let inner = self.inner.lock().unwrap();
        if let Some(id) = inner.running {
            if let Some(j) = inner.jobs.get(&id) {
                j.cancel.store(true, Ordering::Relaxed);
            }
        }
    }

    pub fn stop(&self) {
        self.inner.lock().unwrap().stop = true;
        self.wake.notify_all();
    }

    pub fn running_job(&self) -> Option<u64> {
        self.inner.lock().unwrap().running
    }

    fn set_stem(&self, id: u64, stem: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(j) = inner.jobs.get_mut(&id) {
            j.stem = stem.to_string();
        }
    }

    fn push_event(&self, id: u64, ev: Value) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(j) = inner.jobs.get_mut(&id) {
            j.events.push_back(ev);
            while j.events.len() > JOB_EVENT_RING {
                j.events.pop_front();
            }
        }
    }
}

// The single worker thread. Returns the join handle so shutdown can wait for a
// cancelled job to reach a boundary and release the store lock.
pub fn spawn_worker(st: SharedState) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        let (id, kind, cancel) = {
            let jm = &st.jobs;
            let mut inner = jm.inner.lock().unwrap();
            loop {
                if inner.stop {
                    return;
                }
                if let Some(id) = inner.queue.pop_front() {
                    inner.running = Some(id);
                    let job = inner.jobs.get_mut(&id).unwrap();
                    job.state = "running";
                    job.started_at = Some(crate::verify::now_iso());
                    break (id, job.kind.clone(), job.cancel.clone());
                }
                inner = jm.wake.wait(inner).unwrap();
            }
        };
        st.events.emit("job.started", json!({ "jobId": id, "kind": kind.name() }));

        // The transcript: one JSON-lines file per job under <out>/trace/, flushed per
        // line so a tail (or the API) always sees a parseable prefix.
        // Mirrors docs/frontends/gui.md#jobs.
        let stem = {
            let started = crate::verify::now_iso();
            let compact: String = started.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            format!("{}-{}-j{}", compact, kind.name(), id)
        };
        st.jobs.set_stem(id, &stem);
        let trace_path = st.out.join("trace").join(format!("{}.jsonl", stem));
        std::fs::create_dir_all(st.out.join("trace")).ok();
        let writer: Arc<Mutex<Option<std::fs::File>>> = Arc::new(Mutex::new(
            std::fs::OpenOptions::new().create(true).append(true).open(&trace_path).ok(),
        ));
        // The generation at start and finish brackets the run: the journal entries
        // between the two are the run's changesets (gui.md#jobs).
        write_line(&writer, &json!({ "meta": {
            "id": id, "kind": kind.as_value(), "queuedAt": st.jobs.get(id).map(|j| j["queuedAt"].clone()),
            "startedAt": crate::verify::now_iso(),
            "generation": crate::store::read_generation(&st.out),
        }}));

        let counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let sink: Arc<dyn Fn(&TraceEvent) + Send + Sync> = {
            let st = st.clone();
            let writer = writer.clone();
            let counter = counter.clone();
            Arc::new(move |ev: &TraceEvent| {
                let n = counter.fetch_add(1, Ordering::Relaxed) + 1;
                // The file keeps the whole payload; the browser gets it elided and
                // fetches the one event it wants to read in full.
                write_line(&writer, &json!({ "n": n, "event": ev }));
                let payload = json!({ "jobId": id, "n": n, "event": elide(&json!(ev)) });
                st.jobs.push_event(id, payload.clone());
                st.events.emit("job.trace", payload);
            })
        };
        let trace = Trace::to_sink(TraceLevel::Normal, sink, cancel.clone()).with_run(&stem);
        let result = execute(&st, &kind, &trace);

        let (state, result) = match result {
            Ok(v) if cancel.load(Ordering::Relaxed) => ("cancelled", Some(v)),
            Ok(v) => ("done", Some(v)),
            Err(e) => ("failed", Some(json!({ "error": e }))),
        };
        {
            let mut inner = st.jobs.inner.lock().unwrap();
            inner.running = None;
            let job = inner.jobs.get_mut(&id).unwrap();
            job.state = state;
            job.finished_at = Some(crate::verify::now_iso());
            job.result = result.clone();
        }
        write_line(&writer, &json!({ "outcome": {
            "state": state, "result": result, "finishedAt": crate::verify::now_iso(),
            "generation": crate::store::read_generation(&st.out),
        }}));
        st.events.emit("job.finished", json!({ "jobId": id, "state": state, "result": result }));
        let st2 = st.clone();
        std::thread::spawn(move || super::events::recompute_pending(&st2));
        super::jobs_hook_on_job_finished(&st, &kind);
    })
}

fn execute(st: &SharedState, kind: &JobKind, trace: &Trace) -> Result<Value, String> {
    match kind {
        JobKind::Compile => {
            let report = crate::reconcile::compile(&st.proj(), &st.llm(), &st.out, trace);
            Ok(json!(report))
        }
        JobKind::Gen { entities, force } => {
            // The clicked gen is an approval and holds the build lease, same as the CLI.
            let _build = crate::control::begin_internal_build(&st.proj(), &st.out, "generate")?;
            let runner = crate::acp::runner::AcpRunner::start(&st.proj(), &st.llm(), &st.out)?;
            runner.set_build_token(Some(format!("internal-{}", std::process::id())));
            let store = crate::store::Store::load(&st.out);
            // Binding first, same as `jazyk gen`: owed binds classify each requirement
            // and the bound tests become the acceptance gates.
            if let Err(e) =
                crate::bind::run_all(&store, &runner, &st.gs(), entities, &st.proj().limits, &st.proj().linting, trace)
            {
                trace.line("bind", &e);
            }
            crate::gen::run_all(&store, &runner, &st.gs(), entities, *force, &st.proj().limits, &st.proj().linting, trace)
        }
        JobKind::Verify { targets, test_kind, force } => {
            let runner = crate::acp::runner::AcpRunner::start(&st.proj(), &st.llm(), &st.out)?;
            let store = crate::store::Store::load(&st.out);
            crate::verify::run_all(&store, &runner, &st.gs(), targets, test_kind.as_deref(), *force, trace)
        }
        JobKind::Audit => {
            let store = crate::store::Store::load(&st.out);
            Ok(crate::verify::audit(&store, &st.gs()))
        }
        JobKind::Decompile { scopes } => {
            let runner = crate::acp::runner::AcpRunner::start(&st.proj(), &st.llm(), &st.out)?;
            let store = crate::store::Store::load(&st.out);
            crate::control::release_decompile(&st.proj(), &st.out, scopes);
            crate::decompile::run_all(&st.proj(), &store, &runner, &st.gs(), scopes, trace)
        }
        JobKind::Benchmark { base_url, model } => {
            let mut llm = st.llm();
            if let Some(b) = base_url {
                llm.base_url = b.clone();
            }
            if let Some(m) = model {
                llm.model = m.clone();
            }
            let code = crate::benchmark::run_traced(&llm, &st.out, trace);
            Ok(json!({"exit": code, "results": crate::benchmark::all_results(&st.out)}))
        }
    }
}

// ---- handlers ----

pub async fn post_job(State(st): State<SharedState>, Json(body): Json<Value>) -> Response {
    // Dispatch by worker preference: with an agent attached and preferred, the click
    // records the release and the agent does the work. Mirrors
    // docs/compiler/reconciler.md#dispatch.
    if let Some(stage) = match body["kind"].as_str() {
        Some("compile") => Some("compile"),
        Some("gen") => Some("generate"),
        Some("decompile") => Some("decompile"),
        _ => None,
    } {
        let c = st.control();
        if c.worker != "internal" {
            let agents: Vec<String> = crate::control::workers(&st.out)
                .into_iter()
                .filter(|w| w.kind == "agent")
                .map(|w| w.client)
                .collect();
            if !agents.is_empty() {
                if stage == "decompile" {
                    // Every current scope, or the named ones: the release is the trigger.
                    let mut scopes = str_list(&body["scopes"]);
                    if scopes.is_empty() {
                        let store = crate::store::Store::load(&st.out);
                        scopes = crate::decompile::scopes(&st.proj(), &store, &st.gs()).into_keys().collect();
                    }
                    crate::control::release_decompile(&st.proj(), &st.out, &scopes);
                } else {
                    crate::control::release(&st.proj(), &st.out, Some(stage));
                }
                let snap = super::api::workers_snapshot(&st);
                st.events.emit("control.changed", snap);
                return (
                    StatusCode::ACCEPTED,
                    Json(json!({
                        "dispatched": "agent",
                        "agents": agents,
                        "note": "release recorded; the attached agent picks up the work",
                    })),
                )
                    .into_response();
            }
            if c.worker == "agent" {
                return super::api::err(
                    StatusCode::CONFLICT,
                    "worker is `agent` but no agent is attached; start one or set worker to any or internal",
                );
            }
        }
    }
    let kind = match body["kind"].as_str() {
        Some("compile") => JobKind::Compile,
        Some("benchmark") => JobKind::Benchmark {
            base_url: body["baseUrl"].as_str().map(String::from),
            model: body["model"].as_str().map(String::from),
        },
        Some("gen") => JobKind::Gen {
            entities: str_list(&body["entities"]),
            force: body["force"].as_bool().unwrap_or(false),
        },
        Some("verify") => JobKind::Verify {
            targets: str_list(&body["targets"]),
            test_kind: body["testKind"].as_str().map(|s| s.to_string()),
            force: body["force"].as_bool().unwrap_or(false),
        },
        Some("audit") => JobKind::Audit,
        Some("decompile") => JobKind::Decompile { scopes: str_list(&body["scopes"]) },
        _ => {
            return super::api::err(
                StatusCode::BAD_REQUEST,
                "kind must be compile, gen, verify, audit, or decompile",
            )
        }
    };
    let id = st.jobs.submit(&st, kind);
    (StatusCode::ACCEPTED, Json(json!({ "jobId": id }))).into_response()
}

fn str_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|s| s.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

pub async fn list_jobs(State(st): State<SharedState>) -> Json<Value> {
    Json(json!({ "jobs": st.jobs.list(), "running": st.jobs.running_job() }))
}

pub async fn get_job(State(st): State<SharedState>, UrlPath(id): UrlPath<u64>) -> Response {
    match st.jobs.get(id) {
        Some(v) => Json(v).into_response(),
        None => super::api::err(StatusCode::NOT_FOUND, format!("no job {}", id)),
    }
}

pub async fn cancel_job(State(st): State<SharedState>, UrlPath(id): UrlPath<u64>) -> Response {
    match st.jobs.cancel(&st, id) {
        Some(v) => Json(v).into_response(),
        None => super::api::err(StatusCode::NOT_FOUND, format!("no job {}", id)),
    }
}

fn write_line(writer: &Arc<Mutex<Option<std::fs::File>>>, v: &Value) {
    use std::io::Write;
    if let Some(f) = writer.lock().unwrap().as_mut() {
        let _ = writeln!(f, "{}", v);
        let _ = f.flush();
    }
}

// Remove transcripts past the retention window. Runs once per server start.
pub fn sweep_traces(out: &std::path::Path) {
    let dir = out.join("trace");
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(TRACE_RETENTION_DAYS * 24 * 3600);
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
            continue;
        }
        if let Ok(md) = e.metadata() {
            if md.modified().map(|m| m < cutoff).unwrap_or(false) {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
}

// ---- transcript endpoints ----

// Past jobs from the transcripts on disk, newest first: the meta line plus the
// outcome line when the job finished (a missing outcome means it died mid-run).
pub async fn list_traces(State(st): State<SharedState>) -> Json<Value> {
    let dir = st.out.join("trace");
    let list = tokio::task::spawn_blocking(move || {
        let mut items: Vec<(std::time::SystemTime, Value)> = Vec::new();
        let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let mut lines = text.lines();
            let Some(meta) = lines.next().and_then(|l| serde_json::from_str::<Value>(l).ok()) else { continue };
            let outcome = text
                .lines()
                .last()
                .and_then(|l| serde_json::from_str::<Value>(l).ok())
                .filter(|v| !v["outcome"].is_null());
            let events = text.lines().count().saturating_sub(1 + outcome.is_some() as usize);
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            let mtime = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            items.push((mtime, serde_json::json!({
                "stem": stem,
                "meta": meta["meta"],
                "outcome": outcome.map(|o| o["outcome"].clone()),
                "events": events,
            })));
        }
        items.sort_by(|a, b| b.0.cmp(&a.0));
        items.into_iter().map(|(_, v)| v).take(200).collect::<Vec<_>>()
    })
    .await
    .expect("trace list");
    Json(serde_json::json!({ "traces": list }))
}

fn trace_path(st: &SharedState, stem: &str) -> Option<std::path::PathBuf> {
    if stem.is_empty() || !stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    Some(st.out.join("trace").join(format!("{}.jsonl", stem)))
}

pub async fn get_trace(State(st): State<SharedState>, UrlPath(stem): UrlPath<String>) -> Response {
    let Some(path) = trace_path(&st, &stem) else {
        return super::api::err(StatusCode::BAD_REQUEST, "invalid trace name");
    };
    let result = tokio::task::spawn_blocking(move || -> Option<Value> {
        let text = std::fs::read_to_string(&path).ok()?;
        let mut meta = Value::Null;
        let mut outcome = Value::Null;
        let mut events: Vec<Value> = Vec::new();
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
            if !v["meta"].is_null() {
                meta = v["meta"].clone();
            } else if !v["outcome"].is_null() {
                outcome = v["outcome"].clone();
            } else {
                // Same elision as the live stream, so a reloaded page and a streaming
                // one render the same rows.
                events.push(elide(&v));
            }
        }
        Some(serde_json::json!({ "meta": meta, "outcome": outcome, "events": events }))
    })
    .await
    .expect("trace read");
    match result {
        Some(v) => Json(v).into_response(),
        None => super::api::err(StatusCode::NOT_FOUND, format!("no transcript {}", stem)),
    }
}

// One event of one transcript, with nothing cut: what the activity panel fetches
// when a row is expanded (docs/frontends/gui.md#jobs). A running job's events are
// readable the same way, because the file is flushed per line.
pub async fn get_trace_event(
    State(st): State<SharedState>,
    UrlPath((stem, n)): UrlPath<(String, u64)>,
) -> Response {
    let Some(path) = trace_path(&st, &stem) else {
        return super::api::err(StatusCode::BAD_REQUEST, "invalid trace name");
    };
    let found = tokio::task::spawn_blocking(move || -> Option<Value> {
        let text = std::fs::read_to_string(&path).ok()?;
        text.lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find(|v| v["n"].as_u64() == Some(n))
    })
    .await
    .expect("trace event read");
    match found {
        Some(v) => Json(v).into_response(),
        None => super::api::err(StatusCode::NOT_FOUND, format!("no event {} in {}", n, stem)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elision_cuts_long_strings_and_marks_the_event() {
        let long = "x".repeat(WIRE_STRING_CAP + 500);
        let ev = json!({"kind": "llmRequest", "label": "reconcile-doc a.md", "step": "r1",
            "messages": [{"role": "system", "content": "short"}, {"role": "user", "content": long}]});
        let wire = elide(&ev);
        assert_eq!(wire["elided"], json!(true));
        assert_eq!(wire["messages"][0]["content"], json!("short"));
        assert!(wire["messages"][0]["elided"].is_null());
        let cut = wire["messages"][1]["content"].as_str().unwrap();
        assert!(cut.len() < WIRE_STRING_CAP + 100);
        assert!(cut.ends_with(&format!("[{} chars total]", WIRE_STRING_CAP + 500)));
        // A small event travels untouched, flag included.
        let small = json!({"kind": "note", "label": "a", "text": "b"});
        assert_eq!(elide(&small), small);
    }
}
