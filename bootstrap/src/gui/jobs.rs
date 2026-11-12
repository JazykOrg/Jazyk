// The job manager: compile, generation, verification, and audit run in-process on one
// worker thread, one at a time, in submission order. Every kind contends on the store
// lock and the LLM budget, so serializing them is the point.
// Mirrors docs/frontends/gui.md#jobs.
use super::state::SharedState;
use crate::session::{Trace, TraceEvent, TraceLevel};
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

const JOB_EVENT_RING: usize = 50_000;
const TRACE_RETENTION_DAYS: u64 = 30;
// Finished jobs held in memory, events included; older ones answer from their
// transcript file (docs/frontends/gui.md#jobs).
const RETAINED_FINISHED_JOBS: usize = 50;
// The bytes the in-memory job events may hold across every retained job. Past it the
// oldest finished job gives up its events (it reads back from its transcript, which
// holds every event), then the running job its oldest ones, as the count ring does.
const JOB_EVENT_BYTES: usize = 128 * 1024 * 1024;

// Strings longer than this travel as a preview. The transcript on disk keeps the
// whole payload; a reader asks for the one event it wants to see in full
// (docs/frontends/gui.md#jobs).
const WIRE_STRING_CAP: usize = 2_000;

// One event as it goes to the browser: same shape, long strings cut to a preview
// with their byte count and an `elided` flag beside them.
pub(crate) fn elide(v: &Value) -> Value {
    match v {
        Value::String(s) if s.len() > WIRE_STRING_CAP => {
            json!(format!(
                "{}… [{} chars total]",
                crate::llm::truncate(s, WIRE_STRING_CAP),
                s.len()
            ))
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
    // The whole pipeline for one set: compile, generate, review (docs/frontends/gui.md#jobs).
    Build,
    Workflow {
        steps: Vec<crate::workflow::Step>,
        config_hash: String,
    },
    Scoped {
        set_id: String,
        config_hash: String,
        kind: Box<JobKind>,
    },
    CompileSets {
        set_ids: Vec<String>,
        config_hash: String,
    },
    Compile,
    Gen {
        entities: Vec<String>,
        force: bool,
    },
    Verify {
        targets: Vec<String>,
        test_kind: Option<String>,
        test_paths: Option<Vec<String>>,
        force: bool,
    },
    Audit,
    // Draft docs for unclaimed code under the named scopes (empty: every scope).
    Decompile {
        scopes: Vec<String>,
    },
    // Grade a model: endpoint and model override the resolved settings when given.
    Benchmark {
        base_url: Option<String>,
        model: Option<String>,
    },
}

impl JobKind {
    fn name(&self) -> &'static str {
        match self {
            JobKind::Scoped { kind, .. } => kind.name(),
            JobKind::CompileSets { .. } => "compile",
            JobKind::Build => "build",
            JobKind::Workflow { .. } => "workflow",
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
            JobKind::Scoped { set_id, kind, .. } => {
                let mut v = kind.as_value();
                v["setId"] = json!(set_id);
                v
            }
            JobKind::CompileSets { set_ids, .. } => json!({"kind":"compile", "setIds":set_ids}),
            JobKind::Build => json!({ "kind": "build" }),
            JobKind::Workflow { steps, .. } => json!({"kind":"workflow", "steps":steps}),
            JobKind::Compile => json!({ "kind": "compile" }),
            JobKind::Gen { entities, force } => {
                json!({ "kind": "gen", "entities": entities, "force": force })
            }
            JobKind::Verify {
                targets,
                test_kind,
                test_paths,
                force,
            } => {
                let mut value = json!({ "kind": "verify", "targets": targets, "testKind": test_kind, "force": force });
                if let Some(paths) = test_paths {
                    value["testPaths"] = json!(paths);
                }
                value
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
    agent_selection: Option<Arc<super::agents::Selection>>,
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
    // The selection of a selected build, widened by later requests
    // (docs/frontends/gui.md#selected-builds). Compile and workflow jobs only.
    pub selection: Option<Arc<crate::selection::Handle>>,
    pub started: Option<std::time::SystemTime>,
}

impl Job {
    fn summary(&self) -> Value {
        let mut kind = self.kind.as_value();
        if let Some(items) = self.selection.as_ref().and_then(|h| h.items()) {
            kind["selection"] = json!(items);
        }
        json!({
            "id": self.id,
            "kind": kind,
            "state": self.state,
            "cancelRequested": self.cancel.load(Ordering::Relaxed),
            "queuedAt": self.queued_at,
            "startedAt": self.started_at,
            "finishedAt": self.finished_at,
            "result": self.result,
            "stem": self.stem,
            "agentSelection": self.agent_selection.as_ref().map(|s|s.summary()),
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

impl Inner {
    // Drop finished jobs past the retention cap, oldest first. The queued and the
    // running job always stay. Mirrors docs/frontends/gui.md#jobs.
    fn prune(&mut self) {
        let finished: Vec<u64> = self
            .jobs
            .values()
            .filter(|j| j.state != "queued" && j.state != "running")
            .map(|j| j.id)
            .collect();
        let excess = finished.len().saturating_sub(RETAINED_FINISHED_JOBS);
        for id in finished.into_iter().take(excess) {
            self.jobs.remove(&id);
        }
    }
}

pub struct JobManager {
    inner: Mutex<Inner>,
    wake: Condvar,
    // Locked after `inner`, never before it.
    event_bytes: Mutex<EventBytes>,
}

// The serialized size of each in-memory event per job, their total, and the finished
// jobs whose events left memory for the byte cap.
#[derive(Default)]
struct EventBytes {
    total: usize,
    sizes: BTreeMap<u64, VecDeque<usize>>,
    trimmed: std::collections::BTreeSet<u64>,
}

impl EventBytes {
    fn drop_front(&mut self, id: u64) {
        if let Some(n) = self.sizes.get_mut(&id).and_then(VecDeque::pop_front) {
            self.total -= n;
        }
    }

    fn drop_all(&mut self, id: u64) {
        if let Some(q) = self.sizes.remove(&id) {
            self.total -= q.iter().sum::<usize>();
        }
    }
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
            event_bytes: Mutex::new(EventBytes::default()),
        }
    }

    // Queue a job. A kind already waiting in the queue dedupes to the queued job's id.
    pub fn submit(&self, st: &SharedState, kind: JobKind) -> u64 {
        self.submit_inner(st, kind, false)
            .expect("explicit submission")
    }

    pub fn submit_auto(&self, st: &SharedState, kind: JobKind) -> Option<u64> {
        self.submit_inner(st, kind, true)
    }

    fn stop_paths(st: &SharedState, kind: &JobKind) -> Vec<std::path::PathBuf> {
        let outs = match kind {
            JobKind::Scoped { set_id, .. } => vec![st.proj().root.join(".jazyk").join(set_id)],
            JobKind::CompileSets { set_ids, .. } => set_ids
                .iter()
                .map(|id| st.proj().root.join(".jazyk").join(id))
                .collect(),
            _ => vec![st.out.clone()],
        };
        outs.iter().map(|out| Self::stop_path(out)).collect()
    }

    // One set's stop fence, `local/jobs-stopped`. A fence an earlier version left at the
    // top of the set directory moves there first, so a Stop stays in effect.
    // Mirrors docs/compiler/jazyk-folder.md#files-from-earlier-versions.
    fn stop_path(out: &std::path::Path) -> std::path::PathBuf {
        let path = crate::layout::jobs_stopped_file(out);
        crate::layout::adopt_legacy(&crate::layout::legacy_jobs_stopped_file(out), &path);
        path
    }

    pub fn automatic_stopped(&self, st: &SharedState) -> bool {
        crate::vfs::exists(&Self::stop_path(&st.out))
    }

    // A GUI job's stop flag, read by the task runs it owns.
    fn job_fence(st: &SharedState, id: u64) -> std::path::PathBuf {
        crate::layout::job_cancellation_dir(&st.root.join(".jazyk")).join(id.to_string())
    }

    fn submit_inner(&self, st: &SharedState, kind: JobKind, automatic: bool) -> Option<u64> {
        let kind = if !matches!(
            kind,
            JobKind::Scoped { .. } | JobKind::CompileSets { .. } | JobKind::Benchmark { .. }
        ) {
            if let Some(id) = st.proj().set_id {
                JobKind::Scoped {
                    set_id: id,
                    config_hash: st.proj().config_hash,
                    kind: Box::new(kind),
                }
            } else {
                kind
            }
        } else {
            kind
        };
        let inner = self.inner.lock().unwrap();
        let paths = Self::stop_paths(st, &kind);
        if automatic && paths.iter().any(|p| crate::vfs::exists(p)) {
            return None;
        }
        let resumes_build = match &kind {
            JobKind::Scoped { kind, .. } => {
                matches!(kind.as_ref(), JobKind::Build | JobKind::Compile)
            }
            kind => matches!(
                kind,
                JobKind::Build | JobKind::Compile | JobKind::CompileSets { .. }
            ),
        };
        if !automatic && resumes_build {
            for p in paths {
                let _ = crate::vfs::remove_file(p);
            }
        }
        for qid in &inner.queue {
            if let Some(j) = inner.jobs.get(qid).filter(|j| {
                j.kind == kind
                    && j.agent_selection.as_ref().map(|s| &s.id)
                        == st.agent_selection.as_ref().map(|s| &s.id)
            }) {
                // A request for the whole set widens a queued selected build.
                if let Some(h) = &j.selection {
                    h.widen(None);
                }
                return Some(*qid);
            }
        }
        let selection = selectable(&kind).then(|| crate::selection::Handle::new(None));
        Some(self.enqueue(st, inner, kind, selection))
    }

    fn enqueue(
        &self,
        st: &SharedState,
        mut inner: std::sync::MutexGuard<'_, Inner>,
        kind: JobKind,
        selection: Option<Arc<crate::selection::Handle>>,
    ) -> u64 {
        inner.next_id += 1;
        let id = inner.next_id;
        let job = Job {
            agent_selection: st.agent_selection.clone(),
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
            selection,
            started: None,
        };
        let summary = job.summary();
        inner.jobs.insert(id, job);
        inner.queue.push_back(id);
        drop(inner);
        st.events.emit("job.queued", json!({ "job": summary }));
        self.wake.notify_all();
        id
    }

    // Submit a selected build: widen a queued or running job of the same kind when
    // it can take the items, else queue a new one. Returns the job id and whether an
    // existing job took the request. Mirrors docs/frontends/gui.md#selected-builds.
    pub fn submit_selected(
        &self,
        st: &SharedState,
        kind: JobKind,
        items: Vec<crate::selection::Item>,
    ) -> (u64, bool) {
        let kind = match st.proj().set_id {
            Some(id) => JobKind::Scoped {
                set_id: id,
                config_hash: st.proj().config_hash,
                kind: Box::new(kind),
            },
            None => kind,
        };
        let inner = self.inner.lock().unwrap();
        if matches!(&kind, JobKind::Scoped { kind, .. } if matches!(kind.as_ref(), JobKind::Compile))
        {
            for p in Self::stop_paths(st, &kind) {
                let _ = crate::vfs::remove_file(p);
            }
        }
        for qid in &inner.queue {
            if let Some(j) = inner.jobs.get(qid).filter(|j| {
                j.kind == kind
                    && j.agent_selection.as_ref().map(|s| &s.id)
                        == st.agent_selection.as_ref().map(|s| &s.id)
            }) {
                if let Some(h) = &j.selection {
                    h.widen(Some(items.clone()));
                }
                return (*qid, true);
            }
        }
        if let Some(j) = inner
            .running
            .and_then(|rid| inner.jobs.get(&rid))
            .filter(|j| {
                j.kind == kind
                    && j.agent_selection.as_ref().map(|s| &s.id)
                        == st.agent_selection.as_ref().map(|s| &s.id)
                    && !j.cancel.load(Ordering::Relaxed)
            })
        {
            if let (Some(h), Some(started)) = (&j.selection, j.started) {
                if !saved_since(&st.proj(), &items, started)
                    && h.widen(Some(items.clone())) != crate::selection::Widen::Sealed
                {
                    return (j.id, true);
                }
            }
        }
        let handle = crate::selection::Handle::new(Some(items));
        (self.enqueue(st, inner, kind, Some(handle)), false)
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

    // Fence automatic work and queued follow-ups before cancelling the active run.
    pub fn cancel(&self, st: &SharedState, id: u64) -> Option<Value> {
        let mut inner = self.inner.lock().unwrap();
        let job = inner.jobs.get(&id)?;
        if !matches!(job.state, "running" | "queued") {
            return Some(job.summary());
        }
        let paths = Self::stop_paths(st, &job.kind);
        for p in &paths {
            crate::vfs::create_dir_all(p.parent().unwrap()).ok()?;
            crate::vfs::write(p, b"stopped").ok()?;
        }
        let ids: Vec<u64> = inner
            .jobs
            .values()
            .filter(|j| {
                j.id == id
                    || (j.state == "queued"
                        && Self::stop_paths(st, &j.kind)
                            .iter()
                            .any(|p| paths.contains(p)))
            })
            .map(|j| j.id)
            .collect();
        let mut finished = Vec::new();
        for stopped in &ids {
            let job = inner.jobs.get_mut(stopped).unwrap();
            job.cancel.store(true, Ordering::Relaxed);
            let fence = Self::job_fence(st, *stopped);
            let _ = crate::vfs::create_dir_all(fence.parent().unwrap());
            let _ = crate::vfs::write(&fence, b"cancelled");
            if job.state == "queued" {
                job.state = "cancelled";
                job.finished_at = Some(crate::verify::now_iso());
                finished.push(*stopped);
            }
        }
        inner.queue.retain(|qid| !ids.contains(qid));
        let summary = inner.jobs[&id].summary();
        inner.prune();
        drop(inner);
        for id in finished {
            st.events.emit(
                "job.finished",
                json!({ "jobId": id, "state": "cancelled", "result": Value::Null }),
            );
        }
        Some(summary)
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
        self.push_event_within(id, ev, JOB_EVENT_BYTES);
    }

    fn push_event_within(&self, id: u64, ev: Value, cap: usize) {
        let size = ev.to_string().len();
        let mut inner = self.inner.lock().unwrap();
        let mut bytes = self.event_bytes.lock().unwrap();
        // Jobs pruned from memory leave the account.
        let gone: Vec<u64> = bytes
            .sizes
            .keys()
            .filter(|k| !inner.jobs.contains_key(k))
            .copied()
            .collect();
        for k in gone {
            bytes.drop_all(k);
        }
        bytes.trimmed.retain(|k| inner.jobs.contains_key(k));
        let Some(j) = inner.jobs.get_mut(&id) else {
            return;
        };
        j.events.push_back(ev);
        bytes.sizes.entry(id).or_default().push_back(size);
        bytes.total += size;
        while j.events.len() > JOB_EVENT_RING {
            j.events.pop_front();
            bytes.drop_front(id);
        }
        while bytes.total > cap {
            let finished = inner
                .jobs
                .values()
                .find(|j| !matches!(j.state, "queued" | "running") && !j.events.is_empty())
                .map(|j| j.id);
            match finished {
                Some(old) => {
                    if let Some(j) = inner.jobs.get_mut(&old) {
                        j.events.clear();
                    }
                    bytes.drop_all(old);
                    bytes.trimmed.insert(old);
                }
                None => {
                    let Some(j) = inner.jobs.get_mut(&id).filter(|j| j.events.len() > 1) else {
                        break;
                    };
                    j.events.pop_front();
                    bytes.drop_front(id);
                }
            }
        }
    }

    // Whether a finished job's events left memory for the byte cap: its transcript
    // answers for it (docs/frontends/gui.md#jobs).
    fn events_trimmed(&self, id: u64) -> bool {
        self.event_bytes.lock().unwrap().trimmed.contains(&id)
    }
}

// The single worker thread. Returns the join handle so shutdown can wait for a
// cancelled job to discard its active session and release the store lock.
pub fn spawn_worker(st: SharedState) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        let (id, kind, cancel, selection, agent_selection) = {
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
                    job.started = Some(std::time::SystemTime::now());
                    break (
                        id,
                        job.kind.clone(),
                        job.cancel.clone(),
                        job.selection.clone(),
                        job.agent_selection.clone(),
                    );
                }
                inner = jm.wake.wait(inner).unwrap();
            }
        };
        st.events
            .emit("job.started", json!({ "jobId": id, "kind": kind.name() }));

        // The transcript: one JSON-lines file per job under <out>/trace/, flushed per
        // line so a tail (or the API) always sees a parseable prefix.
        let mut st = st
            .scoped(st.proj().set_id.as_deref())
            .expect("worker scope");
        Arc::get_mut(&mut st).unwrap().agent_selection = agent_selection;
        // Mirrors docs/frontends/gui.md#jobs.
        let stem = {
            let started = crate::verify::now_iso();
            let compact: String = started
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect();
            format!("{}-{}-j{}", compact, kind.name(), id)
        };
        st.jobs.set_stem(id, &stem);
        let trace_path = crate::layout::trace_dir(&st.out).join(format!("{}.jsonl", stem));
        crate::vfs::create_dir_all(crate::layout::trace_dir(&st.out)).ok();
        let writer: Arc<Mutex<Option<std::fs::File>>> = Arc::new(Mutex::new(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&trace_path)
                .ok(),
        ));
        // The generation at start and finish brackets the run: the journal entries
        // between the two are the run's changesets (gui.md#jobs).
        write_line(
            &writer,
            &json!({ "meta": {
                "id": id, "kind": kind.as_value(), "queuedAt": st.jobs.get(id).map(|j| j["queuedAt"].clone()),
                "startedAt": crate::verify::now_iso(),
                "generation": crate::store::read_generation(&st.out),
            }}),
        );

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
        // A panic inside the run fails this job and leaves the worker standing;
        // otherwise the thread would die with the job marked running forever.
        // Mirrors docs/frontends/gui.md#jobs.
        let fence = JobManager::job_fence(&st, id);
        let _ = crate::vfs::remove_file(&fence);
        let _runtime_owner = crate::task_runtime::CancellationScope::enter(Some(&fence));
        let _selection = selection.clone().map(crate::selection::enter);
        let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            execute(&st, &kind, &trace)
        })) {
            Ok(r) => r,
            Err(p) => Err(format!(
                "the job panicked: {}",
                crate::sys::panic_text(p.as_ref())
            )),
        };

        let (state, result) = match result {
            result if cancel.load(Ordering::Relaxed) => (
                "cancelled",
                Some(match result {
                    Ok(v) => v,
                    Err(e) => json!({"error": e}),
                }),
            ),
            Ok(v) if v["state"] == "failed" => ("failed", Some(v)),
            Ok(v) => ("done", Some(v)),
            Err(e) => ("failed", Some(json!({ "error": e }))),
        };
        if let Some(h) = &selection {
            h.seal();
        }
        {
            let mut inner = st.jobs.inner.lock().unwrap();
            inner.running = None;
            let job = inner.jobs.get_mut(&id).unwrap();
            job.state = state;
            job.finished_at = Some(crate::verify::now_iso());
            job.result = result.clone();
            inner.prune();
        }
        write_line(
            &writer,
            &json!({ "outcome": {
                "state": state, "result": result, "finishedAt": crate::verify::now_iso(),
                "generation": crate::store::read_generation(&st.out),
            }}),
        );
        st.events.emit(
            "job.finished",
            json!({ "jobId": id, "state": state, "result": result }),
        );
        let st2 = st.clone();
        std::thread::spawn(move || super::events::recompute_pending(&st2));
        if state == "done" {
            super::jobs_hook_on_job_finished(&st, &kind);
        }
    })
}

