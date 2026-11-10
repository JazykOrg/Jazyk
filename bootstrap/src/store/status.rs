// The committed status and the local build file: the status split, change records,
// generation markers, and causes. Mirrors docs/compiler/graph.md#committed-and-local-state.
use super::*;

// Cheap generation read: one line of build.yaml, no node parsing. A checkout without
// build.yaml reads 0 here until its first commit writes the floor
// (docs/compiler/graph.md#the-generation).
pub fn read_generation(out: &Path) -> u64 {
    crate::vfs::read_to_string(out.join(BUILD_FILE))
        .ok()
        .and_then(|t| {
            t.lines()
                .find(|l| l.starts_with("generation:"))
                .and_then(|l| l.split(':').nth(1))
                .and_then(|v| v.trim().parse::<u64>().ok())
        })
        .unwrap_or(0)
}

// build.yaml: the local half of the status, what one checkout's runs leave behind.
// Mirrors docs/compiler/graph.md#storage-layout and #committed-and-local-state.
pub const BUILD_FILE: &str = "build.yaml";

// The status fields that live in build.yaml, never in the committed status.yaml.
const LOCAL_STATUS_KEYS: [&str; 5] = ["generation", "verdict", "costs", "spent", "diagnostics"];

#[derive(Serialize, Deserialize, Default)]
pub(super) struct BuildFile {
    #[serde(default)]
    pub(super) generation: u64,
    #[serde(default, skip_serializing_if = "Verdict::is_empty")]
    pub(super) verdict: Verdict,
    #[serde(default, skip_serializing_if = "Costs::is_empty")]
    pub(super) costs: Costs,
    #[serde(default)]
    pub(super) spent: Spent,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) diagnostics: BTreeMap<String, u64>,
    // Document path to the content hash of the source file at last reconcile.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) docs: BTreeMap<String, String>,
}

// The highest generation the committed status names: change records and lookalike
// verdicts. A load never starts below it, so ids minted from the generation never
// repeat ones a merged branch committed. Mirrors docs/compiler/graph.md#the-generation.
pub fn generation_floor(status: &Status) -> u64 {
    status
        .changes
        .iter()
        .map(|c| c.generation)
        .chain(status.lookalikes.iter().map(|l| l.generation))
        .max()
        .unwrap_or(0)
}

// The committed status.yaml: the status without its local fields and without the
// records, which live one per file. Mirrors docs/compiler/graph.md#storage-layout.
fn committed_status(status: &Status) -> serde_norway::Value {
    let mut v = serde_norway::to_value(status).unwrap_or(serde_norway::Value::Null);
    if let serde_norway::Value::Mapping(m) = &mut v {
        for k in LOCAL_STATUS_KEYS.iter().chain(STATUS_DIRS.iter()) {
            m.shift_remove(*k);
        }
    }
    v
}

// The generation on disk as a load computes it: build.yaml raised to the floor of
// status.yaml; None for a directory holding neither. Errors name a file that exists
// and does not parse.
pub(super) fn disk_generation(out: &Path) -> Result<Option<u64>, String> {
    let local = yaml_read::<BuildFile>(&out.join(BUILD_FILE))?.map(|b| b.generation);
    let floor = match yaml_read::<Status>(&out.join("status.yaml"))? {
        Some(_) => {
            let mut errors = Vec::new();
            let status = read_status(out, &mut errors);
            if let Some(e) = errors.into_iter().next() {
                return Err(e);
            }
            Some(generation_floor(&status))
        }
        None => None,
    };
    Ok(match (local, floor) {
        (None, None) => None,
        (l, f) => Some(l.unwrap_or(0).max(f.unwrap_or(0))),
    })
}

// The local generation markers: node id to `[created, updated]`.
// Mirrors docs/compiler/graph.md#storage-layout.
pub const MARKERS_FILE: &str = "markers.yaml";
pub(super) type Markers = BTreeMap<String, (Option<String>, Option<String>)>;

// The local causes of the open change records: `<kind> <subject>` to the id,
// generation, and mutation of each record in that file, in file order.
// Mirrors docs/compiler/graph.md#storage-layout.
pub const CAUSES_FILE: &str = "causes.yaml";
type Causes = BTreeMap<String, Vec<(String, u64, usize)>>;

fn cause_key(kind: &str, subject: &str) -> String {
    format!("{} {}", kind, subject)
}

// A change record as its file holds it: no local cause fields.
fn record_file_value(r: &ChangeRecord) -> serde_norway::Value {
    let mut v = serde_norway::to_value(r).unwrap_or(serde_norway::Value::Null);
    if let serde_norway::Value::Mapping(m) = &mut v {
        for k in ["id", "generation", "mutation"] {
            m.shift_remove(k);
        }
    }
    v
}

