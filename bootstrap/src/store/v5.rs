// Store version 5: the committed set holds the current graph and its decisions, never
// work. Work is computed from what is there; what one machine knows beyond that (the
// open change records it holds, parked goals, pending alignment) is its local build
// progress. Mirrors docs/compiler/graph.md#what-needs-building and
// docs/compiler/jazyk-folder.md.
use super::*;
use crate::layout;

// ---- headers ----

// The two fixed comment lines a file of a given kind starts with: what it is, and the
// schema an editor follows. Chosen by the file's path, so every writer gets them.
// Mirrors docs/compiler/jazyk-folder.md#every-file-begins-with-what-it-is.
pub(super) fn header_for(path: &Path) -> Option<String> {
    let parts: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    let n = parts.len();
    let name = parts.last()?.as_str();
    let parent = if n >= 2 { parts[n - 2].as_str() } else { "" };
    let grand = if n >= 3 { parts[n - 3].as_str() } else { "" };
    const EDIT: &str = " Written by jazyk: edit the docs, not this file.";
    const LOCAL: &str = " Written by jazyk, never committed.";
    const PLAIN: &str = " Written by jazyk.";
    let (sentence, tail, schema): (&str, &str, Option<&str>) = match (grand, parent, name) {
        (_, _, "store.yaml") => ("Jazyk store: the version of this set's files.", PLAIN, Some("store")),
        ("local", _, "build.yaml") | (_, "local", "build.yaml") => (
            "Jazyk build state: this machine's commit counter, last verdict, and costs.",
            LOCAL,
            Some("build"),
        ),
        (_, "local", "run.yaml") => (
            "Jazyk build progress: open work this machine knows of, parked goals, pending anchor moves.",
            LOCAL,
            None,
        ),
        (_, "journal", _) if grand == "local" => (
            "Jazyk journal entry: one commit to the graph and why it happened.",
            LOCAL,
            Some("journal"),
        ),
        ("graph", "entities", _) => ("Jazyk entity: one concept your docs name.", EDIT, Some("entity")),
        ("graph", "requirements", _) => (
            "Jazyk requirement: one obligation stated in your docs.",
            EDIT,
            Some("requirement"),
        ),
        ("graph", "criteria", _) => (
            "Jazyk criterion: how one would observe a requirement is met.",
            EDIT,
            Some("criterion"),
        ),
        ("graph", "views", _) => ("Jazyk view: what one diagram shows, curated.", EDIT, Some("view")),
        ("graph", "diagnostics", _) => (
            "Jazyk diagnostic: a finding or question about your docs.",
            EDIT,
            Some("diagnostic"),
        ),
        ("graph", "redirects", _) => ("Jazyk redirect: an id merged into another.", EDIT, Some("redirect")),
        ("graph", "default-views", _) => (
            "Jazyk default view: what one diagram shows, by rule.",
            LOCAL,
            Some("view"),
        ),
        ("graph", "relationships", _) => (
            "Jazyk relationship: connections between two entities, derived from requirements.",
            LOCAL,
            Some("relationship"),
        ),
        ("graph", "state-machines", _) => (
            "Jazyk state machine: one entity's states, derived from requirements.",
            LOCAL,
            Some("state-machine"),
        ),
        ("decisions", "lookalikes", _) => (
            "Jazyk decision: two similar entities judged to be different things.",
            PLAIN,
            Some("lookalike"),
        ),
        ("decisions", "pairs", _) => (
            "Jazyk decision: two requirements judged against each other.",
            PLAIN,
            Some("pair"),
        ),
        ("decisions", "reviews", _) => (
            "Jazyk decision: an entity reviewed against its requirements.",
            PLAIN,
            Some("review"),
        ),
        ("decisions", "conformance", _) => (
            "Jazyk decision: an instance checked against its type.",
            PLAIN,
            Some("conformance"),
        ),
        ("decisions", "structure", _) => (
            "Jazyk decision: a requirement that states no structural link between its entities.",
            PLAIN,
            Some("structure"),
        ),
        ("decisions", "curation", _) => (
            "Jazyk decision: the nodes a curated view's query matched, reviewed.",
            PLAIN,
            Some("curation"),
        ),
        _ if parts.iter().any(|p| p == "sources") && name.ends_with(".yaml") => (
            "Jazyk source: a document as the graph last reflected it, used to spot edits.",
            PLAIN,
            Some("source"),
        ),
        _ => return None,
    };
    let mut h = format!("# {}{}\n", sentence, tail);
    if let Some(s) = schema {
        h.push_str(&format!(
            "# yaml-language-server: $schema=https://jazyk.org/schemas/{}.json\n",
            s
        ));
    }
    Some(h)
}

