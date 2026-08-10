// The control plane: workflow modes and releases (control.yaml), the worker registry
// (workers/), and task leases (leases/), all in the out directory so every consumer
// reads the same intent the same way the queue is the same everywhere.
// Mirrors docs/compiler/reconciler.md#the-control-plane.
use crate::project::Project;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// A worker file older than this (seconds since its heartbeat) is stale and swept.
const WORKER_STALE_SECS: u64 = 90;
// A lease lives this long unless refreshed by activity on the open task.
const LEASE_TTL_SECS: u64 = 120;
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
        let mut c: Control = std::fs::read_to_string(Self::path(out))
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
        std::fs::create_dir_all(out).ok();
        if let Ok(text) = serde_norway::to_string(self) {
            let tmp = Self::path(out).with_extension("yaml.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                std::fs::rename(&tmp, Self::path(out)).ok();
            }
        }
    }
}

// Record a release: approve the pending changes for the stage. Compile approves each
// document at its current content hash; generate approves the current graph
// generation. Unnamed approves both. Mirrors docs/compiler/reconciler.md#modes-and-releases.
pub fn release(proj: &Project, out: &Path, stage: Option<&str>) {
    let mut c = Control::load(proj, out);
    if stage.is_none() || stage == Some("compile") {
        c.released.compile = current_doc_hashes(proj);
    }
    if stage.is_none() || stage == Some("generate") {
        c.released.generate = crate::store::Store::load(out).status.generation;
    }
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
            std::fs::read_to_string(f).ok().map(|t| (rel, crate::model::hash_hex(&t)))
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
    let id = format!("{}-{}", std::process::id(), now() % 100_000);
    let worker = Worker {
        id,
        kind: kind.to_string(),
        client: client.to_string(),
        pid: std::process::id(),
        started_at: now(),
        heartbeat_at: now(),
        task: String::new(),
    };
    let h = WorkerHandle { out: out.to_path_buf(), worker };
    h.write();
    h
}

impl WorkerHandle {
    fn write(&self) {
        std::fs::create_dir_all(workers_dir(&self.out)).ok();
        if let Ok(text) = serde_norway::to_string(&self.worker) {
            std::fs::write(workers_dir(&self.out).join(format!("{}.yaml", self.worker.id)), text).ok();
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
        std::fs::remove_file(workers_dir(&self.out).join(format!("{}.yaml", self.worker.id))).ok();
    }
}

// The live workers. Stale files (heartbeat older than the threshold) are swept as a
// side effect: dead sessions disappear instead of haunting the panel.
pub fn workers(out: &Path) -> Vec<Worker> {
    let mut v = Vec::new();
    let Ok(entries) = std::fs::read_dir(workers_dir(out)) else { return v };
    for e in entries.flatten() {
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        let Ok(w) = serde_norway::from_str::<Worker>(&text) else {
            std::fs::remove_file(e.path()).ok();
            continue;
        };
        if now().saturating_sub(w.heartbeat_at) > WORKER_STALE_SECS {
            std::fs::remove_file(e.path()).ok();
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
}

fn leases_dir(out: &Path) -> PathBuf {
    out.join("leases")
}

fn lease_file(out: &Path, task: &str) -> PathBuf {
    let safe: String = task.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect();
    leases_dir(out).join(format!("{}.yaml", safe))
}

// Claim a task. Create-new semantics decide the winner; an expired lease is
// reclaimable. Err carries the live holder.
pub fn claim(out: &Path, task: &str, worker: &str) -> Result<(), String> {
    std::fs::create_dir_all(leases_dir(out)).ok();
    let path = lease_file(out, task);
    if let Some(l) = read_lease(&path) {
        if l.expires_at > now() {
            return Err(l.worker);
        }
        std::fs::remove_file(&path).ok();
    }
    let lease = Lease { task: task.to_string(), worker: worker.to_string(), claimed_at: now(), expires_at: now() + LEASE_TTL_SECS };
    let text = serde_norway::to_string(&lease).map_err(|e| e.to_string())?;
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut f) => {
            use std::io::Write;
            f.write_all(text.as_bytes()).ok();
            Ok(())
        }
        // Lost the race to another claimant between the check and the create.
        Err(_) => Err(read_lease(&path).map(|l| l.worker).unwrap_or_else(|| "another worker".into())),
    }
}

pub fn refresh_lease(out: &Path, task: &str) {
    let path = lease_file(out, task);
    if let Some(mut l) = read_lease(&path) {
        l.expires_at = now() + LEASE_TTL_SECS;
        if let Ok(text) = serde_norway::to_string(&l) {
            std::fs::write(&path, text).ok();
        }
    }
}

pub fn release_lease(out: &Path, task: &str) {
    std::fs::remove_file(lease_file(out, task)).ok();
}

fn read_lease(path: &Path) -> Option<Lease> {
    serde_norway::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

// The live leases by task. Expired files are swept.
pub fn leases(out: &Path) -> BTreeMap<String, Lease> {
    let mut m = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(leases_dir(out)) else { return m };
    for e in entries.flatten() {
        let Some(l) = read_lease(&e.path()) else {
            std::fs::remove_file(e.path()).ok();
            continue;
        };
        if l.expires_at <= now() {
            std::fs::remove_file(e.path()).ok();
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
// long build never loses it; drop frees it. Mirrors docs/compiler/reconciler.md#workers-and-leases.
pub struct BuildGuard {
    out: PathBuf,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

pub fn begin_internal_build(proj: &Project, out: &Path, stage: &str) -> Result<BuildGuard, String> {
    let held = task_leases(out);
    if let Some((task, l)) = held.iter().next() {
        return Err(format!("worker `{}` holds task `{}`; wait for it to finish or abandon", l.worker, task));
    }
    claim(out, BUILD_LEASE, &format!("internal-{}", std::process::id()))
        .map_err(|holder| format!("a build is already running (lease held by `{}`)", holder))?;
    release(proj, out, Some(stage));
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (out2, stop2) = (out.to_path_buf(), stop.clone());
    std::thread::spawn(move || {
        while !stop2.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_secs(30));
            if stop2.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            refresh_lease(&out2, BUILD_LEASE);
        }
    });
    Ok(BuildGuard { out: out.to_path_buf(), stop })
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
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn lease_claim_is_exclusive_then_reclaimable_after_release() {
        let out = tmp();
        assert!(claim(&out, "docs/a.md", "w1").is_ok());
        assert_eq!(claim(&out, "docs/a.md", "w2"), Err("w1".to_string()));
        release_lease(&out, "docs/a.md");
        assert!(claim(&out, "docs/a.md", "w2").is_ok());
        assert_eq!(task_leases(&out).get("docs/a.md").map(|l| l.worker.clone()), Some("w2".into()));
        std::fs::remove_dir_all(&out).ok();
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
        std::fs::remove_dir_all(&out).ok();
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
        std::fs::remove_dir_all(&out).ok();
    }
}
