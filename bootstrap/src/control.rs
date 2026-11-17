// The control plane: workflow modes and releases (control.yaml), the worker registry
// (workers/), and open work (open/), all in the out directory so every consumer
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
// An open entry lives this long unless refreshed by activity on the open task.
pub const LEASE_TTL_SECS: u64 = 120;
// The task name of an internal build run's entry.
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
        crate::layout::control_file(out)
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

    // Write the file whole. A file on disk that does not parse is a human's text, not
    // state: it moves aside to `control.yaml.bad` first, never overwritten.
    // Mirrors docs/compiler/control-plane.md#modes-and-releases.
    pub fn save(&self, out: &Path) {
        // A newer store is never written (docs/compiler/graph.md#store-version).
        if let Some(why) = crate::store::newer_store(out) {
            eprintln!("[jazyk] error: {}; the control state stays as it is", why);
            return;
        }
        crate::vfs::create_dir_all(crate::layout::local_dir(out)).ok();
        let path = Self::path(out);
        if let Ok(old) = crate::vfs::read_to_string(&path) {
            if serde_norway::from_str::<Control>(&old).is_err() {
                let bad = path.with_extension("yaml.bad");
                crate::vfs::remove_file(&bad).ok();
                if crate::vfs::rename(&path, &bad).is_err() {
                    eprintln!("[jazyk] warning: {} does not parse and cannot be moved aside; not writing it", path.display());
                    return;
                }
                eprintln!(
                    "[jazyk] warning: {} did not parse; moved it to {} and wrote a fresh one",
                    path.display(),
                    bad.display()
                );
            }
        }
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
    c.released
        .compile
        .insert(doc.to_string(), crate::model::hash_hex(after));
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

// Current whole-file revisions, including opaque inputs and unreadable markers.
pub fn current_doc_hashes(proj: &Project) -> BTreeMap<String, String> {
    crate::input::scan(proj)
        .into_iter()
        .map(|(path, input)| {
            (
                path,
                input
                    .map(|(hash, _)| hash)
                    .unwrap_or_else(|_| "unreadable".into()),
            )
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
    crate::layout::workers_dir(out)
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
        // A newer store is never written (docs/compiler/graph.md#store-version).
        if crate::store::newer_version(&self.out).is_some() {
            return;
        }
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
        crate::vfs::remove_file(workers_dir(&self.out).join(format!("{}.yaml", self.worker.id)))
            .ok();
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
    v.sort_by_key(|a| a.started_at);
    v
}

// ---------------------------------------------------------------------------
// open/: what each worker has open. Information only: nothing claims work and
// nothing refuses on these entries; commits are the only synchronization.
// Mirrors docs/compiler/control-plane.md#workers-and-open-work.

// One worker's open task: a goal batch, a ledger row, or a build run. Several workers
// may have the same task open at once; each has its own entry.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Lease {
    pub task: String,
    pub worker: String,
    #[serde(default, rename = "claimId", skip_serializing_if = "String::is_empty")]
    pub claim_id: String,
    pub claimed_at: u64,
    pub expires_at: u64,
    // The goal ids an open batch holds, so the board renders openBy per goal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub goals: Vec<String>,
}

fn open_dir(out: &Path) -> PathBuf {
    crate::layout::open_dir(out)
}

// Injective file-name encoding: alphanumerics, `-`, and `.` stand as they are;
// everything else (`:`, `/`, `%`, `~` included) percent-encodes, so two distinct ids
// never share a file and `~` can join the task and the worker.
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

#[cfg(test)]
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

fn entry_file(out: &Path, task: &str, worker: &str) -> PathBuf {
    open_dir(out).join(format!(
        "{}~{}.yaml",
        encode_lease_id(task),
        encode_lease_id(worker)
    ))
}

/// A protected receipt for one execution's own entry. Expiry may be refreshed.
#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct OwnedLease {
    pub out: PathBuf,
    pub lease: Lease,
}

pub(crate) fn owned_lease(out: &Path, task: &str, worker: &str) -> Option<OwnedLease> {
    if crate::vfs::is_scoped() && crate::session_recovery::active() {
        return crate::session_recovery::owned_claim(out, task)
            .filter(|claim| claim.lease.worker == worker);
    }
    let lease = read_lease(&entry_file(out, task, worker))?;
    (lease.task == task && lease.worker == worker).then(|| OwnedLease {
        out: out.into(),
        lease,
    })
}

/// A restored session opens its task again; another worker's entry never blocks it.
pub(crate) fn claim_restored(
    out: &Path,
    task: &str,
    worker: &str,
    goals: &[String],
) -> Result<(), String> {
    claim_goals(out, task, worker, goals)
}

/// Removes the execution's own entry, never another's or a replacement's.
pub(crate) fn release_owned_lease(saved: &OwnedLease) -> Result<bool, String> {
    let path = entry_file(&saved.out, &saved.lease.task, &saved.lease.worker);
    let Some(current) = read_lease(&path) else {
        return Ok(false);
    };
    if !same_claim(&saved.lease, &current) {
        return Ok(false);
    }
    crate::vfs::remove_file(path).map_err(|e| e.to_string())?;
    crate::session_recovery::released(&saved.out, &saved.lease.task);
    Ok(true)
}

fn same_claim(saved: &Lease, current: &Lease) -> bool {
    !saved.claim_id.is_empty()
        && current.claim_id == saved.claim_id
        && current.task == saved.task
        && current.worker == saved.worker
        && current.claimed_at == saved.claimed_at
        && current.goals == saved.goals
}

#[cfg(not(target_arch = "wasm32"))]
fn refresh_owned_lease(saved: &OwnedLease) -> Result<bool, String> {
    let path = entry_file(&saved.out, &saved.lease.task, &saved.lease.worker);
    let Some(mut current) = read_lease(&path) else {
        return Ok(false);
    };
    if !same_claim(&saved.lease, &current) {
        return Ok(false);
    }
    current.expires_at = now() + LEASE_TTL_SECS;
    write_lease_whole(
        &path,
        &serde_norway::to_string(&current).map_err(|e| e.to_string())?,
    )?;
    Ok(true)
}

fn nonce() -> String {
    let mut bytes = [0u8; 8];
    if getrandom::fill(&mut bytes).is_err() {
        return format!("{}-{}", crate::sys::process_id(), now());
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// Replace an entry whole: a temporary file beside it, then a rename, so a reader never
// sees half an entry.
fn write_lease_whole(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension(format!("{}.tmp", nonce()));
    crate::vfs::write(&tmp, text).map_err(|e| e.to_string())?;
    crate::vfs::rename(&tmp, path).map_err(|e| {
        crate::vfs::remove_file(&tmp).ok();
        e.to_string()
    })
}

// Open a task for this worker. Never refused for another worker's sake.
pub fn claim(out: &Path, task: &str, worker: &str) -> Result<(), String> {
    claim_goals(out, task, worker, &[])
}

// Open a goal batch: the entry names the goals, so the board shows openBy on each.
// Mirrors docs/compiler/control-plane.md#workers-and-open-work.
pub fn claim_goals(out: &Path, task: &str, worker: &str, goals: &[String]) -> Result<(), String> {
    // A newer store records nothing (docs/compiler/graph.md#store-version).
    if let Some(why) = crate::store::newer_store(out) {
        return Err(why);
    }
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce)
        .map_err(|error| format!("Cannot mint control claim identity: {error}"))?;
    let claim_id = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
    crate::vfs::create_dir_all(open_dir(out)).ok();
    let lease = Lease {
        task: task.to_string(),
        worker: worker.to_string(),
        claim_id,
        claimed_at: now(),
        expires_at: now() + LEASE_TTL_SECS,
        goals: goals.to_vec(),
    };
    let text = serde_norway::to_string(&lease).map_err(|e| e.to_string())?;
    write_lease_whole(&entry_file(out, task, worker), &text)?;
    crate::session_recovery::claimed(OwnedLease {
        out: out.into(),
        lease,
    });
    Ok(())
}

pub fn refresh_lease(out: &Path, task: &str, worker: &str) {
    let path = entry_file(out, task, worker);
    if let Some(mut l) = read_lease(&path) {
        l.expires_at = now() + LEASE_TTL_SECS;
        if let Ok(text) = serde_norway::to_string(&l) {
            write_lease_whole(&path, &text).ok();
        }
    }
}

pub fn release_lease(out: &Path, task: &str, worker: &str) {
    if crate::vfs::is_scoped() && crate::session_recovery::active() {
        if let Some(claim) = crate::session_recovery::owned_claim(out, task) {
            let _ = release_owned_lease(&claim);
        }
        return;
    }
    if crate::vfs::remove_file(entry_file(out, task, worker)).is_ok() {
        crate::session_recovery::released(out, task);
    }
}

fn read_lease(path: &Path) -> Option<Lease> {
    serde_norway::from_str(&crate::vfs::read_to_string(path).ok()?).ok()
}

// Every live open entry, oldest first. Expired and unreadable files are swept: an
// entry is information, so a dead worker's simply disappears.
pub fn leases(out: &Path) -> Vec<Lease> {
    let mut v = Vec::new();
    let Ok(entries) = crate::vfs::read_dir(open_dir(out)) else {
        return v;
    };
    for e in entries.flatten() {
        let path = e.path();
        // Temporary files are a writer's, never entries.
        if path.extension().is_none_or(|x| x != "yaml") {
            continue;
        }
        match read_lease(&path) {
            Some(l) if l.expires_at > now() => v.push(l),
            Some(_) => {
                crate::vfs::remove_file(&path).ok();
            }
            None => {}
        }
    }
    v.sort_by(|a, b| a.claimed_at.cmp(&b.claimed_at).then(a.task.cmp(&b.task)));
    v
}

// The build runs open right now.
pub fn builds(out: &Path) -> Vec<Lease> {
    leases(out)
        .into_iter()
        .filter(|l| l.task == BUILD_LEASE)
        .collect()
}

// Whether the build run with this token is open.
pub fn owns_build(out: &Path, token: &str) -> bool {
    builds(out).iter().any(|l| l.worker == token)
}

// The internal loop's run: registered as an open `build` entry while it lives, with a
// heartbeat, and with the implicit release recorded (a typed command or a clicked
// button is an approval). It claims nothing: agents, other builds, and people work
// beside it. Mirrors docs/compiler/control-plane.md#concurrent-work.
pub struct BuildGuard {
    claim: OwnedLease,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl BuildGuard {
    pub fn token(&self) -> &str {
        &self.claim.lease.worker
    }
}

pub fn begin_internal_build(proj: &Project, out: &Path, stage: &str) -> Result<BuildGuard, String> {
    if let Some(error) = &proj.config_error {
        return Err(error.clone());
    }
    // A build, a generation, a verification, or a decompile on a newer store stops
    // here, before any session (docs/compiler/graph.md#store-version).
    if let Some(why) = crate::store::newer_store(out) {
        return Err(why);
    }
    // The project's file view spans every set and owns no graph: a build on it
    // would write the shared `.jazyk/`. Graph operations address one set
    // (docs/frontends/cli.md).
    if proj.set_id.is_none() && proj.set_docs.is_some() {
        return Err("choose a document set: a build works on one set, never on the project's shared .jazyk/".into());
    }
    let token = format!(
        "internal-{}-{}",
        crate::sys::process_id(),
        crate::document_sets::mint_id()
    );
    claim(out, BUILD_LEASE, &token)?;
    let claim =
        owned_lease(out, BUILD_LEASE, &token).ok_or("the build run lost its ownership receipt")?;
    release(proj, out, Some(stage));
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (heartbeat_claim, stop2) = (claim.clone(), stop.clone());
    // The heartbeat keeps a long build's entry fresh. A browser tab runs one backend on
    // one thread, with no thread to spare (docs/frontends/wasm.md#the-seams).
    #[cfg(target_arch = "wasm32")]
    let _ = (&heartbeat_claim, &stop2);
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        std::thread::spawn(move || {
            while !stop2.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_secs(30));
                if stop2.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                if !refresh_owned_lease(&heartbeat_claim).unwrap_or(false) {
                    break;
                }
            }
        });
    }
    Ok(BuildGuard { claim, stop })
}