// ---- hashes ----

// A statement's hash: punctuation-insensitive, so a case or punctuation edit that
// writes no revision keeps its decisions.
pub fn statement_hash(statement: &str) -> String {
    hash_hex(&normalize_statement(statement))
}

// An entity's fact hash: what a review judges. Its own fields, its mentions, its
// children, the requirements naming it, and the edges touching it, order-insensitive,
// so nothing but a change a review would see moves it.
// Mirrors docs/compiler/goals/review-entity.md#created-when.
pub fn entity_fact_hash(store: &Store, id: &str) -> String {
    let Some(e) = store.graph.entities.get(id) else {
        return String::new();
    };
    let mut aliases = e.aliases.clone();
    aliases.sort();
    // Only the mentions the entity's file states: a mention that is the source of a
    // requirement naming the entity comes back at load, so it is covered by `reqs`.
    let by_reference: Vec<&SourceRef> = store
        .graph
        .requirements
        .values()
        .filter(|r| r.entities.iter().any(|x| x == id))
        .filter_map(|r| r.source.as_ref())
        .collect();
    let mut mentions: Vec<String> = e
        .mentions
        .iter()
        .filter(|m| !by_reference.contains(m))
        .map(|m| format!("{}#{}\n{}", m.doc, m.section, m.quote))
        .collect();
    mentions.sort();
    let mut children: Vec<&String> = store
        .graph
        .entities
        .iter()
        .filter(|(_, c)| c.parent.as_deref() == Some(id))
        .map(|(cid, _)| cid)
        .collect();
    children.sort();
    let mut reqs: Vec<&String> = Vec::new();
    let mut edges: Vec<String> = Vec::new();
    for (rid, r) in &store.graph.requirements {
        if r.entities.iter().any(|x| x == id) {
            reqs.push(rid);
        }
        for ed in &r.edges {
            if ed.a == id || ed.b == id {
                edges.push(format!(
                    "{}>{}:{}",
                    ed.a,
                    ed.b,
                    ed.rel_type.as_deref().unwrap_or("")
                ));
            }
        }
    }
    edges.sort();
    let v = serde_json::json!({
        "name": e.name,
        "aliases": aliases,
        "definition": e.definition,
        "stereotype": e.stereotype,
        "parent": e.parent,
        "attributes": e.attributes,
        "mentions": mentions,
        "children": children,
        "requirements": reqs,
        "edges": edges,
    });
    hash_hex(&v.to_string())
}

// The two ids of a pair, smaller first, joined by `~`.
pub fn pair_key(a: &str, b: &str) -> String {
    if a <= b {
        format!("{}~{}", a, b)
    } else {
        format!("{}~{}", b, a)
    }
}

// ---- decision files ----

fn decision_value<T: Serialize>(id: &str, body: &T) -> serde_norway::Value {
    let mut m = serde_norway::Mapping::new();
    m.insert("id".into(), id.into());
    if let Ok(serde_norway::Value::Mapping(fields)) = serde_norway::to_value(body) {
        for (k, v) in fields {
            if k.as_str() != Some("id") {
                m.insert(k, v);
            }
        }
    }
    serde_norway::Value::Mapping(m)
}

