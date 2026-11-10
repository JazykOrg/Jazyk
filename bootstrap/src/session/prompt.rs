use super::*;

// ---- payloads ----

// The fixed session contract; every kind-specific instruction arrives as data inside
// the assembled prompt. Mirrors docs/compiler/sessions.md#the-prompt.
pub(super) const AGENT_CONTRACT: &str =
    include_str!("../../../docs/compiler/goals/prompts/agent-contract.md");

// The feedback contract, filled into the contract's `{feedback}` slot.
pub(super) const FEEDBACK_NOTE: &str =
    include_str!("../../../docs/compiler/goals/prompts/feedback-note.md");

// The per-kind gate lines and the prompt's block framing.
// Mirrors docs/compiler/sessions.md#payload-files.
pub(crate) const GATES: crate::payload::Fragments = crate::payload::Fragments(include_str!(
    "../../../docs/compiler/goals/prompts/gates.md"
));
pub(crate) const BLOCKS: crate::payload::Fragments = crate::payload::Fragments(include_str!(
    "../../../docs/compiler/goals/prompts/session-blocks.md"
));

// The protocol line; {target} is the batch id.
const WORKER_PROTOCOL: &str =
    include_str!("../../../docs/compiler/goals/prompts/worker-protocol.md");
// A session run in process with its batch already open: no claim, no begin_goals.
// Mirrors docs/compiler/sessions.md#the-prompt.
const OPEN_BATCH_PROTOCOL: &str =
    include_str!("../../../docs/compiler/goals/prompts/open-batch-protocol.md");
// The protocol line for a prompt returned by begin_goals itself: the batch is already
// claimed when the agent reads it. Mirrors docs/compiler/sessions.md#the-prompt.
const SERVING_PROTOCOL: &str =
    include_str!("../../../docs/compiler/goals/prompts/serving-protocol.md");

// Which side of the claim the prompt is read from: ahead of it (an ACP session prompt,
// a preview) or after it (the instructions a standalone MCP serving returns).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Protocol {
    Worker,
    Serving,
    OpenBatch,
}

// The agent contract with the feedback note in its `{feedback}` slot: the first
// paragraph says what the session is, the second how to report that the rest of the
// prompt failed it. Mirrors docs/compiler/sessions.md#the-prompt.
pub(crate) fn agent_contract() -> String {
    crate::payload::fill(AGENT_CONTRACT, &[("feedback", FEEDBACK_NOTE.trim_end())])
}

// ---- the assembled prompt ----

// The ## Project block's data: the build identity, the mode, the counts.
// Mirrors docs/compiler/sessions.md#the-prompt.
pub struct ProjectBlock {
    pub generation: u64,
    pub mode: String,
    pub diagnostics: std::collections::BTreeMap<String, u64>,
    pub here: usize,
    pub elsewhere: usize,
    pub blocked: usize,
    // Open ratification proposals: derived facts awaiting a human, not defects.
    pub ratification: usize,
    // The batch id: names the session and the protocol line's {target}.
    pub batch: String,
    // The project's lint rules with their severities, so a `lint` diagnostic names a
    // rule the model has read. Mirrors docs/compiler/sessions.md#the-prompt.
    pub lint: crate::project::Linting,
}

impl ProjectBlock {
    // Derives every goal kind once. A caller holding a derived board passes its goals
    // to `from_goals` instead of deriving the board a second time.
    pub fn compute(store: &Store, batch: &[Goal], mode: &str) -> ProjectBlock {
        let gs = crate::gen::GenSettings::from_out(&store.out);
        let derived: Vec<Goal> = goals::REGISTRY
            .iter()
            .flat_map(|k| k.derive_goals(store, &gs))
            .collect();
        ProjectBlock::from_goals(store, &derived, batch, mode)
    }

    // The block counted from goals already derived (a board's).
    pub fn from_goals(store: &Store, derived: &[Goal], batch: &[Goal], mode: &str) -> ProjectBlock {
        let blocked = derived
            .iter()
            .filter(|g| goals::blocked_on_human(&g.kind))
            .count();
        let total = derived.len() - blocked;
        let here = batch.len();
        ProjectBlock {
            generation: store.status.generation,
            mode: mode.to_string(),
            diagnostics: store.open_diag_counts(),
            ratification: store
                .graph
                .diagnostics
                .values()
                .filter(|d| d.lifecycle == "open" && d.rule == "ratification-pending")
                .count(),
            here,
            elsewhere: total.saturating_sub(here),
            blocked,
            batch: format!("b{}-0", store.status.generation),
            lint: Default::default(),
        }
    }