impl Drop for BuildGuard {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = release_owned_lease(&self.claim);
    }
}

// The open entries other than build runs: agents mid-task.
pub fn task_leases(out: &Path) -> Vec<Lease> {
    leases(out)
        .into_iter()
        .filter(|l| l.task != BUILD_LEASE)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_hash_inventory_includes_opaque_inputs() {
        let _fs = crate::vfs::Scope::enter(&BTreeMap::from([
            (
                ".jazyk/jazyk.toml".into(),
                b"version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Main'\ndocs = ['**']\n".to_vec(),
            ),
            ("main.md".into(), b"# Site\n\nA site.\n".to_vec()),
            ("notes.MARKDOWN".into(), b"# Notes\n\nDetails.\n".to_vec()),
            (
                "example.txt".into(),
                br#"{"name":"supported text document"}"#.to_vec(),
            ),
            (
                "example.json".into(),
                br#"{"name":"unsupported asset"}"#.to_vec(),
            ),
            (
                "logo.svg".into(),
                br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#.to_vec(),
            ),
        ]));
        let project = crate::document_sets::Workspace::load(Path::new("/project"))
            .unwrap()
            .resolve(None)
            .unwrap();
        let hashes = current_doc_hashes(&project);
        let (parsed, _) = crate::reconcile::parse_all(&project);
        let parsed_hashes: BTreeMap<_, _> = parsed
            .into_iter()
            .map(|(path, (hash, _))| (path, hash))
            .collect();
        assert_eq!(hashes, parsed_hashes);
        assert_eq!(
            hashes.keys().map(String::as_str).collect::<Vec<_>>(),
            vec![
                "example.json",
                "example.txt",
                "logo.svg",
                "main.md",
                "notes.MARKDOWN"
            ]
        );
        release(&project, &project.out, Some("compile"));
        assert_eq!(
            Control::load(&project, &project.out).released.compile,
            hashes
        );
        let findings = crate::reconcile::checks(&crate::store::Store::default(), &project);
        assert!(!findings
            .iter()
            .any(|finding| finding.0 == "unsupported-format"));
    }

    #[test]
    fn document_hash_inventory_keeps_new_and_stored_unreadable_inputs() {
        let _fs = crate::vfs::Scope::enter(&BTreeMap::from([
            (
                ".jazyk/jazyk.toml".into(),
                b"version = 1\n[[sets]]\nid = 'ds_main'\nname = 'Main'\ndocs = ['docs/**']\n"
                    .to_vec(),
            ),
            ("docs/main.md".into(), b"# Site\n\nA site.\n".to_vec()),
        ]));
        let mut project = crate::document_sets::Workspace::load(Path::new("/project"))
            .unwrap()
            .resolve(None)
            .unwrap();
        let mut store = crate::store::Store {
            out: project.out.clone(),
            ..Default::default()
        };
        store.sync_docs(&crate::reconcile::parse_all(&project).0);
        store.save_for_test();
        let before = current_doc_hashes(&project);
        crate::vfs::write(project.root.join("docs/main.md"), [0xff, 0xfe]).unwrap();
        crate::vfs::write(project.root.join("docs/new.txt"), [0xff]).unwrap();
        project.explicit_files = Some(vec![
            project.root.join("docs/main.md"),
            project.root.join("docs/new.txt"),
            project.root.join("docs/missing.md"),
        ]);
        let hashes = current_doc_hashes(&project);
        assert_eq!(
            hashes.len(),
            3,
            "A failed read must not disappear from freshness checks"
        );
        assert_ne!(hashes["docs/main.md"], before["docs/main.md"]);
        assert!(
            hashes
                .values()
                .all(|hash| hash.chars().any(|c| !c.is_ascii_hexdigit())),
            "Unreadable markers cannot equal any actual content hash"
        );
        let parsed = crate::reconcile::parse_all(&project).0;
        assert_eq!(
            parsed.len(),
            1,
            "Parsing must preserve the previous tree only"
        );
        assert_eq!(parsed["docs/main.md"].0, before["docs/main.md"]);
        let findings = crate::reconcile::checks(&store, &project);
        for path in hashes.keys() {
            assert!(
                findings.iter().any(|finding| finding.0 == "parse-error"
                    && finding.2 == "error"
                    && finding.1 == [path.as_str()]),
                "{path}"
            );
        }
        release(&project, &project.out, Some("compile"));
        assert_ne!(
            Control::load(&project, &project.out).released.compile["docs/main.md"],
            store.docs["docs/main.md"].content_hash,
            "A release must not approve an unreadable file's retained content"
        );
    }

    // The project's file view (every set, no graph) never takes a build: it would
    // write the shared `.jazyk/` (docs/frontends/cli.md).
    #[test]
    fn a_build_on_the_set_less_file_view_is_refused() {
        let _fs = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
        let project = Project {
            set_docs: Some(vec![vec!["a.md".into()], vec!["b.md".into()]]),
            ..Project::default()
        };
        let out = Path::new("/project/.jazyk");
        let error = begin_internal_build(&project, out, "compile")
            .err()
            .unwrap();
        assert!(error.starts_with("choose a document set"), "{}", error);
        assert!(builds(out).is_empty());
    }

    #[test]
    fn replacement_build_has_a_new_token_and_cannot_be_renewed_by_its_predecessor() {
        let _fs = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
        let project = Project::default();
        let out = Path::new("/project/.jazyk/ds_main");
        let first = begin_internal_build(&project, out, "generate").unwrap();
        let old = first.claim.clone();
        drop(first);
        let replacement = begin_internal_build(&project, out, "generate").unwrap();
        assert_ne!(old.lease.worker, replacement.token());
        let path = entry_file(out, BUILD_LEASE, replacement.token());
        let before = crate::vfs::read(&path).unwrap();
        assert!(!refresh_owned_lease(&old).unwrap());
        assert!(!release_owned_lease(&old).unwrap());
        assert_eq!(crate::vfs::read(path).unwrap(), before);
    }

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
    fn saved_claim_cannot_release_same_worker_same_second_replacement_or_legacy_claim() {
        let _fs = crate::vfs::Scope::enter(&crate::file_tree::Snapshot::new());
        let _recovery = crate::session_recovery::Scope::enter(None);
        let out = std::path::Path::new("/project/.jazyk/ds_main");
        let goals = vec!["g:reconcile-section:docs/main.md#/main".into()];
        claim_goals(out, "batch", "internal-1", &goals).unwrap();
        let saved = owned_lease(out, "batch", "internal-1").unwrap();
        claim_goals(out, "batch", "internal-1", &goals).unwrap();
        let path = entry_file(out, "batch", "internal-1");
        let mut replacement = read_lease(&path).unwrap();
        replacement.claimed_at = saved.lease.claimed_at;
        assert_ne!(replacement.claim_id, saved.lease.claim_id);
        crate::vfs::write(&path, serde_norway::to_string(&replacement).unwrap()).unwrap();
        let before = crate::vfs::read(&path).unwrap();
        assert!(!release_owned_lease(&saved).unwrap());
        assert_eq!(crate::vfs::read(&path).unwrap(), before);
        let mut legacy = owned_lease(out, "batch", "internal-1").unwrap();
        legacy.lease.claim_id.clear();
        assert!(!release_owned_lease(&legacy).unwrap());
        assert_eq!(crate::vfs::read(&path).unwrap(), before);
        let current = owned_lease(out, "batch", "internal-1").unwrap();
        refresh_lease(out, "batch", "internal-1");
        assert!(
            release_owned_lease(&current).unwrap(),
            "Expiry refresh does not replace ownership"
        );
        claim_goals(out, "batch", "internal-1", &goals).unwrap();
        let mut replacement = read_lease(&path).unwrap();
        replacement.claim_id.push('f');
        let foreign = serde_norway::to_string(&replacement).unwrap();
        crate::vfs::write(&path, &foreign).unwrap();
        release_lease(out, "batch", "internal-1");
        assert_eq!(
            crate::vfs::read_to_string(&path).unwrap(),
            foreign,
            "Active cancellation must use its recorded receipt, not the current same-worker claim"
        );
    }

    // Opening is never exclusive: two workers open the same task, each with its own
    // entry; an expired entry is swept, and closing removes only the worker's own.
    // Mirrors docs/compiler/control-plane.md#workers-and-open-work.
    #[test]
    fn two_workers_open_the_same_task_and_nothing_refuses() {
        let out = tmp();
        assert!(claim_goals(&out, "b1-1", "w1", &["g:a".into()]).is_ok());
        assert!(claim_goals(&out, "b1-1", "w2", &["g:a".into()]).is_ok());
        let open = leases(&out);
        assert_eq!(open.len(), 2);
        assert!(open
            .iter()
            .all(|l| l.task == "b1-1" && l.goals == vec!["g:a".to_string()]));
        release_lease(&out, "b1-1", "w1");
        let open = leases(&out);
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].worker, "w2");
        // An expired entry is information that aged out: swept on read.
        let path = entry_file(&out, "b1-1", "w2");
        let mut l = read_lease(&path).unwrap();
        l.expires_at = 1;
        crate::vfs::write(&path, serde_norway::to_string(&l).unwrap()).unwrap();
        assert!(leases(&out).is_empty());
        assert!(!crate::vfs::exists(&path));
        // Builds coexist with each other and with agents.
        claim(&out, BUILD_LEASE, "internal-a").unwrap();
        claim(&out, BUILD_LEASE, "internal-b").unwrap();
        claim(&out, "req:x", "agent-1").unwrap();
        assert_eq!(builds(&out).len(), 2);
        assert!(owns_build(&out, "internal-b"));
        assert_eq!(task_leases(&out).len(), 1);
        crate::vfs::remove_dir_all(&out).ok();
    }

    // A control.yaml that does not parse reads as the project defaults and is never
    // overwritten: the next write moves it aside to control.yaml.bad.
    // Mirrors docs/compiler/control-plane.md#modes-and-releases.
    #[test]
    fn an_unparseable_control_file_moves_aside_before_a_write() {
        let out = tmp();
        let broken = "compile: [auto\n";
        crate::vfs::create_dir_all(crate::layout::local_dir(&out)).unwrap();
        crate::vfs::write(Control::path(&out), broken).unwrap();
        let proj = Project {
            workflow: Default::default(),
            ..Default::default()
        };
        let mut c = Control::load(&proj, &out);
        assert_eq!(c.compile, proj.workflow.compile);
        c.compile = "auto".into();
        c.save(&out);
        assert_eq!(
            crate::vfs::read_to_string(crate::layout::local_dir(&out).join("control.yaml.bad"))
                .unwrap(),
            broken
        );
        assert_eq!(Control::load(&proj, &out).compile, "auto");
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
        assert_eq!(
            all.iter().find(|l| l.task == "b3-1").unwrap().goals,
            vec![id.to_string()]
        );
        release_lease(&out, "b3-1", "w1");
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
