// An end-to-end build of a real example with a scripted executor: the real board,
// the real serving with its gates, the real commit path, derived data, rendering,
// the checks, and the zero-call rebuild, with no model and no network. The runner
// stands where an ACP worker session would and speaks to the same serving the
// in-process runner opens (docs/compiler/sessions.md#execution).
use crate::control::begin_internal_build;
use crate::mcp::{BridgeFlags, McpServer};
use crate::model::*;
use crate::project::Project;
use crate::runner::{BatchRun, SessionReport, SessionRunner};
use crate::session::{Trace, TraceEvent, TraceLevel};
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// One tool call the runner made, with the serving's reply.
#[derive(Clone, Debug)]
pub(crate) struct Call {
    pub batch: String,
    pub name: String,
    pub args: Value,
    pub reply: Value,
}

// A domain term the script extracts: the entity's name and its stereotype. The
// script references a term wherever a sentence names it; "the system" is the «system».
struct Term {
    name: &'static str,
    stereotype: Option<&'static str>,
}

const F1_TERMS: [Term; 5] = [
    Term { name: "Shop", stereotype: Some("system") },
    Term { name: "Customer", stereotype: Some("actor") },
    Term { name: "Order", stereotype: None },
    Term { name: "Product", stereotype: None },
    Term { name: "Catalog", stereotype: None },
];

// A runner that resolves every goal of a batch through the serving's tools with the
// minimal work each gate accepts, and panics on the first refusal: a bounced call is
// a script bug, never something to commit around.
pub(crate) struct ScriptedRunner {
    project: Project,
    out: PathBuf,
    token: Mutex<Option<String>>,
    pub calls: Arc<Mutex<Vec<Call>>>,
}

fn str_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

