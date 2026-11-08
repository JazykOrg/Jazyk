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

// A domain term the script extracts: the entity's name, its stereotype, and the parent
// the fixture's sentences state for it (by name), passed to upsert_entity so the
// containment the documents state lands at extraction and the level counts are the
// documents'. The script references a term wherever a sentence names every word of
// it; "the system" is the «system».
pub(crate) struct Term {
    name: &'static str,
    stereotype: Option<&'static str>,
    parent: Option<&'static str>,
}

const fn term(name: &'static str, stereotype: Option<&'static str>) -> Term {
    Term {
        name,
        stereotype,
        parent: None,
    }
}

const F1_TERMS: [Term; 5] = [
    term("Shop", Some("system")),
    term("Customer", Some("actor")),
    term("Order", None),
    term("Product", None),
    term("Catalog", None),
];

// The e2e-gc fixture's terms (bootstrap/example/e2e-gc/EXPECTED.md): a «system» with
// sixteen services, a Data Layer with six stores, and a lookalike pair. The parent
// comes first in the list and in every sentence that names a child, so it exists when
// the child is created.
const fn service(name: &'static str) -> Term {
    Term {
        name,
        stereotype: Some("service"),
        parent: Some("Platform"),
    }
}

const fn store(name: &'static str) -> Term {
    Term {
        name,
        stereotype: None,
        parent: Some("Data Layer"),
    }
}

const GC_TERMS: [Term; 25] = [
    term("Platform", Some("system")),
    term("Data Layer", None),
    term("Audit Log", None),
    term("Audit Trail", None),
    service("Auth Service"),
    service("Catalog Service"),
    service("Search Service"),
    service("Cart Service"),
    service("Checkout Service"),
    service("Pricing Service"),
    service("Promotion Service"),
    service("Review Service"),
    service("Billing Service"),
    service("Inventory Service"),
    service("Shipping Service"),
    service("Notification Service"),
    service("Analytics Service"),
    service("Reporting Service"),
    service("Support Service"),
    service("Fraud Service"),
    store("User Store"),
    store("Product Store"),
    store("Order Store"),
    store("Payment Store"),
    store("Session Cache"),
];

// One derived requirement the script stages while reconciling a section, in place of
// a quote: the ratification path's input.
struct Derivation {
    section: String,
    statement: String,
    from: Vec<&'static str>,
}

// Batches the script fails without touching the serving, the way a session that ran
// out of rounds does: `remaining` failures on the first batch holding a goal of
// `kind`, then that batch alone (its goals are remembered on the first failure).
struct FailPlan {
    kind: String,
    remaining: u32,
    goals: Option<Vec<String>>,
}

// The board as the runner derived it when a batch started: the goals with their
// class, mandatory flag, and state, and each goal's `explain` text.
pub(crate) struct BoardSnapshot {
    pub batch: String,
    pub goals: Vec<Goal>,
    pub explain: BTreeMap<String, String>,
}

// A runner that resolves every goal of a batch through the serving's tools with the
// minimal work each gate accepts, and panics on the first refusal: a bounced call is
// a script bug, never something to commit around.
pub(crate) struct ScriptedRunner {
    project: Project,
    out: PathBuf,
    terms: &'static [Term],
    // Whether extraction declares the edge of a multi-entity sentence in the same
    // call (no `edges-missing` record, no declare-edges goal), as the extraction skill
    // asks, or leaves it to the declare-edges net.
    extract_edges: bool,
    token: Mutex<Option<String>>,
    derivation: Mutex<Option<Derivation>>,
    fail: Mutex<Option<FailPlan>>,
    pub calls: Arc<Mutex<Vec<Call>>>,
    pub boards: Arc<Mutex<Vec<BoardSnapshot>>>,
}

fn str_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
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