// Every decision file of one kind, `(id, value)`; the id is read from the file.
fn read_decision_files(dir: &Path, errors: &mut Vec<String>) -> Vec<(String, serde_json::Value)> {
    let mut files = Vec::new();
    collect_yaml(dir, &mut files);
    files.sort();
    let mut out = Vec::new();
    for p in files {
        if p.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        match yaml_read::<serde_json::Value>(&p) {
            Ok(Some(v)) => match v.get("id").and_then(|i| i.as_str()) {
                Some(id) => out.push((id.to_string(), v.clone())),
                None => errors.push(format!("{}: a decision carries no id", p.display())),
            },
            Ok(None) => {}
            Err(e) => errors.push(e),
        }
    }
    out
}

pub(super) fn load_decisions(
    out: &Path,
    errors: &mut Vec<String>,
) -> (Decisions, Vec<LookalikeVerdict>) {
    let mut d = Decisions::default();
    let mut lookalikes = Vec::new();
    for (id, v) in read_decision_files(&layout::decisions_dir(out, "lookalikes"), errors) {
        lookalikes.push(LookalikeVerdict {
            pair: v
                .get("pair")
                .and_then(|x| x.as_str())
                .unwrap_or(&id)
                .to_string(),
            verdict: v
                .get("verdict")
                .and_then(|x| x.as_str())
                .unwrap_or(LOOKALIKE_SEPARATE)
                .to_string(),
            score: v.get("score").and_then(|x| x.as_f64()).unwrap_or(0.0),
            generation: 0,
        });
    }
    for (id, v) in read_decision_files(&layout::decisions_dir(out, "pairs"), errors) {
        if let Ok(p) = serde_json::from_value::<PairDecision>(v) {
            d.pairs.insert(id, p);
        }
    }
    for (id, v) in read_decision_files(&layout::decisions_dir(out, "reviews"), errors) {
        if let Some(h) = v.get("facts").and_then(|x| x.as_str()) {
            d.reviews.insert(id, h.to_string());
        }
    }
    for (id, v) in read_decision_files(&layout::decisions_dir(out, "conformance"), errors) {
        if let Some(m) = v.get("facts").and_then(|x| x.as_object()) {
            d.conformance.insert(
                id,
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect(),
            );
        }
    }
    for (id, v) in read_decision_files(&layout::decisions_dir(out, "structure"), errors) {
        if let Some(h) = v.get("statement").and_then(|x| x.as_str()) {
            d.structure.insert(id, h.to_string());
        }
    }
    for (id, v) in read_decision_files(&layout::decisions_dir(out, "curation"), errors) {
        if let Some(a) = v.get("matched").and_then(|x| x.as_array()) {
            d.curation.insert(
                id,
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect(),
            );
        }
    }
    (d, lookalikes)
}

