//! Recoverable staged graph work. Runtime records, never committed project state.
//! See docs/compiler/graph.md#commit-conflicts.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub id: String,
    pub epoch: u64,
    pub owner: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub version: u32,
    pub receipt: Receipt,
    pub identity: Value,
    pub batch: String,
    pub goals: Vec<String>,
    pub snapshot_generation: u64,
    pub created_at: u64,
    pub last_activity: u64,
    #[serde(default)]
    pub heartbeat_at: u64,
    pub expires_at: u64,
    pub sequence: u64,
    pub state: String,
    pub targets: BTreeSet<String>,
    pub mutations: Vec<Value>,
    #[serde(default)]
    pub mutation_ids: Vec<u64>,
    #[serde(default)]
    pub next_mutation_id: u64,
    #[serde(default)]
    pub call_groups: Vec<Vec<u64>>,
    pub payload: Value,
    pub result: Value,
}
impl Record {
    pub fn active(&self) -> bool {
        self.state == "open" && self.expires_at > crate::sys::now_unix()
    }
    pub fn check(&self, receipt: &Receipt) -> Result<(), String> {
        if self.version != 1
            || self.state != "open"
            || !self.active()
            || self.receipt.epoch != receipt.epoch
            || self.receipt.owner != receipt.owner
        {
            return Err(include_str!(concat!(
                env!("OUT_DIR"),
                "/payloads/goals/prompts/inline/open-work/ownership-lost.md"
            ))
            .into());
        }
        Ok(())
    }
    pub fn summary(&self) -> Value {
        json!({"setId":self.identity["setId"],"workId":self.receipt.id,"epoch":self.receipt.epoch,"identity":self.identity,
            "batch":self.batch,"goals":self.goals,"snapshotGeneration":self.snapshot_generation,
            "createdAt":self.created_at,"lastActivity":self.last_activity,"heartbeatAt":self.heartbeat_at,"expiresAt":self.expires_at,
            "state":if self.state=="open" && !self.active(){"expired"}else{&self.state},"targets":self.targets})
    }
}

fn dir(out: &Path) -> PathBuf {
    out.join("local/open")
}
fn path(out: &Path, id: &str) -> Result<PathBuf, String> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err("invalid work id".into());
    }
    Ok(dir(out).join(id))
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn key(out: &Path, id: &str) -> String {
    format!("{}:{id}", crate::model::hash_hex(&out.to_string_lossy()))
}

/// Serializes short metadata operations, never an agent's execution.
pub(crate) struct Lock {
    #[cfg(not(target_arch = "wasm32"))]
    _file: Option<std::fs::File>,
}
impl Lock {
    pub fn acquire(out: &Path) -> Result<Self, String> {
        #[cfg(not(target_arch = "wasm32"))]
        if !crate::vfs::is_scoped() {
            crate::vfs::create_dir_all(out.join("local")).map_err(|e| e.to_string())?;
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(out.join("local/.open.lock"))
                .map_err(|e| e.to_string())?;
            let start = std::time::Instant::now();
            loop {
                match fs2::FileExt::try_lock_exclusive(&file) {
                    Ok(()) => break,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && start.elapsed().as_secs() < crate::limits::STORE_LOCK_WAIT_SECS =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(10))
                    }
                    Err(e) => return Err(format!("open-work-busy: {e}")),
                }
            }
            return Ok(Self { _file: Some(file) });
        }
        let _ = out;
        Ok(Self {
            #[cfg(not(target_arch = "wasm32"))]
            _file: None,
        })
    }
}

pub fn read(out: &Path, id: &str) -> Result<Record, String> {
    let p = path(out, id)?;
    #[cfg(not(target_arch = "wasm32"))]
    if crate::vfs::is_scoped() && crate::hosted::has_execution() {
        let store = crate::hosted::records::store()?;
        let meta = store
            .meta(crate::hosted::records::Stream::OpenWork, &key(out, id))?
            .ok_or("unknown open work")?;
        let record: Record = serde_json::from_value(meta).map_err(|e| e.to_string())?;
        return completed_from_journal(out, record);
    }
    let data = crate::vfs::read_to_string(p.join("events.jsonl")).map_err(|e| e.to_string())?;
    let mut last = None;
    for line in data.split_inclusive('\n') {
        if !line.ends_with('\n') {
            break;
        }
        let record: Record =
            serde_json::from_str(line).map_err(|e| format!("open-work-corrupt: {e}"))?;
        if record.version != 1 {
            return Err("unsupported open work version".into());
        }
        last = Some(record);
    }
    completed_from_journal(
        out,
        last.ok_or_else(|| "open-work-corrupt: no complete event".to_string())?,
    )
}