// The body split into sentences the way the one-sentence gate counts them: a
// terminator, a space, and a capital or digit opens the next one.
fn sentences(body: &str) -> Vec<String> {
    let text = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i < chars.len() {
        cur.push(chars[i]);
        if matches!(chars[i], '.' | '!' | '?') {
            let opens = chars.get(i + 1) == Some(&' ')
                && chars
                    .get(i + 2)
                    .is_some_and(|c| c.is_uppercase() || c.is_ascii_digit());
            if opens || i + 1 == chars.len() {
                out.push(cur.trim().to_string());
                cur.clear();
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn stem(w: &str) -> String {
    let w = w.to_lowercase();
    if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") {
        w[..w.len() - 1].to_string()
    } else {
        w
    }
}

fn terms_in(sentence: &str) -> Vec<&'static Term> {
    let stems: BTreeSet<String> = sentence
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(stem)
        .collect();
    let mut out: Vec<&Term> = F1_TERMS
        .iter()
        .filter(|t| stems.contains(&stem(t.name)))
        .collect();
    if sentence.to_lowercase().contains("the system") {
        if let Some(sys) = F1_TERMS.iter().find(|t| t.stereotype == Some("system")) {
            if !out.iter().any(|t| t.name == sys.name) {
                out.push(sys);
            }
        }
    }
    out
}

// The statement is the sentence with its markdown links reduced to their text.
fn statement_of(sentence: &str) -> String {
    let links = regex::Regex::new(r"\[([^\]]+)\]\([^)]*\)").unwrap();
    links.replace_all(sentence, "$1").to_string()
}

fn facet_of(sentence: &str) -> (&'static str, &'static str) {
    let l = sentence.to_lowercase();
    if l.contains("should not") || l.contains("must not") {
        ("failure-mode", "the sentence says what must not happen when something goes wrong")
    } else if l.contains(" shall ")
        || ["browses", "places", "lists", "submits", "display", "send"]
            .iter()
            .any(|v| l.contains(v))
    {
        ("behavior", "the sentence says what the subject does")
    } else {
        ("constraint", "the sentence states a fact the subject holds")
    }
}

impl ScriptedRunner {
    pub(crate) fn new(project: &Project) -> ScriptedRunner {
        ScriptedRunner {
            project: project.clone(),
            out: project.out.clone(),
            token: Mutex::new(None),
            calls: Default::default(),
        }
    }

    pub(crate) fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    fn call(&self, server: &McpServer, batch: &str, name: &str, args: Value) -> Value {
        let reply = server.handle("tools/call", &json!({"name": name, "arguments": args}));
        let (value, is_error) = match reply {
            Ok(v) => {
                let text = v["content"][0]["text"].as_str().unwrap_or_default().to_string();
                let parsed = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
                (parsed, v["isError"].as_bool().unwrap_or(false))
            }
            Err((_, m)) => (Value::String(m), true),
        };
        self.calls.lock().unwrap().push(Call {
            batch: batch.to_string(),
            name: name.to_string(),
            args: args.clone(),
            reply: value.clone(),
        });
        assert!(
            !is_error,
            "{}: the serving refused {} {}: {}",
            batch, name, args, value
        );
        value
    }

    // The ids of the entities a sentence names, created with the sentence as their
    // mention when the graph and the session do not know them yet.
    fn entity_ids(
        &self,
        server: &McpServer,
        batch: &str,
        section: &str,
        sentence: &str,
        ids: &mut BTreeMap<String, String>,
    ) -> Vec<String> {
        let mut out = Vec::new();
        for term in terms_in(sentence) {
            let key = term.name.to_lowercase();
            if let Some(id) = ids.get(&key) {
                out.push(id.clone());
                continue;
            }
            let mut args = json!({"name": term.name, "mention": {"section": section, "quote": sentence}});
            if let Some(s) = term.stereotype {
                args["stereotype"] = json!(s);
            }
            let reply = self.call(server, batch, "upsert_entity", args);
            let id = reply["id"].as_str().expect("an entity id").to_string();
            ids.insert(key, id.clone());
            out.push(id);
        }
        assert!(!out.is_empty(), "the script names no entity in `{}`", sentence);
        out
    }

    fn reconcile_section(
        &self,
        server: &McpServer,
        batch: &str,
        snapshot: &Store,
        goal: &Goal,
        ids: &mut BTreeMap<String, String>,
    ) {
        let (doc, sec) = split_section_ref(&goal.target).expect("a section target");
        let section = &snapshot.docs[&doc].sections[&sec];
        let skip = if section.kind == "preamble" { 0 } else { 1 };
        let body = section.raw.lines().skip(skip).collect::<Vec<_>>().join(" ");
        let full_ref = format!("{}#{}", doc, sec);
        let norm = crate::store::normalize_statement;
        // What the section already yielded, by normalized quote: an unchanged
        // sentence is a no-op, not a re-extraction.
        let recorded: BTreeSet<String> = snapshot
            .graph
            .requirements
            .values()
            .filter_map(|r| r.source.as_ref())
            .filter(|s| s.doc == doc && s.section == sec)
            .map(|s| norm(&s.quote))
            .collect();
        let mut fresh: Vec<String> = sentences(&body)
            .into_iter()
            .filter(|s| !recorded.contains(&norm(s)))
            .collect();
        // A stale anchor is the sentence that replaced it, revised in place; one with
        // no replacement is a fact the document dropped.
        for anchor in str_list(&goal.change["staleAnchors"]) {
            if fresh.is_empty() {
                self.call(
                    server,
                    batch,
                    "delete_requirement",
                    json!({"id": anchor, "reason": "the document no longer states it"}),
                );
                continue;
            }
            let sentence = fresh.remove(0);
            self.call(
                server,
                batch,
                "update_requirement",
                json!({"id": anchor, "statement": statement_of(&sentence), "section": full_ref, "quote": sentence}),
            );
        }
        let mut yielded = !recorded.is_empty();
        for sentence in fresh {
            let entities = self.entity_ids(server, batch, &full_ref, &sentence, ids);
            let (facet, why) = facet_of(&sentence);
            self.call(
                server,
                batch,
                "upsert_requirement",
                json!({
                    "statement": statement_of(&sentence),
                    "section": full_ref,
                    "quote": sentence,
                    "entities": entities,
                    "facets": [{"facet": facet, "reasoning": why}],
                }),
            );
            yielded = true;
        }
        let coverage = if yielded {
            json!({"section": full_ref, "state": "covered"})
        } else {
            json!({"section": full_ref, "state": "non-normative", "note": "the section names what the others state"})
        };
        self.call(server, batch, "set_coverage", coverage);
        self.call(
            server,
            batch,
            "mark_goal_done",
            json!({"goal": goal.id, "justification": "Every sentence of the section is recorded with its verbatim quote and the section is marked."}),
        );
    }

    fn review_entity(&self, server: &McpServer, batch: &str, snapshot: &Store, goal: &Goal) {
        let target = snapshot.resolve_id(&goal.target).to_string();
        let (similar, related) = crate::goals::lookalikes(snapshot, &target);
        let lookalikes: Vec<Value> = similar
            .iter()
            .chain(related.iter())
            .map(|(id, name)| {
                json!({"id": id, "verdict": "separate", "reason": format!("{} is its own concept with its own statements", name)})
            })
            .collect();
        let diagnostics: Vec<Value> = crate::goals::review_diagnostics(snapshot, &target)
            .iter()
            .map(|d| json!({"id": d, "verdict": "stands"}))
            .collect();
        let parts: Vec<Value> = crate::goals::unparented_parts(snapshot, &target)
            .iter()
            .map(|(_, part, _)| {
                json!({"id": part, "verdict": "kept", "reason": "a rule-carrier the documents place under no whole"})
            })
            .collect();
        let unreferenced: Vec<Value> = crate::goals::unreferenced_statements(snapshot, &target)
            .iter()
            .map(|(rid, _)| json!({"id": rid, "verdict": "word-match", "reason": "names the entity in passing"}))
            .collect();
        let mut evidence = json!({"definition": "current"});
        for (key, list) in [
            ("lookalikes", lookalikes),
            ("diagnostics", diagnostics),
            ("parts", parts),
            ("unreferenced", unreferenced),
        ] {
            if !list.is_empty() {
                evidence[key] = json!(list);
            }
        }
        let rules = self.project.linting.warnings.len() + self.project.linting.errors.len();
        if rules > 0 {
            evidence["lint"] = json!((1..=rules)
                .map(|n| json!({"rule": n, "verdict": "clean"}))
                .collect::<Vec<_>>());
        }
        self.call(
            server,
            batch,
            "mark_goal_done",
            json!({"goal": goal.id, "justification": "The statements agree with the definition and no lookalike or finding asks for a change.", "evidence": evidence}),
        );
    }

    fn declare_edges(&self, server: &McpServer, batch: &str, snapshot: &Store, goal: &Goal) {
        let rid = snapshot.resolve_id(&goal.target).to_string();
        let r = &snapshot.graph.requirements[&rid];
        let ents: Vec<String> = r
            .entities
            .iter()
            .map(|e| snapshot.resolve_id(e).to_string())
            .collect();
        let name = |id: &str| snapshot.graph.entities[id].name.clone();
        let names: BTreeSet<String> = ents.iter().map(|e| name(e)).collect();
        let by_name = |n: &str| ents.iter().find(|e| name(e) == n).cloned().unwrap();
        let has = |a: &str, b: &str| names.contains(a) && names.contains(b);
        let edge = if has("Catalog", "Product") {
            json!({"a": by_name("Catalog"), "b": by_name("Product"), "type": "aggregation", "cardinality": "1..*"})
        } else if has("Order", "Product") {
            json!({"a": by_name("Order"), "b": by_name("Product"), "type": "aggregation", "cardinality": "1..*"})
        } else {
            json!({"a": ents[0], "b": ents[1], "type": "association"})
        };
        self.call(
            server,
            batch,
            "update_requirement",
            json!({"id": rid, "edges": [edge]}),
        );
        self.call(
            server,
            batch,
            "mark_goal_done",
            json!({"goal": goal.id, "justification": "The sentence relates the two entities structurally and the edge records the relationship it states.", "evidence": "declared"}),
        );
    }

    fn curate_view(&self, server: &McpServer, batch: &str, snapshot: &Store, goal: &Goal) {
        let matched: Vec<String> = goal.change["matched"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| m["id"].as_str().map(String::from))
            .collect();
        let view = &snapshot.graph.views[&goal.target];
        let add: Vec<String> = matched
            .iter()
            .filter(|m| !view.members.contains(m))
            .cloned()
            .collect();
        if !add.is_empty() {
            self.call(
                server,
                batch,
                "update_view",
                json!({"id": goal.target, "add_members": add}),
            );
        }
        let evidence: Vec<Value> = matched
            .iter()
            .map(|id| json!({"id": id, "verdict": "added"}))
            .collect();
        self.call(
            server,
            batch,
            "mark_goal_done",
            json!({"goal": goal.id, "justification": "The step follows the one its trigger names, in document order.", "evidence": evidence}),
        );
    }

    fn place_anchors(&self, server: &McpServer, batch: &str, goal: &Goal) {
        for anchor in str_list(&goal.change["anchors"]) {
            self.call(server, batch, "orphan_anchor", json!({"id": anchor}));
        }
        self.call(
            server,
            batch,
            "mark_goal_done",
            json!({"goal": goal.id, "justification": "Each anchor whose quote died stays on its section for the extraction session to re-record."}),
        );
    }

    fn verdict_only(&self, server: &McpServer, batch: &str, goal: &Goal, evidence: Value, why: &str) {
        self.call(
            server,
            batch,
            "mark_goal_done",
            json!({"goal": goal.id, "justification": why, "evidence": evidence}),
        );
    }
}

impl SessionRunner for ScriptedRunner {
    fn agent_name(&self) -> String {
        "scripted".into()
    }

    fn set_build_token(&self, token: Option<String>) {
        *self.token.lock().unwrap() = token;
    }

    fn run_item(&self, batch: &BatchRun, trace: &Trace) -> SessionReport {
        // The serving the in-process runner opens: one batch, the build's token.
        let server = McpServer::with_bridge(
            self.project.clone(),
            self.out.clone(),
            vec!["compile".into()],
            true,
            BridgeFlags {
                ephemeral: true,
                only: Some(batch.id.clone()),
                build_token: self.token.lock().unwrap().clone(),
                ..Default::default()
            },
        );
        server
            .handle(
                "initialize",
                &json!({"protocolVersion": "2024-11-05", "clientInfo": {"name": "jazyk-scripted"}}),
            )
            .expect("the serving initializes");
        let gen_before = crate::store::read_generation(&self.out);
        trace.event(crate::runner::session_start(batch));
        self.call(&server, &batch.id, "begin_goals", json!({"goals": batch.goal_ids()}));
        // The graph as the batch sees it: the same committed state the session's
        // snapshot holds.
        let snapshot = Store::load(&self.out);
        let mut ids: BTreeMap<String, String> = snapshot
            .graph
            .entities
            .iter()
            .map(|(id, e)| (e.name.to_lowercase(), id.clone()))
            .collect();
        for goal in &batch.goals {
            match goal.kind.as_str() {
                "reconcile-section" => {
                    self.reconcile_section(&server, &batch.id, &snapshot, goal, &mut ids)
                }
                "review-entity" => self.review_entity(&server, &batch.id, &snapshot, goal),
                "rejudge-pair" => self.verdict_only(
                    &server,
                    &batch.id,
                    goal,
                    json!({"verdict": "consistent"}),
                    "The two statements constrain different facts and neither contradicts the other.",
                ),
                "declare-edges" => self.declare_edges(&server, &batch.id, &snapshot, goal),
                "curate-view" => self.curate_view(&server, &batch.id, &snapshot, goal),
                "dedupe-candidates" => self.verdict_only(
                    &server,
                    &batch.id,
                    goal,
                    json!("separate"),
                    "The two names share a word and nothing else; their statements are about different things.",
                ),
                "place-anchors" => self.place_anchors(&server, &batch.id, goal),
                other => panic!("the script has no step for a {} goal: {:?}", other, goal),
            }
        }
        self.call(&server, &batch.id, "done", json!({"summary": "scripted batch"}));
        let gen_after = crate::store::read_generation(&self.out);
        let (applied, resolved) =
            crate::runner::journal_diff(&self.out, gen_before, gen_after, &batch.goal_ids());
        trace.event(TraceEvent::SessionDone {
            label: batch.id.clone(),
            goals: batch.goal_ids(),
            staged: applied,
            rounds: 1,
            mode: "done".into(),
            summary: String::new(),
        });
        SessionReport {
            applied,
            rounds: 1,
            tokens: 0,
            cached: 0,
            failed: None,
            resolved,
            stopped: None,
        }
    }

    fn ask(&self, _s: &str, _u: &str, _l: &str, _st: &str) -> Result<String, String> {
        Err("the scripted runner answers no one-shot prompt".into())
    }

    fn ask_traced(
        &self,
        _s: &str,
        _u: &str,
        _l: &str,
        _st: &str,
        _t: Option<&Trace>,
    ) -> Result<String, String> {
        Err("the scripted runner answers no one-shot prompt".into())
    }
}

// A runner a converged store must never reach.
struct PanickingRunner;

impl SessionRunner for PanickingRunner {
    fn agent_name(&self) -> String {
        "never".into()
    }
    fn set_build_token(&self, _token: Option<String>) {}
    fn run_item(&self, batch: &BatchRun, _trace: &Trace) -> SessionReport {
        panic!("a rebuild of a converged store ran a session: {:?}", batch.goal_ids());
    }
    fn ask(&self, _s: &str, _u: &str, _l: &str, _st: &str) -> Result<String, String> {
        panic!("a rebuild of a converged store asked the model");
    }
    fn ask_traced(
        &self,
        _s: &str,
        _u: &str,
        _l: &str,
        _st: &str,
        _t: Option<&Trace>,
    ) -> Result<String, String> {
        panic!("a rebuild of a converged store asked the model");
    }
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}

// A fresh copy of example/f1 under the temp dir, as a project of its own.
fn f1_copy(label: &str) -> (PathBuf, Project) {
    let dir = std::env::temp_dir().join(format!(
        "jazyk-e2e-{}-{}",
        label,
        crate::sys::process_id()
    ));
    crate::vfs::remove_dir_all(&dir).ok();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("example").join("f1");
    copy_tree(&src, &dir);
    let project = Project::load(&dir);
    assert!(project.config_error.is_none(), "{:?}", project.config_error);
    assert_eq!(project.set_id.as_deref(), Some("ds_main"));
    (dir, project)
}

fn capture() -> (Trace, Arc<Mutex<Vec<TraceEvent>>>) {
    let events: Arc<Mutex<Vec<TraceEvent>>> = Default::default();
    let sink = events.clone();
    let trace = Trace::to_sink(
        TraceLevel::Normal,
        Arc::new(move |ev| sink.lock().unwrap().push(ev.clone())),
        Default::default(),
    );
    (trace, events)
}

// One build as `jazyk compile` runs it, under the build lease, with the given runner.
fn build(project: &Project, runner: &dyn SessionRunner) -> (super::BuildReport, Vec<TraceEvent>) {
    let (trace, events) = capture();
    let guard = begin_internal_build(project, &project.out, "compile").expect("the build lease");
    let report = super::compile_under(project, &project.out, &trace, None, runner, guard);
    let events = events.lock().unwrap().clone();
    (report, events)
}

fn goal_events<'a>(events: &'a [TraceEvent], which: &str) -> Vec<&'a String> {
    events
        .iter()
        .filter_map(|e| match e {
            TraceEvent::Goal { goal, event, .. } if event == which => Some(goal),
            _ => None,
        })
        .collect()
}

