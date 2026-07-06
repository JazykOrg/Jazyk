// The turn harness: one focused LLM session with tools. Wires the model to the tool
// registry through a codec, stages mutations, and hands the finished changeset back to
// the reconciler for commit. Mirrors docs2/compiler/turns.md.
use crate::llm::{self, Llm};
use crate::model::WorkItem;
use crate::project::{Limits, Linting};
use crate::store::Store;
use crate::tools::{catalog, toolset, ToolDef, ToolSession, WorkScope};
use serde_json::{json, Value};

// ---- trace ----

#[derive(Clone, Copy, PartialEq)]
pub enum TraceLevel {
    Quiet,
    Normal,
    Verbose,
}

#[derive(Clone)]
pub struct Trace {
    pub level: TraceLevel,
}

impl Trace {
    fn on(&self) -> bool {
        self.level != TraceLevel::Quiet
    }
    pub fn line(&self, prefix: &str, s: &str) {
        if self.on() {
            eprintln!("[{}] {}", prefix, s);
        }
    }
    fn verbose(&self, prefix: &str, s: &str) {
        if self.level == TraceLevel::Verbose {
            eprintln!("[{}] {}", prefix, s);
        }
    }
}

// ---- codecs ----

enum Action {
    Call { id: Option<String>, name: String, args: Value },
    Text(String),
}

trait Codec {
    // The extra system-prompt section this codec needs (tool docs for the text codec).
    fn system_suffix(&self, tools: &[&ToolDef]) -> String;
    fn tools_param(&self, tools: &[&ToolDef]) -> Option<Vec<Value>>;
    fn parse(&self, msg: &Value) -> Vec<Action>;
    // The message that carries a tool result back to the model.
    fn result_msg(&self, call_id: &Option<String>, name: &str, result: &Value) -> Value;
    // The corrective message when a reply contained no usable action.
    fn nudge(&self) -> Value;
}

struct NativeCodec;

impl Codec for NativeCodec {
    // Pacing is the codec's to give: native batches, text goes one action per reply.
    // The shared system prompt stays codec-neutral. Mirrors docs2/compiler/turns.md#codecs.
    fn system_suffix(&self, _tools: &[&ToolDef]) -> String {
        "\n\nBatch ALL tool calls for one section into a single reply: the searches, the upserts, and its coverage mark together.".to_string()
    }
    fn tools_param(&self, tools: &[&ToolDef]) -> Option<Vec<Value>> {
        Some(
            tools
                .iter()
                .map(|t| {
                    json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}})
                })
                .collect(),
        )
    }
    fn parse(&self, msg: &Value) -> Vec<Action> {
        let mut out = Vec::new();
        if let Some(text) = msg["content"].as_str() {
            if !text.trim().is_empty() {
                out.push(Action::Text(text.to_string()));
            }
        }
        if let Some(calls) = msg["tool_calls"].as_array() {
            for c in calls {
                let name = c["function"]["name"].as_str().unwrap_or_default().to_string();
                let args = match c["function"]["arguments"].as_str() {
                    Some(s) => serde_json::from_str(s).unwrap_or(json!({})),
                    None => c["function"]["arguments"].clone(),
                };
                out.push(Action::Call {
                    id: c["id"].as_str().map(|s| s.to_string()),
                    name,
                    args,
                });
            }
        }
        out
    }
    fn result_msg(&self, call_id: &Option<String>, name: &str, result: &Value) -> Value {
        json!({
            "role": "tool",
            "tool_call_id": call_id.clone().unwrap_or_else(|| name.to_string()),
            "content": result.to_string()
        })
    }
    fn nudge(&self) -> Value {
        json!({"role": "user", "content": "Reply by calling a tool. When the work is complete, call done."})
    }
}

struct TextCodec;