fn completed_from_journal(out: &Path, mut record: Record) -> Result<Record, String> {
    if record.version != 1 {
        return Err("unsupported open work version".into());
    }
    #[cfg(not(target_arch = "wasm32"))]
    if crate::vfs::is_scoped()
        && crate::hosted::has_execution()
        && record.state == "committed"
        && !record.payload.is_null()
    {
        // The host may not have checkpointed the graph after the commit tool yet.
        // Keep the accepted changeset recoverable until a durable workspace proves
        // its journal receipt. A host crash can then replay it instead of losing it.
        let mut published = false;
        for g in record.snapshot_generation + 1..=crate::store::read_generation(out) {
            if crate::goals::read_journal_entry(&crate::layout::journal_file(out, g), g)
                .is_some_and(|e| e.work_id.as_deref() == Some(&record.receipt.id))
            {
                published = true;
                break;
            }
        }
        if !published {
            record.state = "open".into();
            record.result = Value::Null;
            return Ok(record);
        }
    }
    if record.state == "open" && !crate::vfs::exists(&out.join("local/pending-commit.json")) {
        for generation in record.snapshot_generation + 1..=crate::store::read_generation(out) {
            if let Some(entry) = crate::goals::read_journal_entry(
                &crate::layout::journal_file(out, generation),
                generation,
            ) {
                if entry.work_id.as_deref() == Some(&record.receipt.id) {
                    record.state = "committed".into();
                    record.result = json!({"committed":true,"generation":generation,"workId":record.receipt.id,"overwrites":entry.overwrites,"replayed":true});
                    break;
                }
            }
        }
    }
    Ok(record)
}