    // The project's lint rules ride on the block. Mirrors
    // docs/compiler/sessions.md#the-prompt.
    pub fn with_lint(mut self, lint: &crate::project::Linting) -> ProjectBlock {
        self.lint = lint.clone();
        self
    }

    pub(super) fn render(&self) -> String {
        let line = |name: &str, vars: &[(&str, &str)]| BLOCKS.fill(name, vars) + "\n";
        let mut s = line("project", &[]);
        s.push_str(&line(
            "project-generation",
            &[
                ("generation", &self.generation.to_string()),
                ("mode", &self.mode),
            ],
        ));
        // The judgment findings on the graph; the checks' findings arrive with the
        // build's tail. Mirrors docs/compiler/sessions.md#the-prompt.
        let ratification = self.ratification.to_string();
        if self.diagnostics.is_empty() {
            s.push_str(&line("project-diagnostics-none", &[]));
        } else {
            let parts: Vec<String> = self
                .diagnostics
                .iter()
                .map(|(sev, n)| {
                    BLOCKS.fill(
                        "project-diagnostic-count",
                        &[("count", &n.to_string()), ("severity", sev)],
                    )
                })
                .collect();
            let note = if self.ratification > 0 {
                BLOCKS.fill(
                    "project-diagnostics-ratification",
                    &[("count", &ratification)],
                )
            } else {
                String::new()
            };
            s.push_str(&line(
                "project-diagnostics",
                &[("counts", &parts.join(", ")), ("ratification", &note)],
            ));
        }
        let note = if self.ratification > 0 {
            BLOCKS.fill("project-board-ratification", &[("count", &ratification)])
        } else {
            String::new()
        };
        s.push_str(&line(
            "project-board",
            &[
                ("here", &self.here.to_string()),
                ("elsewhere", &self.elsewhere.to_string()),
                ("blocked", &self.blocked.to_string()),
                ("ratification", &note),
            ],
        ));
        let rules: Vec<String> = self
            .lint
            .warnings
            .iter()
            .map(|r| ("warning", r))
            .chain(self.lint.errors.iter().map(|r| ("error", r)))
            .enumerate()
            .map(|(i, (severity, rule))| {
                BLOCKS.fill(
                    "project-lint-rule",
                    &[
                        ("index", &(i + 1).to_string()),
                        ("severity", severity),
                        ("rule", rule),
                    ],
                )
            })
            .collect();
        if !rules.is_empty() {
            s.push_str(&line("project-lint", &[("rules", &rules.join("; "))]));
        }
        s
    }
}

// The gate in one line, per kind, from gates.md. The kind's page under
// docs/compiler/goals/ states the full gate; this is the line the goal block carries.
pub fn gate_line(kind: &str) -> &'static str {
    GATES
        .sections()
        .into_iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, line)| line)
        .unwrap_or_else(|| GATES.get("unknown"))
}

// The change in one line: the condensed change payload plus the cause.
pub fn change_line(goal: &Goal) -> String {
    let mut s = if goal.change.is_null() {
        "(no change payload)".to_string()
    } else {
        llm::truncate(&change_words(&goal.change), 240).to_string()
    };
    if let Some(c) = &goal.cause {
        s.push_str(&format!(" (g{} via {})", c.generation, c.via));
    }
    s
}

// A change record as `field: value` pairs, one line, never raw JSON: `dirty: doc
// docs/orders.md, section /orders; unprocessed`, `revised: req:orders-6; fields:
// statement, transition`. Nested objects flatten to their own pairs, lists join with
// commas, a bare true is the field name alone, and false or empty fields are dropped.
// Mirrors docs/compiler/sessions.md#the-prompt.
pub fn change_words(v: &Value) -> String {
    fn scalar(v: &Value) -> Option<String> {
        match v {
            Value::Null => None,
            Value::Bool(false) => None,
            Value::Bool(true) => Some(String::new()),
            Value::String(s) if s.is_empty() => None,
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            Value::Array(a) if a.is_empty() => None,
            Value::Array(a) => {
                let parts: Vec<String> = a.iter().filter_map(scalar).collect();
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join(", "))
                }
            }
            Value::Object(o) if o.is_empty() => None,
            Value::Object(o) => {
                let parts: Vec<String> = o
                    .iter()
                    .filter_map(|(k, v)| scalar(v).map(|s| pair(k, &s)))
                    .collect();
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join(", "))
                }
            }
        }
    }
    fn pair(k: &str, v: &str) -> String {
        if v.is_empty() {
            k.to_string()
        } else {
            format!("{} {}", k, v)
        }
    }
    match v {
        Value::Object(o) => {
            let parts: Vec<String> = o
                .iter()
                .filter_map(|(k, v)| {
                    let s = scalar(v)?;
                    Some(if s.is_empty() {
                        k.to_string()
                    } else {
                        format!("{}: {}", k, s)
                    })
                })
                .collect();
            if parts.is_empty() {
                "(no change payload)".to_string()
            } else {
                parts.join("; ")
            }
        }
        other => scalar(other).unwrap_or_else(|| "(no change payload)".to_string()),
    }
}

