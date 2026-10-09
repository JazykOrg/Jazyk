// The control plane: workflow modes and releases (control.yaml), the worker registry
// (workers/), and task leases (leases/), all in the out directory so every consumer
// reads the same intent the same way the queue is the same everywhere.
// Mirrors docs/compiler/control-plane.md.
use crate::project::Project;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn now() -> u64 {
    crate::sys::now_unix()
}

// A worker file older than this (seconds since its heartbeat) is stale and swept.
const WORKER_STALE_SECS: u64 = 90;
// A lease lives this long unless refreshed by activity on the open task.
pub const LEASE_TTL_SECS: u64 = 120;
// The internal loop's coarse claim: one lease for a whole run.
pub const BUILD_LEASE: &str = "build";

// ---------------------------------------------------------------------------
// control.yaml: modes and releases.

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Control {
    // auto | manual, defaulted from [workflow] in jazyk.toml when the file is absent.
    pub compile: String,
    pub generate: String,
    // internal | agent | any: who acts on a GUI release.
    pub worker: String,
    pub released: Released,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Released {
    // Document -> content hash approved for reconciliation.
    pub compile: BTreeMap<String, String>,
    // Graph generation approved for generation and binding work.
    pub generate: u64,
    // Scopes approved for decompilation. A submitted draft covering a scope consumes
    // it; there is no auto mode. Mirrors docs/consumers/decompile.md#triggering.
    pub decompile: Vec<String>,
}

impl Control {
    pub fn path(out: &Path) -> PathBuf {
        out.join("control.yaml")
    }

    // Load the live control state; absent fields fall back to the project defaults.
    pub fn load(proj: &Project, out: &Path) -> Control {
        let mut c: Control = crate::vfs::read_to_string(Self::path(out))
            .ok()
            .and_then(|t| serde_norway::from_str(&t).ok())
            .unwrap_or_default();
        if c.compile.is_empty() {
            c.compile = proj.workflow.compile.clone();
        }
        if c.generate.is_empty() {
            c.generate = proj.workflow.generate.clone();
        }
        if c.worker.is_empty() {
            c.worker = proj.workflow.worker.clone();
        }
        c
    }

    pub fn save(&self, out: &Path) {
        crate::vfs::create_dir_all(out).ok();
        if let Ok(text) = serde_norway::to_string(self) {
            let tmp = Self::path(out).with_extension("yaml.tmp");
            if crate::vfs::write(&tmp, text).is_ok() {
                crate::vfs::rename(&tmp, Self::path(out)).ok();
            }
        }
    }
}

// Record a release: approve the pending changes for the stage. Compile approves each
// document at its current content hash; generate approves the current graph
// generation. Unnamed approves both. Mirrors docs/compiler/control-plane.md#modes-and-releases.
pub fn release(proj: &Project, out: &Path, stage: Option<&str>) {
    let mut c = Control::load(proj, out);
    if stage.is_none() || stage == Some("compile") {
        c.released.compile = current_doc_hashes(proj);
    }
    if stage.is_none() || stage == Some("generate") {
        // A pending document edit is journaled first, so the approved generation is
        // the one the next build or serving will see: a release once approved 31
        // while four deleted documents waited, and the next serving's sync moved the
        // store to 32 and re-gated every bind goal.
        let mut s = crate::store::Store::load(out);
        let (parsed, _) = crate::reconcile::parse_all(proj);
        s.sync_docs(&parsed);
        c.released.generate = s.status.generation;
    }
    c.save(out);
}

// A dual write carries the compile release: a document released at its text before
// the write is released at its text after, since the commit absorbed the new text and
// nothing in it waits to be read. A document that was not released stays pending.
// Mirrors docs/compiler/control-plane.md#modes-and-releases.
pub fn carry_release(out: &Path, doc: &str, before: &str, after: &str) {
    let Ok(text) = crate::vfs::read_to_string(Control::path(out)) else {
        return;
    };
    let Ok(mut c) = serde_norway::from_str::<Control>(&text) else {
        return;
    };
    let old = crate::model::hash_hex(before);
    if c.released.compile.get(doc) != Some(&old) {
        return;
    }
    c.released.compile.insert(doc.to_string(), crate::model::hash_hex(after));
    c.save(out);
}

// Approve decompilation scopes. Scopes accumulate until a submitted draft consumes
// them. Mirrors docs/consumers/decompile.md#triggering.
pub fn release_decompile(proj: &Project, out: &Path, scopes: &[String]) {
    let mut c = Control::load(proj, out);
    for s in scopes {
        if !c.released.decompile.contains(s) {
            c.released.decompile.push(s.clone());
        }
    }
    c.save(out);
}

// A submitted draft covering a scope consumes its release.
pub fn consume_decompile(proj: &Project, out: &Path, scope: &str) {
    let mut c = Control::load(proj, out);
    c.released.decompile.retain(|s| s != scope);
    c.save(out);
}

// The current content hash of every matched document, the compile approval unit.
pub fn current_doc_hashes(proj: &Project) -> BTreeMap<String, String> {
    proj.doc_files()
        .iter()
        .filter_map(|f| {
            let rel = f
                .strip_prefix(&proj.root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| f.to_string_lossy().to_string());
            crate::md::read_doc(f)
                .ok()
                .map(|t| (rel, crate::model::hash_hex(&t)))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// workers/: the registry of attached worker sessions.

#[derive(Serialize, Deserialize, Clone)]
pub struct Worker {
    pub id: String,
    // internal | gui | agent
    pub kind: String,
    // The MCP client name when there is one.
    pub client: String,
    pub pid: u32,
    pub started_at: u64,
    pub heartbeat_at: u64,
    // The task currently held, empty when idle.
    pub task: String,
}

fn workers_dir(out: &Path) -> PathBuf {
    out.join("workers")
}

// A registration that lives as long as the handle: refresh() heartbeats, drop
// deregisters. Crash-safe: a file whose heartbeat went stale is swept by any reader.
pub struct WorkerHandle {
    out: PathBuf,
    worker: Worker,
}

pub fn register(out: &Path, kind: &str, client: &str) -> WorkerHandle {
    let id = format!("{}-{}", crate::sys::process_id(), now() % 100_000);
    let worker = Worker {
        id,
        kind: kind.to_string(),
        client: client.to_string(),
        pid: crate::sys::process_id(),
        started_at: now(),
        heartbeat_at: now(),
        task: String::new(),
    };
    let h = WorkerHandle {
        out: out.to_path_buf(),
        worker,
    };
    h.write();
    h
}

impl WorkerHandle {
    fn write(&self) {
        crate::vfs::create_dir_all(workers_dir(&self.out)).ok();
        if let Ok(text) = serde_norway::to_string(&self.worker) {
            crate::vfs::write(
                workers_dir(&self.out).join(format!("{}.yaml", self.worker.id)),
                text,
            )
            .ok();
        }
    }

    // Heartbeat, and record what the worker holds right now.
    pub fn refresh(&mut self, task: Option<&str>) {
        self.worker.heartbeat_at = now();
        if let Some(t) = task {
            self.worker.task = t.to_string();
        }
        self.write();
    }

    pub fn id(&self) -> &str {
        &self.worker.id
    }

    pub fn set_client(&mut self, client: &str) {
        self.worker.client = client.to_string();
        self.write();
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        crate::vfs::remove_file(workers_dir(&self.out).join(format!("{}.yaml", self.worker.id))).ok();
    }
}

// The live workers. Stale files (heartbeat older than the threshold) are swept as a
// side effect: dead sessions disappear instead of haunting the panel.
pub fn workers(out: &Path) -> Vec<Worker> {
    let mut v = Vec::new();
    let Ok(entries) = crate::vfs::read_dir(workers_dir(out)) else {
        return v;
    };
    for e in entries.flatten() {
        let Ok(text) = crate::vfs::read_to_string(e.path()) else {
            continue;
        };
        let Ok(w) = serde_norway::from_str::<Worker>(&text) else {
            crate::vfs::remove_file(e.path()).ok();
            continue;
        };
        if now().saturating_sub(w.heartbeat_at) > WORKER_STALE_SECS {
            crate::vfs::remove_file(e.path()).ok();
            continue;
        }
        v.push(w);
    }
    v.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    v
}

// ---------------------------------------------------------------------------
// leases/: exclusive claims on tasks.

#[derive(Serialize, Deserialize, Clone)]
pub struct Lease {
    pub task: String,
    pub worker: String,
    pub claimed_at: u64,
    pub expires_at: u64,
    // The goal ids a batch lease holds, so the board renders claimedBy per goal.
    // Mirrors docs/compiler/control-plane.md#workers-and-leases.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub goals: Vec<String>,
}

fn leases_dir(out: &Path) -> PathBuf {
    out.join("leases")
}

// Injective file-name encoding for lease ids: alphanumerics, `-`, and `.` stand as
// they are; everything else (`:`, `/`, `%` included) percent-encodes, so two distinct
// ids never share a file.
pub(crate) fn encode_lease_id(task: &str) -> String {
    let mut s = String::with_capacity(task.len());
    for b in task.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' => s.push(b as char),
            _ => s.push_str(&format!("%{:02X}", b)),
        }
    }
    s
}

pub(crate) fn decode_lease_id(safe: &str) -> Option<String> {
    let mut out: Vec<u8> = Vec::with_capacity(safe.len());
    let bytes = safe.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = safe.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn lease_file(out: &Path, task: &str) -> PathBuf {
    leases_dir(out).join(format!("{}.yaml", encode_lease_id(task)))
}

// Claim a task. Create-new semantics decide the winner; an expired lease is
// reclaimable. Err carries the live holder.
pub fn claim(out: &Path, task: &str, worker: &str) -> Result<(), String> {
    claim_goals(out, task, worker, &[])
}

// Claim a goal batch: the lease names the goals it holds, so the board shows
// claimedBy on each. Mirrors docs/compiler/control-plane.md#workers-and-leases.
pub fn claim_goals(out: &Path, task: &str, worker: &str, goals: &[String]) -> Result<(), String> {
    crate::vfs::create_dir_all(leases_dir(out)).ok();
    let path = lease_file(out, task);
    if let Some(l) = read_lease(&path) {
        // Re-entrant for the same worker: a retry after its own failed attempt
        // refreshes the claim instead of waiting out its own lease.
        if l.expires_at > now() && l.worker != worker {
            return Err(l.worker);
        }
        crate::vfs::remove_file(&path).ok();
    }
    let lease = Lease {
        task: task.to_string(),
        worker: worker.to_string(),
        claimed_at: now(),
        expires_at: now() + LEASE_TTL_SECS,
        goals: goals.to_vec(),
    };
    let text = serde_norway::to_string(&lease).map_err(|e| e.to_string())?;
    match crate::vfs::create_new(&path, &text) {
        Ok(()) => Ok(()),
        // Lost the race to another claimant between the check and the create.
        Err(_) => Err(read_lease(&path)
            .map(|l| l.worker)
            .unwrap_or_else(|| "another worker".into())),
    }
}

pub fn refresh_lease(out: &Path, task: &str) {
    let path = lease_file(out, task);
    if let Some(mut l) = read_lease(&path) {
        l.expires_at = now() + LEASE_TTL_SECS;
        if let Ok(text) = serde_norway::to_string(&l) {
            crate::vfs::write(&path, text).ok();
        }
    }
}

pub fn release_lease(out: &Path, task: &str) {
    crate::vfs::remove_file(lease_file(out, task)).ok();
}

fn read_lease(path: &Path) -> Option<Lease> {
    serde_norway::from_str(&crate::vfs::read_to_string(path).ok()?).ok()
}

// The live leases by task. Expired files are swept.
pub fn leases(out: &Path) -> BTreeMap<String, Lease> {
    let mut m = BTreeMap::new();
    let Ok(entries) = crate::vfs::read_dir(leases_dir(out)) else {
        return m;
    };
    for e in entries.flatten() {
        let Some(l) = read_lease(&e.path()) else {
            crate::vfs::remove_file(e.path()).ok();
            continue;
        };
        if l.expires_at <= now() {
            crate::vfs::remove_file(e.path()).ok();
            continue;
        }
        m.insert(l.task.clone(), l);
    }
    m
}

// The live build lease, when the internal loop holds one.
pub fn build_lease(out: &Path) -> Option<Lease> {
    leases(out).remove(BUILD_LEASE)
}

// The internal loop's entry contract: refuse while an agent is mid-task, then hold
// the coarse build lease for the run and record the implicit release (a typed command
// or a clicked button is an approval). A background thread heartbeats the lease so a
// long build never loses it; drop frees it. Mirrors docs/compiler/control-plane.md#workers-and-leases.
pub struct BuildGuard {
    _project_lock: Option<crate::document_sets::ProjectLock>,
    out: PathBuf,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

pub fn begin_internal_build(proj: &Project, out: &Path, stage: &str) -> Result<BuildGuard, String> {
    if let Some(error) = &proj.config_error { return Err(error.clone()); }
    let project_lock = if proj.set_id.is_some() {
        Some(crate::document_sets::ProjectLock::acquire(&proj.root, "project.lock").map_err(|e| e.1)?)
    } else { None };
    let held = task_leases(out);
    if let Some((task, l)) = held.iter().next() {
        return Err(format!(
            "worker `{}` holds task `{}`; wait for it to finish or abandon",
            l.worker, task
        ));
    }
    claim(
        out,
        BUILD_LEASE,
        &format!("internal-{}", crate::sys::process_id()),
    )
    .map_err(|holder| format!("a build is already running (lease held by `{}`)", holder))?;
    release(proj, out, Some(stage));
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (out2, stop2) = (out.to_path_buf(), stop.clone());
    // The heartbeat keeps a long build's lease fresh so a watchdog does not reclaim
    // it. A browser tab runs one backend on one thread, with nothing to reclaim it
    // and no thread to spare (docs/frontends/wasm.md#the-seams).
    #[cfg(target_arch = "wasm32")]
    let _ = (&out2, &stop2);
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() { std::thread::spawn(move || {
        while !stop2.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_secs(30));
            if stop2.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            refresh_lease(&out2, BUILD_LEASE);
        }
    }); }
    Ok(BuildGuard {
        _project_lock: project_lock,
        out: out.to_path_buf(),
        stop,
    })
}

impl Drop for BuildGuard {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        release_lease(&self.out, BUILD_LEASE);
    }
}

// Any live per-task lease (not the build lease): an agent is mid-task.
pub fn task_leases(out: &Path) -> BTreeMap<String, Lease> {
    let mut m = leases(out);
    m.remove(BUILD_LEASE);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!(
            "jazyk-control-test-{}-{}",
            crate::sys::process_id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        crate::vfs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn lease_claim_is_exclusive_then_reclaimable_after_release() {
        let out = tmp();
        assert!(claim(&out, "docs/a.md", "w1").is_ok());
        assert_eq!(claim(&out, "docs/a.md", "w2"), Err("w1".to_string()));
        release_lease(&out, "docs/a.md");
        assert!(claim(&out, "docs/a.md", "w2").is_ok());
        assert_eq!(
            task_leases(&out).get("docs/a.md").map(|l| l.worker.clone()),
            Some("w2".into())
        );
        crate::vfs::remove_dir_all(&out).ok();
    }

    // The lease file name is injective: a goal id with `:` and `/` round-trips, and
    // two ids that differ only in those characters never share a file.
    #[test]
    fn lease_file_names_round_trip_goal_ids() {
        let id = "g:reconcile-section:docs/orders.md#/orders/holds";
        let safe = encode_lease_id(id);
        assert!(!safe.contains(':') && !safe.contains('/') && !safe.contains('#'));
        assert_eq!(decode_lease_id(&safe).as_deref(), Some(id));
        let colon = encode_lease_id("a:b");
        let slash = encode_lease_id("a/b");
        let under = encode_lease_id("a_b");
        assert_ne!(colon, slash);
        assert_ne!(colon, under);
        assert_ne!(slash, under);
        // A claimed batch lease names its goals and reads back through leases().
        let out = tmp();
        claim_goals(&out, "b3-1", "w1", &[id.to_string()]).unwrap();
        let all = leases(&out);
        assert_eq!(all["b3-1"].goals, vec![id.to_string()]);
        release_lease(&out, "b3-1");
        assert!(leases(&out).is_empty());
        crate::vfs::remove_dir_all(&out).ok();
    }

    #[test]
    fn workers_register_heartbeat_and_deregister_on_drop() {
        let out = tmp();
        {
            let mut h = register(&out, "agent", "test-client");
            h.refresh(Some("docs/a.md"));
            let ws = workers(&out);
            assert_eq!(ws.len(), 1);
            assert_eq!(ws[0].task, "docs/a.md");
        }
        assert!(workers(&out).is_empty());
        crate::vfs::remove_dir_all(&out).ok();
    }

    #[test]
    fn control_defaults_flow_from_project_and_release_records_hashes() {
        let out = tmp();
        let mut proj = Project::default();
        proj.workflow.generate = "auto".into();
        let c = Control::load(&proj, &out);
        assert_eq!(c.compile, "manual");
        assert_eq!(c.generate, "auto");
        release(&proj, &out, Some("generate"));
        let c = Control::load(&proj, &out);
        assert_eq!(c.released.generate, 0);
        assert_eq!(c.compile, "manual");
        crate::vfs::remove_dir_all(&out).ok();
    }
}