// The status record directories, committed. Mirrors docs/compiler/graph.md#storage-layout.
pub const STATUS_DIRS: [&str; 5] = ["changes", "parked", "failed", "alignment", "lookalikes"];

// One record file: `id` first, then the record's fields.
fn record_value<T: Serialize>(id: &str, record: &T) -> serde_norway::Value {
    let mut m = serde_norway::Mapping::new();
    m.insert("id".into(), id.into());
    if let Ok(serde_norway::Value::Mapping(fields)) = serde_norway::to_value(record) {
        for (k, v) in fields {
            if k.as_str() != Some("id") {
                m.insert(k, v);
            }
        }
    }
    serde_norway::Value::Mapping(m)
}

// Every YAML file under a directory, recursively, in path order.
fn yaml_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_yaml(dir, &mut out);
    out.retain(|p| {
        !p.file_name()
            .map(|n| n.to_string_lossy().starts_with('.'))
            .unwrap_or(true)
    });
    out.sort();
    out
}

// The status as the disk holds it: status.yaml, then every record file. A version 3
// status.yaml carries the records inline; both forms read into one status.
// Mirrors docs/compiler/graph.md#storage-layout and #store-version.
pub(super) fn read_status(out: &Path, errors: &mut Vec<String>) -> Status {
    let mut status: Status = shard(&out.join("status.yaml"), errors);
    let causes: Causes = shard(&out.join(CAUSES_FILE), errors);
    let mut changes: Vec<ChangeRecord> = Vec::new();
    for p in yaml_files(&out.join("changes")) {
        let items = match yaml_read::<serde_norway::Value>(&p) {
            Ok(Some(serde_norway::Value::Sequence(items))) => items,
            Ok(Some(v)) => vec![v],
            Ok(None) => Vec::new(),
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        let mut file: Vec<ChangeRecord> = Vec::new();
        for item in items {
            match serde_norway::from_value::<ChangeRecord>(item) {
                Ok(r) => file.push(r),
                Err(e) => errors.push(format!("{}: {}", p.display(), e)),
            }
        }
        // The cause is local: a record another checkout wrote reads with generation 0.
        let local = file
            .first()
            .and_then(|r| causes.get(&cause_key(&r.kind, &r.subject)))
            .filter(|c| c.len() == file.len());
        for (i, r) in file.iter_mut().enumerate() {
            match local {
                Some(c) => {
                    r.id = c[i].0.clone();
                    r.generation = c[i].1;
                    r.mutation = c[i].2;
                }
                None => {
                    r.id = format!("c0-{}", crate::ids::hash(&cause_key(&r.kind, &r.subject)));
                    r.generation = 0;
                    r.mutation = 0;
                }
            }
        }
        changes.extend(file);
    }
    // Records in the order they were written: by generation, then by file.
    changes.sort_by_key(|r| r.generation);
    status.changes.extend(changes);
    fn records<T: serde::de::DeserializeOwned>(dir: &Path, errors: &mut Vec<String>) -> Vec<T> {
        let mut out = Vec::new();
        for p in yaml_files(dir) {
            let read = yaml_read::<serde_norway::Value>(&p).and_then(|v| {
                let Some(mut v) = v else { return Ok(None) };
                if let serde_norway::Value::Mapping(m) = &mut v {
                    // The parked form carries its own id; the others are named by it.
                    if !dir.ends_with("parked") {
                        m.remove("id");
                    }
                }
                serde_norway::from_value::<T>(v)
                    .map(Some)
                    .map_err(|e| format!("{}: {}", p.display(), e))
            });
            match read {
                Ok(Some(r)) => out.push(r),
                Ok(None) => {}
                Err(e) => errors.push(e),
            }
        }
        out
    }
    status
        .parked
        .extend(records::<ParkedGoal>(&out.join("parked"), errors));
    status
        .failed
        .extend(records::<FailedGoal>(&out.join("failed"), errors));
    status
        .alignment
        .extend(records::<DocAlignment>(&out.join("alignment"), errors));
    status
        .lookalikes
        .extend(records::<LookalikeVerdict>(&out.join("lookalikes"), errors));
    status
}

// Write one file per status record and delete the files of records that cleared.
// Mirrors docs/compiler/graph.md#storage-layout.
fn save_status_records(out: &Path, status: &Status) {
    let mut written: BTreeSet<PathBuf> = BTreeSet::new();
    let mut put = |path: PathBuf, value: serde_norway::Value| {
        report_write(&path, write_yaml(&path, &value));
        written.insert(path);
    };
    let mut by_subject: BTreeMap<(String, String), Vec<&ChangeRecord>> = BTreeMap::new();
    for r in &status.changes {
        by_subject
            .entry((r.kind.clone(), r.subject.clone()))
            .or_default()
            .push(r);
    }
    let mut causes = Causes::new();
    for ((kind, subject), rs) in by_subject {
        let path = out
            .join("changes")
            .join(file_id(&kind))
            .join(format!("{}.yaml", node_stem(&subject).0));
        let value = if rs.len() == 1 {
            record_file_value(rs[0])
        } else {
            serde_norway::Value::Sequence(rs.iter().map(|r| record_file_value(r)).collect())
        };
        causes.insert(
            cause_key(&kind, &subject),
            rs.iter()
                .map(|r| (r.id.clone(), r.generation, r.mutation))
                .collect(),
        );
        put(path, value);
    }
    let causes_path = out.join(CAUSES_FILE);
    report_write(&causes_path, write_yaml(&causes_path, &causes));
    let name = |dir: &str, id: &str| out.join(dir).join(format!("{}.yaml", node_stem(id).0));
    for p in &status.parked {
        put(
            name("parked", &p.goal.id),
            serde_norway::to_value(p).unwrap_or(serde_norway::Value::Null),
        );
    }
    for f in &status.failed {
        put(name("failed", &f.goal.id), record_value(&f.goal.id, f));
    }
    for a in &status.alignment {
        put(name("alignment", &a.doc), record_value(&a.doc, a));
    }
    for l in &status.lookalikes {
        put(name("lookalikes", &l.pair), record_value(&l.pair, l));
    }
    for dir in STATUS_DIRS {
        for p in yaml_files(&out.join(dir)) {
            if !written.contains(&p) {
                if let Err(e) = crate::vfs::remove_file(&p) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        eprintln!("[jazyk] error: cannot remove {}: {}", p.display(), e);
                    }
                }
            }
        }
    }
}

