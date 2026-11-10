use super::*;

// The scope of one goal in the batch: what the per-goal gates key on.
// Mirrors docs/compiler/sessions.md#anatomy.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct GoalScope {
    pub goal: String,
    pub kind: String,
    pub mandatory: bool,
    pub target: String,
    pub doc: Option<String>,
    // The sections a reconcile-section goal owns (the coverage contract).
    pub sections: Vec<String>,
    // Requirement ids whose quote stopped locating; the gate holds the session to
    // addressing each one. See docs/compiler/graph.md#validation-gates.
    pub stale_anchors: Vec<String>,
    // Anchor ids a place-anchors goal must decide; place_anchor and orphan_anchor
    // accept no others, and the gate holds the session to every one.
    pub proposals: Vec<String>,
    // The requirement count an abstract-entity goal splits (the caps variant), zero
    // otherwise; the mutation budget grows by it, one move per grouped statement.
    // Mirrors docs/compiler/sessions.md#budgets.
    pub split_count: usize,
    // The goal's change payload: the evidence gates read what it lists (a
    // curate-view goal's matched nodes, a derive-criteria goal's requirements).
    pub change: Value,
}

impl GoalScope {
    pub fn from_goal(g: &Goal) -> GoalScope {
        let list = |key: &str| -> Vec<String> {
            g.change[key]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut sections = list("dirtySections");
        let mut doc = None;
        match g.kind.as_str() {
            "reconcile-section" => match split_section_ref(&g.target) {
                Some((d, s)) => {
                    doc = Some(d);
                    if !sections.contains(&s) {
                        sections.push(s);
                    }
                }
                // The legacy per-document item: the target is the document and the
                // sections ride in the change payload.
                None => doc = Some(g.target.clone()),
            },
            "place-anchors" => doc = Some(g.target.clone()),
            _ => {}
        }
        let mut proposals = list("proposals");
        for a in list("anchors") {
            if !proposals.contains(&a) {
                proposals.push(a);
            }
        }
        let split_count = if g.kind == "abstract-entity" {
            g.change["limits"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|l| l["limit"] == "requirements-per-entity")
                .filter_map(|l| l["count"].as_u64())
                .max()
                .unwrap_or(0) as usize
        } else if g.kind == "derive-criteria" {
            // Three criteria per listed requirement, the density the skill asks for
            // (docs/compiler/sessions.md#budgets).
            3 * (g.change["none"].as_array().map(|a| a.len()).unwrap_or(0)
                + g.change["revised"].as_array().map(|a| a.len()).unwrap_or(0))
        } else {
            0
        };
        GoalScope {
            goal: g.id.clone(),
            kind: g.kind.clone(),
            mandatory: g.mandatory,
            target: g.target.clone(),
            doc,
            sections,
            stale_anchors: list("staleAnchors"),
            proposals,
            split_count,
            change: g.change.clone(),
        }
    }
}

// The scope a session works in: one goal batch, or a serving surface outside one.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WorkScope {
    // The batch id: names the session and the feedback entries.
    pub batch: String,
    pub goals: Vec<GoalScope>,
    // The serving surface for scopes outside a goal batch (mcp-write, mcp-read).
    pub serving: String,
    // The serving's own target (the entity a generation session owns, for file
    // ownership); a generate goal in the batch overrides it.
    pub target: String,
}

impl WorkScope {
    pub fn for_batch(batch: &str, goals: &[Goal]) -> WorkScope {
        WorkScope {
            batch: batch.to_string(),
            goals: goals.iter().map(GoalScope::from_goal).collect(),
            serving: String::new(),
            target: goals.first().map(|g| g.target.clone()).unwrap_or_default(),
        }
    }

    // The per-target work item the current serving claims, as a one-goal batch.
    pub fn from_item(item: &WorkItem) -> WorkScope {
        let goal = item.to_goal(GoalState::Open);
        let mut s = WorkScope::for_batch(&item.goal_id(), std::slice::from_ref(&goal));
        s.target = item.target.clone();
        s
    }

    // The staged-mutation budget the batch earns over the registry constant: a caps
    // split stages one move per grouped statement, so a session holding one grows by
    // the count it splits. Mirrors docs/compiler/sessions.md#budgets.
    pub fn mutation_budget(&self) -> usize {
        crate::limits::SESSION_MUTATIONS + self.goals.iter().map(|g| g.split_count).sum::<usize>()
    }

    // A serving surface with no open batch (mcp-write, mcp-read, chat).
    pub fn serving(name: &str) -> WorkScope {
        WorkScope {
            batch: String::new(),
            goals: Vec::new(),
            serving: name.to_string(),
            target: String::new(),
        }
    }

    pub fn goal_ids(&self) -> Vec<String> {
        self.goals.iter().map(|g| g.goal.clone()).collect()
    }

    pub fn kinds(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for g in &self.goals {
            if !v.contains(&g.kind) {
                v.push(g.kind.clone());
            }
        }
        v
    }

    pub fn goal(&self, id: &str) -> Option<&GoalScope> {
        self.goals.iter().find(|g| g.goal == id)
    }

    // The batch's document (locality is one document per batch).
    pub fn doc(&self) -> Option<String> {
        self.goals.iter().find_map(|g| g.doc.clone())
    }

    // The document whose sections the reconcile-section goals own.
    pub fn reconcile_doc(&self) -> Option<String> {
        self.goals
            .iter()
            .find(|g| g.kind == "reconcile-section")
            .and_then(|g| g.doc.clone())
    }

    pub fn reconcile_scopes(&self) -> Vec<&GoalScope> {
        self.goals
            .iter()
            .filter(|g| g.kind == "reconcile-section")
            .collect()
    }

    // The document a place-anchors goal decides.
    pub fn place_doc(&self) -> Option<String> {
        self.goals
            .iter()
            .find(|g| g.kind == "place-anchors")
            .and_then(|g| g.doc.clone())
    }

    pub fn stale_anchors(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for g in &self.goals {
            for a in &g.stale_anchors {
                if !v.contains(a) {
                    v.push(a.clone());
                }
            }
        }
        v
    }

    pub fn proposals(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for g in &self.goals {
            for p in &g.proposals {
                if !v.contains(p) {
                    v.push(p.clone());
                }
            }
        }
        v
    }

    // The entity a generation session owns; file ownership checks name it.
    pub fn gen_target(&self) -> String {
        self.goals
            .iter()
            .find(|g| g.kind == "generate")
            .map(|g| g.target.clone())
            .unwrap_or_else(|| self.target.clone())
    }

    // The feedback entry's task: the batch's goal kinds, or the serving surface.
    pub fn feedback_task(&self) -> String {
        if self.goals.is_empty() {
            self.serving.clone()
        } else {
            self.kinds().join("+")
        }
    }
}

// How one goal of the batch ended, as the session claimed it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum GoalOutcome {
    Done {
        justification: String,
        evidence: Value,
    },
    Failed {
        reason: String,
    },
}

// One session's tool serving: reads answer from the snapshot, writes stage into the changeset.
// The build's own bookkeeping rules, settled by the harness and never a session's to
// resolve. Mirrors docs/compiler/model/diagnostic.md#lifecycle-and-triage.
// Mirrors docs/compiler/tools.md#read-tools: `criterion-missing` clears when a
// derive-criteria goal lands a criterion, and a review can do neither.
pub fn harness_owned_rule(rule: &str) -> bool {
    rule == "incomplete-build"
        || rule == "ratification-pending"
        || rule == "criterion-missing"
        || rule.starts_with("unstable-")
}