impl Codec for TextCodec {
    fn system_suffix(&self, tools: &[&ToolDef]) -> String {
        let mut s = String::from(
            "\n\nTOOL PROTOCOL: you have no native tool support. Reply with EXACTLY ONE JSON object per message, nothing else:\n{\"tool\": \"<name>\", \"args\": { ... }}\nThe result comes back as a message starting with RESULT:. Then reply with your next action. Available tools:\n",
        );
        for t in tools {
            s.push_str(&format!("- {}: {} args schema: {}\n", t.name, t.description, t.parameters));
        }
        s
    }
    fn tools_param(&self, _tools: &[&ToolDef]) -> Option<Vec<Value>> {
        None
    }
    fn parse(&self, msg: &Value) -> Vec<Action> {
        let content = msg["content"].as_str().unwrap_or_default();
        if let Some(obj) = llm::extract_json_object(content) {
            if let Ok(v) = serde_json::from_str::<Value>(&obj) {
                if let Some(name) = v["tool"].as_str() {
                    return vec![Action::Call {
                        id: None,
                        name: name.to_string(),
                        args: v["args"].clone(),
                    }];
                }
            }
        }
        if content.trim().is_empty() {
            Vec::new()
        } else {
            vec![Action::Text(content.to_string())]
        }
    }
    fn result_msg(&self, _call_id: &Option<String>, _name: &str, result: &Value) -> Value {
        json!({"role": "user", "content": format!("RESULT: {}", result)})
    }
    fn nudge(&self) -> Value {
        json!({"role": "user", "content": "Reply with exactly one JSON action object: {\"tool\": \"<name>\", \"args\": {...}}. When the work is complete, use the done tool."})
    }
}

// ---- prompts ----

const RECONCILE_SYSTEM: &str = r#"You are the compilation turn of jazyk, a natural language compiler. Your job: bring the semantic graph in line with one document's changed sections, by calling tools.

The graph holds entities (domain concepts), EARS requirements attached to entities, and a coverage mark per section.

Work section by section, finishing one before starting the next. For ONE section:
1. Apply this test to every sentence: does it say what the system or one of its parts IS, DOES, USES, ALLOWS, REQUIRES, or LIMITS? If yes, it is a requirement. Documentation rarely says 'shall'; rephrase the sentence into an EARS shall statement and keep the source sentence verbatim as the quote. Statements of composition and technology choice pass the test: "The gateway is a REST service built with Go" yields TWO requirements ("The gateway shall be a REST service.", "The gateway shall be built with Go."), one atomic fact each, both quoting that same sentence. Never put two facts in one ears statement. Access and permission rules pass the test too: "All management operations can be performed by Admins only." IS a requirement ("The user management system shall allow only Admins to perform management operations."), not background.
2. A sentence ending in a colon followed by a list is a claim about EACH item. The lead-in sentence alone states nothing; never record it as a requirement by itself. Record one requirement per list item, quoting that item's own bullet line verbatim. An item naming an actor, a component, a sub-system, or a stored field also introduces that entity.
3. For every entity a requirement mentions: call search first. Reuse an existing entity when it means the same concept, even under another name: "backend", "backend system", and "the Warehouse backend" are ONE entity. When you reuse under a different wording, record that wording with update_entity add_aliases. Create with upsert_entity only when search finds nothing that means the same thing. Tools take ids (ent:...), never display names.
4. Tag each requirement with the entity the statement is about: its own grammatical subject. Never substitute a broader system for a named part ("The inventory system manages products" is about the inventory system, not the application containing it). One sentence introduces at most one entity for its subject: "This software is a warehouse management system" defines ONE entity, not two.
5. Record each requirement with upsert_requirement. The quote is copied character for character from the section body shown to you; for a bulleted item, quote that single bullet line exactly as it appears. Never paraphrase, merge, or reflow a quote.
6. Then set_coverage for the section, exactly once, after its extraction: covered when you recorded (or the pack shows the section already yielded) a requirement sourced from it. non-normative is the EXCEPTION, allowed only when NO sentence passed the test: navigation pages that only link elsewhere, glossaries defining outside-world terms, changelogs, roadmap wish lists. If any sentence is about the system, extract from it instead.

Then repeat for the next dirty section. Stale anchors are a contract: for each one, if the document still states the fact, re-record it with upsert_requirement (the same statement with a fresh verbatim quote updates it in place); if the fact is gone, delete_requirement. done is rejected while a stale anchor is untouched. When every dirty section has its coverage mark, call done with a one-line summary. If done is rejected, repair exactly what the error names, then call done again.