/// Caller holds the metadata lock. Hosted compare-and-append is atomic across hosts.
pub fn save(out: &Path, record: &mut Record, expected: Option<u64>) -> Result<(), String> {
    let p = path(out, &record.receipt.id)?;
    let previous = match read(out, &record.receipt.id) {
        Ok(r) => Some(r),
        Err(error) if crate::vfs::exists(&p.join("events.jsonl")) => return Err(error),
        Err(_) => None,
    };
    if previous.as_ref().map(|r| r.sequence) != expected {
        return Err("open-work-moved: retry with current epoch".into());
    }
    record.sequence = expected.map_or(0, |n| n + 1);
    record.heartbeat_at = record
        .expires_at
        .saturating_sub(crate::limits::OPEN_WORK_TTL_SECS);
    #[cfg(not(target_arch = "wasm32"))]
    if crate::vfs::is_scoped() && crate::hosted::has_execution() {
        let result = crate::hosted::records::store()?.compare_open_work(
            &key(out, &record.receipt.id),
            expected,
            &serde_json::to_value(&*record).map_err(|e| e.to_string())?,
        );
        if result.is_ok() {
            crate::portable::emit_event("open-work", record.summary());
        }
        return result;
    }
    crate::vfs::create_dir_all(&p).map_err(|e| e.to_string())?;
    if expected.is_none()
        || previous
            .as_ref()
            .is_some_and(|r| r.receipt.epoch != record.receipt.epoch || r.state != record.state)
    {
        let kind = if record.state == "discarded" {
            "discard"
        } else if record.state == "committed" {
            "commit"
        } else if expected.is_none() {
            "open"
        } else {
            "adopt"
        };
        let audit = json!({"kind":kind,"workId":record.receipt.id,"epoch":record.receipt.epoch,"sequence":record.sequence,"state":record.state,"identity":record.identity,"at":crate::sys::now_unix()});
        let audit_dir = out.join("local/journal");
        crate::vfs::create_dir_all(&audit_dir).map_err(|e| e.to_string())?;
        crate::vfs::append_line(audit_dir.join("open-work.jsonl"), &audit.to_string())
            .map_err(|e| e.to_string())?;
    }
    let mut bytes = serde_json::to_vec(record).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        use std::io::{Read, Seek, Write};
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(p.join("events.jsonl"))
            .map_err(|e| e.to_string())?;
        let mut old = Vec::new();
        f.read_to_end(&mut old).map_err(|e| e.to_string())?;
        let complete = old.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        f.set_len(complete as u64).map_err(|e| e.to_string())?;
        f.seek(std::io::SeekFrom::End(0))
            .map_err(|e| e.to_string())?;
        f.write_all(&bytes)
            .and_then(|()| f.sync_all())
            .map_err(|e| e.to_string())?;
    } else {
        crate::vfs::append(p.join("events.jsonl"), &bytes).map_err(|e| e.to_string())?;
    }
    #[cfg(target_arch = "wasm32")]
    crate::vfs::append(p.join("events.jsonl"), &bytes).map_err(|e| e.to_string())?;
    if record.state != "open" {
        // Explicit close removes uncommitted payload history. The lifecycle journal
        // keeps identity-only receipts and the graph journal keeps committed changes.
        crate::store::write_atomic(&p.join("events.jsonl"), &bytes).map_err(|e| e.to_string())?;
    }
    // Events are authoritative; a stale projection is harmless after a crash.
    if let Err(error) = crate::store::write_atomic(
        &p.join("manifest.yaml"),
        format!("# Jazyk open work: persisted uncommitted graph changeset metadata.\n# yaml-language-server: $schema=https://jazyk.org/schemas/tools.json#/definitions/openWorkSummary\n{}",serde_norway::to_string(&record.summary()).map_err(|e|e.to_string())?).as_bytes(),
    ) {
        eprintln!("[jazyk] open work saved; manifest projection unavailable: {error}");
    }
    #[cfg(not(target_arch = "wasm32"))]
    if !crate::vfs::is_scoped() {
        std::fs::File::open(&p)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_arch = "wasm32")]
    crate::portable::emit_event("open-work", record.summary());
    Ok(())
}

pub fn list(out: &Path) -> Result<Vec<Record>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    if crate::vfs::is_scoped() && crate::hosted::has_execution() {
        let prefix = format!("{}:", crate::model::hash_hex(&out.to_string_lossy()));
        return crate::hosted::records::store()?
            .keys(crate::hosted::records::Stream::OpenWork)?
            .into_iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(_, v)| {
                let r: Record = serde_json::from_value(v).map_err(|e| e.to_string())?;
                completed_from_journal(out, r)
            })
            .collect();
    }
    let mut records = Vec::new();
    if !crate::vfs::exists(&dir(out)) {
        return Ok(records);
    }
    for entry in crate::vfs::read_dir(dir(out)).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if crate::vfs::is_dir(&entry.path()) {
            records.push(read(out, &entry.file_name().to_string_lossy())?);
        }
    }
    records.sort_by(|a, b| a.receipt.id.cmp(&b.receipt.id));
    Ok(records)
}