fn session_entries(out: &Path, to: u64) -> Vec<JournalEntry> {
    crate::goals::read_journal_range(out, 0, to)
        .into_iter()
        .filter(|e| e.kind == "session")
        .collect()
}

fn assert_clean_build(report: &super::BuildReport, events: &[TraceEvent]) {
    assert!(report.converged(), "verdict: {}", report.verdict);
    assert_eq!((report.failed, report.parked, report.errors), (0, 0, 0), "{:?}", report);
    assert_eq!(report.coverage_pct, 100);
    let failed: Vec<&TraceEvent> = events
        .iter()
        .filter(|e| matches!(e, TraceEvent::SessionFailed { .. }))
        .collect();
    assert!(failed.is_empty(), "{:?}", failed);
    assert!(goal_events(events, "parked").is_empty(), "{:?}", goal_events(events, "parked"));
    assert!(goal_events(events, "failed").is_empty(), "{:?}", goal_events(events, "failed"));
}

// Builds the f1 copy from an empty store to `converged` and returns the runner that
// did it.
fn converge_f1(label: &str) -> (PathBuf, Project, ScriptedRunner, super::BuildReport) {
    let (dir, project) = f1_copy(label);
    let runner = ScriptedRunner::new(&project);
    let started = std::time::Instant::now();
    let (report, events) = build(&project, &runner);
    eprintln!(
        "[e2e {}] fresh build: {} in {:.1}s, {} sessions",
        label,
        report.verdict,
        started.elapsed().as_secs_f64(),
        report.sessions
    );
    assert_clean_build(&report, &events);
    (dir, project, runner, report)
}