// Write every decision file and delete the files of decisions that lapsed.
pub(super) fn save_decisions(out: &Path, d: &Decisions, lookalikes: &[LookalikeVerdict]) {
    let mut written: BTreeSet<PathBuf> = BTreeSet::new();
    let mut put = |kind: &str, id: &str, value: serde_norway::Value| {
        let path = layout::decisions_dir(out, kind).join(format!("{}.yaml", node_stem(id).0));
        report_write(&path, write_yaml(&path, &value));
        written.insert(path);
    };
    for l in lookalikes {
        put(
            "lookalikes",
            &l.pair,
            decision_value(
                &l.pair,
                &serde_json::json!({"verdict": l.verdict, "score": l.score}),
            ),
        );
    }
    for (id, p) in &d.pairs {
        put("pairs", id, decision_value(id, p));
    }
    for (id, h) in &d.reviews {
        put(
            "reviews",
            id,
            decision_value(id, &serde_json::json!({ "facts": h })),
        );
    }
    for (id, m) in &d.conformance {
        put(
            "conformance",
            id,
            decision_value(id, &serde_json::json!({ "facts": m })),
        );
    }
    for (id, h) in &d.structure {
        put(
            "structure",
            id,
            decision_value(id, &serde_json::json!({ "statement": h })),
        );
    }
    for (id, m) in &d.curation {
        put(
            "curation",
            id,
            decision_value(id, &serde_json::json!({ "matched": m })),
        );
    }
    for kind in DECISION_KINDS {
        let mut files = Vec::new();
        collect_yaml(&layout::decisions_dir(out, kind), &mut files);
        for p in files {
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

// ---- build progress ----

// What one machine knows beyond the committed state: the open change records it holds,
// the goals a build parked, the pending alignment proposals, the anchors placed for
// re-evaluation. Local, never committed; a clone or a merge computes its work again.
// Mirrors docs/compiler/graph.md#a-builds-progress.
#[derive(Serialize, Deserialize, Default)]
pub(super) struct RunFile {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) changes: Vec<ChangeRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) parked: Vec<ParkedGoal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) alignment: Vec<DocAlignment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) reevaluate: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) roots: Vec<String>,
}

// A finding (generation 0, never caused by a commit here) stays out: the next load
// derives it again from the committed files.
pub(super) fn save_run(out: &Path, status: &Status) {
    let run = RunFile {
        changes: status
            .changes
            .iter()
            .filter(|c| c.generation != 0)
            .cloned()
            .collect(),
        parked: status.parked.clone(),
        alignment: status.alignment.clone(),
        reevaluate: status.reevaluate.clone(),
        roots: status.roots.clone(),
    };
    let path = layout::run_file(out);
    report_write(&path, write_yaml(&path, &run));
}

// ---- the projection: decisions and failures from the state in memory ----

