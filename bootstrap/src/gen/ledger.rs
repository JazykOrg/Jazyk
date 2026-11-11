use super::*;

// The ledger: gen/ledger.yaml. Two maps: generation state per entity, verification
// state per requirement. Mirrors docs/consumers/gen.md#the-ledger.
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Ledger {
    // Set when gen/ledger.yaml existed but did not parse: the error. The ledger then
    // refuses to save, so a torn or hand-broken file is never replaced by an empty one.
    // Mirrors docs/consumers/gen.md#the-ledger.
    #[serde(skip)]
    pub unreadable: Option<String>,
    // Files that belong to the deliverable, not to an entity: command setup and
    // shared composition points, including a written artifact. Any task may extend
    // one while preserving its parts; exclusive entity ownership never applies.
    // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub support: Vec<String>,
    // The deliverable's decided form, written by the first run and carried by every
    // task package after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub medium: Option<Medium>,
    // Present only when the deliverable's medium must be produced by a tool.
    // Mirrors docs/consumers/gen.md#the-build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<Build>,
    // Rows recorded over an open error diagnostic: requirement id to the diagnostic
    // ids open at record time. A record, never a verdict; a later record of the row
    // under a clean graph clears its entry.
    // Mirrors docs/consumers/gen.md#rows-recorded-over-an-open-contradiction.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contradicted: BTreeMap<String, Vec<String>>,
    // Project tasks a ledger session added, keyed by task id. Only these are
    // swept once no ledger names them.
    // Mirrors docs/consumers/gen.md#session-tasks-are-swept-when-unused.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tasks: BTreeMap<String, TaskOwner>,
    #[serde(default)]
    pub entities: BTreeMap<String, EntityGen>,
    #[serde(default)]
    pub requirements: BTreeMap<String, ReqRow>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskOwner {
    pub created_by: String,
    pub created_at: String,
    // The task definition's hash when the session last saved it. A different
    // current definition is an author's edit: the task is the author's.
    pub definition: String,
}

impl Ledger {
    // Flag or clear one row against the diagnostics open right now. Called by every
    // record of a row, so the map never outlives the dispute.
    // Mirrors docs/consumers/gen.md#rows-recorded-over-an-open-contradiction.
    pub fn record_contradicted(&mut self, store: &Store, rid: &str) -> Vec<String> {
        let ids: Vec<String> = open_errors_on(store, rid)
            .iter()
            .filter_map(|d| d["id"].as_str().map(String::from))
            .collect();
        if ids.is_empty() {
            self.contradicted.remove(rid);
        } else {
            self.contradicted.insert(rid.to_string(), ids.clone());
        }
        ids
    }
}

// The open error diagnostics naming a requirement, suppressed excluded: what a
// package says before a session writes against a disputed statement. Sorted by id.
// Mirrors docs/consumers/gen.md#rows-recorded-over-an-open-contradiction.
pub fn open_errors_on(store: &Store, rid: &str) -> Vec<Value> {
    let rid = store.resolve_id(rid);
    let mut out: Vec<(&String, &crate::model::Diagnostic)> = store
        .graph
        .diagnostics
        .iter()
        .filter(|(_, d)| {
            d.lifecycle == "open"
                && d.severity == "error"
                && d.triage.as_deref() != Some("suppressed")
                && d.subjects.iter().any(|s| store.resolve_id(s) == rid)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(b.0));
    out.iter()
        .map(|(id, d)| json!({"id": id, "rule": d.rule, "message": d.message}))
        .collect()
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EntityGen {
    pub fact_hash: String,
    #[serde(default)]
    pub requirements: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
    // Saved tasks this generation uses beyond its tests and the build.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<String>,
    // The unattached remainder, measured at record time: how much of what this
    // entity's generation produced no requirement claims.
    // Mirrors docs/consumers/gen.md#the-unattached-remainder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unattached: Option<Unattached>,
    // Display time only. Completion uses the sequence because timestamps have
    // one-second resolution. Mirrors docs/consumers/gen.md#the-ledger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_at: Option<String>,
    #[serde(default, skip_serializing_if = "record_sequence_is_zero")]
    pub record_sequence: u64,
}

pub(super) fn record_sequence_is_zero(sequence: &u64) -> bool {
    *sequence == 0
}

// Generated mass attached to no requirement: owned files no row names, significant
// lines outside every site's run, and their share of the entity's significant lines.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Unattached {
    pub files: u64,
    pub lines: u64,
    pub ratio: f64,
    // No site anchors any owned line: the medium carries no markers and the ratio
    // says nothing. Mirrors docs/consumers/gen.md#the-unattached-remainder.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub markerless: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReqRow {
    pub entity: String,
    #[serde(default)]
    pub files: Vec<String>,
    // Anchored implementing sites, recorded from stripped markers at mark time.
    // Mirrors docs/consumers/gen.md#traceability.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sites: Vec<Site>,
    pub test: TestRef,
    // One test per criterion, keyed by criterion id; `test` mirrors the first when
    // criteria exist. Mirrors docs/consumers/gen.md#the-ledger.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tests: BTreeMap<String, CritTest>,
    pub hashes: RowHashes,
    #[serde(default = "verdict_none")]
    pub verdict: String, // none | pass | fail
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<String>,
    // The exit code of the last programmatic run. Present without a verdict means the
    // command ran and told us nothing: the runner failed, not the requirement
    // (docs/consumers/gen.md#a-test-that-could-not-run-says-nothing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

fn verdict_none() -> String {
    "none".into()
}

// The test of one criterion: its reference, the criterion statement hash it encodes,
// the artifact hash, and the last verdict. Mirrors docs/consumers/gen.md#the-ledger.
#[derive(Serialize, Deserialize, Clone)]
pub struct CritTest {
    pub test: TestRef,
    pub criterion_hash: String,
    #[serde(default)]
    pub test_hash: String,
    #[serde(default = "verdict_none")]
    pub verdict: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TestRef {
    pub kind: String,  // programmatic | llm
    pub label: String, // freeform, the generator's own words
    pub artifact: String,
    pub name: String,
    pub run: String,
    #[serde(default = "dot")]
    pub cwd: String,
}

pub(super) fn same_test_ref(a: &TestRef, b: &TestRef) -> bool {
    same_test_artifact(a, b) && a.run == b.run && a.cwd == b.cwd
}

pub(super) fn same_test_artifact(a: &TestRef, b: &TestRef) -> bool {
    a.kind == b.kind && a.artifact == b.artifact && a.name == b.name
}

pub(super) fn dot() -> String {
    ".".into()
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct RowHashes {
    pub requirement: String,
    pub test: String,
    pub files: String,
}

impl Ledger {
    // The deliverable-wide file. The rows live one per file beside it.
    // Mirrors docs/consumers/gen.md#the-ledger.
    pub fn path(out: &Path) -> PathBuf {
        out.join("gen").join("ledger.yaml")
    }

    fn row_dir(out: &Path, kind: &str) -> PathBuf {
        out.join("gen").join(kind)
    }

    fn row_path(out: &Path, kind: &str, id: &str) -> PathBuf {
        Self::row_dir(out, kind).join(format!("{}.yaml", crate::store::node_stem(id).0))
    }

    fn row_files(out: &Path, kind: &str) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = crate::vfs::read_dir(Self::row_dir(out, kind))
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension().and_then(|x| x.to_str()) == Some("yaml")
                            && !p
                                .file_name()
                                .map(|n| n.to_string_lossy().starts_with('.'))
                                .unwrap_or(true)
                    })
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files
    }

    // Every file the ledger reads: what a watcher fingerprints.
    pub fn files(out: &Path) -> Vec<PathBuf> {
        let mut v = vec![Self::path(out)];
        v.extend(Self::row_files(out, "entities"));
        v.extend(Self::row_files(out, "requirements"));
        v
    }

    // Parsed once per content: the status of every row reloads the ledger to see
    // whether its entity was generated here, and a novel's two hundred rows parsed
    // the same file two hundred times, 28 seconds for one Inbox read in a tab. The
    // cache is keyed on the paths and the texts, so any write reads fresh.
    pub fn load(out: &Path) -> Ledger {
        use std::hash::{Hash, Hasher};
        static CACHE: std::sync::Mutex<Option<(PathBuf, u64, Ledger)>> =
            std::sync::Mutex::new(None);
        let path = Self::path(out);
        let texts: Vec<(PathBuf, String)> = Self::files(out)
            .into_iter()
            .filter_map(|p| crate::vfs::read_to_string(&p).ok().map(|t| (p, t)))
            .collect();
        if texts.is_empty() {
            return Ledger::default();
        }
        // A file that exists but does not parse makes the whole ledger read as empty
        // and read-only; an empty file is no ledger yet.
        let parse = |texts: &[(PathBuf, String)]| -> Ledger {
            let unreadable = |why: String| {
                eprintln!("[jazyk] error: the ledger did not load: {}", why);
                Ledger {
                    unreadable: Some(why),
                    ..Default::default()
                }
            };
            let mut ledger = Ledger::default();
            for (p, text) in texts {
                if text.trim().is_empty() {
                    continue;
                }
                let kind = p
                    .parent()
                    .and_then(|d| d.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if *p == path {
                    match serde_norway::from_str::<Ledger>(text) {
                        // Read first; a version 3 ledger carries its rows inline.
                        Ok(l) => ledger = l,
                        Err(e) => return unreadable(format!("{}: {}", p.display(), e)),
                    }
                    continue;
                }
                let mut v: serde_norway::Value = match serde_norway::from_str(text) {
                    Ok(v) => v,
                    Err(e) => return unreadable(format!("{}: {}", p.display(), e)),
                };
                let id = match &mut v {
                    serde_norway::Value::Mapping(m) => {
                        m.remove("id").and_then(|i| i.as_str().map(String::from))
                    }
                    _ => None,
                };
                let Some(id) = id else {
                    return unreadable(format!("{}: a ledger row carries no id", p.display()));
                };
                let read = match kind.as_str() {
                    "entities" => serde_norway::from_value::<EntityGen>(v).map(|r| {
                        ledger.entities.insert(id, r);
                    }),
                    _ => serde_norway::from_value::<ReqRow>(v).map(|r| {
                        ledger.requirements.insert(id, r);
                    }),
                };
                if let Err(e) = read {
                    return unreadable(format!("{}: {}", p.display(), e));
                }
            }
            ledger
        };
        if crate::vfs::is_scoped() {
            return parse(&texts);
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        texts.hash(&mut h);
        let key = h.finish();
        if let Ok(guard) = CACHE.lock() {
            if let Some((p, k, l)) = guard.as_ref() {
                if *p == path && *k == key {
                    return l.clone();
                }
            }
        }
        let ledger: Ledger = parse(&texts);
        if let Ok(mut guard) = CACHE.lock() {
            *guard = Some((path, key, ledger.clone()));
        }
        ledger
    }

    // One file per row and the deliverable-wide file, each through a sibling temporary
    // file renamed into place and only when its bytes changed; the files of rows that
    // left the ledger are deleted. Refused while a file on disk did not parse.
    // Mirrors docs/consumers/gen.md#the-ledger.
    pub fn save(&self, out: &Path) -> Result<(), String> {
        let path = Self::path(out);
        // A newer store's ledger is never written (docs/compiler/graph.md#store-version).
        if let Some(why) = crate::store::newer_store(out) {
            eprintln!(
                "[jazyk] error: refusing to write {}: {}",
                path.display(),
                why
            );
            return Err(why);
        }
        if let Some(why) = &self.unreadable {
            let e = format!(
                "refusing to write {}: the ledger did not load: {}",
                path.display(),
                why
            );
            eprintln!("[jazyk] error: {}", e);
            return Err(e);
        }
        let errors: std::cell::RefCell<Vec<String>> = Default::default();
        let write = |p: &Path, text: &str| {
            if crate::vfs::read_to_string(p).ok().as_deref() == Some(text) {
                return;
            }
            if let Err(e) = crate::store::write_atomic(p, text.as_bytes()) {
                errors
                    .borrow_mut()
                    .push(format!("cannot write {}: {}", p.display(), e));
            }
        };
        let row = |id: &str, value: serde_norway::Result<serde_norway::Value>| -> Option<String> {
            let mut m = serde_norway::Mapping::new();
            m.insert("id".into(), id.into());
            if let Ok(serde_norway::Value::Mapping(fields)) = value {
                m.extend(fields);
            }
            serde_norway::to_string(&m).ok()
        };
        let mut written: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
        for (id, r) in &self.entities {
            let p = Self::row_path(out, "entities", id);
            if let Some(t) = row(id, serde_norway::to_value(r)) {
                write(&p, &t);
            }
            written.insert(p);
        }
        for (id, r) in &self.requirements {
            let p = Self::row_path(out, "requirements", id);
            if let Some(t) = row(id, serde_norway::to_value(r)) {
                write(&p, &t);
            }
            written.insert(p);
        }
        let mut wide = serde_norway::to_value(self).unwrap_or(serde_norway::Value::Null);
        if let serde_norway::Value::Mapping(m) = &mut wide {
            m.shift_remove("entities");
            m.shift_remove("requirements");
        }
        match serde_norway::to_string(&wide) {
            Ok(t) => write(&path, &t),
            Err(e) => {
                errors
                    .borrow_mut()
                    .push(format!("cannot serialize {}: {}", path.display(), e))
            }
        }
        for kind in ["entities", "requirements"] {
            for p in Self::row_files(out, kind) {
                if !written.contains(&p) {
                    crate::vfs::remove_file(&p).ok();
                }
            }
        }
        let errors = errors.into_inner();
        if errors.is_empty() {
            Ok(())
        } else {
            let e = errors.join("; ");
            eprintln!("[jazyk] error: the ledger did not save: {}", e);
            Err(format!("the ledger did not save: {}", e))
        }
    }

    // The ledger's writer lock, `<out>/.ledger.lock`: every load, modify, and save
    // runs under it, so two processes never lose each other's rows. Reentrant on one
    // thread; an in-memory tree belongs to one thread and takes no file lock. Never
    // held while a test command runs. Mirrors docs/consumers/gen.md#the-ledger.
    pub fn lock(out: &Path) -> LedgerLock {
        let path = out.join(".ledger.lock");
        let nested = LEDGER_LOCKS.with(|held| {
            let mut held = held.borrow_mut();
            let n = held.entry(path.clone()).or_insert(0);
            *n += 1;
            *n > 1
        });
        #[cfg(not(target_arch = "wasm32"))]
        if !nested && !crate::vfs::is_scoped() {
            crate::vfs::create_dir_all(out).ok();
            let opened = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path);
            match opened {
                Ok(file) => {
                    let started = std::time::Instant::now();
                    let mut warned = false;
                    while fs2::FileExt::try_lock_exclusive(&file).is_err() {
                        if !warned && started.elapsed() >= std::time::Duration::from_secs(10) {
                            eprintln!(
                                "[jazyk] waiting for the ledger writer holding {}",
                                path.display()
                            );
                            warned = true;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    return LedgerLock {
                        path,
                        file: Some(file),
                    };
                }
                Err(e) => eprintln!(
                    "[jazyk] warning: cannot open {}: {}; writing the ledger without the lock",
                    path.display(),
                    e
                ),
            }
        }
        let _ = nested;
        LedgerLock {
            path,
            #[cfg(not(target_arch = "wasm32"))]
            file: None,
        }
    }

    // Load, change, and save under the ledger lock. The change's error, or a failed
    // save, is the result; nothing is reported as recorded that did not land.
    pub fn update<R>(
        out: &Path,
        change: impl FnOnce(&mut Ledger) -> Result<R, String>,
    ) -> Result<R, String> {
        let _lock = Self::lock(out);
        let mut ledger = Self::load(out);
        let r = change(&mut ledger)?;
        ledger.save(out)?;
        Ok(r)
    }
}

pub struct LedgerLock {
    path: PathBuf,
    #[cfg(not(target_arch = "wasm32"))]
    file: Option<std::fs::File>,
}

thread_local! {
    static LEDGER_LOCKS: std::cell::RefCell<BTreeMap<PathBuf, usize>> = const { std::cell::RefCell::new(BTreeMap::new()) };
}

impl Drop for LedgerLock {
    fn drop(&mut self) {
        LEDGER_LOCKS.with(|held| {
            let mut held = held.borrow_mut();
            if let Some(n) = held.get_mut(&self.path) {
                *n -= 1;
                if *n == 0 {
                    held.remove(&self.path);
                }
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(f) = &self.file {
            fs2::FileExt::unlock(f).ok();
        }
    }
}