// A fresh f1 converges through the real board, serving, gates, and commit path:
// five reconcile-section goals open the build, the judgment and GC work they open
// resolves behind them, every statement carries its verbatim quote, the derived data
// and the diagrams land, the checks are clean, and the journal holds one session
// entry per commit with the justifications. Mirrors bootstrap/VALIDATION.md#f1.
#[test]
fn f1_compiles_from_an_empty_store_to_converged_with_a_scripted_executor() {
    let (dir, project, runner, report) = converge_f1("fresh");
    let out = project.out.clone();
    let store = Store::load(&out);
    let calls = runner.calls();

    // Entities and requirements as the documents state them, every requirement a
    // located quote and no derived or decreed fact anywhere.
    let names: BTreeSet<String> = store.graph.entities.values().map(|e| e.name.clone()).collect();
    for n in ["Shop", "Customer", "Order", "Product", "Catalog"] {
        assert!(names.contains(n), "{:?}", names);
    }
    assert_eq!(store.graph.entities.len(), 5, "{:?}", names);
    assert_eq!(store.graph.requirements.len(), 12, "{:#?}", store.graph.requirements.keys());
    for (id, r) in &store.graph.requirements {
        let src = r.source.as_ref().unwrap_or_else(|| panic!("{} has no quote", id));
        assert!(r.provenance.is_none(), "{} carries {:?}", id, r.provenance);
        assert!(
            store.quote_locates(&src.doc, &src.section, &src.quote),
            "{}: `{}` does not locate in {}#{}",
            id,
            src.quote,
            src.doc,
            src.section
        );
        assert!(!r.entities.is_empty() && !r.facets.is_empty(), "{}: {:?}", id, r);
    }
    for (id, e) in &store.graph.entities {
        assert!(e.provenance.is_none() && !e.mentions.is_empty(), "{}: {:?}", id, e);
    }
    // Every section with a body is covered.
    for (doc, rec) in &store.docs {
        for (r, sec) in &rec.sections {
            if crate::goals::section_has_body(sec) {
                let c = rec.coverage.get(r).unwrap_or_else(|| panic!("{}#{} unmarked", doc, r));
                assert_eq!(c.state, "covered", "{}#{}", doc, r);
            }
        }
    }
    // Derived data: relationships from the declared edges, the default views, and
    // every view drawn to .puml and .svg.
    assert!(!store.graph.relationships.is_empty());
    assert!(
        store.graph.relationships.values().any(|rel| rel.strongest() == "aggregation"),
        "{:?}",
        store.graph.relationships.keys()
    );
    assert!(!store.graph.views.is_empty());
    assert!(store.graph.views.values().any(|v| v.default), "every view was curated");
    for id in store.graph.views.keys() {
        let rel = id.strip_prefix("view:").unwrap();
        for ext in ["puml", "svg"] {
            let path = out.join("diagrams").join(format!("{}.{}", rel, ext));
            assert!(path.exists(), "{} missing", path.display());
        }
    }
    // The checks are clean of errors and the verdict stands in status.yaml.
    let findings = super::checks(&store, &project);
    let errors: Vec<_> = findings.iter().filter(|f| f.2 == "error").collect();
    assert!(errors.is_empty(), "{:?}", errors);
    assert!(store.status.verdict.converged(), "{}", store.status.verdict);
    assert!(store.status.parked.is_empty() && store.status.failed.is_empty());
    // What rides blocked is the ledger: one bind goal per requirement, gated behind
    // the manual generate release, never compile work.
    let control = crate::control::Control::load(&project, &out);
    let board = crate::board::Board::derive(&store, &project, &control);
    let blocked: Vec<&Goal> = board
        .goals
        .iter()
        .filter(|g| matches!(g.state, GoalState::Blocked { .. }))
        .collect();
    assert_eq!(blocked.len(), report.blocked, "{:?}", blocked);
    assert_eq!(blocked.len(), store.graph.requirements.len());
    assert!(blocked.iter().all(|g| g.kind == "bind"), "{:?}", blocked);
    // Compile work was released by the typed command.
    assert_eq!(control.released.compile.len(), 2, "{:?}", control.released.compile);

    // The journal: one session entry per commit, each resolution with its
    // justification; the goals the runner marked are the goals the journal resolved.
    let sessions = session_entries(&out, store.status.generation);
    let done_calls = calls.iter().filter(|c| c.name == "done").count();
    assert_eq!(sessions.len(), done_calls, "{:?}", sessions.iter().map(|e| e.generation).collect::<Vec<_>>());
    let marked: BTreeSet<String> = calls
        .iter()
        .filter(|c| c.name == "mark_goal_done")
        .filter_map(|c| c.args["goal"].as_str().map(String::from))
        .collect();
    let mut resolved: BTreeSet<String> = BTreeSet::new();
    for e in &sessions {
        assert!(!e.resolved_goals.is_empty(), "g{} resolved nothing", e.generation);
        for r in &e.resolved_goals {
            assert!(!r.justification.trim().is_empty(), "{} without justification", r.goal);
            resolved.insert(r.goal.clone());
        }
    }
    assert_eq!(marked, resolved);
    eprintln!("[e2e fresh] resolved: {:?}", marked);
    // The kinds a fresh f1 runs: extraction, then the judgment it opens, then the
    // edges. The scripted extraction places every behavior in its default flow view,
    // so no curate-view burst derives here (a model's extraction did in
    // bootstrap/VALIDATION.md#f1); the runner carries the step for when one does.
    let kinds: BTreeSet<String> = marked
        .iter()
        .map(|g| g.split(':').nth(1).unwrap_or("").to_string())
        .collect();
    for k in ["reconcile-section", "review-entity", "rejudge-pair", "declare-edges"] {
        assert!(kinds.contains(k), "{:?}", kinds);
    }
    let count = |kind: &str| marked.iter().filter(|g| g.starts_with(&format!("g:{}:", kind))).count();
    assert_eq!(count("reconcile-section"), 5, "{:?}", marked);
    assert_eq!(count("review-entity"), store.graph.entities.len(), "{:?}", marked);
    // Every session ran begin_goals, its work, then done, and every done committed.
    let mut by_batch: BTreeMap<String, Vec<&Call>> = BTreeMap::new();
    for c in &calls {
        by_batch.entry(c.batch.clone()).or_default().push(c);
    }
    assert_eq!(by_batch.len(), report.sessions as usize);
    for (batch, cs) in &by_batch {
        assert_eq!(cs.first().map(|c| c.name.as_str()), Some("begin_goals"), "{}", batch);
        let last = cs.last().unwrap();
        assert_eq!(last.name, "done", "{}", batch);
        assert_eq!(last.reply["committed"], true, "{}: {}", batch, last.reply);
        assert_eq!(last.reply["batch"], json!(batch), "{}", last.reply);
    }
    for c in calls.iter().filter(|c| c.name == "set_coverage") {
        assert_eq!(c.args["state"], "covered", "{:?}", c.args);
    }
    crate::vfs::remove_dir_all(&dir).ok();
}