Rules:
- Entities are the system's own parts, actors, and domain objects: a component, a type, a field, a user role, a stored record, a product. Never file paths, CLI flags, markdown terms, or generic phrases. The document itself (a glossary, a roadmap, an overview) is not an entity.
- Technologies, languages, and third-party tools named in a statement (React, Go, PostgreSQL) belong in the ears text, NOT as entities. "The gateway shall be built with Go" references the entity gateway only.
- Extract only obligations the source itself states; never invent facts the text does not carry. But grammar does not matter: a plain declarative sentence about the system is an obligation, and a sentence naming what something is built with, composed of, or responsible for is a requirement, not background.
- When a requirement ties two entities structurally, declare the pair in edges with a relationship type. A sub-system list is the common case: "the sub-systems are: X, Y" ties each sub-system to its parent.
- Prefer attaching detail to a requirement over minting a new entity; mint a sub-entity only when statements are about it directly.
- Never set scope on an entity unless the documents explicitly name a bounded context. An invented scope splits one concept into two.
- The ears text may rephrase the statement into EARS form, but the quote must stay a verbatim copy of the source sentence.
- A tool error names what was wrong and how to repair the call; fix it and continue.
- Staging nothing is a correct outcome. If the graph already reflects the sections (the pack lists what each section already yielded), set coverage and finish. Prefer a no-op over cosmetic rewording of existing definitions or statements; stability of the graph across builds matters more than polish. Stage only what the document supports."#;

const REVIEW_SYSTEM: &str = r#"You are the review turn of jazyk, a natural language compiler. Your job: judge one entity whose facts changed, by calling tools.

Work in this order:
1. Read the entity and its requirements (gathered across all documents) in the pack below.
2. If the definition no longer matches the requirements as a whole, refresh it with update_entity.
3. Judge every lookalike candidate listed below. A name variant ("backend" vs "backend system"), a synonym, or the same thing at different detail is the SAME concept: merge with merge_entities (keep the better-established id) and say why. Merging is the expected outcome for lookalikes; keeping both is the exception and needs a reason. The absorbed name survives as an alias and its requirements follow automatically.
4. Report real problems with report_diagnostic: rule contradiction for requirements that cannot all hold, duplicate-entity for two entities that are one concept, ambiguity for a statement open to more than one reading, missing-link for a concept the documents rely on but never define.
5. If requirements tie this entity to another structurally but declare no edges, add them with update_requirement (keep ears and entities unchanged, supply edges with a relationship type).
6. If an open diagnostic shown in the pack no longer holds, resolve it with resolve_diagnostic.
7. Call done with a one-line summary.

Rules:
- Documentation is loose by design. Flag only findings the document author can act on. Do not demand formal-spec completeness (persistence details, versioning, exhaustive cases).
- Severity: error only when two statements cannot both hold; warning for real but repairable issues; info for observations.
- If everything is coherent, call done immediately with no mutations."#;

// ---- initial packs ----