fn execute(st: &SharedState, kind: &JobKind, trace: &Trace) -> Result<Value, String> {
    match kind {
        JobKind::Scoped {
            set_id,
            config_hash,
            kind,
        } => {
            let scope = st.scoped(Some(set_id)).map_err(|e| e.1)?;
            if scope.proj().config_hash != *config_hash {
                return Err(
                    "document-set configuration changed while this job was queued; compile again"
                        .into(),
                );
            }
            let result = execute(&scope, kind, trace);
            if !trace.is_cancelled() {
                super::jobs_hook_on_job_finished(&scope, kind);
            }
            result
        }
        JobKind::CompileSets {
            set_ids,
            config_hash,
        } => {
            let mut results = Vec::new();
            for id in set_ids {
                if trace.is_cancelled() {
                    break;
                }
                let kind = JobKind::Scoped {
                    set_id: id.clone(),
                    config_hash: config_hash.clone(),
                    kind: Box::new(JobKind::Compile),
                };
                st.events.emit("compile.set.started", json!({"setId":id}));
                trace.line("compile", &format!("document set {}", id));
                let result = match execute(st, &kind, trace) {
                    Ok(v) => v,
                    Err(e) => json!({"error":e}),
                };
                st.events
                    .emit("compile.set.finished", json!({"setId":id,"result":result}));
                results.push(json!({"setId":id,"result":result}));
            }
            Ok(json!({"sets":results}))
        }
        JobKind::Workflow { steps, config_hash } => {
            let workspace = crate::document_sets::Workspace::load(&st.root).map_err(|e| e.1)?;
            if &workspace.hash != config_hash {
                return Err("Project settings changed while the workflow was queued. Review and start it again.".into());
            }
            crate::workflow::run(&st.proj(), &st.llm(), steps, trace)
        }
        JobKind::Build => {
            let mut steps = Vec::new();
            trace.line("build", "compile");
            steps
                .push(json!({"step": "compile", "result": execute(st, &JobKind::Compile, trace)?}));
            let store = crate::store::Store::load(&st.out);
            if !trace.is_cancelled() && !crate::gen::pending(&store, &st.gs()).is_empty() {
                trace.line("build", "generate");
                let gen = JobKind::Gen {
                    entities: vec![],
                    force: false,
                };
                steps.push(json!({"step": "generate", "result": execute(st, &gen, trace)?}));
            }
            if !trace.is_cancelled() && !crate::gen::Ledger::load(&st.out).requirements.is_empty() {
                trace.line("build", "review");
                let verify = JobKind::Verify {
                    targets: vec![],
                    test_kind: None,
                    test_paths: None,
                    force: false,
                };
                steps.push(json!({"step": "review", "result": execute(st, &verify, trace)?}));
            }
            Ok(json!({ "steps": steps }))
        }
        JobKind::Compile => {
            let report = crate::reconcile::compile(&st.proj(), &st.llm(), &st.out, trace);
            Ok(json!(report))
        }
        JobKind::Gen { entities, force } => {
            // The clicked gen is an approval and holds the build lease, same as the CLI.
            let _build = crate::control::begin_internal_build(&st.proj(), &st.out, "generate")?;
            let runner = crate::acp::runner::AcpRunner::start_selected(
                &st.proj(),
                &st.llm(),
                &st.out,
                st.agent_selection.as_ref().map(|s| s.agent.name.as_str()),
            )?;
            runner.set_build_token(Some(_build.token().to_owned()));
            let store = crate::store::Store::load(&st.out);
            crate::gen::run_all(&store, &runner, &st.gs(), entities, *force, trace)
        }
        JobKind::Verify {
            targets,
            test_kind,
            test_paths,
            force,
        } => {
            if let Some(paths) = test_paths {
                return crate::verify::artifacts::run_workspace(
                    &st.root,
                    st.proj().set_id.as_deref(),
                    paths,
                    trace,
                    |project| {
                        Ok(Box::new(crate::acp::runner::AcpRunner::start(
                            project,
                            &st.llm(),
                            &project.out,
                        )?))
                    },
                );
            }
            let runner = crate::acp::runner::AcpRunner::start_selected(
                &st.proj(),
                &st.llm(),
                &st.out,
                st.agent_selection.as_ref().map(|s| s.agent.name.as_str()),
            )?;
            let store = crate::store::Store::load(&st.out);
            crate::verify::run_all(
                &store,
                &runner,
                &st.gs(),
                targets,
                test_kind.as_deref(),
                *force,
                trace,
            )
        }
        JobKind::Audit => {
            let store = crate::store::Store::load(&st.out);
            Ok(crate::verify::audit(&store, &st.gs()))
        }
        JobKind::Decompile { scopes } => {
            let runner = crate::acp::runner::AcpRunner::start_selected(
                &st.proj(),
                &st.llm(),
                &st.out,
                st.agent_selection.as_ref().map(|s| s.agent.name.as_str()),
            )?;
            let store = crate::store::Store::load(&st.out);
            crate::control::release_decompile(&st.proj(), &st.out, scopes);
            crate::decompile::run_all(&st.proj(), &store, &runner, &st.gs(), scopes, trace)
        }
        JobKind::Benchmark { base_url, model } => {
            let mut llm = st.llm();
            if let Some(b) = base_url {
                // The configured key rides only to the configured endpoint.
                if !crate::llm::same_endpoint(b, &llm.base_url) {
                    llm.api_key.clear();
                }
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

// Submission reads the workspace summary (every set's store and board) and the
// control plane: off the runtime, like every other heavy read.
pub async fn post_job(State(st): State<SharedState>, Json(body): Json<Value>) -> Response {
    tokio::task::spawn_blocking(move || submit_sync(&st, body))
        .await
        .expect("job submission")
}

fn submit_sync(st: &SharedState, body: Value) -> Response {
    let selection = match crate::selection::parse(&body["selection"]) {
        Ok(s) => s,
        Err(e) => return super::api::err(StatusCode::BAD_REQUEST, e),
    };
    if let Some(items) = &selection {
        if body["kind"] != "compile" && body["kind"] != "workflow" {
            return super::api::err(
                StatusCode::BAD_REQUEST,
                "a selection narrows compile and workflow jobs only",
            );
        }
        if st.proj().set_id.is_none() {
            return super::api::err(
                StatusCode::BAD_REQUEST,
                "a selection needs a document set: send setId",
            );
        }
        if let Err(e) = crate::selection::validate(&st.proj(), items) {
            return super::api::err(StatusCode::BAD_REQUEST, e);
        }
        let c = st.control();
        if c.worker == "agent" {
            return super::api::err(
                StatusCode::CONFLICT,
                "a selected build runs on the internal worker; set worker to any or internal",
            );
        }
    }
    if body["kind"] == "compile" && st.proj().set_id.is_none() {
        let w = match crate::document_sets::Workspace::load(&st.root) {
            Ok(w) => w,
            Err((code, e)) => return super::api::err(StatusCode::from_u16(code).unwrap(), e),
        };
        let ids = if let Some(ids) = body["setIds"].as_array() {
            let mut selected = Vec::new();
            for id in ids {
                match id.as_str().and_then(|id| w.select(id).ok()) {
                    Some(s) => selected.push(s.id.clone()),
                    None => {
                        return super::api::err(StatusCode::BAD_REQUEST, "unknown document set")
                    }
                }
            }
            w.sets
                .iter()
                .filter(|s| selected.contains(&s.id))
                .map(|s| s.id.clone())
                .collect()
        } else {
            w.summary()["sets"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s["dirty"] == true && s["state"] != "empty")
                .filter_map(|s| s["id"].as_str().map(str::to_string))
                .collect()
        };
        let id = st.jobs.submit(
            st,
            JobKind::CompileSets {
                set_ids: ids,
                config_hash: w.hash,
            },
        );
        return (StatusCode::ACCEPTED, Json(json!({"jobId":id}))).into_response();
    }
    let tasks_only = body["kind"] == "workflow"
        && crate::workflow::parse(&body["steps"]).is_ok_and(|steps| {
            steps
                .iter()
                .all(|s| matches!(s, crate::workflow::Step::Task { .. }))
        });
    if st.proj().set_id.is_none()
        && body["kind"] != "benchmark"
        && !tasks_only
        && !(body["kind"] == "verify" && body.get("testPaths").is_some())
    {
        return super::api::err(
            StatusCode::BAD_REQUEST,
            "select a document set for this job",
        );
    }
    // Dispatch by worker preference: with an agent attached and preferred, the click
    // records the release and the agent does the work. Mirrors
    // docs/compiler/control-plane.md#dispatch.
    if let Some(stage) = match body["kind"].as_str() {
        _ if selection.is_some() => None,
        Some("compile") | Some("build") => Some("compile"),
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
                        scopes = crate::decompile::scopes(&st.proj(), &store, &st.gs())
                            .into_keys()
                            .collect();
                    }
                    crate::control::release_decompile(&st.proj(), &st.out, &scopes);
                } else {
                    crate::control::release(&st.proj(), &st.out, Some(stage));
                }
                let snap = super::api::workers_snapshot(st);
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
        Some("workflow") => match crate::workflow::parse(&body["steps"]) {
            Ok(steps) => match crate::document_sets::Workspace::load(&st.root) {
                Ok(w) => JobKind::Workflow {
                    steps,
                    config_hash: w.hash,
                },
                Err((_, e)) => return super::api::err(StatusCode::BAD_REQUEST, e),
            },
            Err(e) => return super::api::err(StatusCode::BAD_REQUEST, e),
        },
        Some("build") => JobKind::Build,
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
            test_paths: match crate::verify::artifacts::parse_paths(&body) {
                Ok(paths) => paths,
                Err(error) => return super::api::err(StatusCode::BAD_REQUEST, error),
            },
            force: body["force"].as_bool().unwrap_or(false),
        },
        Some("audit") => JobKind::Audit,
        Some("decompile") => JobKind::Decompile {
            scopes: str_list(&body["scopes"]),
        },
        _ => {
            return super::api::err(
                StatusCode::BAD_REQUEST,
                "kind must be build, compile, gen, verify, audit, decompile, or benchmark",
            )
        }
    };
    if let Some(items) = selection {
        let (id, steered) = st.jobs.submit_selected(st, kind, items);
        let mut reply = json!({ "jobId": id });
        if steered {
            reply["steered"] = json!(true);
        }
        return (StatusCode::ACCEPTED, Json(reply)).into_response();
    }
    let id = st.jobs.submit(st, kind);
    (StatusCode::ACCEPTED, Json(json!({ "jobId": id }))).into_response()
}

// The kinds a selection narrows: compile and workflow, in a set.
fn selectable(kind: &JobKind) -> bool {
    match kind {
        JobKind::Scoped { kind, .. } => {
            matches!(kind.as_ref(), JobKind::Compile | JobKind::Workflow { .. })
        }
        _ => false,
    }
}

// Whether a selected document was saved after a running build started: the build
// reads the documents as they were then, so the request needs its own build.
fn saved_since(
    proj: &crate::project::Project,
    items: &[crate::selection::Item],
    started: std::time::SystemTime,
) -> bool {
    proj.doc_files().into_iter().any(|f| {
        let rel = f
            .strip_prefix(&proj.root)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        items
            .iter()
            .any(|i| rel == i.path || rel.starts_with(&format!("{}/", i.path)))
            && std::fs::metadata(&f)
                .and_then(|m| m.modified())
                .is_ok_and(|m| m >= started)
    })
}

fn str_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

pub async fn list_jobs(State(st): State<SharedState>) -> Json<Value> {
    Json(json!({ "jobs": st.jobs.list(), "running": st.jobs.running_job() }))
}

pub async fn get_job(State(st): State<SharedState>, UrlPath(id): UrlPath<u64>) -> Response {
    let in_memory = match st.jobs.get(id) {
        Some(v) if !st.jobs.events_trimmed(id) => return Json(v).into_response(),
        other => other,
    };
    // Out of memory: the transcript holds the whole job (docs/frontends/gui.md#jobs).
    let dir = crate::layout::trace_dir(&st.out);
    let from_file = tokio::task::spawn_blocking(move || job_from_transcript(&dir, id))
        .await
        .expect("job transcript read");
    match from_file.or(in_memory) {
        Some(v) => Json(v).into_response(),
        None => super::api::err(StatusCode::NOT_FOUND, format!("no job {}", id)),
    }
}

// Rebuild a job's API shape from its transcript: the newest file whose stem ends in
// `-j<id>` (ids restart with the process, so an older server's file can share one).
// The metadata line gives the kind and the times, the outcome line the state and
// result; a transcript without an outcome died mid-run and reads as failed.
fn job_from_transcript(dir: &std::path::Path, id: u64) -> Option<Value> {
    let suffix = format!("-j{}", id);
    let mut candidates: Vec<(String, String)> = Vec::new();
    for e in crate::vfs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
            continue;
        }
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if !stem.ends_with(&suffix) {
            continue;
        }
        if let Ok(text) = crate::vfs::read_to_string(&p) {
            candidates.push((stem.to_string(), text));
        }
    }
    let started = |text: &str| -> String {
        text.lines()
            .next()
            .and_then(|l| serde_json::from_str::<Value>(l).ok())
            .and_then(|m| m["meta"]["startedAt"].as_str().map(str::to_string))
            .unwrap_or_default()
    };
    candidates.sort_by_key(|(_, text)| std::cmp::Reverse(started(text)));
    let (stem, text) = candidates.into_iter().next()?;
    let mut meta = Value::Null;
    let mut outcome = Value::Null;
    let mut events: Vec<Value> = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if !v["meta"].is_null() {
            meta = v["meta"].clone();
        } else if !v["outcome"].is_null() {
            outcome = v["outcome"].clone();
        } else if v["n"].is_u64() {
            events.push(json!({ "jobId": id, "n": v["n"], "event": elide(&v["event"]) }));
        }
    }
    if meta.is_null() {
        return None;
    }
    let (state, result) = if outcome.is_null() {
        (
            json!("failed"),
            json!({ "error": "the job ended without an outcome" }),
        )
    } else {
        (outcome["state"].clone(), outcome["result"].clone())
    };
    Some(json!({
        "id": id,
        "kind": meta["kind"],
        "state": state,
        "queuedAt": meta["queuedAt"],
        "startedAt": meta["startedAt"],
        "finishedAt": outcome["finishedAt"],
        "result": result,
        "stem": stem,
        "events": events,
    }))
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
    let dir = crate::layout::trace_dir(out);
    let Ok(rd) = crate::vfs::read_dir(&dir) else {
        return;
    };
    let cutoff = std::time::SystemTime::now()
        - std::time::Duration::from_secs(TRACE_RETENTION_DAYS * 24 * 3600);
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
            continue;
        }
        if let Ok(md) = e.metadata() {
            if md.modified().map(|m| m < cutoff).unwrap_or(false) {
                let _ = crate::vfs::remove_file(&p);
            }
        }
    }
}

