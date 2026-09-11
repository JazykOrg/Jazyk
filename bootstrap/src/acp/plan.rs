// Build progress as an ACP plan: one entry per goal batch, keyed by the batch id,
// titled by the batch's locality and goal kinds, flipped pending → in_progress →
// completed as the build advances. Every commit re-derives the board, and the plan
// is republished whole with the projection re-formed, per the protocol's replace
// semantics. Blocked goals ride as their own pending entries carrying the reason.
// Mirrors docs/frontends/acp.md#plans.
use crate::board::{Batch, Board};
use crate::model::{Goal, GoalState};
use crate::project::Project;
use crate::session::TraceEvent;
use agent_client_protocol::schema::v1::{Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus};
use std::path::{Path, PathBuf};

// The verb a plan entry leads with, per goal kind.
fn verb(kind: &str) -> &'static str {
    match kind {
        "place-anchors" => "place anchors",
        "reconcile-section" => "reconcile",
        "rejudge-pair" => "rejudge",
        "review-entity" => "review",
        "conform-instance" => "conform",
        "declare-edges" => "declare edges",
        "dedupe-candidates" => "dedupe",
        "curate-view" => "curate",
        "split-view" => "split",
        "abstract-entity" => "abstract",
        "retrace" => "retrace",
        "bind" => "bind",
        "generate" => "generate",
        "verify" => "verify",
        "ratify" => "ratify",
        "answer" => "answer",
        _ => "resolve",
    }
}

// One goal as an entry title: the verb and the target, pairs spaced for reading.
pub fn goal_title(kind: &str, target: &str) -> String {
    let target = match kind {
        "rejudge-pair" | "dedupe-candidates" => target.replacen('~', " ~ ", 1),
        _ => target.to_string(),
    };
    format!("{} {}", verb(kind), target)
}

// The locality key without its kind word: `doc docs/cli.md` → `docs/cli.md`.
fn locality_label(locality: &str) -> &str {
    locality.split_once(' ').map(|(_, r)| r).unwrap_or(locality)
}

fn plural(unit: &str) -> String {
    match unit.strip_suffix('y') {
        Some(stem) => format!("{}ies", stem),
        None => format!("{}s", unit),
    }
}

// A batch's title: one goal titles itself; several of one kind name the locality
// and count their units; a mixed batch names its verbs and the locality.
// Mirrors docs/frontends/acp.md#plans.
pub fn batch_title(goals: &[&Goal], locality: &str) -> String {
    match goals {
        [] => locality_label(locality).to_string(),
        [g] => goal_title(&g.kind, &g.target),
        _ => {
            let mut verbs: Vec<&str> = Vec::new();
            for g in goals {
                let v = verb(&g.kind);
                if !verbs.contains(&v) {
                    verbs.push(v);
                }
            }
            let label = locality_label(locality);
            if verbs.len() == 1 {
                let unit = goals[0].unit.as_str();
                let unit = if unit.is_empty() { "goal" } else { unit };
                format!("{} {} ({} {})", verbs[0], label, goals.len(), plural(unit))
            } else {
                format!("{} {} ({} goals)", verbs.join(" + "), label, goals.len())
            }
        }
    }
}

fn entry(content: &str, status: PlanEntryStatus) -> PlanEntry {
    PlanEntry::new(content, PlanEntryPriority::Medium, status)
}

fn title_of(board: &Board, b: &Batch) -> String {
    let goals: Vec<&Goal> = b.goals.iter().filter_map(|id| board.goal(id)).collect();
    batch_title(&goals, &b.locality)
}

// The projection as pending entries: the batches the scheduler would form, then the
// blocked goals, each its own pending entry carrying the reason, so a plan that ends
// with blocked entries is the same statement the verdict makes.
// Mirrors docs/frontends/acp.md#plans.
pub fn pending_entries(board: &Board, skip_batch: Option<&str>) -> Vec<PlanEntry> {
    let mut entries: Vec<PlanEntry> = Vec::new();
    for b in &board.batches {
        if Some(b.id.as_str()) == skip_batch {
            continue;
        }
        entries.push(entry(&title_of(board, b), PlanEntryStatus::Pending));
    }
    for g in &board.goals {
        let GoalState::Blocked { ref on } = g.state else {
            continue;
        };
        let reason = board
            .readiness
            .get(&g.id)
            .and_then(|r| r.reason())
            .unwrap_or(on);
        entries.push(entry(
            &format!("{} (blocked: {})", goal_title(&g.kind, &g.target), reason),
            PlanEntryStatus::Pending,
        ));
    }
    entries
}

// The pending-work plan a proxy pushes outside its own builds: the projection with
// nothing in progress. Empty entries clear a stale checklist, per the replace
// semantics. Mirrors docs/frontends/acp.md#lsp-and-the-proxy.
pub fn pending_plan(proj: &Project, out: &Path) -> Plan {
    let board = Board::compute(proj, out);
    Plan::new(pending_entries(&board, None))
}

pub fn fingerprint(plan: &Plan) -> String {
    serde_json::to_string(plan).unwrap_or_default()
}

// Follows one build's trace and keeps the whole plan current: batches finished stay
// completed, the running batch is in progress, and the tail is the board's own
// projection, recomputed from the store the build commits to.
// Mirrors docs/frontends/acp.md#plans.
pub struct PlanTracker {
    proj: Project,
    out: PathBuf,
    done: Vec<(String, String)>,
    current: Option<(String, String)>,
}