impl Store {
    // Bring the decisions and the `goal-failed` diagnostics in line with what is open
    // in memory: whatever has no open record was judged, so its decision holds the
    // hashes it was judged under; whatever has one is pending, so its decision goes. A
    // failed goal is a `goal-failed` diagnostic; one no longer failed is removed.
    // Runs before every save. Mirrors docs/compiler/graph.md#what-needs-building.
    pub(super) fn project_decisions(&mut self) {
        let open = |kind: &[&str], subject: &str, changes: &[ChangeRecord]| {
            changes
                .iter()
                .any(|c| kind.contains(&c.kind.as_str()) && c.subject == subject)
        };
        let changes = self.status.changes.clone();

        // Pairs: a neighbor pair neither side lists as pending was judged.
        let scoring = self.pair_scoring();
        let mut pending: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for c in &changes {
            if c.kind == CHANGE_REQ_CREATED || c.kind == CHANGE_REQ_REVISED {
                let p: BTreeSet<String> = self
                    .pair_review_candidates_with(c, &scoring)
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect();
                pending.entry(c.subject.clone()).or_default().extend(p);
            }
        }
        let mut pairs: BTreeMap<String, PairDecision> = BTreeMap::new();
        for (rid, r) in &self.graph.requirements {
            for other in self.pair_review_neighbors(rid) {
                let waiting = pending.get(rid).is_some_and(|p| p.contains(&other))
                    || pending.get(&other).is_some_and(|p| p.contains(rid));
                if waiting {
                    continue;
                }
                let Some(o) = self.graph.requirements.get(&other) else {
                    continue;
                };
                let key = pair_key(rid, &other);
                let verdict = self
                    .decisions
                    .pairs
                    .get(&key)
                    .map(|p| p.verdict.clone())
                    .unwrap_or_else(|| "consistent".to_string());
                let statements = BTreeMap::from([
                    (rid.clone(), statement_hash(&r.statement)),
                    (other.clone(), statement_hash(&o.statement)),
                ]);
                pairs.insert(
                    key,
                    PairDecision {
                        verdict,
                        statements,
                    },
                );
            }
        }
        self.decisions.pairs = pairs;

        // Reviews: an entity with no open entity-changed record was reviewed as it is.
        let mut reviews = BTreeMap::new();
        for id in self.graph.entities.keys() {
            if !open(&[CHANGE_ENTITY], id, &changes) {
                reviews.insert(id.clone(), entity_fact_hash(self, id));
            }
        }
        self.decisions.reviews = reviews;

        // Conformance: an instance with no open instance-changed record conforms.
        let mut conformance = BTreeMap::new();
        for (inst, ty) in crate::derive::instance_types(self) {
            if !open(&[CHANGE_INSTANCE], &inst, &changes) {
                conformance.insert(
                    inst.clone(),
                    BTreeMap::from([
                        (inst.clone(), entity_fact_hash(self, &inst)),
                        (ty.clone(), entity_fact_hash(self, &ty)),
                    ]),
                );
            }
        }
        self.decisions.conformance = conformance;

        // Structure: a requirement naming several entities with no edges and no open
        // edges-missing record was judged not structural.
        let mut structure = BTreeMap::new();
        for (rid, r) in &self.graph.requirements {
            if r.entities.len() >= 2
                && r.edges.is_empty()
                && !open(&[CHANGE_EDGES_MISSING], rid, &changes)
            {
                structure.insert(rid.clone(), statement_hash(&r.statement));
            }
        }
        self.decisions.structure = structure;

        // Curation: a curated view's query matches, reviewed unless a query-match
        // record is open on the view.
        let instances = crate::derive::instance_types(self);
        let mut curation = BTreeMap::new();
        for (vid, v) in &self.graph.views {
            if v.default {
                continue;
            }
            let Some(q) = &v.query else { continue };
            if open(&[CHANGE_QUERY_MATCH], vid, &changes) {
                if let Some(prev) = self.decisions.curation.get(vid) {
                    curation.insert(vid.clone(), prev.clone());
                }
                continue;
            }
            let matched = crate::derive::query_matches(self, q, &v.excluded, &instances);
            curation.insert(vid.clone(), matched);
        }
        self.decisions.curation = curation;

        // Failures: one open goal-failed diagnostic per failed goal.
        let mut keep: BTreeSet<String> = BTreeSet::new();
        for f in self.status.failed.clone() {
            let did = format!("diag:goal-failed-{}", crate::ids::hash_full(&f.goal.id));
            keep.insert(did.clone());
            let mut goal = f.goal.clone();
            goal.cause = None;
            goal.hints.clear();
            goal.state = GoalState::default();
            let diagnostic = Diagnostic {
                rule: RULE_GOAL_FAILED.to_string(),
                severity: "warning".to_string(),
                subjects: vec![goal.target.clone()],
                message: format!("{} on {} failed: {}", goal.kind, goal.target, f.reason),
                reasoning: Some(f.reason.clone()),
                lifecycle: "open".to_string(),
                triage: None,
                prompt: None,
                answer: None,
                goal: Some(Box::new(goal)),
                created: None,
                updated: None,
            };
            let same = self.graph.diagnostics.get(&did).is_some_and(|d| {
                d.lifecycle == "open"
                    && d.goal == diagnostic.goal
                    && d.reasoning == diagnostic.reasoning
            });
            if !same {
                self.graph.diagnostics.insert(did, diagnostic);
            }
        }
        self.graph
            .diagnostics
            .retain(|id, d| d.rule != RULE_GOAL_FAILED || keep.contains(id));
    }

    // ---- findings: the work the committed state shows ----