// ---- transcript endpoints ----

// Past jobs from the transcripts on disk, newest first: the meta line plus the
// outcome line when the job finished (a missing outcome means it died mid-run).
// Newest is by the start the metadata line records; the file's mtime only breaks
// ties, since a copied or restored out directory gives every transcript one mtime.
// Only the first line of every transcript is read to order them; the newest 200
// are read whole for their outcome and event count.
pub async fn list_traces(State(st): State<SharedState>) -> Json<Value> {
    let dir = crate::layout::trace_dir(&st.out);
    let list = tokio::task::spawn_blocking(move || {
        let first_line = |p: &std::path::Path| -> Option<String> {
            use std::io::BufRead;
            let mut line = String::new();
            std::io::BufReader::new(std::fs::File::open(p).ok()?)
                .read_line(&mut line)
                .ok()?;
            let line = line.strip_suffix('\n').unwrap_or(&line);
            Some(line.strip_suffix('\r').unwrap_or(line).to_string())
        };
        let mut candidates: Vec<((String, std::time::SystemTime), std::path::PathBuf, Value)> =
            Vec::new();
        let Ok(rd) = crate::vfs::read_dir(&dir) else {
            return Vec::new();
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(meta) = first_line(&p).and_then(|l| serde_json::from_str::<Value>(&l).ok())
            else {
                continue;
            };
            let mtime = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            let started = meta["meta"]["startedAt"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            candidates.push(((started, mtime), p, meta));
        }
        candidates.sort_by(|a, b| b.0.cmp(&a.0));
        let mut items: Vec<Value> = Vec::new();
        for (_, p, meta) in candidates {
            if items.len() == 200 {
                break;
            }
            let Ok(text) = crate::vfs::read_to_string(&p) else {
                continue;
            };
            let outcome = text
                .lines()
                .last()
                .and_then(|l| serde_json::from_str::<Value>(l).ok())
                .filter(|v| !v["outcome"].is_null());
            let events = text
                .lines()
                .count()
                .saturating_sub(1 + outcome.is_some() as usize);
            let stem = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            items.push(serde_json::json!({
                "stem": stem,
                "meta": meta["meta"],
                "outcome": outcome.map(|o| o["outcome"].clone()),
                "events": events,
            }));
        }
        items
    })
    .await
    .expect("trace list");
    Json(serde_json::json!({ "traces": list }))
}

fn trace_path(st: &SharedState, stem: &str) -> Option<std::path::PathBuf> {
    if stem.is_empty()
        || !stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(crate::layout::trace_dir(&st.out).join(format!("{}.jsonl", stem)))
}

pub async fn get_trace(State(st): State<SharedState>, UrlPath(stem): UrlPath<String>) -> Response {
    let Some(path) = trace_path(&st, &stem) else {
        return super::api::err(StatusCode::BAD_REQUEST, "invalid trace name");
    };
    let result = tokio::task::spawn_blocking(move || -> Option<Value> {
        let text = crate::vfs::read_to_string(&path).ok()?;
        let mut meta = Value::Null;
        let mut outcome = Value::Null;
        let mut events: Vec<Value> = Vec::new();
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
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
        let text = crate::vfs::read_to_string(&path).ok()?;
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

    // A Stop fence lives in the set's `local/`; one an earlier version left at the top
    // of the set directory moves there and stays in effect.
    // Mirrors docs/compiler/jazyk-folder.md#files-from-earlier-versions.
    #[test]
    fn the_stop_fence_lives_under_local_and_an_earlier_one_moves_there() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join(".jazyk/my-set-ks8mf");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("jobs-stopped"), "stopped").unwrap();
        let path = JobManager::stop_path(&out);
        assert_eq!(path, out.join("local/jobs-stopped"));
        assert!(path.exists(), "an earlier Stop stays in effect");
        assert!(!out.join("jobs-stopped").exists());
        std::fs::remove_file(&path).unwrap();
        assert!(!JobManager::stop_path(&out).exists());
    }

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

    #[tokio::test]
    async fn large_tool_payload_survives_transcript_and_expanded_event_fetch() {
        let dir = tempfile::tempdir().unwrap();
        let project = crate::project::Project::load(dir.path());
        let state = Arc::new(super::super::state::AppState {
            agent_choices: Arc::new(super::super::agents::Choices::default()),
            agent_selection: None,
            root: dir.path().into(),
            out: dir.path().join(".jazyk"),
            gs: std::sync::RwLock::new(crate::gen::GenSettings::resolve(&project)),
            proj: std::sync::RwLock::new(project),
            llm: std::sync::RwLock::new(crate::llm::Llm {
                api: "chat_completions".into(),
                reasoning_summary: None,
                base_url: String::new(),
                model: String::new(),
                api_key: String::new(),
                temperature: None,
                trace: None,
            }),
            cli_opts: Default::default(),
            token: None,
            gui_origin: "https://jazyk.dev".into(),
            dev: false,
            shutdown: Default::default(),
            events: super::super::events::EventHub::new(),
            board: Default::default(),
            last_pending: Default::default(),
            jobs: Arc::new(JobManager::new()),
            chat: Default::default(),
            backoff: Default::default(),
        });
        let context = json!({
            "task": {"id":"application-suite","command":"npm test","reports":["reports/junit.xml"]},
            "baseline": {"state":"completed","stdout":format!("{}END OF BASELINE", "test \"quoted\" → passed\n".repeat(700))}
        });
        let payload =
            json!({"content":[{"type":"text","text":context.to_string()}],"isError":false});
        assert!(payload.to_string().len() > 8_000);
        let captured = Arc::new(Mutex::new(Vec::new()));
        let sink = captured.clone();
        let trace = Trace::to_sink(
            TraceLevel::Normal,
            Arc::new(move |event| {
                sink.lock().unwrap().push(json!(event));
            }),
            Default::default(),
        )
        .with_transcript(&state.out, "audit");
        let stem = trace.run().unwrap();
        trace.event(TraceEvent::ToolResult {
            label: "generate application".into(),
            activity: "task-1".into(),
            name: "task_context".into(),
            summary: crate::session::condense(&payload, 160),
            full: crate::session::full_payload(&payload),
        });

        let stored = std::fs::read_to_string(
            crate::layout::trace_dir(&state.out).join(format!("{stem}.jsonl")),
        )
        .unwrap();
        let event: Value = serde_json::from_str(stored.lines().nth(1).unwrap()).unwrap();
        let full = event["event"]["full"].as_str().unwrap();
        assert_eq!(serde_json::from_str::<Value>(full).unwrap(), payload);
        assert_eq!(captured.lock().unwrap()[0], event["event"]);

        let listed = get_trace(State(state.clone()), UrlPath(stem.clone())).await;
        assert_eq!(listed.status(), StatusCode::OK);
        let listed: Value = serde_json::from_slice(
            &axum::body::to_bytes(listed.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        let wire = &listed["events"][0]["event"];
        assert_eq!(wire["elided"], true);
        assert!(wire["full"].as_str().unwrap().chars().count() < WIRE_STRING_CAP + 100);
        assert_eq!(wire, &elide(&event["event"]));

        let expanded = get_trace_event(State(state), UrlPath((stem, 1))).await;
        assert_eq!(expanded.status(), StatusCode::OK);
        let expanded: Value = serde_json::from_slice(
            &axum::body::to_bytes(expanded.into_body(), usize::MAX)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(expanded, event);
        assert_eq!(
            serde_json::from_str::<Value>(expanded["event"]["full"].as_str().unwrap()).unwrap(),
            payload
        );
    }

    fn finished(id: u64, state: &'static str) -> Job {
        Job {
            agent_selection: None,
            id,
            kind: JobKind::Audit,
            state,
            queued_at: String::new(),
            started_at: None,
            finished_at: None,
            result: None,
            events: VecDeque::from([json!({"n": 1})]),
            cancel: Arc::new(AtomicBool::new(false)),
            stem: String::new(),
            selection: None,
            started: None,
        }
    }

    // Past the byte bound the oldest finished job gives up its events first and is
    // marked to answer from its transcript; the running job then drops its oldest
    // events, never its newest. Mirrors docs/frontends/gui.md#jobs.
    #[test]
    fn job_events_stay_within_the_byte_bound() {
        let jobs = JobManager::new();
        {
            let mut inner = jobs.inner.lock().unwrap();
            for id in [1, 2] {
                let mut j = finished(id, "done");
                j.events.clear();
                inner.jobs.insert(id, j);
            }
            let mut j = finished(3, "running");
            j.events.clear();
            inner.jobs.insert(3, j);
        }
        let ev = |n: u64| json!({"n": n, "pad": "x".repeat(90)});
        let size = ev(1).to_string().len();
        let cap = size * 4;
        jobs.push_event_within(1, ev(1), cap);
        jobs.push_event_within(2, ev(1), cap);
        jobs.push_event_within(3, ev(1), cap);
        jobs.push_event_within(3, ev(2), cap);
        assert!(!jobs.events_trimmed(1) && !jobs.events_trimmed(2));
        jobs.push_event_within(3, ev(3), cap);
        assert!(jobs.events_trimmed(1), "the oldest finished job goes first");
        assert!(!jobs.events_trimmed(2));
        assert_eq!(jobs.get(1).unwrap()["events"], json!([]));
        jobs.push_event_within(3, ev(4), cap);
        jobs.push_event_within(3, ev(5), cap);
        assert!(jobs.events_trimmed(2));
        let running = jobs.get(3).unwrap();
        let ns: Vec<u64> = running["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["n"].as_u64().unwrap())
            .collect();
        assert_eq!(ns, vec![2, 3, 4, 5]);
        assert!(jobs.event_bytes.lock().unwrap().total <= cap);
        jobs.inner.lock().unwrap().jobs.remove(&1);
        jobs.push_event_within(3, ev(6), cap);
        assert!(!jobs.events_trimmed(1), "a pruned job leaves the account");
    }

    #[test]
    fn pruning_keeps_the_latest_finished_jobs_and_every_live_one() {
        let mut inner = Inner {
            jobs: BTreeMap::new(),
            queue: VecDeque::new(),
            next_id: 0,
            running: None,
            stop: false,
        };
        for id in 1..=(RETAINED_FINISHED_JOBS as u64 + 10) {
            inner.jobs.insert(id, finished(id, "done"));
        }
        inner.jobs.insert(1000, finished(1000, "running"));
        inner.jobs.insert(1001, finished(1001, "queued"));
        inner.prune();
        assert_eq!(inner.jobs.len(), RETAINED_FINISHED_JOBS + 2);
        assert!(!inner.jobs.contains_key(&1));
        assert!(!inner.jobs.contains_key(&10));
        assert!(inner.jobs.contains_key(&11));
        assert!(inner.jobs.contains_key(&1000) && inner.jobs.contains_key(&1001));
    }

    #[test]
    fn a_pruned_job_reads_back_from_its_transcript() {
        let dir = std::env::temp_dir().join(format!("jz-jobs-transcript-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let long = "y".repeat(WIRE_STRING_CAP + 10);
        std::fs::write(
            dir.join("20260101T000000Z-audit-j7.jsonl"),
            format!(
                "{}\n{}\n{}\n{}\n",
                json!({"meta": {"id": 7, "kind": {"kind": "audit"}, "queuedAt": "q", "startedAt": "s", "generation": 3}}),
                json!({"n": 1, "event": {"kind": "note", "text": "a"}}),
                json!({"n": 2, "event": {"kind": "note", "text": long}}),
                json!({"outcome": {"state": "done", "result": {"ok": true}, "finishedAt": "f", "generation": 4}}),
            ),
        )
        .unwrap();
        // A stale file from an earlier server shares the id but started earlier.
        std::fs::write(
            dir.join("20250101T000000Z-compile-j7.jsonl"),
            format!(
                "{}\n",
                json!({"meta": {"id": 7, "kind": {"kind": "compile"}, "startedAt": "0"}})
            ),
        )
        .unwrap();
        let v = job_from_transcript(&dir, 7).unwrap();
        assert_eq!(v["id"], 7);
        assert_eq!(v["kind"]["kind"], "audit");
        assert_eq!(v["state"], "done");
        assert_eq!(v["result"]["ok"], true);
        assert_eq!(v["finishedAt"], "f");
        assert_eq!(v["stem"], "20260101T000000Z-audit-j7");
        let events = v["events"].as_array().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["jobId"], 7);
        assert_eq!(events[0]["n"], 1);
        assert_eq!(events[1]["event"]["elided"], true);
        // Without an outcome the job died mid-run.
        std::fs::write(
            dir.join("20260101T000000Z-audit-j8.jsonl"),
            format!(
                "{}\n",
                json!({"meta": {"id": 8, "kind": {"kind": "audit"}, "startedAt": "s"}})
            ),
        )
        .unwrap();
        assert_eq!(job_from_transcript(&dir, 8).unwrap()["state"], "failed");
        assert!(job_from_transcript(&dir, 9).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