impl PlanTracker {
    pub fn new(proj: &Project, out: &Path) -> PlanTracker {
        PlanTracker {
            proj: proj.clone(),
            out: out.to_path_buf(),
            done: Vec::new(),
            current: None,
        }
    }

    fn current_is(&self, label: &str) -> bool {
        self.current.as_ref().is_some_and(|(id, _)| id == label)
    }

    // A ledger goal runs as its own single-goal batch labeled by the goal id; its
    // end rides the ledger events, not sessionDone.
    fn current_is_ledger(&self) -> bool {
        self.current.as_ref().is_some_and(|(id, _)| {
            id.starts_with("g:bind:")
                || id.starts_with("g:generate:")
                || id.starts_with("g:verify:")
        })
    }

    fn complete_current(&mut self) {
        if let Some(cur) = self.current.take() {
            self.done.push(cur);
        }
    }

    // Feed one trace event; when it moves the plan, the whole plan to publish.
    pub fn on_event(&mut self, ev: &TraceEvent) -> Option<Plan> {
        match ev {
            TraceEvent::Board { .. } => {}
            TraceEvent::BatchStart { label, .. } => {
                let board = Board::compute(&self.proj, &self.out);
                let title = board
                    .batches
                    .iter()
                    .find(|b| b.id == *label)
                    .map(|b| title_of(&board, b))
                    .unwrap_or_else(|| label.clone());
                self.current = Some((label.clone(), title));
                return Some(self.publish_with(&board));
            }
            TraceEvent::SessionStart {
                label,
                task,
                target,
                ..
            } => {
                // Board batches announced themselves at batchStart; a ledger run has
                // no batchStart, so its session opens the entry. A leftover entry
                // completes when the board no longer holds its goal open.
                if self.current_is(label) {
                    return None;
                }
                let board = Board::compute(&self.proj, &self.out);
                if let Some((id, title)) = self.current.take() {
                    if !board.open(&id) {
                        self.done.push((id, title));
                    }
                }
                self.current = Some((label.clone(), goal_title(task, target)));
                return Some(self.publish_with(&board));
            }
            TraceEvent::SessionDone { label, .. } => {
                if self.current_is(label) {
                    self.complete_current();
                }
            }
            TraceEvent::SessionFailed { label, .. } => {
                if self.current_is(label) {
                    // The goals stand open again; the re-formed projection carries
                    // them as pending.
                    self.current = None;
                }
            }
            TraceEvent::GenEntityDone { .. } | TraceEvent::VerifyRowDone { .. } => {
                if self.current_is_ledger() {
                    self.complete_current();
                } else {
                    return None;
                }
            }
            TraceEvent::GenEntityFailed { .. } => {
                if self.current_is_ledger() {
                    self.current = None;
                } else {
                    return None;
                }
            }
            _ => return None,
        }
        Some(self.publish())
    }

    fn publish(&self) -> Plan {
        let board = Board::compute(&self.proj, &self.out);
        self.publish_with(&board)
    }

    fn publish_with(&self, board: &Board) -> Plan {
        let mut entries: Vec<PlanEntry> = Vec::new();
        for (_, title) in &self.done {
            entries.push(entry(title, PlanEntryStatus::Completed));
        }
        if let Some((_, title)) = &self.current {
            entries.push(entry(title, PlanEntryStatus::InProgress));
        }
        entries.extend(pending_entries(
            board,
            self.current.as_ref().map(|(id, _)| id.as_str()),
        ));
        Plan::new(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goal(kind: &str, target: &str, unit: &str) -> Goal {
        Goal {
            id: format!("g:{}:{}", kind, target),
            kind: kind.to_string(),
            class: "compile".to_string(),
            mandatory: true,
            target: target.to_string(),
            unit: unit.to_string(),
            change: serde_json::Value::Null,
            cause: None,
            state: GoalState::Open,
            hints: Vec::new(),
        }
    }

    #[test]
    fn batch_titles_name_the_locality_and_the_goal_kinds() {
        let a = goal("reconcile-section", "docs/cli.md#/commands", "section");
        let b = goal("reconcile-section", "docs/cli.md#/output", "section");
        let c = goal("reconcile-section", "docs/cli.md#/exit-codes", "section");
        assert_eq!(
            batch_title(&[&a, &b, &c], "doc docs/cli.md"),
            "reconcile docs/cli.md (3 sections)"
        );
        assert_eq!(
            batch_title(&[&a], "doc docs/cli.md"),
            "reconcile docs/cli.md#/commands"
        );
        let p = goal("rejudge-pair", "req:order-3~req:cart-2", "pair");
        assert_eq!(
            batch_title(&[&p], "req req:order-3"),
            "rejudge req:order-3 ~ req:cart-2"
        );
        let e = goal("abstract-entity", "ent:order", "entity");
        assert_eq!(batch_title(&[&e], "ent ent:order"), "abstract ent:order");
        let r = goal("review-entity", "ent:order", "entity");
        assert_eq!(
            batch_title(&[&p, &r], "ent ent:order"),
            "rejudge + review ent:order (2 goals)"
        );
        assert_eq!(
            batch_title(
                &[&r, &goal("review-entity", "ent:cart", "entity")],
                "ent ent:order"
            ),
            "review ent:order (2 entities)"
        );
    }

    #[test]
    fn ledger_goals_title_as_their_verb_and_target() {
        assert_eq!(goal_title("generate", "ent:order"), "generate ent:order");
        assert_eq!(goal_title("verify", "req:order-3"), "verify req:order-3");
    }
}