    // Compute the open work the graph, its decisions, and its sources show, and add
    // every finding this machine's records do not already hold. Runs at every load.
    // Mirrors docs/compiler/graph.md#change-records.
    pub(super) fn add_findings(&mut self) {
        let mut found: Vec<ChangeRecord> = Vec::new();
        let rec = |kind: &str, subject: &str, via: &str, detail: serde_json::Value| {
            let mut r = ChangeRecord::new(0, 0, 0, kind, subject, via);
            r.detail = detail;
            r
        };

        // Sections the graph does not reflect yet: no coverage mark.
        for (doc, d) in &self.docs {
            for (r, sec) in &d.sections {
                if d.coverage.contains_key(r) {
                    continue;
                }
                let mut detail = serde_json::json!({ "doc": doc, "section": r });
                if let Some(prev) = &sec.reflected {
                    detail = section_diff_detail(
                        &Section {
                            raw: prev.clone(),
                            ..sec.clone()
                        },
                        &sec.raw,
                    );
                    detail["doc"] = serde_json::json!(doc);
                    detail["section"] = serde_json::json!(r);
                }
                found.push(rec(
                    CHANGE_SECTION_DIRTY,
                    &format!("{}#{}", doc, r),
                    "section",
                    detail,
                ));
            }
        }

        // Anchors whose quote no longer locates in its section.
        let locates = |doc: &str, section: &str, quote: &str| {
            self.docs
                .get(doc)
                .and_then(|d| d.sections.get(section))
                .is_some_and(|s| text_contains(&s.raw, quote))
        };
        let mut stale: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (rid, r) in &self.graph.requirements {
            if let Some(src) = &r.source {
                if self.docs.contains_key(&src.doc) && !locates(&src.doc, &src.section, &src.quote)
                {
                    stale
                        .entry(self.surviving_section(&src.doc, &src.section))
                        .or_default()
                        .push(rid.clone());
                }
            }
        }
        for (eid, e) in &self.graph.entities {
            for m in &e.mentions {
                if self.docs.contains_key(&m.doc) && !locates(&m.doc, &m.section, &m.quote) {
                    let at = self.surviving_section(&m.doc, &m.section);
                    let list = stale.entry(at).or_default();
                    if !list.contains(eid) {
                        list.push(eid.clone());
                    }
                }
            }
        }
        for (at, anchors) in stale {
            found.push(rec(
                CHANGE_ANCHOR_STALE,
                &at,
                "section",
                serde_json::json!({ "anchors": anchors }),
            ));
        }

        // Requirement pairs without a decision that matches both statements now.
        for (rid, r) in &self.graph.requirements {
            let nbrs = self.pair_review_neighbors(rid);
            if nbrs.is_empty() {
                continue;
            }
            let mine = statement_hash(&r.statement);
            let judged: Vec<String> = nbrs
                .iter()
                .filter(|o| {
                    let Some(other) = self.graph.requirements.get(*o) else {
                        return false;
                    };
                    self.decisions
                        .pairs
                        .get(&pair_key(rid, o))
                        .is_some_and(|d| {
                            d.statements.get(rid) == Some(&mine)
                                && d.statements.get(*o) == Some(&statement_hash(&other.statement))
                        })
                })
                .cloned()
                .collect();
            if judged.len() < nbrs.len() {
                found.push(rec(
                    CHANGE_REQ_REVISED,
                    rid,
                    "decision",
                    serde_json::json!({ "judged": judged }),
                ));
            }
        }

        // Entities whose facts differ from their review.
        for id in self.graph.entities.keys() {
            if self.decisions.reviews.get(id) != Some(&entity_fact_hash(self, id)) {
                found.push(rec(CHANGE_ENTITY, id, "decision", serde_json::Value::Null));
            }
        }

        // Instances whose or whose type's facts differ from their conformance check.
        for (inst, ty) in crate::derive::instance_types(self) {
            let now = BTreeMap::from([
                (inst.clone(), entity_fact_hash(self, &inst)),
                (ty.clone(), entity_fact_hash(self, &ty)),
            ]);
            if self.decisions.conformance.get(&inst) != Some(&now) {
                found.push(rec(
                    CHANGE_INSTANCE,
                    &inst,
                    "decision",
                    serde_json::json!({ "type": ty }),
                ));
            }
        }

        // Requirements naming several entities with no edges and no structure decision.
        for (rid, r) in &self.graph.requirements {
            if r.entities.len() >= 2
                && r.edges.is_empty()
                && self.decisions.structure.get(rid) != Some(&statement_hash(&r.statement))
            {
                found.push(rec(
                    CHANGE_EDGES_MISSING,
                    rid,
                    "entities",
                    serde_json::Value::Null,
                ));
            }
        }

        // Curated views whose query matches a node the curation decision does not list.
        let instances = crate::derive::instance_types(self);
        for (vid, v) in &self.graph.views {
            let (false, Some(q)) = (v.default, &v.query) else {
                continue;
            };
            let seen = self
                .decisions
                .curation
                .get(vid)
                .cloned()
                .unwrap_or_default();
            let added: Vec<String> = crate::derive::query_matches(self, q, &v.excluded, &instances)
                .into_iter()
                .filter(|m| !seen.contains(m))
                .collect();
            if !added.is_empty() {
                found.push(rec(
                    CHANGE_QUERY_MATCH,
                    vid,
                    "query",
                    serde_json::json!({ "added": added }),
                ));
            }
        }

        // Curated views naming a node that is gone.
        for (vid, v) in &self.graph.views {
            if v.default {
                continue;
            }
            let dead = |id: &String| {
                let r = self.resolve_id(id);
                !(self.graph.entities.contains_key(r) || self.graph.requirements.contains_key(r))
            };
            for (list, ids) in [
                ("members", v.members.iter().collect::<Vec<_>>()),
                ("collapse", v.collapse.iter().collect()),
                ("excluded", v.excluded.iter().map(|e| &e.id).collect()),
            ] {
                let gone: Vec<&String> = ids.into_iter().filter(|i| dead(i)).collect();
                if !gone.is_empty() {
                    found.push(rec(
                        CHANGE_VIEW_MEMBER_GONE,
                        vid,
                        list,
                        serde_json::json!({ "gone": gone }),
                    ));
                }
            }
        }

        // Questions with no answer, and facts no document states yet.
        for (did, d) in &self.graph.diagnostics {
            if d.lifecycle == "open"
                && d.prompt.is_some()
                && d.answer.is_none()
                && !matches!(d.triage.as_deref(), Some("suppressed") | Some("wontfix"))
            {
                found.push(rec(
                    CHANGE_PROMPT_UNANSWERED,
                    did,
                    "prompt",
                    serde_json::Value::Null,
                ));
            }
        }
        for (id, p) in self
            .graph
            .requirements
            .iter()
            .filter_map(|(id, r)| r.provenance.as_ref().map(|p| (id, p)))
            .chain(
                self.graph
                    .entities
                    .iter()
                    .filter_map(|(id, e)| e.provenance.as_ref().map(|p| (id, p))),
            )
            .chain(
                self.graph
                    .criteria
                    .iter()
                    .filter_map(|(id, c)| c.provenance.as_ref().map(|p| (id, p))),
            )
        {
            if provenance_is_pending(p) {
                found.push(rec(
                    CHANGE_PROVENANCE_PENDING,
                    id,
                    "provenance",
                    serde_json::json!({ "provenance": p.kind() }),
                ));
            }
        }

        for f in found {
            if !self
                .status
                .changes
                .iter()
                .any(|c| c.kind == f.kind && c.subject == f.subject)
            {
                self.status.changes.push(f);
            }
        }

        // Failed goals: every open goal-failed diagnostic carrying its goal.
        for d in self.graph.diagnostics.values() {
            if d.rule != RULE_GOAL_FAILED || d.lifecycle != "open" {
                continue;
            }
            let Some(goal) = &d.goal else { continue };
            if !self.status.failed.iter().any(|f| f.goal.id == goal.id) {
                self.status.failed.push(FailedGoal {
                    goal: (**goal).clone(),
                    reason: d.reasoning.clone().unwrap_or_else(|| d.message.clone()),
                });
            }
        }
    }