fn reconcile_pack(store: &Store, item: &WorkItem, budget: usize) -> String {
    let mut s = String::new();
    let doc = &item.target;
    s.push_str(&format!("# Work item: reconcile document {}\n", doc));
    if let Some(rec) = store.docs.get(doc) {
        let covered = rec.coverage.len();
        s.push_str(&format!("sections: {} total, {} with coverage\n", rec.sections.len(), covered));
    }

    // Known entities: this document's neighborhood first, then the rest of the graph.
    let mut lines: Vec<String> = Vec::new();
    let mut listed: Vec<&String> = Vec::new();
    for (id, e) in &store.graph.entities {
        if e.mentions.iter().any(|m| &m.doc == doc) {
            lines.push(format!("- {} ({}): {}", id, e.name, crate::llm::truncate(e.definition.as_deref().unwrap_or(""), 80)));
            listed.push(id);
        }
    }
    for (id, e) in &store.graph.entities {
        if lines.len() >= 40 {
            lines.push(format!("- (and {} more; use search)", store.graph.entities.len() - lines.len() + 1));
            break;
        }
        if !listed.contains(&id) {
            lines.push(format!("- {} ({}): {}", id, e.name, crate::llm::truncate(e.definition.as_deref().unwrap_or(""), 80)));
        }
    }
    if !lines.is_empty() {
        s.push_str("\n## Known entities (search before creating new ones)\n");
        s.push_str(&lines.join("\n"));
        s.push('\n');
    }

    if !item.stale_anchors.is_empty() {
        s.push_str("\n## Stale anchors (their source text changed or vanished; re-anchor, update, or delete)\n");
        for a in &item.stale_anchors {
            if let Some(r) = store.graph.requirements.get(a) {
                s.push_str(&format!("- {}: {} (was quoted: \"{}\")\n", a, r.ears, crate::llm::truncate(&r.source.quote, 100)));
            } else if let Some(e) = store.graph.entities.get(a) {
                s.push_str(&format!("- {} (entity {}): a mention's section changed\n", a, e.name));
            }
        }
    }

    s.push_str("\n## Dirty sections\n");
    let per_section = budget.saturating_sub(s.len()) / item.dirty_sections.len().max(1);
    if let Some(rec) = store.docs.get(doc) {
        for r in &item.dirty_sections {
            if let Some(sec) = rec.sections.get(r) {
                let cov = rec
                    .coverage
                    .get(r)
                    .map(|c| c.state.clone())
                    .unwrap_or_else(|| "unprocessed".to_string());
                s.push_str(&format!("\n### {}#{} ({}) [coverage: {}]\n", doc, r, sec.title, cov));
                if sec.raw.len() <= per_section {
                    s.push_str(&sec.raw);
                } else {
                    s.push_str(&crate::llm::truncate(&sec.raw, per_section));
                    s.push_str(&format!("\n(truncated; read_section {}#{} for the rest)", doc, r));
                }
                s.push('\n');
                // What the section already yielded: an unchanged statement is a no-op,
                // and a coverage claim must see the requirements anchored here before
                // judging the section non-normative.
                let existing: Vec<String> = store
                    .graph
                    .requirements
                    .iter()
                    .filter(|(_, q)| &q.source.doc == doc && &q.source.section == r)
                    .map(|(id, q)| format!("- {}: {}", id, q.ears))
                    .collect();
                if !existing.is_empty() {
                    s.push_str("Already extracted from this section (leave unchanged statements alone):\n");
                    s.push_str(&existing.join("\n"));
                    s.push('\n');
                }
            }
        }
    }
    s
}

fn review_pack(store: &Store, entity_id: &str, budget: usize, lint: &Linting) -> String {
    let mut s = String::new();
    s.push_str(&format!("# Work item: review entity {}\n\n", entity_id));
    match crate::context::assemble(
        store,
        entity_id,
        &crate::context::Focus { parents: 1, mentions: 1, requirements: 2 },
        budget.saturating_sub(1200),
    ) {
        Ok(pack) => s.push_str(&pack.pack),
        Err(e) => s.push_str(&format!("(context error: {})\n", e)),
    }
    // Lookalike candidates: token-overlap hits on the entity's name, excluding itself.
    if let Some(e) = store.graph.entities.get(entity_id) {
        let hits = store.search(&e.name);
        let others: Vec<String> = hits
            .iter()
            .filter(|(id, _, _)| id != entity_id)
            .map(|(id, name, def)| format!("- {} ({}): {}", id, name, crate::llm::truncate(def, 80)))
            .collect();
        if !others.is_empty() {
            s.push_str("\n## Lookalike candidates (merge only if truly the same concept)\n");
            s.push_str(&others.join("\n"));
            s.push('\n');
        }
    }
    // Project lint rules run in review turns; violations use report_diagnostic rule `lint`.
    if !lint.warnings.is_empty() || !lint.errors.is_empty() {
        s.push_str("\n## Project lint rules\nReport a violation with report_diagnostic, rule `lint`, and the severity listed.\n");
        for w in &lint.warnings {
            s.push_str(&format!("- (warning) {}\n", w));
        }
        for e in &lint.errors {
            s.push_str(&format!("- (error) {}\n", e));
        }
    }
    s
}