// An immediate rebuild of a converged store derives no goal, runs no session, and
// moves no generation: the runner that would panic is never reached.
// Mirrors docs/compiler/compilation.md#convergence.
#[test]
fn a_rebuild_of_a_converged_f1_derives_nothing_and_calls_no_model() {
    let (dir, project, _runner, _) = converge_f1("rebuild");
    let out = project.out.clone();
    let before = Store::load(&out);
    let journal_before = crate::goals::read_journal_range(&out, 0, before.status.generation).len();
    let started = std::time::Instant::now();
    let (report, events) = build(&project, &PanickingRunner);
    eprintln!("[e2e rebuild] {} in {:.1}s", report.verdict, started.elapsed().as_secs_f64());
    assert_clean_build(&report, &events);
    assert_eq!((report.goals, report.sessions, report.applied), (0, 0, 0), "{:?}", report);
    assert!(
        events.iter().any(|e| matches!(e, TraceEvent::Board { goals: 0, .. })),
        "no empty board event"
    );
    assert!(!events.iter().any(|e| matches!(e, TraceEvent::BatchStart { .. })));
    let after = Store::load(&out);
    assert_eq!(after.status.generation, before.status.generation);
    assert_eq!(
        crate::goals::read_journal_range(&out, 0, after.status.generation).len(),
        journal_before
    );
    assert!(after.status.verdict.converged(), "{}", after.status.verdict);
    assert_eq!(after.graph.requirements.len(), before.graph.requirements.len());
    crate::vfs::remove_dir_all(&dir).ok();
}