// A document record as its file holds it: no whole-file hash, no line numbers.
// Mirrors docs/compiler/graph.md#storage-layout.
pub(super) fn doc_record_value(rec: &DocRecord) -> serde_norway::Value {
    let mut v = serde_norway::to_value(rec).unwrap_or(serde_norway::Value::Null);
    if let serde_norway::Value::Mapping(m) = &mut v {
        m.shift_remove("contentHash");
        if let Some(serde_norway::Value::Mapping(sections)) = m.get_mut("sections") {
            for (_, sec) in sections.iter_mut() {
                if let serde_norway::Value::Mapping(sm) = sec {
                    sm.shift_remove("lines");
                }
            }
        }
    }
    v
}

// Recompute each section's line range from the raw texts in document order: the
// heading kinds tile the document in tree order, a fenced block sits inside its holder.
// A record that still carries its lines (version 3) keeps them.
// Mirrors docs/compiler/graph.md#storage-layout and docs/compiler/parsing.md#section-tree.
pub fn fill_section_lines(rec: &mut DocRecord) {
    let holder = |k: &str| matches!(k, "preamble" | "root" | "heading");
    if rec.sections.values().all(|s| s.lines[1] > 0) {
        return;
    }
    let mut children: BTreeMap<Option<String>, Vec<(usize, bool, String)>> = BTreeMap::new();
    for (r, s) in &rec.sections {
        if holder(&s.kind) {
            children.entry(s.parent.clone()).or_default().push((
                s.order,
                s.kind != "preamble",
                r.clone(),
            ));
        }
    }
    for list in children.values_mut() {
        list.sort_by_key(|a| (a.1, a.0));
    }
    let mut order: Vec<String> = Vec::new();
    let mut stack: Vec<String> = children
        .get(&None)
        .map(|l| l.iter().rev().map(|(_, _, r)| r.clone()).collect())
        .unwrap_or_default();
    while let Some(r) = stack.pop() {
        if let Some(kids) = children.get(&Some(r.clone())) {
            for (_, _, k) in kids.iter().rev() {
                stack.push(k.clone());
            }
        }
        order.push(r);
    }
    let mut line = 0usize;
    let mut starts: BTreeMap<String, usize> = BTreeMap::new();
    for r in &order {
        if let Some(s) = rec.sections.get_mut(r) {
            let n = s.raw.split('\n').count();
            s.lines = [line, line + n];
            starts.insert(r.clone(), line);
            line += n;
        }
    }
    let blocks: Vec<String> = rec
        .sections
        .iter()
        .filter(|(_, s)| !holder(&s.kind))
        .map(|(r, _)| r.clone())
        .collect();
    for r in blocks {
        let (parent, raw) = {
            let s = &rec.sections[&r];
            (s.parent.clone(), s.raw.clone())
        };
        let Some(p) =
            parent.and_then(|p| rec.sections.get(&p).map(|ps| (ps.lines[0], ps.raw.clone())))
        else {
            continue;
        };
        let at =
            p.1.find(&raw)
                .map(|i| p.1[..i].matches('\n').count())
                .unwrap_or(0);
        let n = raw.split('\n').count();
        if let Some(s) = rec.sections.get_mut(&r) {
            s.lines = [p.0 + at, p.0 + at + n];
        }
    }
}