fn terms_in<'a>(terms: &'a [Term], sentence: &str) -> Vec<&'a Term> {
    let stems: BTreeSet<String> = sentence
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(stem)
        .collect();
    let mut out: Vec<&Term> = terms
        .iter()
        .filter(|t| t.name.split(' ').all(|w| stems.contains(&stem(w))))
        .collect();
    if sentence.to_lowercase().contains("the system") {
        if let Some(sys) = terms.iter().find(|t| t.stereotype == Some("system")) {
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
        (
            "failure-mode",
            "the sentence says what must not happen when something goes wrong",
        )
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
        Self::with_terms(project, &F1_TERMS, false)
    }

    pub(crate) fn with_terms(
        project: &Project,
        terms: &'static [Term],
        extract_edges: bool,
    ) -> ScriptedRunner {
        ScriptedRunner {
            project: project.clone(),
            out: project.out.clone(),
            terms,
            extract_edges,
            token: Mutex::new(None),
            derivation: Mutex::new(None),
            fail: Mutex::new(None),
            calls: Default::default(),
            boards: Default::default(),
        }
    }

    // While reconciling `section`, also stage `statement` as a derived requirement
    // about the named terms (once).
    pub(crate) fn derive_in(&self, section: &str, statement: &str, from: &[&'static str]) {
        *self.derivation.lock().unwrap() = Some(Derivation {
            section: section.to_string(),
            statement: statement.to_string(),
            from: from.to_vec(),
        });
    }

    // Fail the first batch holding a goal of `kind` this many times before working it.
    pub(crate) fn fail_batches_of(&self, kind: &str, times: u32) {
        *self.fail.lock().unwrap() = Some(FailPlan {
            kind: kind.to_string(),
            remaining: times,
            goals: None,
        });
    }

    pub(crate) fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    pub(crate) fn boards(&self) -> std::sync::MutexGuard<'_, Vec<BoardSnapshot>> {
        self.boards.lock().unwrap()
    }

    // The scripted failure a batch meets, when the plan names it.
    fn scripted_failure(&self, batch: &BatchRun) -> Option<String> {
        let mut plan = self.fail.lock().unwrap();
        let p = plan.as_mut()?;
        if p.remaining == 0 {
            return None;
        }
        let hit = match &p.goals {
            Some(ids) => batch.goals.iter().any(|g| ids.contains(&g.id)),
            None => batch.goals.iter().any(|g| g.kind == p.kind),
        };
        if !hit {
            return None;
        }
        if p.goals.is_none() {
            p.goals = Some(batch.goal_ids());
        }
        p.remaining -= 1;
        Some(format!(
            "scripted: the session ran out of rounds on {} ({} more to go)",
            batch.id, p.remaining
        ))
    }

    // The board as a session would find it when the batch starts, kept for assertions
    // on class and readiness mid-build; the GC goals carry their `explain` text.
    fn snapshot_board(&self, batch: &BatchRun) {
        let store = Store::load(&self.out);
        let control = crate::control::Control::load(&self.project, &self.out);
        let board = crate::board::Board::derive(&store, &self.project, &control);
        let explain = board
            .goals
            .iter()
            .filter(|g| g.class == "gc")
            .filter_map(|g| board.explain(&store, &g.id).map(|e| (g.id.clone(), e)))
            .collect();
        self.boards.lock().unwrap().push(BoardSnapshot {
            batch: batch.id.clone(),
            goals: board.goals.clone(),
            explain,
        });
    }

    fn call(&self, server: &McpServer, batch: &str, name: &str, args: Value) -> Value {
        let reply = server.handle("tools/call", &json!({"name": name, "arguments": args}));
        let (value, is_error) = match reply {
            Ok(v) => {
                let text = v["content"][0]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
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

    // The entities a sentence names as (id, name, parent id), created with the
    // sentence as their mention when the graph and the session do not know them yet.
    fn entity_ids(
        &self,
        server: &McpServer,
        batch: &str,
        section: &str,
        sentence: &str,
        ids: &mut BTreeMap<String, String>,
    ) -> Vec<(String, String, Option<String>)> {
        let mut out = Vec::new();
        for term in terms_in(self.terms, sentence) {
            let key = term.name.to_lowercase();
            let parent = term.parent.map(|p| {
                ids.get(&p.to_lowercase()).cloned().unwrap_or_else(|| {
                    panic!(
                        "the script creates {} before its parent {} exists",
                        term.name, p
                    )
                })
            });
            if let Some(id) = ids.get(&key) {
                out.push((id.clone(), term.name.to_string(), parent));
                continue;
            }
            let mut args =
                json!({"name": term.name, "mention": {"section": section, "quote": sentence}});
            if let Some(s) = term.stereotype {
                args["stereotype"] = json!(s);
            }
            if let Some(p) = &parent {
                args["parent"] = json!(p);
            }
            let reply = self.call(server, batch, "upsert_entity", args);
            let id = reply["id"].as_str().expect("an entity id").to_string();
            ids.insert(key, id.clone());
            out.push((id, term.name.to_string(), parent));
        }
        assert!(
            !out.is_empty(),
            "the script names no entity in `{}`",
            sentence
        );
        out
    }

    // The one edge the script declares among a sentence's entities, given as (id,
    // name, parent id): the containment when one is another's parent, the f1
    // aggregations by name, else an association between the first two.
    fn edge_among(ents: &[(String, String, Option<String>)]) -> Value {
        let whole_part = ents.iter().find_map(|a| {
            ents.iter()
                .find(|b| b.2.as_deref() == Some(a.0.as_str()))
                .map(|b| (a.0.clone(), b.0.clone()))
        });
        if let Some((whole, part)) = whole_part {
            return json!({"a": whole, "b": part, "type": "composition"});
        }
        let by_name = |n: &str| ents.iter().find(|e| e.1 == n).map(|e| e.0.clone());
        if let (Some(c), Some(p)) = (by_name("Catalog"), by_name("Product")) {
            return json!({"a": c, "b": p, "type": "aggregation", "cardinality": "1..*"});
        }
        if let (Some(o), Some(p)) = (by_name("Order"), by_name("Product")) {
            return json!({"a": o, "b": p, "type": "aggregation", "cardinality": "1..*"});
        }
        json!({"a": ents[0].0, "b": ents[1].0, "type": "association"})
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
            let ents = self.entity_ids(server, batch, &full_ref, &sentence, ids);
            let entities: Vec<&String> = ents.iter().map(|e| &e.0).collect();
            let (facet, why) = facet_of(&sentence);
            let mut args = json!({
                "statement": statement_of(&sentence),
                "section": full_ref,
                "quote": sentence,
                "entities": entities,
                "facets": [{"facet": facet, "reasoning": why}],
            });
            if self.extract_edges && ents.len() >= 2 {
                args["edges"] = json!([Self::edge_among(&ents)]);
            }
            self.call(server, batch, "upsert_requirement", args);
            yielded = true;
        }
        // The derivation the test asked for: a statement the documents do not state,
        // with its upstream entities and reasoning, in place of a quote.
        let derivation = {
            let mut d = self.derivation.lock().unwrap();
            if d.as_ref().is_some_and(|d| d.section == full_ref) {
                d.take()
            } else {
                None
            }
        };
        if let Some(d) = derivation {
            let from: Vec<String> = d
                .from
                .iter()
                .map(|n| ids.get(&n.to_lowercase()).cloned().expect("a known term"))
                .collect();
            self.call(
                server,
                batch,
                "upsert_requirement",
                json!({
                    "statement": d.statement,
                    "entities": from,
                    "provenance": {"derived": {"from": from, "reasoning": "the section implies it without stating it; the proposal offers the sentence"}},
                    "facets": [{"facet": "constraint", "reasoning": "the sentence states a fact the subject holds"}],
                }),
            );
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
        let ents: Vec<(String, String, Option<String>)> = r
            .entities
            .iter()
            .map(|e| {
                let id = snapshot.resolve_id(e).to_string();
                let ent = &snapshot.graph.entities[&id];
                (id, ent.name.clone(), ent.parent.clone())
            })
            .collect();
        self.call(
            server,
            batch,
            "update_requirement",
            json!({"id": rid, "edges": [Self::edge_among(&ents)]}),
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

    // The fan-out variant: group the level's children by the heading of the section
    // that first mentions each, one grouping per heading, named after it
    // (docs/compiler/goals/abstract-entity.md#the-fan-out-variant).
    fn abstract_entity(&self, server: &McpServer, batch: &str, snapshot: &Store, goal: &Goal) {
        assert!(
            goal.change["fan_out"].is_number(),
            "the script groups levels only, not the caps variant: {:?}",
            goal.change
        );
        let target = snapshot.resolve_id(&goal.target).to_string();
        assert!(
            snapshot.graph.entities.contains_key(&target),
            "the script groups a node's level, not a scope root: {}",
            goal.target
        );
        let children: Vec<(&String, &Entity)> = snapshot
            .graph
            .entities
            .iter()
            .filter(|(_, e)| e.parent.as_deref() == Some(target.as_str()))
            .collect();
        // The section listing a child is the one of its mentions in the node's own
        // sections (where the node is mentioned too) that lists the most of its
        // siblings: a heading that enumerates the level, not a passing mention
        // elsewhere (docs/compiler/goals/abstract-entity.md#fan-out-hints).
        let key = |m: &SourceRef| (m.doc.clone(), m.section.clone());
        let own: BTreeSet<(String, String)> = snapshot.graph.entities[&target]
            .mentions
            .iter()
            .map(key)
            .collect();
        let mut listed: BTreeMap<(String, String), usize> = BTreeMap::new();
        for (_, e) in &children {
            for m in &e.mentions {
                *listed.entry(key(m)).or_default() += 1;
            }
        }
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (id, e) in &children {
            let m = e
                .mentions
                .iter()
                .max_by_key(|m| (own.contains(&key(m)), listed[&key(m)]))
                .unwrap_or_else(|| panic!("{} has no mention to group by", id));
            let title = snapshot.docs[&m.doc].sections[&m.section].title.clone();
            groups.entry(title).or_default().push(id.to_string());
        }
        for (title, members) in &groups {
            assert!(
                members.len() >= 2,
                "{} lists one child only: {:?}",
                title,
                members
            );
            self.call(
                server,
                batch,
                "group_entities",
                json!({
                    "name": title,
                    "definition": format!("{} groups the services the documents list under that heading.", title),
                    "members": members,
                    "reasoning": format!("the documents list these members together under the {} heading", title),
                }),
            );
        }
        self.call(
            server,
            batch,
            "mark_goal_done",
            json!({"goal": goal.id, "justification": "The level is grouped along the headings the documents list its members under, one grouping per heading."}),
        );
    }

    // A level view over its member limit: the entities drawn for their edges into the
    // level move to a sub-view and are excluded from the level view with notes naming
    // it, so the level's own children stay and nothing is dropped
    // (docs/compiler/goals/split-view.md#gate).
    fn split_view(&self, server: &McpServer, batch: &str, snapshot: &Store, goal: &Goal) {
        let view = &snapshot.graph.views[&goal.target];
        let node = snapshot
            .graph
            .entities
            .keys()
            .find(|id| {
                crate::derive::level_view_id(snapshot, id).as_deref() == Some(goal.target.as_str())
            })
            .cloned()
            .unwrap_or_else(|| panic!("{} is not a node's level view", goal.target));
        let moved: Vec<String> = view
            .members
            .iter()
            .filter(|m| {
                snapshot
                    .graph
                    .entities
                    .get(*m)
                    .and_then(|e| e.parent.as_deref())
                    != Some(node.as_str())
            })
            .cloned()
            .collect();
        assert!(
            !moved.is_empty(),
            "{} shows only the level's children",
            goal.target
        );
        let reply = self.call(
            server,
            batch,
            "upsert_view",
            json!({
                "kind": view.kind,
                "title": format!("{}: neighbours", view.title),
                "members": moved,
                "reasoning": "the entities drawn for their edges into the level, detailed beside it",
            }),
        );
        let sub = reply["id"].as_str().expect("a view id").to_string();
        let exclude: Vec<Value> = moved
            .iter()
            .map(|m| json!({"id": m, "note": format!("belongs to {}", sub)}))
            .collect();
        self.call(
            server,
            batch,
            "update_view",
            json!({"id": goal.target, "exclude": exclude}),
        );
        self.call(
            server,
            batch,
            "mark_goal_done",
            json!({"goal": goal.id, "justification": "The level view keeps the level's own children; the entities drawn for their edges into it moved to a sub-view the exclusion notes name, so every member survives."}),
        );
    }

    fn verdict_only(
        &self,
        server: &McpServer,
        batch: &str,
        goal: &Goal,
        evidence: Value,
        why: &str,
    ) {
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
        self.snapshot_board(batch);
        // A planned failure: the session spent something and committed nothing, as one
        // that ran out of rounds does, so the build's retry rule sees an attempt.
        if let Some(error) = self.scripted_failure(batch) {
            trace.event(crate::runner::session_start(batch));
            return SessionReport {
                applied: 0,
                rounds: 1,
                tokens: 1,
                cached: 0,
                failed: Some(error),
                resolved: Vec::new(),
                stopped: None,
            };
        }
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
        self.call(
            &server,
            &batch.id,
            "begin_goals",
            json!({"goals": batch.goal_ids()}),
        );
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
                "abstract-entity" => self.abstract_entity(&server, &batch.id, &snapshot, goal),
                "split-view" => self.split_view(&server, &batch.id, &snapshot, goal),
                other => panic!("the script has no step for a {} goal: {:?}", other, goal),
            }
        }
        self.call(
            &server,
            &batch.id,
            "done",
            json!({"summary": "scripted batch"}),
        );
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
        panic!(
            "a rebuild of a converged store ran a session: {:?}",
            batch.goal_ids()
        );
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

// A fresh copy of one example under the temp dir, as a project of its own.
fn example_copy(example: &str, label: &str) -> (PathBuf, Project) {
    let dir =
        std::env::temp_dir().join(format!("jazyk-e2e-{}-{}", label, crate::sys::process_id()));
    crate::vfs::remove_dir_all(&dir).ok();
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("example")
        .join(example);
    copy_tree(&src, &dir);
    let project = Project::load(&dir);
    assert!(project.config_error.is_none(), "{:?}", project.config_error);
    assert_eq!(project.set_id.as_deref(), Some("ds_main"));
    (dir, project)
}

fn f1_copy(label: &str) -> (PathBuf, Project) {
    example_copy("f1", label)
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
    assert_eq!(
        (report.failed, report.parked, report.errors),
        (0, 0, 0),
        "{:?}",
        report
    );
    assert_eq!(report.coverage_pct, 100);
    let failed: Vec<&TraceEvent> = events
        .iter()
        .filter(|e| matches!(e, TraceEvent::SessionFailed { .. }))
        .collect();
    assert!(failed.is_empty(), "{:?}", failed);
    assert!(
        goal_events(events, "parked").is_empty(),
        "{:?}",
        goal_events(events, "parked")
    );
    assert!(
        goal_events(events, "failed").is_empty(),
        "{:?}",
        goal_events(events, "failed")
    );
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
    let names: BTreeSet<String> = store
        .graph
        .entities
        .values()
        .map(|e| e.name.clone())
        .collect();
    for n in ["Shop", "Customer", "Order", "Product", "Catalog"] {
        assert!(names.contains(n), "{:?}", names);
    }
    assert_eq!(store.graph.entities.len(), 5, "{:?}", names);
    assert_eq!(
        store.graph.requirements.len(),
        12,
        "{:#?}",
        store.graph.requirements.keys()
    );
    for (id, r) in &store.graph.requirements {
        let src = r
            .source
            .as_ref()
            .unwrap_or_else(|| panic!("{} has no quote", id));
        assert!(r.provenance.is_none(), "{} carries {:?}", id, r.provenance);
        assert!(
            store.quote_locates(&src.doc, &src.section, &src.quote),
            "{}: `{}` does not locate in {}#{}",
            id,
            src.quote,
            src.doc,
            src.section
        );
        assert!(
            !r.entities.is_empty() && !r.facets.is_empty(),
            "{}: {:?}",
            id,
            r
        );
    }
    for (id, e) in &store.graph.entities {
        assert!(
            e.provenance.is_none() && !e.mentions.is_empty(),
            "{}: {:?}",
            id,
            e
        );
    }
    // Every section with a body is covered.
    for (doc, rec) in &store.docs {
        for (r, sec) in &rec.sections {
            if crate::goals::section_has_body(sec) {
                let c = rec
                    .coverage
                    .get(r)
                    .unwrap_or_else(|| panic!("{}#{} unmarked", doc, r));
                assert_eq!(c.state, "covered", "{}#{}", doc, r);
            }
        }
    }
    // Derived data: relationships from the declared edges, the default views, and
    // every view drawn to .puml and .svg.
    assert!(!store.graph.relationships.is_empty());
    assert!(
        store
            .graph
            .relationships
            .values()
            .any(|rel| rel.strongest() == "aggregation"),
        "{:?}",
        store.graph.relationships.keys()
    );
    assert!(!store.graph.views.is_empty());
    assert!(
        store.graph.views.values().any(|v| v.default),
        "every view was curated"
    );
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
    // What rides blocked is the ledger: binding and adaptive development are both
    // gated behind the manual generate release, never compile work.
    let control = crate::control::Control::load(&project, &out);
    let board = crate::board::Board::derive(&store, &project, &control);
    let blocked: Vec<&Goal> = board
        .goals
        .iter()
        .filter(|g| matches!(g.state, GoalState::Blocked { .. }))
        .collect();
    assert_eq!(blocked.len(), report.blocked, "{:?}", blocked);
    let expected_blocked: BTreeSet<String> = store
        .graph
        .requirements
        .keys()
        .map(|id| format!("g:bind:{}", id))
        .chain(
            store
                .graph
                .entities
                .keys()
                .map(|id| format!("g:generate:{}", id)),
        )
        .collect();
    assert_eq!(
        blocked
            .iter()
            .map(|g| g.id.clone())
            .collect::<BTreeSet<_>>(),
        expected_blocked
    );
    // Compile work was released by the typed command.
    assert_eq!(
        control.released.compile.len(),
        2,
        "{:?}",
        control.released.compile
    );

    // The journal: one session entry per commit, each resolution with its
    // justification; the goals the runner marked are the goals the journal resolved.
    let sessions = session_entries(&out, store.status.generation);
    let done_calls = calls.iter().filter(|c| c.name == "done").count();
    assert_eq!(
        sessions.len(),
        done_calls,
        "{:?}",
        sessions.iter().map(|e| e.generation).collect::<Vec<_>>()
    );
    let marked: BTreeSet<String> = calls
        .iter()
        .filter(|c| c.name == "mark_goal_done")
        .filter_map(|c| c.args["goal"].as_str().map(String::from))
        .collect();
    let mut resolved: BTreeSet<String> = BTreeSet::new();
    for e in &sessions {
        assert!(
            !e.resolved_goals.is_empty(),
            "g{} resolved nothing",
            e.generation
        );
        for r in &e.resolved_goals {
            assert!(
                !r.justification.trim().is_empty(),
                "{} without justification",
                r.goal
            );
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
    for k in [
        "reconcile-section",
        "review-entity",
        "rejudge-pair",
        "declare-edges",
    ] {
        assert!(kinds.contains(k), "{:?}", kinds);
    }
    let count = |kind: &str| {
        marked
            .iter()
            .filter(|g| g.starts_with(&format!("g:{}:", kind)))
            .count()
    };
    assert_eq!(count("reconcile-section"), 5, "{:?}", marked);
    assert_eq!(
        count("review-entity"),
        store.graph.entities.len(),
        "{:?}",
        marked
    );
    // Every session ran begin_goals, its work, then done, and every done committed.
    let mut by_batch: BTreeMap<String, Vec<&Call>> = BTreeMap::new();
    for c in &calls {
        by_batch.entry(c.batch.clone()).or_default().push(c);
    }
    assert_eq!(by_batch.len(), report.sessions as usize);
    for (batch, cs) in &by_batch {
        assert_eq!(
            cs.first().map(|c| c.name.as_str()),
            Some("begin_goals"),
            "{}",
            batch
        );
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
    eprintln!(
        "[e2e rebuild] {} in {:.1}s",
        report.verdict,
        started.elapsed().as_secs_f64()
    );
    assert_clean_build(&report, &events);
    assert_eq!(
        (report.goals, report.sessions, report.applied),
        (0, 0, 0),
        "{:?}",
        report
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, TraceEvent::Board { goals: 0, .. })),
        "no empty board event"
    );
    assert!(!events
        .iter()
        .any(|e| matches!(e, TraceEvent::BatchStart { .. })));
    let after = Store::load(&out);
    assert_eq!(after.status.generation, before.status.generation);
    assert_eq!(
        crate::goals::read_journal_range(&out, 0, after.status.generation).len(),
        journal_before
    );
    assert!(after.status.verdict.converged(), "{}", after.status.verdict);
    assert_eq!(
        after.graph.requirements.len(),
        before.graph.requirements.len()
    );
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
        vec![
            ("place-anchors".to_string(), 1),
            ("reconcile-section".to_string(), 1)
        ],
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
        .filter(|c| {
            c.name == "mark_goal_done"
                && c.args["goal"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("g:reconcile-section:")
        })
        .map(|c| c.args["goal"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        marks,
        vec!["g:reconcile-section:docs/system.md#/shop/customer".to_string()]
    );

    // Identity: every requirement keeps its id; the untouched ones keep their text,
    // the edited one carries the new sentence under the same id.
    let after = Store::load(&out);
    assert_eq!(
        after.graph.requirements.len(),
        before.graph.requirements.len()
    );
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

fn assert_no_errors(store: &Store, project: &Project) {
    let findings = super::checks(store, project);
    let errors: Vec<_> = findings.iter().filter(|f| f.2 == "error").collect();
    assert!(errors.is_empty(), "{:?}", errors);
}

fn assert_diagrams_rendered(store: &Store, out: &Path) {
    for id in store.graph.views.keys() {
        let rel = id.strip_prefix("view:").unwrap();
        for ext in ["puml", "svg"] {
            let path = out.join("diagrams").join(format!("{}.{}", rel, ext));
            assert!(path.exists(), "{} missing", path.display());
        }
    }
}

// The next build after a graph change opens with exactly `kinds` (kind, count) and
// converges: downstream judgment, never the alignment or ingest a dirty section opens.
// A rebuild of a converged store derives no goal and runs no session.
fn assert_rebuild_is_a_no_op(project: &Project, label: &str) {
    let out = project.out.clone();
    let before = Store::load(&out);
    let (report, events) = build(project, &PanickingRunner);
    eprintln!("[e2e {}] rebuild: {}", label, report.verdict);
    assert!(report.converged(), "verdict: {}", report.verdict);
    assert_eq!(
        (report.goals, report.sessions, report.applied),
        (0, 0, 0),
        "{:?}",
        report
    );
    assert!(!events
        .iter()
        .any(|e| matches!(e, TraceEvent::BatchStart { .. })));
    assert_eq!(
        Store::load(&out).status.generation,
        before.status.generation
    );
}

// The GC burst, end to end on bootstrap/example/e2e-gc (its EXPECTED.md): sixteen
// children under one node derive a mandatory fan-out `abstract-entity`, the Data
// Layer's level view over its member limit derives `split-view`, and a cross-document
// lookalike pair derives `dedupe-candidates`. Each waits for its cone, the Platform's
// own `split-view` yields to the fan-out, the burst resolves them through the real
// gates, the graph satisfies the limits, the new views render, the build converges
// with the groupings' ratification proposals riding as blocked, and a rebuild is a
// no-op. Mirrors docs/compiler/compilation.md#compile-and-garbage-collection.
#[test]
fn e2e_gc_runs_the_gc_burst_through_the_gates_and_converges() {
    let (dir, project) = example_copy("e2e-gc", "gc");
    let runner = ScriptedRunner::with_terms(&project, &GC_TERMS, true);
    let started = std::time::Instant::now();
    let (report, events) = build(&project, &runner);
    eprintln!(
        "[e2e gc] fresh build: {} in {:.1}s, {} sessions",
        report.verdict,
        started.elapsed().as_secs_f64(),
        report.sessions
    );
    for e in &events {
        if let TraceEvent::BatchStart {
            label,
            class,
            goals,
            ..
        } = e
        {
            let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
            for g in goals {
                *kinds
                    .entry(g["kind"].as_str().unwrap_or("?").to_string())
                    .or_default() += 1;
            }
            eprintln!("[e2e gc] batch {} ({}): {:?}", label, class, kinds);
        }
    }
    assert_clean_build(&report, &events);
    let out = project.out.clone();
    let store = Store::load(&out);
    let calls = runner.calls();

    let fan_out = "g:abstract-entity:ent:platform";
    let split = "g:split-view:view:class/data-layer";
    let platform_split = "g:split-view:view:component/platform";
    let dedupe = "g:dedupe-candidates:ent:audit-log~ent:audit-trail";
    let marked: BTreeMap<String, String> = calls
        .iter()
        .filter(|c| c.name == "mark_goal_done")
        .filter_map(|c| {
            c.args["goal"]
                .as_str()
                .map(|g| (g.to_string(), c.batch.clone()))
        })
        .collect();
    for g in [fan_out, split, dedupe] {
        assert!(
            marked.contains_key(g),
            "{} never resolved: {:?}",
            g,
            marked.keys()
        );
    }
    // The trace printed each burst with its count over the limit.
    let bursts: Vec<(&String, &String, u64)> = events
        .iter()
        .filter_map(|e| match e {
            TraceEvent::GcBurst {
                goal_kind,
                target,
                count,
                ..
            } => Some((goal_kind, target, *count)),
            _ => None,
        })
        .collect();
    assert!(
        bursts
            .iter()
            .any(|(k, t, n)| k.as_str() == "abstract-entity"
                && t.as_str() == "ent:platform"
                && *n == 16),
        "{:?}",
        bursts
    );
    assert!(
        bursts.iter().any(|(k, t, n)| k.as_str() == "split-view"
            && t.as_str() == "view:class/data-layer"
            && *n == 21),
        "{:?}",
        bursts
    );
    assert!(
        bursts
            .iter()
            .all(|(_, t, _)| t.as_str() != "view:component/platform"),
        "{:?}",
        bursts
    );

    // Class and readiness as the board showed them while the build ran: the fan-out
    // goal was gc and mandatory, waited on its cone before its burst and was ready in
    // the batch that ran it; the Platform's split-view yielded to it and never ran.
    let boards = runner.boards();
    let ran = boards
        .iter()
        .position(|b| b.batch == marked[fan_out])
        .expect("the fan-out batch's board");
    let at_run = boards[ran]
        .goals
        .iter()
        .find(|g| g.id == fan_out)
        .expect("the fan-out goal");
    assert_eq!(
        (at_run.class.as_str(), at_run.mandatory, &at_run.state),
        ("gc", true, &GoalState::Open),
        "{:?}",
        at_run
    );
    assert!(
        boards[ran].explain[fan_out].contains("ready now"),
        "{}",
        boards[ran].explain[fan_out]
    );
    assert!(
        boards[..ran]
            .iter()
            .any(|b| b.explain.get(fan_out).is_some_and(|e| e.contains("waits:"))),
        "the fan-out goal never waited on its cone"
    );
    assert!(
        boards.iter().any(|b| b
            .explain
            .get(platform_split)
            .is_some_and(|e| e.contains(&format!("fan-out first: {}", fan_out)))),
        "the Platform's split-view never yielded to the fan-out"
    );
    let split_goal = boards
        .iter()
        .find_map(|b| b.goals.iter().find(|g| g.id == split))
        .expect("the split-view goal");
    assert_eq!(
        (split_goal.class.as_str(), split_goal.mandatory),
        ("gc", false)
    );
    let dedupe_goal = boards
        .iter()
        .find_map(|b| b.goals.iter().find(|g| g.id == dedupe))
        .expect("the dedupe goal");
    assert_eq!(
        (dedupe_goal.class.as_str(), dedupe_goal.mandatory),
        ("gc", false)
    );
    assert_eq!(dedupe_goal.change["score"], 0.5, "{}", dedupe_goal.change);
    // The sixteen `X Service` entities share one generic token and the same two
    // documents; its weight keeps every pair of them under the threshold, so the audit
    // pair is the only dedupe goal any board ever held.
    let dedupes: BTreeSet<&str> = boards
        .iter()
        .flat_map(|b| b.goals.iter())
        .filter(|g| g.kind == "dedupe-candidates")
        .map(|g| g.id.as_str())
        .collect();
    assert_eq!(dedupes, BTreeSet::from([dedupe]), "{:?}", dedupes);
    drop(boards);

    // The level: two groupings under the Platform, each derived from exactly its
    // members, the sixteen services under them.
    let platform = &store.graph.entities["ent:platform"];
    assert_eq!(platform.parent, None);
    let groupings: Vec<(&String, &Entity)> = store
        .graph
        .entities
        .iter()
        .filter(|(_, e)| e.parent.as_deref() == Some("ent:platform"))
        .collect();
    let names: BTreeSet<&str> = groupings.iter().map(|(_, e)| e.name.as_str()).collect();
    assert_eq!(
        names,
        BTreeSet::from(["Storefront", "Back Office"]),
        "{:?}",
        names
    );
    let services: Vec<&String> = store
        .graph
        .entities
        .iter()
        .filter(|(_, e)| e.stereotype.as_deref() == Some("service"))
        .map(|(id, _)| id)
        .collect();
    assert_eq!(services.len(), 16);
    for (gid, g) in &groupings {
        let members: BTreeSet<String> = store
            .graph
            .entities
            .iter()
            .filter(|(_, e)| e.parent.as_deref() == Some(gid.as_str()))
            .map(|(id, _)| id.clone())
            .collect();
        assert_eq!(members.len(), 8, "{}: {:?}", gid, members);
        match &g.provenance {
            Some(Provenance::Derived { from, .. }) => {
                assert_eq!(
                    from.iter().cloned().collect::<BTreeSet<_>>(),
                    members,
                    "{}",
                    gid
                )
            }
            other => panic!("{} carries {:?}", gid, other),
        }
        assert!(
            g.mentions.is_empty() && g.definition.is_some(),
            "{}: {:?}",
            gid,
            g
        );
        assert!(
            members.iter().all(|m| services.contains(&m)),
            "{}: {:?}",
            gid,
            members
        );
    }
    // The split: the level view keeps its six stores, excludes the sixteen services
    // with notes naming the sub-view that lists them, and both are under the limit.
    let view = &store.graph.views["view:class/data-layer"];
    assert!(!view.default);
    assert_eq!(view.members.len(), 5, "{:?}", view.members);
    assert!(view
        .members
        .iter()
        .all(|m| store.graph.entities[m].parent.as_deref() == Some("ent:data-layer")));
    assert_eq!(view.excluded.len(), 16, "{:?}", view.excluded);
    let sub_ids: BTreeSet<String> = view
        .excluded
        .iter()
        .map(|x| x.note.trim_start_matches("belongs to ").to_string())
        .collect();
    assert_eq!(sub_ids.len(), 1, "{:?}", sub_ids);
    let sub = &store.graph.views[sub_ids.iter().next().unwrap()];
    assert_eq!(
        (sub.kind.as_str(), sub.members.len()),
        ("class", 16),
        "{:?}",
        sub
    );
    assert!(view.excluded.iter().all(|x| sub.members.contains(&x.id)));
    assert!(
        crate::derive::threshold_crossings(&store).is_empty(),
        "{:?}",
        crate::derive::threshold_crossings(&store)
            .iter()
            .map(|c| (&c.subject, c.count))
            .collect::<Vec<_>>()
    );
    assert!(store.graph.views.contains_key("view:component/platform"));
    for v in ["view:component/storefront", "view:component/back-office"] {
        assert!(
            store.graph.views.contains_key(v),
            "{} missing: {:?}",
            v,
            store.graph.views.keys()
        );
    }
    assert_diagrams_rendered(&store, &out);
    // The lookalikes, kept apart as scripted, and the verdict remembered: the pair's
    // entry at its score, no standing record, and nothing to derive.
    assert!(store.graph.entities.contains_key("ent:audit-log"));
    assert!(store.graph.entities.contains_key("ent:audit-trail"));
    let remembered = store
        .status
        .lookalike("ent:audit-log~ent:audit-trail")
        .unwrap_or_else(|| panic!("no lookalikes entry: {:?}", store.status.lookalikes));
    assert_eq!(
        (remembered.verdict.as_str(), remembered.score),
        ("separate", 0.5)
    );
    assert!(
        !store
            .status
            .changes
            .iter()
            .any(|c| c.kind == crate::goals::CHANGE_LOOKALIKE),
        "{:?}",
        store.status.changes
    );
    assert!(crate::goals::pending_lookalikes(&store).is_empty());
    let resolved: BTreeSet<String> = session_entries(&out, store.status.generation)
        .iter()
        .flat_map(|e| e.resolved_goals.iter().map(|r| r.goal.clone()))
        .collect();
    for g in [fan_out, split, dedupe] {
        assert!(resolved.contains(g), "{} not in the journal", g);
    }
    // Converged, the groupings' proposals blocked on a human, nothing parked or failed.
    assert!(store.status.verdict.converged(), "{}", store.status.verdict);
    assert!(store.status.parked.is_empty() && store.status.failed.is_empty());
    let control = crate::control::Control::load(&project, &out);
    let board = crate::board::Board::derive(&store, &project, &control);
    for (gid, _) in &groupings {
        let ratify = format!("g:ratify:{}", gid);
        let g = board
            .goal(&ratify)
            .unwrap_or_else(|| panic!("{} missing", ratify));
        assert!(
            matches!(g.state, GoalState::Blocked { .. }),
            "{:?}",
            g.state
        );
        assert!(store
            .graph
            .diagnostics
            .values()
            .any(|d| d.rule == "ratification-pending"
                && d.lifecycle == "open"
                && d.subjects == vec![gid.to_string()]));
    }
    assert!(
        board.open_goals().is_empty(),
        "{:?}",
        board.open_goals().iter().map(|g| &g.id).collect::<Vec<_>>()
    );
    assert_no_errors(&store, &project);
    assert_rebuild_is_a_no_op(&project, "gc");
    crate::vfs::remove_dir_all(&dir).ok();
}

fn journal_len(out: &Path) -> usize {
    let g = crate::store::read_generation(out);
    crate::goals::read_journal_range(out, 0, g).len()
}

// A document is not dirty against the graph: a sync reports nothing to reconcile.
fn assert_not_dirty(project: &Project) {
    let (parsed, _) = super::parse_all(project);
    let mut s = Store::load(&project.out);
    for (doc, (hash, _)) in &parsed {
        assert_eq!(&s.docs[doc].content_hash, hash, "{} is dirty", doc);
    }
    assert!(s.sync_docs(&parsed).is_empty(), "a sync found work");
}

// A derived requirement staged by extraction carries a ratification proposal with a
// prompt, and the `ratify` goal rides blocked on a human. Accepting the proposal through
// the one answer engine every frontend calls lands the sentence in the document, flips
// the fact to a quote on it, resolves the proposal, journals one `ratify` entry,
// dirties no section, and the next build derives nothing.
// Mirrors docs/compiler/goals/ratify.md#accept.
#[test]
fn accepting_a_ratification_proposal_lands_the_sentence_and_flips_the_fact_to_a_quote() {
    let (dir, project) = f1_copy("ratify");
    let runner = ScriptedRunner::new(&project);
    let sentence = "Each Customer holds exactly one account with the Shop.";
    runner.derive_in(
        "docs/system.md#/shop/customer",
        sentence,
        &["Customer", "Shop"],
    );
    let started = std::time::Instant::now();
    let (report, events) = build(&project, &runner);
    eprintln!(
        "[e2e ratify] fresh build: {} in {:.1}s",
        report.verdict,
        started.elapsed().as_secs_f64()
    );
    assert_clean_build(&report, &events);
    let out = project.out.clone();
    let before = Store::load(&out);

    // The derived fact and its proposal.
    let (rid, r) = before
        .graph
        .requirements
        .iter()
        .find(|(_, r)| r.source.is_none())
        .expect("the derived requirement");
    assert!(
        matches!(r.provenance, Some(Provenance::Derived { .. })),
        "{:?}",
        r.provenance
    );
    assert_eq!(r.statement, sentence);
    assert!(rid.starts_with("req:x-"), "{}", rid);
    assert!(before
        .status
        .has_change(crate::store::CHANGE_PROVENANCE_PENDING, rid));
    let (did, d) = before
        .graph
        .diagnostics
        .iter()
        .find(|(_, d)| d.rule == "ratification-pending" && d.subjects == vec![rid.clone()])
        .expect("the proposal");
    assert_eq!(d.lifecycle, "open");
    let prompt = d.prompt.as_ref().expect("a prompt");
    let edit = prompt.options[0]
        .edit
        .clone()
        .expect("the edit option first");
    assert_eq!(
        (edit.doc.as_str(), edit.new_text.as_str()),
        ("docs/system.md", sentence)
    );
    assert!(
        edit.old_text.is_empty(),
        "a derived fact's sentence is inserted, not a replacement"
    );
    assert!(prompt
        .options
        .iter()
        .any(|o| o.answer.as_deref() == Some("retract")));
    let control = crate::control::Control::load(&project, &out);
    let board = crate::board::Board::derive(&before, &project, &control);
    let ratify = board
        .goal(&format!("g:ratify:{}", rid))
        .expect("the ratify goal");
    assert!(
        matches!(ratify.state, GoalState::Blocked { .. }),
        "{:?}",
        ratify.state
    );
    let text_before = crate::vfs::read_to_string(dir.join("docs/system.md")).unwrap();
    assert!(!text_before.contains(sentence));
    let journal_before = journal_len(&out);

    // The human accepts, as the LSP code action, the GUI, chat, and `jazyk answer` do.
    let reply = crate::answer::answer(&project, &out, did, crate::answer::Reply::Choice(0), None)
        .expect("the answer lands");
    assert_eq!(
        (reply["status"].as_str(), reply["resolved"].as_bool()),
        (Some("applied"), Some(true)),
        "{}",
        reply
    );

    let after = Store::load(&out);
    assert_eq!(
        after.status.generation,
        before.status.generation + 1,
        "one changeset"
    );
    let r = &after.graph.requirements[rid];
    assert!(r.provenance.is_none(), "{:?}", r.provenance);
    let src = r.source.as_ref().expect("a quote");
    assert_eq!(
        (src.doc.as_str(), src.section.as_str(), src.quote.as_str()),
        ("docs/system.md", edit.section.as_str(), sentence)
    );
    assert!(after.quote_locates(&src.doc, &src.section, &src.quote));
    assert_eq!(r.statement, sentence);
    let text = crate::vfs::read_to_string(dir.join("docs/system.md")).unwrap();
    assert!(text.contains(sentence), "{}", text);
    let d = &after.graph.diagnostics[did];
    assert_ne!(d.lifecycle, "open");
    assert_eq!(
        d.answer.as_ref().map(|a| a.status.as_str()),
        Some("applied")
    );
    assert!(!after
        .status
        .has_change(crate::store::CHANGE_PROVENANCE_PENDING, rid));
    let entries = crate::goals::read_journal_range(&out, 0, after.status.generation);
    assert_eq!(entries.len(), journal_before + 1);
    assert_eq!(entries.last().unwrap().kind, "ratify");
    let board = crate::board::Board::derive(&after, &project, &control);
    assert!(board.goal(&format!("g:ratify:{}", rid)).is_none());
    assert_not_dirty(&project);
    assert_no_errors(&after, &project);
    // No section was dirtied, so no `place-anchors` or `reconcile-section` follows the
    // insert, and the accepted sentence is the human's review: the mention it added to
    // Customer and Shop writes no `entity-changed`, the statement did not change, so
    // the next build derives zero goals and makes no model call.
    // Mirrors docs/compiler/goals/ratify.md#accept.
    assert!(
        !after
            .status
            .changes
            .iter()
            .any(|c| c.kind == crate::store::CHANGE_ENTITY
                && c.generation == after.status.generation),
        "{:?}",
        after.status.changes
    );
    assert_rebuild_is_a_no_op(&project, "ratify");
    crate::vfs::remove_dir_all(&dir).ok();
}

// A dual write through the chat tool that wraps `Store::dual_write`: the sentence and
// the requirement move in one changeset, the requirement keeps its id under the new
// quote, the section is not re-dirtied (one journal entry, and the next build derives
// zero goals). Mirrors docs/compiler/compilation.md#edit-paths.
#[test]
fn a_dual_write_moves_the_sentence_and_the_requirement_together_without_redirtying() {
    let (dir, project, _runner, _) = converge_f1("dual");
    let out = project.out.clone();
    let before = Store::load(&out);
    let old = "A Product has a name and a price.";
    let new = "A Product has a name, a price, and a weight.";
    let rid = before
        .graph
        .requirements
        .iter()
        .find(|(_, r)| r.source.as_ref().is_some_and(|s| s.quote == old))
        .map(|(id, _)| id.clone())
        .expect("the Product statement");
    let journal_before = journal_len(&out);

    let server = McpServer::with_bridge(
        project.clone(),
        out.clone(),
        vec!["chat".into()],
        false,
        BridgeFlags::default(),
    );
    let reply = server
        .handle(
            "tools/call",
            &json!({"name": "revise_requirement", "arguments": {"id": rid, "new_text": new, "statement": new}}),
        )
        .expect("the tool answers");
    let v: Value = serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(v["committed"], true, "{}", v);

    let doc = crate::vfs::read_to_string(dir.join("docs/catalog.md")).unwrap();
    assert!(doc.contains(new) && !doc.contains(old), "{}", doc);
    let after = Store::load(&out);
    assert_eq!(
        after.status.generation,
        before.status.generation + 1,
        "one changeset"
    );
    assert_eq!(
        after.graph.requirements.len(),
        before.graph.requirements.len()
    );
    let r = &after.graph.requirements[&rid];
    assert_eq!(r.source.as_ref().unwrap().quote, new);
    assert_eq!(r.statement, new);
    assert!(r.provenance.is_none(), "{:?}", r.provenance);
    for (id, was) in &before.graph.requirements {
        if id != &rid {
            assert_eq!(after.graph.requirements[id].source, was.source, "{}", id);
        }
    }
    let entries = crate::goals::read_journal_range(&out, 0, after.status.generation);
    assert_eq!(entries.len(), journal_before + 1);
    let entry = entries.last().unwrap();
    assert_eq!(entry.kind, "dual-write");
    assert!(
        entry.mutations.iter().any(|m| m["op"] == "edit_doc_prose"),
        "{:?}",
        entry.mutations
    );
    assert_not_dirty(&project);
    // Not re-dirtied: no `place-anchors` or `reconcile-section` follows the write, and
    // the rewritten sentence is the human's review: Product's mention followed it
    // without an `entity-changed` record. The revised statement keeps its
    // `requirement-revised` record, so its pairs would be judged; on f1 no neighbor
    // shares two content tokens with it, so the next build derives zero goals.
    // Mirrors docs/compiler/compilation.md#edit-paths.
    assert!(
        !after
            .status
            .changes
            .iter()
            .any(|c| c.kind == crate::store::CHANGE_ENTITY
                && c.generation == after.status.generation),
        "{:?}",
        after.status.changes
    );
    assert!(
        after
            .status
            .has_change(crate::store::CHANGE_REQ_REVISED, &rid),
        "{:?}",
        after.status.changes
    );
    assert_rebuild_is_a_no_op(&project, "dual");
    crate::vfs::remove_dir_all(&dir).ok();
}

// The store version: outside a build, a version-2 out directory (one shard per kind)
// reads as empty and refuses a commit with `store-version`; a build archives it whole
// to `<out>.bak` (the old shard included), reconciles from empty to converged in node
// files, and stamps the current version. Mirrors docs/compiler/graph.md#store-version.
#[test]
fn a_build_archives_a_store_of_another_version_and_a_reader_treats_it_as_empty() {
    let (dir, project) = f1_copy("version");
    let out = project.out.clone();
    let status = "version: 2\ngeneration: 7\n";
    crate::vfs::create_dir_all(out.join("graph")).unwrap();
    crate::vfs::write(out.join("status.yaml"), status).unwrap();
    crate::vfs::write(
        out.join("graph").join("entities.yaml"),
        "ent:cart:\n  name: Cart\n",
    )
    .unwrap();

    // Read-only outside a build: empty, and a writer is refused without touching disk.
    let mut read = Store::load(&out);
    assert!(read.graph.entities.is_empty() && read.graph.requirements.is_empty());
    assert_eq!(read.status.generation, 0);
    let report = read.apply(
        vec![crate::store::Op::CreateEntity {
            id: "ent:cart".into(),
            entity: Entity {
                name: "Cart".into(),
                ..Default::default()
            },
        }],
        &crate::store::Commit::store("decree"),
    );
    assert_eq!(report.applied, 0);
    assert!(
        report
            .skipped
            .iter()
            .any(|s| s.starts_with("store-version")),
        "{:?}",
        report.skipped
    );
    assert_eq!(
        crate::vfs::read_to_string(out.join("status.yaml")).unwrap(),
        status
    );
    assert!(out.join("graph").join("entities.yaml").exists());
    assert!(!out.join("graph").join("entities").exists());

    // The build archives and starts over.
    let runner = ScriptedRunner::new(&project);
    let (report, events) = build(&project, &runner);
    eprintln!(
        "[e2e version] build over a version-2 store: {}",
        report.verdict
    );
    assert_clean_build(&report, &events);
    let bak = out.with_file_name(format!(
        "{}.bak",
        out.file_name().unwrap().to_string_lossy()
    ));
    assert!(
        bak.join("graph").join("entities.yaml").exists(),
        "{} not archived whole",
        bak.display()
    );
    assert_eq!(
        crate::vfs::read_to_string(bak.join("status.yaml")).unwrap(),
        status
    );
    assert!(!out.join("graph").join("entities.yaml").exists());
    assert!(out.join("graph").join("entities").is_dir());
    let store = Store::load(&out);
    assert_eq!(store.status.version, crate::store::STORE_VERSION);
    assert_eq!(store.status.version, 4);
    assert!(store.status.verdict.converged(), "{}", store.status.verdict);
    assert_eq!(store.graph.requirements.len(), 12);
    assert!(crate::vfs::read_to_string(out.join("status.yaml"))
        .unwrap()
        .contains("version: 4"));
    crate::vfs::remove_dir_all(&bak).ok();
    crate::vfs::remove_dir_all(&dir).ok();
}

// A batch that exhausts its unfinished-goal attempts parks its goals:
// `status.yaml` carries the reason, `Board::explain` shows it, the build is incomplete
// with an `incomplete-build` diagnostic, and the next build resumes the parked goals
// first and converges. Mirrors docs/compiler/reconciler.md#parked-and-failed.
#[test]
fn a_batch_exhausting_its_attempts_parks_its_goals_and_the_next_build_resumes_them_first() {
    let (dir, project) = f1_copy("parked");
    let runner = ScriptedRunner::new(&project);
    runner.fail_batches_of("review-entity", crate::limits::GOAL_ATTEMPTS);
    let started = std::time::Instant::now();
    let (report, events) = build(&project, &runner);
    eprintln!(
        "[e2e parked] build: {} in {:.1}s, {} sessions, {} parked",
        report.verdict,
        started.elapsed().as_secs_f64(),
        report.sessions,
        report.parked
    );
    let failed: Vec<(u32, String)> = events
        .iter()
        .filter_map(|e| match e {
            TraceEvent::SessionFailed { attempt, label, .. } => Some((*attempt, label.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        failed.len(),
        crate::limits::GOAL_ATTEMPTS as usize,
        "{:?}",
        failed
    );
    assert_eq!(
        failed
            .iter()
            .map(|(attempt, _)| *attempt)
            .collect::<Vec<_>>(),
        (1..=crate::limits::GOAL_ATTEMPTS).collect::<Vec<_>>(),
        "{:?}",
        failed
    );
    assert!(
        failed.iter().all(|(_, label)| label == &failed[0].1),
        "each retry is a fresh session on the same batch: {:?}",
        failed
    );
    let parked: BTreeMap<String, String> = events
        .iter()
        .filter_map(|e| match e {
            TraceEvent::Goal {
                goal,
                event,
                reason,
                ..
            } if event == "parked" => Some((goal.clone(), reason.clone().unwrap_or_default())),
            _ => None,
        })
        .collect();
    assert!(!parked.is_empty());
    // The whole batch parks: the review goals and whatever judgment batched with them.
    let batch_goals: BTreeSet<String> = events
        .iter()
        .find_map(|e| match e {
            TraceEvent::BatchStart { label, goals, .. } if label == &failed[0].1 => Some(
                goals
                    .iter()
                    .filter_map(|g| g["id"].as_str().map(String::from))
                    .collect(),
            ),
            _ => None,
        })
        .expect("the failed batch");
    assert_eq!(parked.keys().cloned().collect::<BTreeSet<_>>(), batch_goals);
    assert!(
        parked.keys().any(|g| g.starts_with("g:review-entity:")),
        "{:?}",
        parked
    );
    let expected_reason = format!(
        "left open after {} attempts in session {}",
        crate::limits::GOAL_ATTEMPTS,
        failed.last().unwrap().1
    );
    for reason in parked.values() {
        assert_eq!(reason, &expected_reason);
    }
    assert!(!report.converged(), "{}", report.verdict);
    assert_eq!(
        (report.parked, report.failed),
        (parked.len(), 0),
        "{:?}",
        report
    );
    let out = project.out.clone();
    let store = Store::load(&out);
    let stored: BTreeMap<String, String> = store
        .status
        .parked
        .iter()
        .map(|p| (p.goal.id.clone(), p.reason.clone().unwrap_or_default()))
        .collect();
    assert_eq!(stored, parked);
    assert!(
        store
            .status
            .parked
            .iter()
            .all(|p| p.goal.class == "compile" && p.goal.mandatory),
        "{:?}",
        store.status.parked
    );
    assert!(
        store
            .graph
            .diagnostics
            .values()
            .any(|d| d.rule == "incomplete-build" && d.lifecycle == "open"),
        "{:?}",
        store
            .graph
            .diagnostics
            .values()
            .map(|d| &d.rule)
            .collect::<Vec<_>>()
    );
    let control = crate::control::Control::load(&project, &out);
    let board = crate::board::Board::derive(&store, &project, &control);
    for id in parked.keys() {
        let text = board
            .explain(&store, id)
            .unwrap_or_else(|| panic!("{} not on the board", id));
        assert!(
            text.contains(&format!("parked: {}", expected_reason)),
            "{}",
            text
        );
        assert!(text.contains(", compile, mandatory, parked"), "{}", text);
    }
    assert_eq!(board.counts().parked, parked.len());

    // The next build resumes the parked goals first and converges.
    let started = std::time::Instant::now();
    let (report, events) = build(&project, &runner);
    eprintln!(
        "[e2e parked] resume: {} in {:.1}s, {} sessions",
        report.verdict,
        started.elapsed().as_secs_f64(),
        report.sessions
    );
    assert_clean_build(&report, &events);
    let first: BTreeSet<String> = events
        .iter()
        .find_map(|e| match e {
            TraceEvent::BatchStart { goals, .. } => Some(
                goals
                    .iter()
                    .filter_map(|g| g["id"].as_str().map(String::from))
                    .collect(),
            ),
            _ => None,
        })
        .expect("a batch");
    assert_eq!(
        first,
        parked.keys().cloned().collect::<BTreeSet<_>>(),
        "the parked goals did not run first"
    );
    let resolved: BTreeSet<&String> = goal_events(&events, "resolved").into_iter().collect();
    assert!(
        parked.keys().all(|g| resolved.contains(g)),
        "{:?}",
        resolved
    );
    let after = Store::load(&out);
    assert!(after.status.parked.is_empty() && after.status.failed.is_empty());
    assert!(after.status.verdict.converged(), "{}", after.status.verdict);
    assert!(!after
        .graph
        .diagnostics
        .values()
        .any(|d| d.rule == "incomplete-build" && d.lifecycle == "open"));
    assert_eq!(after.graph.requirements.len(), 12);
    crate::vfs::remove_dir_all(&dir).ok();
}

// A selected build runs only the goals in its selection: the selected document's
// sections reconcile ahead of the root document they would otherwise wait for, the
// other document's sections stay open, and the verdict stays honest. A selection
// widened before the next derivation reports on the trace and runs in the same
// build. Mirrors docs/compiler/compilation.md#selected-builds.
#[test]
fn a_selected_build_runs_only_its_selection_and_takes_a_widening() {
    use crate::selection::{Handle, Item};
    let (_dir, project) = f1_copy("selected");
    let runner = ScriptedRunner::new(&project);
    let doc = |p: &str| Item {
        path: p.into(),
        lines: None,
    };
    let handle = Handle::new(Some(vec![doc("docs/catalog.md")]));
    let (report, events) = {
        let _g = crate::selection::enter(handle.clone());
        build(&project, &runner)
    };
    let batch_targets: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            TraceEvent::BatchStart { goals, .. } => Some(goals.clone()),
            _ => None,
        })
        .flatten()
        .filter_map(|g| g["target"].as_str().map(str::to_string))
        .collect();
    assert!(
        batch_targets
            .iter()
            .any(|t| t.starts_with("docs/catalog.md")),
        "{:?}",
        batch_targets
    );
    assert!(
        !batch_targets
            .iter()
            .any(|t| t.starts_with("docs/system.md")),
        "{:?}",
        batch_targets
    );
    assert!(events.iter().any(|e| matches!(
        e,
        TraceEvent::Board {
            selection: Some(_),
            ..
        }
    )));
    assert!(!report.converged(), "verdict: {}", report.verdict);
    assert!(handle.sealed());
    let board = crate::board::Board::compute(&project, &project.out);
    assert!(board
        .open_goals()
        .iter()
        .any(|g| g.kind == "reconcile-section" && g.target.starts_with("docs/system.md")));
    assert!(!board
        .open_goals()
        .iter()
        .any(|g| g.kind == "reconcile-section" && g.target.starts_with("docs/catalog.md")));

    // A second selected build, widened to the whole set before it derives.
    let handle = Handle::new(Some(vec![doc("docs/catalog.md")]));
    assert_eq!(
        handle.widen(Some(vec![doc("docs/system.md")])),
        crate::selection::Widen::Widened
    );
    let (report, events) = {
        let _g = crate::selection::enter(handle.clone());
        build(&project, &runner)
    };
    assert!(events
        .iter()
        .any(|e| matches!(e, TraceEvent::Selection { .. })));
    assert!(report.converged(), "verdict: {}", report.verdict);
}

// The selected board lists only the goals in the selection, counts the rest, and
// resolves a line range to the sections it overlaps.
#[test]
fn the_selected_board_lists_only_the_selection() {
    let (_dir, project) = f1_copy("selected-board");
    let body = serde_json::json!({"selection": [{"path": "docs/catalog.md", "lines": [5, 6]}]});
    let v = crate::webapi::board::selected_board_value(&project, &project.out, &body).unwrap();
    let targets: Vec<&str> = v["goals"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|g| g["target"].as_str())
        .collect();
    assert!(!targets.is_empty(), "{v}");
    assert!(
        targets.iter().all(|t| t.starts_with("docs/catalog.md")),
        "{:?}",
        targets
    );
    assert!(
        targets.iter().any(|t| t.ends_with("/product")),
        "{:?}",
        targets
    );
    assert!(v["outside"].as_u64().unwrap() > 0);
    assert_eq!(v["selection"][0]["documents"][0], "docs/catalog.md");
    let bad = serde_json::json!({"selection": [{"path": "docs/nope.md"}]});
    assert!(crate::webapi::board::selected_board_value(&project, &project.out, &bad).is_err());
}