// Assemble the session prompt for a batch, in the fixed order: the agent contract
// (feedback note as its second paragraph), the active skills, the project block, one
// block per goal, the loaded status block, the protocol line.
// Mirrors docs/compiler/sessions.md#the-prompt.
pub fn session_prompt(
    store: &Store,
    batch: &[Goal],
    loaded: &LoadedSet,
    skills: &SkillState,
    project_block: &ProjectBlock,
) -> String {
    session_prompt_elided(
        store,
        batch,
        loaded,
        skills,
        project_block,
        true,
        &BTreeSet::new(),
        Protocol::Worker,
    )
}

// The elided form the MCP serving uses after a serving's first batch: the agent
// contract and the skills already delivered ship as one-line references instead of
// their payloads; the project, goals, and loaded blocks always ship whole.
// Mirrors docs/frontends/mcp.md#compilation-over-mcp.
pub fn session_prompt_elided(
    _store: &Store,
    batch: &[Goal],
    loaded: &LoadedSet,
    skills: &SkillState,
    project_block: &ProjectBlock,
    include_contract: bool,
    delivered_skills: &BTreeSet<String>,
    protocol: Protocol,
) -> String {
    let mut s = if include_contract {
        agent_contract()
    } else {
        BLOCKS.get("contract-elided").to_string() + "\n"
    };
    if !s.ends_with('\n') {
        s.push('\n');
    }
    for (name, payload) in skills.active_payloads() {
        if delivered_skills.contains(&name) {
            s.push('\n');
            s.push_str(&BLOCKS.fill("skill-delivered", &[("name", &name)]));
            s.push('\n');
            continue;
        }
        s.push('\n');
        s.push_str(&BLOCKS.fill("skill-active", &[("name", &name)]));
        s.push('\n');
        s.push_str(payload);
        if !s.ends_with('\n') {
            s.push('\n');
        }
    }
    s.push('\n');
    s.push_str(&project_block.render());
    if batch
        .iter()
        .any(|g| matches!(g.kind.as_str(), "bind" | "generate" | "verify"))
    {
        s.push('\n');
        s.push_str(BLOCKS.get("task-execution"));
        s.push('\n');
        s.push_str(crate::task_runtime::PROMPT);
    }
    s.push('\n');
    s.push_str(BLOCKS.get("goals"));
    s.push('\n');
    let item = |name: &str, vars: &[(&str, &str)]| format!("  {}\n", BLOCKS.fill(name, vars));
    // The contract paragraph renders once per kind, under the first goal of that kind;
    // later goals of the kind point at it. Mirrors docs/compiler/sessions.md#the-prompt.
    let mut contracted: BTreeSet<String> = BTreeSet::new();
    // A hint repeated verbatim from an earlier goal renders once.
    let mut hinted: BTreeSet<String> = BTreeSet::new();
    for g in batch {
        s.push_str(&BLOCKS.fill(
            "goal",
            &[
                ("id", &g.id),
                (
                    "necessity",
                    if g.mandatory { "mandatory" } else { "optional" },
                ),
            ],
        ));
        s.push('\n');
        if let Some(k) = goals::kind(&g.kind) {
            let paragraph = k.prompt().trim();
            if !paragraph.is_empty() {
                if contracted.insert(g.kind.clone()) {
                    for line in paragraph.lines() {
                        s.push_str("  ");
                        s.push_str(line);
                        s.push('\n');
                    }
                } else {
                    s.push_str(&item("goal-contract-pointer", &[("kind", &g.kind)]));
                }
            }
        }
        s.push_str(&item("goal-change", &[("change", &change_line(g))]));
        s.push_str(&item("goal-gate", &[("gate", gate_line(&g.kind))]));
        let fresh: Vec<&String> = g
            .hints
            .iter()
            .filter(|h| hinted.insert((*h).clone()))
            .collect();
        if !fresh.is_empty() {
            let line: Vec<&str> = fresh.iter().map(|h| h.as_str()).collect();
            s.push_str(&item("goal-hints", &[("hints", &line.join("; "))]));
        }
    }
    s.push('\n');
    let pinned: BTreeSet<String> = batch.iter().map(|g| g.target.clone()).collect();
    s.push_str(&loaded.render_status(&skills.index_line(), skills.rendered_chars(), &pinned));
    // The items themselves follow the status block: the set is read, not only listed.
    // Mirrors docs/compiler/sessions.md#the-prompt.
    if !loaded.initial_pack.is_empty() {
        s.push('\n');
        s.push_str(BLOCKS.get("loaded-items"));
        s.push('\n');
        for t in &loaded.initial_pack {
            // An item's own `## id` header nests under the Loaded block.
            for line in t.trim_end().lines() {
                if let Some(rest) = line.strip_prefix("## ") {
                    s.push_str("### ");
                    s.push_str(rest);
                } else {
                    s.push_str(line);
                }
                s.push('\n');
            }
        }
    }
    s.push('\n');
    let line = match protocol {
        Protocol::Worker => WORKER_PROTOCOL,
        Protocol::Serving => SERVING_PROTOCOL,
        Protocol::OpenBatch => OPEN_BATCH_PROTOCOL,
    };
    s.push_str(&crate::payload::fill(
        line,
        &[("target", &project_block.batch)],
    ));
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

// The initially loaded set for a batch: the registry's pack computes it from the
// goals' hints; the skills of the batch's goal kinds activate from the first round.
// Mirrors docs/compiler/context.md#the-loaded-set.
pub fn initial_loaded(store: &Store, batch: &[Goal]) -> (LoadedSet, SkillState) {
    let mut loaded = LoadedSet::new(crate::limits::CONTEXT_BUDGET);
    let mut skills = SkillState::new();
    for g in batch {
        for s in goals::skills_for(&g.kind, store, &g.target) {
            skills.pin(s);
        }
    }
    let mut kinds_seen: Vec<&str> = Vec::new();
    for g in batch {
        if kinds_seen.contains(&g.kind.as_str()) {
            continue;
        }
        kinds_seen.push(&g.kind);
        let Some(k) = goals::kind(&g.kind) else {
            continue;
        };
        let of_kind: Vec<Goal> = batch.iter().filter(|x| x.kind == g.kind).cloned().collect();
        for line in k.pack(store, &of_kind).lines() {
            let full = line.contains(" full");
            let Some(target) = line
                .trim_start_matches("- ")
                .split_whitespace()
                .find(|t| resolves(store, t))
            else {
                continue;
            };
            if loaded.contains(target) {
                continue;
            }
            // A goal's own target is primary, and so is each side of a pair
            // target: a judgment cannot run on stubs of its own subjects.
            let primary = batch.iter().any(|g| {
                target == format!("alignment:{}", g.target)
                    || g.target == target
                    || g.target.split('~').any(|part| part == target)
            });
            if full && (primary || !loaded.over_high_water(0)) {
                let text = loaded.load(store, target, 1).ok();
                // The mark is a ceiling, not a suggestion: a supporting item that
                // lands the set past it downgrades to a stub, and the model loads
                // it back deliberately when it earns the budget. A goal's own
                // target stays full: the goal cannot be worked without its subject.
                if !primary && loaded.used() > loaded.high_water {
                    loaded.unload(target);
                    stub_or_defer(&mut loaded, store, target);
                } else if let Some(t) = text {
                    // What stays loaded in full is read in the prompt, not only listed.
                    loaded.initial_pack.push(t);
                }
            } else if primary {
                loaded.load_stub(store, target);
            } else {
                stub_or_defer(&mut loaded, store, target);
            }
        }
    }
    (loaded, skills)
}

// A supporting item enters as a stub while the stub fits the budget; past it the item
// is named on the status's Not loaded line instead. Mirrors
// docs/compiler/context.md#the-loaded-set.
fn stub_or_defer(loaded: &mut LoadedSet, store: &Store, target: &str) {
    loaded.load_stub(store, target);
    if loaded.used() > loaded.budget {
        loaded.unload(target);
        loaded.deferred.push(target.to_string());
    }
}

// A pack line's loadable target: a node, a document, a section, or `scope:<scope>`,
// the top level of a scope (docs/compiler/concepts/levels.md#the-scope-root).
pub(super) fn resolves(store: &Store, target: &str) -> bool {
    let id = store.resolve_id(target);
    if let Some(doc) = id.strip_prefix("alignment:") {
        return store.status.alignment.iter().any(|block| block.doc == doc);
    }
    if let Some(scope) = crate::board::scope_target(id) {
        return !crate::board::scope_root(store, scope).is_empty();
    }
    store.graph.entities.contains_key(id)
        || store.graph.requirements.contains_key(id)
        || store.graph.views.contains_key(id)
        || store.graph.diagnostics.contains_key(id)
        || store.docs.contains_key(id)
        || crate::model::split_section_ref(id)
            .map(|(d, s)| {
                store
                    .docs
                    .get(&d)
                    .map(|r| r.sections.contains_key(&s))
                    .unwrap_or(false)
            })
            .unwrap_or(false)
}

// Render the prompt a batch would receive, exactly as the model would see it: the
// same code, no LLM call, nothing written. `jazyk preview` and the GUI use it.
// Mirrors docs/compiler/sessions.md#preview.
pub fn preview(store: &Store, batch: &[Goal]) -> String {
    preview_with(store, batch, &Default::default())
}

// The same preview with the project's lint rules on the project block.
pub fn preview_with(store: &Store, batch: &[Goal], lint: &crate::project::Linting) -> String {
    let (loaded, skills) = initial_loaded(store, batch);
    let pb = ProjectBlock::compute(store, batch, "auto").with_lint(lint);
    session_prompt(store, batch, &loaded, &skills, &pb)
}

// The prompt of a session run in process with its batch already open (a benchmark
// case, a snippet, a replay): the same prompt with the open-batch protocol line.
// Mirrors docs/compiler/sessions.md#the-prompt.
pub fn open_batch_prompt(store: &Store, batch: &[Goal], lint: &crate::project::Linting) -> String {
    let (loaded, skills) = initial_loaded(store, batch);
    let pb = ProjectBlock::compute(store, batch, "auto").with_lint(lint);
    let prompt = session_prompt_elided(
        store,
        batch,
        &loaded,
        &skills,
        &pb,
        true,
        &BTreeSet::new(),
        Protocol::OpenBatch,
    );
    with_opening_contract(prompt, batch)
}

// A batch holding a generate goal opens with the one generation contract after the
// assembled prompt, whichever session form runs it (an owned worker, a benchmark
// case, a snippet, a replay); its begin_generation replies then omit it.
// Mirrors docs/compiler/goals/generate.md#what-the-model-sees.
pub fn with_opening_contract(mut prompt: String, batch: &[Goal]) -> String {
    if batch.iter().any(|goal| goal.kind == "generate") {
        prompt.push_str("\n\n");
        prompt.push_str(&crate::gen::contract());
    }
    prompt
}

// One line naming a requirement's provenance, whichever kind it carries.
pub fn provenance_line(r: &crate::model::Requirement) -> String {
    match r.provenance() {
        Some(crate::model::ProvenanceRef::Quote(s)) => {
            format!("{}#{} \"{}\"", s.doc, s.section, s.quote)
        }
        Some(crate::model::ProvenanceRef::Derived { from, reasoning }) => {
            format!("derived from {} ({})", from.join(", "), reasoning)
        }
        Some(crate::model::ProvenanceRef::Decree { author, at, note }) => {
            format!(
                "decreed by {} at {}{}",
                author,
                at,
                note.map(|n| format!(" ({})", n)).unwrap_or_default()
            )
        }
        None => "(no provenance)".to_string(),
    }
}

pub fn condense(v: &Value, n: usize) -> String {
    llm::truncate(&v.to_string(), n)
}

// Keep the original serialized value behind a condensed line. Wire elision and
// host event admission limits belong to the transport, not the retained JSON.
pub fn full_payload(v: &Value) -> Option<String> {
    let s = v.to_string();
    if s.len() <= 160 {
        None
    } else {
        Some(s)
    }
}

// The section a tool call names, when it belongs to the session's document. Every
// runner emits a `section` event from it (docs/compiler/sessions.md#trace-events).
pub fn named_section(args: &Value, doc: &str) -> Option<String> {
    let raw = args["section"]
        .as_str()
        .or_else(|| args["mention"]["section"].as_str())?;
    match crate::model::split_section_ref(raw) {
        Some((d, sec)) => (d == doc).then_some(sec),
        None => raw.starts_with('/').then(|| raw.to_string()),
    }
}

// The rule and message a refused tool call carries: a jazyk refusal is
// `{"error": {"rule", "message"}}`; anything else is its own message under
// `fallback` (docs/compiler/tools.md).
pub fn refusal(text: &str, fallback: &str) -> (String, String) {
    let parsed: Value =
        serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()));
    let rule = parsed["error"]["rule"]
        .as_str()
        .unwrap_or(fallback)
        .to_string();
    let message = parsed["error"]["message"]
        .as_str()
        .unwrap_or(text)
        .to_string();
    (rule, message)
}