impl Store {
    // The status alone, inside the commit marker. Callers hold the lock and checked
    // the refusal. Every status write stamps the store version.
    pub(super) fn save_status(&self) {
        if let Some(why) = self.unreadable() {
            eprintln!(
                "[jazyk] error: refusing to write {}: {}",
                self.out.display(),
                why
            );
            return;
        }
        let _marker = CommitMarker::begin(&self.out);
        self.save_status_files();
    }

    // A status write outside a changeset (a build's opening, absorbed derivations,
    // parked and failed goals, the verdict): under the lock and refused like a commit,
    // so a store that is unreadable, of another version, or behind the disk never
    // overwrites what it has not seen. Mirrors docs/compiler/graph.md#concurrency.
    pub fn write_status(&self, what: &str) -> Result<(), String> {
        let _flock = FileLock::acquire(&self.out);
        if let Some(why) = self.refuse(what) {
            return Err(why);
        }
        self.save_status();
        Ok(())
    }

    pub(super) fn save_status_files(&self) {
        // status.yaml, then build.yaml: the generation there is the commit point.
        // Mirrors docs/compiler/graph.md#writes-and-unreadable-files.
        let mut status = self.status.clone();
        status.version = STORE_VERSION;
        save_status_records(&self.out, &status);
        let path = self.out.join("status.yaml");
        report_write(&path, write_yaml(&path, &committed_status(&status)));
        let build = BuildFile {
            generation: status.generation,
            verdict: status.verdict,
            costs: status.costs,
            spent: status.spent,
            diagnostics: status.diagnostics,
            docs: self
                .docs
                .iter()
                .filter(|(_, r)| !r.content_hash.is_empty())
                .map(|(d, r)| (d.clone(), r.content_hash.clone()))
                .collect(),
        };
        // Every file the commit point covers is on disk before it moves.
        sync_touched_dirs();
        let path = self.out.join(BUILD_FILE);
        report_write(&path, write_yaml(&path, &build));
        sync_touched_dirs();
    }

    // The reconciler re-derives the board after a commit and records the goals the
    // commit opened on that generation's journal entry, under the lock.
    pub fn record_opened_goals(&mut self, generation: u64, opened: Vec<OpenedGoal>) {
        let _flock = FileLock::acquire(&self.out);
        let path = self.journal_path(generation);
        let Some(mut entry) = yaml_to::<JournalEntry>(&path) else {
            return;
        };
        entry.opened_goals = opened;
        report_write(&path, write_yaml(&path, &entry));
    }

    // Resolving a goal clears the records it stood on, except the created and revised
    // records a pair stands on: those keep their other candidates, the commit marked
    // the partner judged, and the sweep clears them when none remains.
    // Mirrors docs/compiler/graph.md#change-records.
    pub fn clear_resolved(&mut self, ids: &[String]) {
        let ids: Vec<String> = ids
            .iter()
            .filter(|id| {
                !self.status.changes.iter().any(|c| {
                    c.id == **id
                        && matches!(c.kind.as_str(), CHANGE_REQ_CREATED | CHANGE_REQ_REVISED)
                })
            })
            .cloned()
            .collect();
        self.clear_changes(&ids);
    }

    // Clear the named records. Persists at once.
    pub fn clear_changes(&mut self, ids: &[String]) {
        if ids.is_empty() {
            return;
        }
        let _flock = FileLock::acquire(&self.out);
        if self.refuse("clear-changes").is_some() {
            return;
        }
        self.status.changes.retain(|c| !ids.contains(&c.id));
        self.save_status();
    }

    // Land a batch of records in status.yaml (superseding same-kind same-subject ones).
    pub(super) fn commit_records(&mut self, batch: RecordBatch) -> Vec<ChangeRecord> {
        let records = batch.records;
        for r in &records {
            self.status.record_change(r.clone());
        }
        records
    }
}