    // The section an anchor sits in, or its nearest surviving ancestor, or the
    // document's first section when none survives.
    fn surviving_section(&self, doc: &str, section: &str) -> String {
        let Some(d) = self.docs.get(doc) else {
            return format!("{}#{}", doc, section);
        };
        let mut at = section.to_string();
        loop {
            if d.sections.contains_key(&at) {
                return format!("{}#{}", doc, at);
            }
            match at.rfind('/') {
                Some(0) | None => break,
                Some(i) => at.truncate(i),
            }
        }
        let first = d
            .sections
            .iter()
            .min_by_key(|(_, s)| s.lines[0])
            .map(|(r, _)| r.clone())
            .unwrap_or_else(|| "/".to_string());
        format!("{}#{}", doc, first)
    }
}

// ---- the version 4 layout ----

// What a version 4 store kept where version 5 keeps nothing, or keeps it elsewhere.
// Removed or moved by the build's opening that rewrites the store.
// Mirrors docs/compiler/graph.md#store-version.
pub(super) fn retire_v4_layout(out: &Path) {
    let remove_dir = |p: PathBuf| {
        crate::vfs::remove_dir_all(&p).ok();
    };
    let remove_file = |p: PathBuf| {
        crate::vfs::remove_file(&p).ok();
    };
    for d in [
        "changes",
        "parked",
        "failed",
        "alignment",
        "lookalikes",
        "docs",
    ] {
        remove_dir(out.join(d));
    }
    for d in ["default-views", "relationships", "state-machines"] {
        remove_dir(out.join("graph").join(d));
    }
    for f in [
        "status.yaml",
        "build.yaml",
        "markers.yaml",
        "causes.yaml",
        "feedback.jsonl",
    ] {
        remove_file(out.join(f));
    }
    // The journal and the control plane move into local/; rebuilt output and logs go:
    // the next build's tail rewrites the output, and logs are history.
    let moves = [
        (out.join("journal"), layout::journal_dir(out)),
        (out.join("control.yaml"), layout::control_file(out)),
        (out.join("workers"), layout::workers_dir(out)),
        (out.join("leases"), layout::leases_dir(out)),
        (out.join("decompile"), layout::decompile_dir(out)),
        (
            out.join("deliverable-baseline"),
            layout::deliverable_baseline_dir(out),
        ),
    ];
    for (from, to) in moves {
        if !from.vexists() {
            continue;
        }
        if let Some(parent) = to.parent() {
            crate::vfs::create_dir_all(parent).ok();
        }
        if from.vis_dir() && to.vis_dir() {
            // The build that is upgrading already wrote here (its own lease, say):
            // what it wrote stays, and the old entries join it.
            if let Ok(rd) = crate::vfs::read_dir(&from) {
                for e in rd.flatten() {
                    let target = to.join(e.file_name());
                    if !target.vexists() {
                        crate::vfs::rename(e.path(), &target).ok();
                    }
                }
            }
            remove_dir(from);
        } else {
            // A file the opening build wrote from defaults gives way to the old one,
            // which holds this operator's modes and releases.
            crate::vfs::rename(&from, &to).ok();
        }
    }
    remove_file(out.join(".ledger.lock"));
    if let Some(jazyk) = layout::jazyk_dir_of(out) {
        remove_file(jazyk.join("project.lock"));
    }
    for d in [
        "diagrams",
        "docsgen",
        "last-build",
        "preview",
        "snapshots",
        "trace",
        "sessions",
    ] {
        remove_dir(out.join(d));
    }
    for f in [".lock", ".leases.lock", ".committing"] {
        remove_file(out.join(f));
    }
}