// ---- the loop ----

pub struct TurnOutput {
    pub session: ToolSession,
    pub rounds: u32,
    pub failed: Option<String>,
}

fn condense(v: &Value, n: usize) -> String {
    llm::truncate(&v.to_string(), n)
}

pub fn run_turn(llm: &Llm, snapshot: Store, item: &WorkItem, limits: &Limits, lint: &Linting, trace: &Trace) -> TurnOutput {
    let prefix = format!("{} {}", item.task, item.target);
    let scope = match item.task.as_str() {
        "reconcile-doc" => WorkScope {
            task: item.task.clone(),
            doc: Some(item.target.clone()),
            target_sections: item.dirty_sections.clone(),
            stale_anchors: item.stale_anchors.clone(),
        },
        _ => WorkScope { task: item.task.clone(), doc: None, target_sections: Vec::new(), stale_anchors: Vec::new() },
    };
    let (system, pack) = match item.task.as_str() {
        "reconcile-doc" => (RECONCILE_SYSTEM, reconcile_pack(&snapshot, item, limits.context_budget)),
        _ => (REVIEW_SYSTEM, review_pack(&snapshot, &item.target, limits.context_budget, lint)),
    };
    let names = toolset(&item.task);
    let all_defs = catalog();
    let defs: Vec<&ToolDef> = all_defs.iter().filter(|t| names.contains(&t.name)).collect();
    let mut session = ToolSession::new(snapshot, scope, limits.turn_mutations, limits.context_budget);

    trace.line(&prefix, &format!("turn start ({} dirty, {} stale)", item.dirty_sections.len(), item.stale_anchors.len()));
    trace.verbose(&prefix, &format!("--- context pack ---\n{}\n--- end pack ---", pack));

    // Codec selection with a first-round probe: native unless the run already learned otherwise.
    let mut mode = llm::tools_mode();
    if mode == 0 {
        if let Ok(env) = std::env::var("JAZYK_CODEC") {
            mode = match env.as_str() {
                "text" => 2,
                "native" => 1,
                _ => 0,
            };
            if mode != 0 {
                llm::set_tools_mode(mode);
            }
        }
    }

    'codec: loop {
        let codec: Box<dyn Codec> = if mode == 2 { Box::new(TextCodec) } else { Box::new(NativeCodec) };
        let mut messages = vec![
            json!({"role": "system", "content": format!("{}{}", system, codec.system_suffix(&defs))}),
            json!({"role": "user", "content": pack.clone()}),
        ];
        let tools_param = codec.tools_param(&defs);
        let mut invalid_streak = 0u32;
        let mut rounds = 0u32;
        // The round budget scales with extraction density: a one-action-per-reply model
        // needs a round per mutation, so a dense work item gets at least 8 rounds per
        // dirty section. Mirrors docs2/compiler/turns.md#budgets.
        let round_budget = limits.turn_rounds.max(item.dirty_sections.len() as u32 * 8);

        while rounds < round_budget {
            rounds += 1;
            let label = format!("{} r{}", prefix, rounds);
            let msg = match llm.chat_messages(&messages, tools_param.as_deref(), &label) {
                Ok(m) => m,
                Err(e) if e.starts_with("tools-rejected:") && mode != 2 => {
                    trace.line(&prefix, "endpoint rejected native tools; downgrading to the text codec for this run");
                    llm::set_tools_mode(2);
                    mode = 2;
                    continue 'codec;
                }
                Err(e) => {
                    return TurnOutput { session, rounds, failed: Some(e) };
                }
            };
            let actions = codec.parse(&msg);
            // First-round probe: a prose-only reply under the native codec means the model
            // does not drive tools natively; downgrade once, sticky for the run.
            if rounds == 1
                && mode != 2
                && llm::tools_mode() == 0
                && !actions.iter().any(|a| matches!(a, Action::Call { .. }))
            {
                trace.line(&prefix, "model answered prose without tool calls; downgrading to the text codec for this run");
                llm::set_tools_mode(2);
                mode = 2;
                continue 'codec;
            }
            if mode != 2 && llm::tools_mode() == 0 && actions.iter().any(|a| matches!(a, Action::Call { .. })) {
                llm::set_tools_mode(1);
                mode = 1;
            }
            messages.push(msg.clone());

            if !actions.iter().any(|a| matches!(a, Action::Call { .. })) {
                invalid_streak += 1;
                if invalid_streak >= 3 {
                    // Implicit done: a model that goes silent with staged work is treated
                    // as having called done; the same commit gates apply.
                    if session.finish_implicit("(implicit: the model stopped calling tools)") {
                        trace.line(&prefix, &format!("✓ implicit done ({} staged, {} rounds)", session.staged.len(), rounds));
                        return TurnOutput { session, rounds, failed: None };
                    }
                    return TurnOutput {
                        session,

                        rounds,
                        failed: Some("three consecutive replies without a usable tool call".into()),
                    };
                }
                messages.push(codec.nudge());
                continue;
            }

            let mut errored = false;
            for action in actions {
                match action {
                    Action::Text(t) => {
                        let t = t.trim();
                        if !t.is_empty() {
                            trace.line(&prefix, &format!("· {}", llm::truncate(t, 200)));
                        }
                    }
                    Action::Call { id, name, args } => {
                        trace.line(&prefix, &format!("→ {} {}", name, condense(&args, 160)));
                        trace.verbose(&prefix, &format!("full args: {}", args));
                        let result = match session.dispatch(&name, &args) {
                            Ok(v) => {
                                trace.line(&prefix, &format!("← {}", condense(&v, 160)));
                                trace.verbose(&prefix, &format!("full result: {}", v));
                                v
                            }
                            Err(e) => {
                                errored = true;
                                trace.line(&prefix, &format!("✗ {}: {}", e.rule, e.message));
                                e.to_value()
                            }
                        };
                        messages.push(codec.result_msg(&id, &name, &result));
                        if session.done.is_some() {
                            trace.line(&prefix, &format!("✓ done ({} staged, {} rounds): {}", session.staged.len(), rounds, session.done.clone().unwrap_or_default()));
                            return TurnOutput { session, rounds, failed: None };
                        }
                    }
                }
            }
            if errored {
                invalid_streak += 1;
                if invalid_streak >= 3 {
                    return TurnOutput {
                        session,

                        rounds,
                        failed: Some("three consecutive rounds with rejected tool calls".into()),
                    };
                }
            } else {
                invalid_streak = 0;
            }
        }
        // Same implicit-done rule at the round budget: commit valid staged work.
        if session.finish_implicit("(implicit: round budget exhausted)") {
            trace.line(&prefix, &format!("✓ implicit done at round budget ({} staged)", session.staged.len()));
            return TurnOutput { session, rounds: round_budget, failed: None };
        }
        return TurnOutput {
            session,

            rounds: round_budget,
            failed: Some(format!("round budget ({}) exhausted without done", round_budget)),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_codec_parses_single_action() {
        let c = TextCodec;
        let msg = json!({"role": "assistant", "content": "I will search first.\n{\"tool\": \"search\", \"args\": {\"query\": \"cart\"}}"});
        let actions = c.parse(&msg);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::Call { name, args, .. } => {
                assert_eq!(name, "search");
                assert_eq!(args["query"], "cart");
            }
            _ => panic!("expected a call"),
        }
    }

    #[test]
    fn text_codec_prose_is_text() {
        let c = TextCodec;
        let msg = json!({"role": "assistant", "content": "The document describes a shop."});
        let actions = c.parse(&msg);
        assert!(matches!(actions[0], Action::Text(_)));
    }

    #[test]
    fn native_codec_parses_tool_calls() {
        let c = NativeCodec;
        let msg = json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{"id": "c1", "function": {"name": "done", "arguments": "{\"summary\": \"ok\"}"}}]
        });
        let actions = c.parse(&msg);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::Call { id, name, args } => {
                assert_eq!(id.as_deref(), Some("c1"));
                assert_eq!(name, "done");
                assert_eq!(args["summary"], "ok");
            }
            _ => panic!("expected a call"),
        }
    }
}
