// The durable task queue: the reconciler's schedule as derived state, computable by any
// process from the docs on disk, the graph, the ledger, and status.yaml. What lets an
// external agent perform compilation with the same semantics as the internal loop.
// Mirrors docs/compiler/reconciler.md#the-task-queue.
use crate::model::WorkItem;
use crate::project::Project;
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

pub struct Queue {
    // Compilation tasks in dependency order: reconcile-doc by level, then
    // review-requirement, then review-entity. The first is ready; the rest name what
    // blocks them.
    pub compile: Vec<Value>,
    pub bind: Vec<Value>,
    pub generate: Vec<Value>,
    pub verify: Vec<Value>,
    pub verdict: String,
    // Open diagnostic counts by severity: the verdict never travels alone
    // (docs/compiler/compilation.md#convergence).
    pub open_diags: std::collections::BTreeMap<String, u64>,
    // An open judged diagnostic names a subject the graph no longer holds: the
    // deterministic tail has settling to do (docs/compiler/compilation.md#waves).
    pub dangling_diags: bool,
}

impl Queue {
    pub fn compile_empty(&self) -> bool {
        self.compile.is_empty()
    }

    // The compile queue as a tool answer. Zero tasks carries the verdict: nothing to
    // do is an answer. Mirrors docs/compiler/tools.md#compilation-tools.
    pub fn compilation_answer(&self) -> Value {
        if self.compile.is_empty() {
            let mut v = json!({
                "tasks": [],
                "verdict": self.verdict,
                "note": if self.generate.is_empty() {
                    "nothing to compile; the graph reflects the docs"
                } else {
                    "nothing to compile; generation has pending tasks (generation_tasks lists them)"
                },
            });
            if !self.open_diags.is_empty() {
                v["openDiagnostics"] = json!(self.open_diags);
            }
            return v;
        }
        // Sections render as full references, the same shape read_section takes.
        let tasks: Vec<Value> = self
            .compile
            .iter()
            .map(|t| {
                let mut t = t.clone();
                if let (Some(doc), Some(secs)) = (
                    t["target"].as_str().map(|s| s.to_string()),
                    t["dirtySections"].as_array().cloned(),
                ) {
                    t["dirtySections"] = json!(secs
                        .iter()
                        .filter_map(|s| s.as_str())
                        .map(|s| format!("{}#{}", doc, s))
                        .collect::<Vec<_>>());
                }
                t
            })
            .collect();
        json!({"tasks": tasks, "next": "begin_compilation claims the first ready task"})
    }

