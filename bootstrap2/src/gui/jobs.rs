// The job manager: compile, generation, verification, and audit run in-process on one
// worker thread, one at a time, in submission order. Every kind contends on the store
// lock and the LLM budget, so serializing them is the point.
// Mirrors docs2/frontends/gui.md#jobs.
use super::state::SharedState;
use crate::turn::{Trace, TraceEvent, TraceLevel};
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

const JOB_EVENT_RING: usize = 500;

#[derive(Clone, Debug, PartialEq)]
pub enum JobKind {
    Compile,
    Gen { entities: Vec<String>, force: bool },
    Verify { targets: Vec<String>, test_kind: Option<String>, force: bool },
    Audit,
}

impl JobKind {
    fn name(&self) -> &'static str {
        match self {
            JobKind::Compile => "compile",
            JobKind::Gen { .. } => "gen",
            JobKind::Verify { .. } => "verify",
            JobKind::Audit => "audit",
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
    // Recent trace events, for a client that opens the job after it started.
    pub events: VecDeque<Value>,
    pub cancel: Arc<AtomicBool>,
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

        let sink: Arc<dyn Fn(&TraceEvent) + Send + Sync> = {
            let st = st.clone();
            Arc::new(move |ev: &TraceEvent| {
                let payload = json!({ "jobId": id, "event": ev });
                st.jobs.push_event(id, payload.clone());
                st.events.emit("job.trace", payload);
            })
        };
        let trace = Trace::to_sink(TraceLevel::Normal, sink, cancel.clone());
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
        st.events.emit("job.finished", json!({ "jobId": id, "state": state, "result": result }));
        let st2 = st.clone();
        std::thread::spawn(move || super::events::recompute_pending(&st2));
        super::jobs_hook_on_job_finished(&st, &kind);
    })
}

fn execute(st: &SharedState, kind: &JobKind, trace: &Trace) -> Result<Value, String> {
    match kind {
        JobKind::Compile => {
            let report = crate::reconcile::compile(&st.proj, &st.llm, &st.out, trace);
            Ok(json!(report))
        }
        JobKind::Gen { entities, force } => {
            let store = crate::store::Store::load(&st.out);
            crate::gen::run_all(&store, &st.llm, &st.gs, entities, *force, trace)
        }
        JobKind::Verify { targets, test_kind, force } => {
            let store = crate::store::Store::load(&st.out);
            crate::verify::run_all(&store, &st.llm, &st.gs, targets, test_kind.as_deref(), *force, trace)
        }
        JobKind::Audit => {
            let store = crate::store::Store::load(&st.out);
            Ok(crate::verify::audit(&store, &st.gs))
        }
    }
}

// ---- handlers ----

pub async fn post_job(State(st): State<SharedState>, Json(body): Json<Value>) -> Response {
    let kind = match body["kind"].as_str() {
        Some("compile") => JobKind::Compile,
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
        _ => return super::api::err(StatusCode::BAD_REQUEST, "kind must be compile, gen, verify, or audit"),
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