pub fn create(out: &Path, owner: &str, identity: Value, payload: Value) -> Result<Record, String> {
    let mut entropy = [0u8; 16];
    getrandom::fill(&mut entropy).map_err(|e| e.to_string())?;
    let id = format!(
        "w-{}",
        entropy
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let now = crate::sys::now_unix();
    let session = &payload["session"];
    let mut r = Record {
        version: 1,
        receipt: Receipt {
            id,
            epoch: 1,
            owner: owner.into(),
        },
        identity,
        batch: payload["id"].as_str().unwrap_or_default().into(),
        goals: session["scope"]["goals"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|g| g["id"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        snapshot_generation: session["snapshot"]["status"]["generation"]
            .as_u64()
            .unwrap_or(0),
        created_at: now,
        last_activity: now,
        heartbeat_at: now,
        expires_at: now + crate::limits::OPEN_WORK_TTL_SECS,
        sequence: 0,
        state: "open".into(),
        targets: BTreeSet::new(),
        mutations: Vec::new(),
        mutation_ids: Vec::new(),
        next_mutation_id: 0,
        call_groups: Vec::new(),
        payload,
        result: Value::Null,
    };
    update_targets(&mut r);
    save(out, &mut r, None)?;
    Ok(r)
}

pub fn update_targets(r: &mut Record) {
    let next = r.payload["session"]["staged"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut used = BTreeSet::new();
    let mut ids = Vec::new();
    let mut group = Vec::new();
    for mutation in &next {
        let old =
            r.mutations.iter().enumerate().find(|(i, m)| {
                !used.contains(i) && *m == mutation && r.mutation_ids.get(*i).is_some()
            });
        let id = if let Some((i, _)) = old {
            used.insert(i);
            r.mutation_ids[i]
        } else {
            let id = r.next_mutation_id;
            r.next_mutation_id += 1;
            group.push(id);
            id
        };
        ids.push(id);
    }
    r.call_groups
        .iter_mut()
        .for_each(|g| g.retain(|id| ids.contains(id)));
    r.call_groups.retain(|g| !g.is_empty());
    if !group.is_empty() {
        r.call_groups.push(group);
    }
    r.mutation_ids = ids;
    r.mutations = next;
    r.targets = r
        .mutations
        .iter()
        .flat_map(|m| mutation_targets(m, &r.payload["session"]["snapshot"]["graph"]))
        .collect();
    r.goals = r.payload["goals"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|g| g["id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
}

/// The explicit and collateral write footprint. References read for validation are not locks.
pub fn targets(m: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["id", "keep", "absorb"] {
        if let Some(id) = m[key].as_str() {
            out.push(id.into());
        }
    }
    if let Some(doc) = m["doc"].as_str() {
        out.push(
            m["section"]
                .as_str()
                .map_or_else(|| format!("input:{doc}"), |s| format!("{doc}#{s}")),
        );
    }
    if let Some(children) = m["children"].as_array() {
        out.extend(children.iter().filter_map(|v| v.as_str().map(String::from)));
    }
    for key in ["from", "to"] {
        if let (Some(doc), Some(sec)) = (m[key]["doc"].as_str(), m[key]["section"].as_str()) {
            out.push(format!("{doc}#{sec}"));
        }
    }
    out
}

fn mutation_targets(m: &Value, graph: &Value) -> BTreeSet<String> {
    let mut touched: BTreeSet<_> = targets(m).into_iter().collect();
    fn refers(v: &Value, id: &str) -> bool {
        match v {
            Value::String(s) => s == id,
            Value::Array(a) => a.iter().any(|v| refers(v, id)),
            Value::Object(m) => m.values().any(|v| refers(v, id)),
            _ => false,
        }
    }
    if m["op"].as_str().is_some_and(|op| {
        op.starts_with("delete_")
            || matches!(
                op,
                "merge_entities" | "merge_requirements" | "dissolve_entity"
            )
    }) {
        if let Some(id) = m["absorb"].as_str().or_else(|| m["id"].as_str()) {
            for kind in [
                "entities",
                "requirements",
                "criteria",
                "views",
                "diagnostics",
            ] {
                if let Some(nodes) = graph[kind].as_object() {
                    for (node, value) in nodes {
                        if refers(value, id) {
                            touched.insert(node.clone());
                        }
                    }
                }
            }
        }
    }
    touched
}

pub fn warnings(out: &Path, mine: &Record) -> Result<Vec<Value>, String> {
    let store = crate::store::Store::load(out);
    let canon = |targets: &BTreeSet<String>| {
        targets
            .iter()
            .map(|id| store.resolve_id(id).to_string())
            .collect::<BTreeSet<_>>()
    };
    let mine_targets = canon(&mine.targets);
    Ok(list(out)?.into_iter().filter(|r|r.receipt.id!=mine.receipt.id && r.active() && !canon(&r.targets).is_disjoint(&mine_targets)).map(|r|{
        let mutations:Vec<_>=r.mutations.iter().filter(|m|!canon(&mutation_targets(m,&r.payload["session"]["snapshot"]["graph"])).is_disjoint(&mine_targets)).collect();
        json!({"work":r.summary(),"mutations":mutations,"message":include_str!(concat!(env!("OUT_DIR"),"/payloads/goals/prompts/session/open-work-warning.md"))})
    }).collect())
}