    // The work item behind a queue entry, by target; None when it is not in the queue.
    pub fn find(&self, target: Option<&str>) -> Option<WorkItem> {
        let entry = match target {
            Some(t) => self.compile.iter().find(|e| e["target"] == t || e["task"] == t)?,
            // The first task a worker can actually act on: ready, not gated, unclaimed.
            None => self
                .compile
                .iter()
                .find(|e| e["ready"] == true && e["gated"] != true && e["claimedBy"].is_null())?,
        };
        if entry["ready"] != true {
            return None;
        }
        Some(WorkItem {
            task: entry["kind"].as_str().unwrap_or_default().replace("reconcile-document", "reconcile-doc"),
            target: entry["target"].as_str().unwrap_or_default().to_string(),
            dirty_sections: entry["dirtySections"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default(),
            stale_anchors: entry["staleAnchors"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default(),
        })
    }
}

// Compute the queue. Syncs a throwaway in-memory store against the docs on disk (never
// saved), so the dirty set, uncovered sections, and stale anchors are current; reviews
// come from status.pending; generation and verification from the ledger.
pub fn compute(proj: &Project, out: &Path) -> Queue {
    let mut store = Store::load(out);
    let verdict = store.status.verdict.clone();
    let open_diags = store.open_diag_counts();
    let dangling_diags = store.has_dangling_diags();
    let gs = crate::gen::GenSettings::resolve(proj);
    let (parsed, links) = crate::reconcile::parse_all(proj);
    let mut dirty = store.sync_docs(&parsed);

    // Uncovered sections and quote-stale anchors beyond the diff: an interrupted build
    // left them; the queue is how any consumer resumes them. Same filters the fix-up
    // pass and the uncovered-section check use.
    let in_dirty: BTreeSet<String> = dirty.iter().map(|d| d.doc.clone()).collect();
    let mut extra: Vec<crate::store::DirtyDoc> = Vec::new();
    for (doc, rec) in &store.docs {
        let uncovered: Vec<String> = rec
            .sections
            .iter()
            .filter(|(r, sec)| {
                let skip = if sec.kind == "heading" { 1 } else { 0 };
                !rec.coverage.contains_key(*r) && !sec.raw.lines().skip(skip).all(|l| l.trim().is_empty())
            })
            .map(|(r, _)| r.clone())
            .collect();
        let stale: Vec<String> = store
            .graph
            .requirements
            .iter()
            .filter(|(_, q)| &q.source.doc == doc)
            .filter(|(_, q)| !store.quote_locates(&q.source.doc, &q.source.section, &q.source.quote))
            .map(|(id, _)| id.clone())
            .collect();
        if uncovered.is_empty() && stale.is_empty() {
            continue;
        }
        if let Some(d) = dirty.iter_mut().find(|d| &d.doc == doc) {
            for r in uncovered {
                if !d.dirty_sections.contains(&r) {
                    d.dirty_sections.push(r);
                }
            }
            for a in stale {
                if !d.stale_anchors.contains(&a) {
                    d.stale_anchors.push(a);
                }
            }
        } else if !in_dirty.contains(doc) {
            extra.push(crate::store::DirtyDoc { doc: doc.clone(), dirty_sections: uncovered, stale_anchors: stale });
        }
    }
    dirty.extend(extra);
    for d in &mut dirty {
        d.dirty_sections.sort();
    }
    dirty.retain(|d| !d.dirty_sections.is_empty() || !d.stale_anchors.is_empty());

    let levels = crate::reconcile::schedule_levels(&dirty, &links, proj);
    let mut compile: Vec<Value> = Vec::new();
    let mut first_level_seen = false;
    for (i, level) in levels.iter().enumerate() {
        for d in level {
            compile.push(json!({
                "kind": "reconcile-document",
                "target": d.doc,
                "dirtySections": d.dirty_sections,
                "staleAnchors": d.stale_anchors,
                "ready": !first_level_seen,
                "blockedBy": if first_level_seen { json!(format!("level {} documents reconcile first", i)) } else { Value::Null },
            }));
        }
        if !level.is_empty() {
            first_level_seen = true;
        }
    }
    let reconcile_open = !compile.is_empty();

    // Reviews owed, recorded at commit. Pair reviews before entity reviews.
    let pending_reqs: std::collections::BTreeSet<&String> = store.status.pending.requirements.iter().collect();
    let pair: Vec<&String> = store
        .status
        .pending
        .requirements
        .iter()
        .filter(|rid| store.pair_review_due(rid))
        // A pair scheduled from both ends runs once; the smaller id carries the task
        // and completion mirrors to the other.
        .filter(|rid| {
            let nbrs = store.pair_review_neighbors(rid);
            !(nbrs.len() == 1
                && nbrs[0].as_str() < rid.as_str()
                && pending_reqs.contains(&nbrs[0])
                && store.pair_review_neighbors(&nbrs[0]).iter().any(|x| x == *rid))
        })
        .collect();
    for rid in &pair {
        compile.push(json!({
            "kind": "review-requirement",
            "target": rid,
            "ready": !reconcile_open,
            "blockedBy": if reconcile_open { json!("reconcile tasks first") } else { Value::Null },
        }));
    }
    let pair_open = reconcile_open || !pair.is_empty();
    for eid in &store.status.pending.entities {
        if !store.graph.entities.contains_key(eid) {
            continue;
        }
        compile.push(json!({
            "kind": "review-entity",
            "target": eid,
            "ready": !pair_open,
            "blockedBy": if pair_open { json!("reconcile and pair-review tasks first") } else { Value::Null },
        }));
    }
    // A pending pair review whose requirement lost its neighbors is complete by
    // definition; it would otherwise block entity reviews forever.
    // (Cleared lazily at finish; listed nowhere.)

    // Parked work resumes first: it is ready by definition.
    for p in &store.status.parked {
        if compile.iter().any(|e| e["target"] == p.target.as_str()) {
            continue;
        }
        let kind = if p.task == "reconcile-doc" { "reconcile-document" } else { p.task.as_str() };
        compile.push(json!({
            "kind": kind,
            "target": p.target,
            "dirtySections": p.dirty_sections,
            "staleAnchors": p.stale_anchors,
            "ready": true,
            "parked": true,
        }));
    }

    let compile_open = !compile.is_empty();
    // Binding first: a requirement's statement must be final before a test encodes
    // it, and generation waits for its entity's bindings.
    // Mirrors docs/compiler/reconciler.md#the-task-queue.
    let bind: Vec<Value> = crate::bind::pending(&store, &gs)
        .into_iter()
        .map(|mut p| {
            p["ready"] = json!(!compile_open);
            if compile_open {
                p["blockedBy"] = json!("compilation first; the statement is not settled");
            }
            p
        })
        .collect();
    let bind_entities: BTreeSet<String> =
        bind.iter().filter_map(|p| p["entity"].as_str().map(String::from)).collect();
    let bind_reqs: BTreeSet<String> =
        bind.iter().filter_map(|p| p["requirement"].as_str().map(String::from)).collect();
    let generate: Vec<Value> = crate::gen::pending(&store, &gs)
        .into_iter()
        .map(|mut p| {
            let ent = p["entity"].as_str().unwrap_or_default().to_string();
            let blocked = compile_open || bind_entities.contains(&ent);
            p["ready"] = json!(!blocked);
            if blocked {
                p["blockedBy"] = json!(if compile_open {
                    "compilation first; the graph is not settled"
                } else {
                    "binding first; the entity's requirements are not all bound"
                });
            }
            p
        })
        .collect();
    let gen_entities: BTreeSet<String> =
        generate.iter().filter_map(|p| p["entity"].as_str().map(String::from)).collect();
    let verify: Vec<Value> = crate::verify::pending(&store, &gs, Some("stale"), None)
        .into_iter()
        // Unbound rows are bind work and unimplemented rows are generation work;
        // neither is a verification task (docs/consumers/bind.md#what-the-verdict-means).
        .filter(|p| p["status"] != "unimplemented" && p["reason"] != "not-generated")
        .filter(|p| !bind_reqs.contains(p["requirement"].as_str().unwrap_or_default()))
        .map(|mut p| {
            let ent = p["entity"].as_str().unwrap_or_default().to_string();
            let blocked = compile_open || gen_entities.contains(&ent) || bind_entities.contains(&ent);
            p["ready"] = json!(!blocked);
            if blocked {
                p["blockedBy"] = json!(if compile_open { "compilation first" } else { "binding and generation first" });
            }
            p
        })
        .collect();

    let mut q = Queue { compile, bind, generate, verify, verdict, open_diags, dangling_diags };

    // Manual mode gates work behind a release; leases mark claimed tasks. Reviews and
    // verification are never gated. Mirrors docs/compiler/control-plane.md.
    let control = crate::control::Control::load(proj, out);
    // A gated task is not ready, and blockedBy names the one release phrase used
    // everywhere: the flags never contradict each other.
    if control.compile == "manual" {
        for t in q.compile.iter_mut().filter(|t| t["kind"] == "reconcile-document") {
            let doc = t["target"].as_str().unwrap_or_default();
            let current = store.docs.get(doc).map(|r| r.content_hash.as_str()).unwrap_or_default();
            if control.released.compile.get(doc).map(String::as_str) != Some(current) {
                t["gated"] = json!(true);
                t["ready"] = json!(false);
                t["blockedBy"] = json!("awaiting release: `jazyk release compile` (or the GUI) approves it");
            }
        }
    }
    if control.generate == "manual" && control.released.generate != store.status.generation {
        // Bind tasks write test files into the deliverable, so they gate with
        // generation (docs/consumers/bind.md#when-binding-runs).
        for t in q.generate.iter_mut().chain(q.bind.iter_mut()) {
            t["gated"] = json!(true);
            t["ready"] = json!(false);
            t["blockedBy"] = json!("awaiting release: `jazyk release generate` (or the GUI) approves it");
        }
    }
    let leases = crate::control::task_leases(out);
    for t in q.compile.iter_mut().chain(q.bind.iter_mut()).chain(q.generate.iter_mut()) {
        let target = t["target"]
            .as_str()
            .or_else(|| t["requirement"].as_str())
            .or_else(|| t["entity"].as_str())
            .unwrap_or_default();
        if let Some(l) = leases.get(target) {
            t["claimedBy"] = json!(l.worker);
            t["ready"] = json!(false);
            t["blockedBy"] = json!(format!(
                "claimed by worker `{}` (the lease lapses {}s after its last heartbeat)",
                l.worker,
                crate::control::LEASE_TTL_SECS
            ));
        }
    }
    q
}

// Actionable work: ready, not gated, not claimed by someone else. What monitor
// notices count and what --once waits for.
pub fn actionable(tasks: &[Value]) -> usize {
    tasks
        .iter()
        .filter(|t| t["ready"] != false && t["gated"] != true && t["claimedBy"].is_null())
        .count()
}

// Gated work: queued but awaiting a release.
pub fn gated(tasks: &[Value]) -> usize {
    tasks.iter().filter(|t| t["gated"] == true).count()
}