// One edited sentence dirties one section: the next build derives work on that
// section alone, the untouched statements keep their ids and text, the edited one
// keeps its id under the new quote, and the build converges again.
// Mirrors docs/compiler/compilation.md#incremental-builds.
#[test]
fn editing_one_sentence_of_f1_reconciles_only_its_section_and_keeps_the_other_ids() {
    let (dir, project, runner, _) = converge_f1("edit");
    let out = project.out.clone();
    let before = Store::load(&out);
    let old = "Each Customer must have a unique Email.";
    let new = "Each Customer must have a unique Email address.";
    let edited_id = before
        .graph
        .requirements
        .iter()
        .find(|(_, r)| r.source.as_ref().is_some_and(|s| s.quote == old))
        .map(|(id, _)| id.clone())
        .expect("the Email statement");
    let path = dir.join("docs/system.md");
    let text = crate::vfs::read_to_string(&path).unwrap();
    assert!(text.contains(old));
    crate::vfs::write(&path, text.replace(old, new)).unwrap();
    let calls_before = runner.calls().len();

    let started = std::time::Instant::now();
    let (report, events) = build(&project, &runner);
    eprintln!(
        "[e2e edit] {} in {:.1}s, {} sessions",
        report.verdict,
        started.elapsed().as_secs_f64(),
        report.sessions
    );
    assert_clean_build(&report, &events);
    assert!(report.sessions >= 1, "{:?}", report);

    // The dirty set: the board opens with work on the edited section alone, the
    // document's anchor placement (tier 0) and the section's reconcile goal, and
    // every batch the build ran holds one of the two kinds or the judgment they open.
    let opening = events
        .iter()
        .find_map(|e| match e {
            TraceEvent::Board { goals, kinds, .. } => Some((*goals, kinds.clone())),
            _ => None,
        })
        .expect("a board event");
    assert_eq!(opening.0, 2, "{:?}", opening);
    assert_eq!(
        opening.1,
        vec![("place-anchors".to_string(), 1), ("reconcile-section".to_string(), 1)],
        "{:?}",
        opening
    );
    let batches: Vec<Vec<Value>> = events
        .iter()
        .filter_map(|e| match e {
            TraceEvent::BatchStart { goals, .. } => Some(goals.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(batches.len() as u32, report.sessions);
    for g in batches.iter().take(2).flatten() {
        let kind = g["kind"].as_str().unwrap();
        let target = g["target"].as_str().unwrap();
        assert!(
            (kind == "reconcile-section" && target == "docs/system.md#/shop/customer")
                || (kind == "place-anchors" && target == "docs/system.md"),
            "{:?}",
            batches
        );
    }
    let marks: Vec<String> = runner.calls()[calls_before..]
        .iter()
        .filter(|c| c.name == "mark_goal_done" && c.args["goal"].as_str().unwrap_or("").starts_with("g:reconcile-section:"))
        .map(|c| c.args["goal"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(marks, vec!["g:reconcile-section:docs/system.md#/shop/customer".to_string()]);

    // Identity: every requirement keeps its id; the untouched ones keep their text,
    // the edited one carries the new sentence under the same id.
    let after = Store::load(&out);
    assert_eq!(after.graph.requirements.len(), before.graph.requirements.len());
    for (id, was) in &before.graph.requirements {
        let now = after
            .graph
            .requirements
            .get(id)
            .unwrap_or_else(|| panic!("{} vanished", id));
        if id == &edited_id {
            assert_eq!(now.source.as_ref().unwrap().quote, new);
            assert_eq!(now.statement, new);
        } else {
            assert_eq!(now.statement, was.statement, "{}", id);
            assert_eq!(now.source, was.source, "{}", id);
        }
    }
    for id in before.graph.entities.keys() {
        assert!(after.graph.entities.contains_key(id), "{} vanished", id);
    }
    assert!(after.status.generation > before.status.generation);
    let findings = super::checks(&after, &project);
    let errors: Vec<_> = findings.iter().filter(|f| f.2 == "error").collect();
    assert!(errors.is_empty(), "{:?}", errors);
    crate::vfs::remove_dir_all(&dir).ok();
}
