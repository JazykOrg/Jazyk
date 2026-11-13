use super::*;

// What the repeated-call guard and the feedback cap remember, carried between the
// per-call sessions of a serving with no open batch
// (docs/compiler/sessions.md#repeated-calls).
#[derive(Default, Clone)]
pub struct CallMemory {
    repeats: std::collections::BTreeMap<String, u32>,
    last_answers: std::collections::BTreeMap<String, (String, (u64, usize))>,
    feedback_count: usize,
    file_hashes: std::collections::BTreeMap<String, String>,
}

impl CallMemory {
    // A new unit starts a fresh guard, but the file versions the serving last saw
    // stay valid: a literal edit after a new ledger claim still applies to them.
    // Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
    pub fn for_new_unit(&self) -> CallMemory {
        CallMemory {
            file_hashes: self.file_hashes.clone(),
            ..Default::default()
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ToolSession {
    pub snapshot: Store,
    pub scope: WorkScope,
    pub staged: Vec<Op>,
    pub done: Option<String>,
    // The session's working set, re-rendered condensed on every mutating reply.
    pub loaded: LoadedSet,
    // The skills rendered this session, active or inactive, capped.
    pub skills: SkillState,
    // Resolved [gen] settings for the generation and verification tools.
    pub gen: crate::gen::GenSettings,
    // Who is driving this session, recorded on every feedback entry so a record names
    // its caller. Set by the harness that owns the session.
    pub caller: crate::feedback::Caller,
    // Goal outcomes the session recorded: resolved with a justification, or failed.
    pub(super) outcomes: std::collections::BTreeMap<String, GoalOutcome>,
    // The repeated-call guard, keyed per open batch: the same call with the same
    // arguments has the same answer (docs/compiler/sessions.md#repeated-calls).
    pub(super) repeats: std::collections::BTreeMap<String, u32>,
    // The last answer per repeat key, with the generation and staged-mutation count it
    // was given under: a repeat whose answer changed is no repeat.
    pub(super) last_answers: std::collections::BTreeMap<String, (String, (u64, usize))>,
    pub(super) refusals: u32,
    // Feedback entries this session already recorded; capped so a confused model
    // cannot flood the log (docs/compiler/tools.md#feedback-tool).
    pub(super) feedback_count: usize,
    // What an external agent reported its run billed for this batch, (tokens, cached);
    // the commit takes it when the harness measured nothing
    // (docs/compiler/tools.md#spend-tool).
    pub(super) reported_spend: Option<(u64, u64)>,
    // What a review's word-match verdicts staged, said once on the mark's reply.
    pub(super) review_staged_note: Option<String>,
    // Goal lines already previewed this session: a repeat is counted, not repeated.
    pub(super) previewed: std::collections::BTreeSet<String>,
    pub(super) mutation_limit: usize,
    // Staged entities (id -> entity) so lookup-before-create sees this session's own creates.
    pub(super) staged_entities: std::collections::BTreeMap<String, Entity>,
    // The session's own staged findings, by their stage-minted ids: reads see
    // them, and a re-report answers with the id it updates.
    pub(super) staged_diags: std::collections::BTreeMap<String, Diagnostic>,
    pub(super) staged_reqs: BTreeSet<String>,
    // Staged views (id -> view) so a repeated upsert lands on the staged one and
    // update_view sees this session's own creates.
    pub(super) staged_views: std::collections::BTreeMap<String, View>,
    // Parents this session set on existing entities: the cycle gate reads them.
    pub(super) staged_parents: std::collections::BTreeMap<String, String>,
    pub(super) taken_ids: BTreeSet<String>,
    // The condensed status block as last rendered on a reply: an unchanged set
    // answers with one line instead of the same listing again.
    last_condensed: std::cell::RefCell<String>,
    // True only while finish_implicit drives `done`: the implicit path commits around
    // an unmarked dirty section instead of bouncing (docs/compiler/sessions.md#budgets).
    pub(super) implicit_done: bool,
    // The project's lint rules, numbered as the project block renders them: a review
    // names each one in its evidence (docs/compiler/goals/review-entity.md#gate).
    pub lint: crate::project::Linting,
    // The session's opening prompt carries the task execution contract and, for a
    // batch holding a generate goal, the generation contract: begin_binding and
    // begin_generation answer as they do to an owned worker
    // (docs/compiler/goals/generate.md#what-the-model-sees).
    #[serde(default)]
    pub opening_contracts: bool,
    // The whole-file hash each file tool path last returned to this session: the
    // version a literal edit without expectedHash applies to.
    // Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
    #[serde(default)]
    pub(super) file_hashes: std::collections::BTreeMap<String, String>,
}

// A begin_binding or begin_generation reply to a session whose opening prompt
// already carries the task execution contract: the binding contract alone, and the
// generation contract only when the opening did not carry it. One rule for an owned
// MCP worker and an in-process session. Mirrors docs/compiler/goals/bind.md#what-the-model-sees.
pub fn opened_package(name: &str, value: &mut Value, generation_in_prompt: bool) {
    match name {
        "begin_binding" => value["instructions"] = json!(crate::bind::contract()),
        "begin_generation" if generation_in_prompt => {
            if let Some(o) = value.as_object_mut() {
                o.remove("instructions");
            }
        }
        "begin_generation" => value["instructions"] = json!(crate::gen::contract()),
        _ => {}
    }
}

impl ToolSession {
    pub fn new(
        snapshot: Store,
        scope: WorkScope,
        mutation_limit: usize,
        default_budget: usize,
    ) -> ToolSession {
        // Placeholder; sessions that reach the gen tools (MCP, the runner) overwrite
        // it with the project-resolved settings.
        let gen = crate::gen::GenSettings::from_out(&snapshot.out);
        // The batch's goal kinds activate their skills from the first round.
        let mut skills = SkillState::new();
        for g in &scope.goals {
            for s in crate::goals::skills_for(&g.kind, &snapshot, &g.target) {
                skills.pin(s);
            }
        }
        // A caps split earns the moves it needs (docs/compiler/sessions.md#budgets).
        let mutation_limit = mutation_limit.max(scope.mutation_budget());
        ToolSession {
            loaded: LoadedSet::new(default_budget),
            skills,
            snapshot,
            scope,
            staged: Vec::new(),
            done: None,
            gen,
            caller: Default::default(),
            outcomes: Default::default(),
            repeats: Default::default(),
            last_answers: Default::default(),
            refusals: 0,
            feedback_count: 0,
            reported_spend: None,
            review_staged_note: None,
            previewed: std::collections::BTreeSet::new(),
            mutation_limit,
            staged_entities: Default::default(),
            staged_diags: Default::default(),
            staged_reqs: Default::default(),
            staged_views: Default::default(),
            staged_parents: Default::default(),
            taken_ids: Default::default(),
            last_condensed: Default::default(),
            implicit_done: false,
            lint: Default::default(),
            opening_contracts: false,
            file_hashes: Default::default(),
        }
    }

    // Continue the guard and the feedback cap of earlier calls in the same unit.
    pub fn adopt_memory(&mut self, memory: CallMemory) {
        self.repeats = memory.repeats;
        self.last_answers = memory.last_answers;
        self.feedback_count = memory.feedback_count;
        self.file_hashes = memory.file_hashes;
    }

    pub fn memory(&self) -> CallMemory {
        CallMemory {
            repeats: self.repeats.clone(),
            last_answers: self.last_answers.clone(),
            feedback_count: self.feedback_count,
            file_hashes: self.file_hashes.clone(),
        }
    }

    // The commit this session lands as: kind `session`, the batch's goal ids, and the
    // resolutions with their justifications. Mirrors docs/compiler/graph.md#journal.
    pub fn commit(&self, rounds: u32, tokens: Option<u64>) -> crate::store::Commit {
        let resolved = self
            .outcomes
            .iter()
            .filter_map(|(goal, o)| match o {
                GoalOutcome::Done {
                    justification,
                    evidence,
                } => Some(Resolved {
                    goal: goal.clone(),
                    justification: justification.clone(),
                    evidence: evidence.clone(),
                }),
                GoalOutcome::Failed { .. } => None,
            })
            .collect();
        // The harness's measurement wins; an external agent's own report fills in
        // when there is none (docs/compiler/tools.md#spend-tool).
        let (tokens, cached, tokens_source) = match (tokens, self.reported_spend) {
            (Some(t), _) => (Some(t), None, None),
            (None, Some((t, c))) => (
                Some(t),
                (c > 0).then_some(c),
                Some(crate::model::TOKENS_SOURCE_AGENT.to_string()),
            ),
            (None, None) => (None, None, None),
        };
        crate::store::Commit {
            kind: "session".to_string(),
            batch: self.scope.goal_ids(),
            resolved,
            rounds,
            tokens,
            cached,
            tokens_source,
            all_or_nothing: false,
        }
    }

    // The goals the session failed, with their reasons, for the serving to record.
    pub fn failed_goals(&self) -> Vec<(String, String)> {
        self.outcomes
            .iter()
            .filter_map(|(goal, o)| match o {
                GoalOutcome::Failed { reason } => Some((goal.clone(), reason.clone())),
                _ => None,
            })
            .collect()
    }

    fn pinned_targets(&self) -> BTreeSet<String> {
        let mut p: BTreeSet<String> = self.scope.goals.iter().map(|g| g.target.clone()).collect();
        for g in &self.scope.goals {
            p.extend(g.stale_anchors.iter().cloned());
            if let Some(d) = &g.doc {
                for s in &g.sections {
                    p.insert(format!("{}#{}", d, s));
                }
            }
        }
        p
    }

    pub fn render_status_full(&self) -> String {
        self.with_staged_notes(self.loaded.render_status(
            &self.skills.index_line(),
            self.skills.rendered_chars(),
            &self.pinned_targets(),
        ))
    }

    // The staged note per loaded item: the listing renders the committed snapshot,
    // and a line a staged mutation touches says so, so the model never mistakes
    // its own pending work for a lost write (a Codex session read the old statement
    // under updated: true as a lost edit). Mirrors docs/compiler/context.md#the-loaded-set.
    fn staged_notes(&self) -> BTreeMap<String, &'static str> {
        let mut notes: BTreeMap<String, &'static str> = BTreeMap::new();
        for op in &self.staged {
            let (id, note) = match op {
                Op::UpdateRequirement { id, statement, .. } => (
                    id.clone(),
                    if statement.is_some() {
                        "statement revised"
                    } else {
                        "revised"
                    },
                ),
                Op::UpdateEntity { id, .. }
                | Op::UpdateCriterion { id, .. }
                | Op::UpdateView { id, .. } => (id.clone(), "revised"),
                Op::DeleteEntity { id, .. }
                | Op::DeleteRequirement { id, .. }
                | Op::DeleteCriterion { id, .. }
                | Op::DeleteView { id, .. } => (id.clone(), "delete"),
                _ => continue,
            };
            notes.insert(id, note);
        }
        notes
    }

    fn with_staged_notes(&self, listing: String) -> String {
        let notes = self.staged_notes();
        if notes.is_empty() {
            return listing;
        }
        listing
            .lines()
            .map(|line| {
                if let Some(rest) = line.strip_prefix("- ") {
                    let id = rest.split_whitespace().next().unwrap_or("");
                    if let Some(n) = notes.get(id) {
                        return format!("{}   (staged: {})", line.trim_end(), n);
                    }
                }
                line.to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }

    pub(super) fn condensed_status(&self) -> String {
        // The listing only when it changed; twenty-six updates echoing the same two
        // thousand characters was repetition. Mirrors docs/compiler/context.md#the-loaded-set.
        let full =
            self.with_staged_notes(self.loaded.render_condensed(self.skills.rendered_chars()));
        let mut last = self.last_condensed.borrow_mut();
        if *last == full {
            return format!(
                "## Loaded: unchanged ({} items, {} of {} chars; skills {} besides, uncounted; graph_status renders it in full)",
                self.loaded.items.len(),
                crate::context::fmt_k(self.loaded.used()),
                crate::context::fmt_k(self.loaded.budget),
                crate::context::fmt_k(self.skills.rendered_chars())
            );
        }
        *last = full.clone();
        full
    }

    // Past the high-water mark, load and expand are refused with the unload
    // candidates named, until something is unloaded. Mirrors docs/compiler/context.md#policy.
    pub(super) fn refuse_past_high_water(
        &self,
        what: &str,
        target: Option<&str>,
    ) -> Result<(), ToolError> {
        // Skills show in the total but never trigger the refusal: measured over the
        // items alone, and a target already held as a stub is reloaded in place, so
        // its own chars never count against it. Mirrors docs/compiler/context.md#the-loaded-set.
        let held = target
            .and_then(|t| self.loaded.items.iter().find(|i| i.target == t))
            .map(|i| i.chars)
            .unwrap_or(0);
        if self.loaded.used().saturating_sub(held) <= self.loaded.high_water {
            return Ok(());
        }
        let pinned = self.pinned_targets();
        let mut candidates = self.loaded.unload_candidates(&pinned);
        if candidates.is_empty() {
            candidates = self
                .loaded
                .items
                .iter()
                .map(|i| i.target.clone())
                .filter(|t| !pinned.contains(t))
                .collect();
        }
        // The thing being loaded is never the thing to unload.
        candidates.retain(|c| Some(c.as_str()) != target);
        Err(ToolError::new(
            "context-full",
            format!(
                "{} refused: unload one of: {}, then retry. The loaded set is at {} chars, past the high-water mark {} (budget {}). If this refusal seems wrong, say so with report_feedback.",
                what,
                if candidates.is_empty() {
                    "(any loaded item)".to_string()
                } else {
                    candidates.join(", ")
                },
                self.loaded.used() + self.skills.rendered_chars(),
                self.loaded.high_water,
                self.loaded.budget,
            ),
        ))
    }

    // ---- the per-goal gates ----

    // Implicit done: commit valid staged work when the model forgot the finish
    // contract or ran out of rounds. One dishonest `covered` claim must not sink the
    // rest of the turn's work: drop the offending coverage marks (those sections stay
    // unprocessed; the next build resumes them) and try once more. Mirrors
    // docs/compiler/sessions.md#budgets.
    pub fn finish_implicit(&mut self, summary: &str) -> bool {
        if self.staged.is_empty() {
            return false;
        }
        self.implicit_done = true;
        if self.dispatch("done", &json!({"summary": summary})).is_ok() {
            return true;
        }
        let staged_req_sources: Vec<(String, String)> = self
            .staged
            .iter()
            .filter_map(|o| match o {
                Op::CreateRequirement { requirement, .. } => requirement
                    .source
                    .as_ref()
                    .map(|s| (s.doc.clone(), s.section.clone())),
                _ => None,
            })
            .collect();
        let snap = &self.snapshot;
        self.staged.retain(|op| match op {
            Op::SetCoverage {
                doc,
                section,
                state,
                ..
            } if state == "covered" => {
                snap.graph
                    .requirements
                    .values()
                    .any(|r| r.anchored_at(doc, section))
                    || staged_req_sources
                        .iter()
                        .any(|(d, s)| d == doc && s == section)
            }
            _ => true,
        });
        !self.staged.is_empty() && self.dispatch("done", &json!({"summary": summary})).is_ok()
    }

    pub(super) fn stage(&mut self, op: Op) -> Result<(), ToolError> {
        if self.staged.len() >= self.mutation_limit {
            return Err(ToolError::new(
                "mutation-budget",
                format!(
                    include_str!(concat!(
                        env!("OUT_DIR"),
                        "/payloads/goals/prompts/inline/tools-session/the-changeset-s-mutation-budget-is-exhausted-5319c.md"
                    )),
                    self.mutation_limit
                ),
            ));
        }
        self.staged.push(op);
        Ok(())
    }
}
