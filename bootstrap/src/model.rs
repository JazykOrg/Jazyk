// The semantic graph node types, change records, journal entries, and status.
// Mirrors docs/compiler/model.md, docs/compiler/graph.md, and graph.schema.yaml.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

fn default_scope() -> String {
    "public".to_string()
}
fn default_lifecycle() -> String {
    "open".to_string()
}
fn is_default_scope(s: &String) -> bool {
    s == "public"
}
fn is_open(s: &String) -> bool {
    s == "open"
}
fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero(n: &u64) -> bool {
    *n == 0
}

// A located quote: the verbatim text is found by string search inside the section's raw body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceRef {
    pub doc: String,
    pub section: String,
    pub quote: String,
}

// Where a fact comes from. Exactly one per fact. On disk the kind is the key of a
// one-entry map: `{quote: {doc, section, quote}}`, `{derived: {from, reasoning}}`,
// `{decree: {author, at, note}}`. Mirrors docs/compiler/model.md#provenance.
#[derive(Clone, Debug, PartialEq)]
pub enum Provenance {
    Quote(SourceRef),
    Derived {
        from: Vec<String>,
        reasoning: String,
    },
    Decree {
        author: String,
        at: String,
        note: Option<String>,
    },
}

// The on-disk shape of a provenance: one key naming the kind. The same shape in YAML
// and JSON, which a tagged enum would not give.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvenanceMap {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quote: Option<SourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    derived: Option<DerivedMap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    decree: Option<DecreeMap>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DerivedMap {
    from: Vec<String>,
    reasoning: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecreeMap {
    author: String,
    at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

impl Serialize for Provenance {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let map = match self {
            Provenance::Quote(s) => ProvenanceMap {
                quote: Some(s.clone()),
                derived: None,
                decree: None,
            },
            Provenance::Derived { from, reasoning } => ProvenanceMap {
                quote: None,
                derived: Some(DerivedMap {
                    from: from.clone(),
                    reasoning: reasoning.clone(),
                }),
                decree: None,
            },
            Provenance::Decree { author, at, note } => ProvenanceMap {
                quote: None,
                derived: None,
                decree: Some(DecreeMap {
                    author: author.clone(),
                    at: at.clone(),
                    note: note.clone(),
                }),
            },
        };
        map.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Provenance {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let map = ProvenanceMap::deserialize(deserializer)?;
        match (map.quote, map.derived, map.decree) {
            (Some(s), None, None) => Ok(Provenance::Quote(s)),
            (None, Some(d), None) => Ok(Provenance::Derived {
                from: d.from,
                reasoning: d.reasoning,
            }),
            (None, None, Some(d)) => Ok(Provenance::Decree {
                author: d.author,
                at: d.at,
                note: d.note,
            }),
            _ => Err(serde::de::Error::custom(
                "provenance is exactly one of quote, derived, decree",
            )),
        }
    }
}

// A borrowed view of a requirement's provenance, whichever field carries it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProvenanceRef<'a> {
    Quote(&'a SourceRef),
    Derived {
        from: &'a [String],
        reasoning: &'a str,
    },
    Decree {
        author: &'a str,
        at: &'a str,
        note: Option<&'a str>,
    },
}

impl Provenance {
    pub fn as_ref(&self) -> ProvenanceRef<'_> {
        match self {
            Provenance::Quote(s) => ProvenanceRef::Quote(s),
            Provenance::Derived { from, reasoning } => ProvenanceRef::Derived { from, reasoning },
            Provenance::Decree { author, at, note } => ProvenanceRef::Decree {
                author,
                at,
                note: note.as_deref(),
            },
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Provenance::Quote(_) => "quote",
            Provenance::Derived { .. } => "derived",
            Provenance::Decree { .. } => "decree",
        }
    }
}

// One named attribute of an entity: a type where prose states structure, a value where
// the entity is an instance. Mirrors docs/compiler/model/entity.md#fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attribute {
    pub name: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub provenance: Provenance,
}

// A per-node bump above a built-in limit. On disk it is the bare number under
// `limits: {<limit>: n}`; the decree behind it lives in the journal.
// Mirrors docs/compiler/graph.md#per-node-bumps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LimitBump {
    pub value: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entity {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
    #[serde(default = "default_scope", skip_serializing_if = "is_default_scope")]
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stereotype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<Attribute>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<SourceRef>,
    // Present on an entity no document states (derived or decreed structure).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub limits: BTreeMap<String, LimitBump>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
}

impl Default for Entity {
    fn default() -> Self {
        Entity {
            name: String::new(),
            aliases: Vec::new(),
            definition: None,
            scope: default_scope(),
            stereotype: None,
            parent: None,
            attributes: Vec::new(),
            mentions: Vec::new(),
            provenance: None,
            limits: BTreeMap::new(),
            confidence: None,
            reasoning: None,
            created: None,
            updated: None,
        }
    }
}

// One relationship a requirement causes. Directional: `a` acts on `b`.
// Mirrors docs/compiler/model/requirement.md#edges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReqEdge {
    pub a: String,
    pub b: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub rel_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cardinality: Option<String>,
}

// The cardinality forms an edge may carry.
pub const CARDINALITIES: [&str; 4] = ["1", "0..1", "1..*", "*"];

// The state change a requirement describes. Mirrors docs/compiler/model/requirement.md#transition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub subject: String,
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<String>,
}

// One judged facet of a requirement. Mirrors docs/compiler/model/requirement.md#facets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Facet {
    pub facet: String,
    pub reasoning: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure: Option<String>,
}

pub const FACETS: [&str; 4] = ["behavior", "constraint", "failure-mode", "quality"];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Requirement {
    pub statement: String,
    pub entities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<ReqEdge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facets: Vec<Facet>,
    // The quote form of provenance. A requirement has exactly one of `source` or
    // `provenance`. Mirrors docs/compiler/model/requirement.md#fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
}

impl Requirement {
    // The one provenance, whichever field carries it. None only on a requirement that
    // never got one (a fixture built from `Default`).
    pub fn provenance(&self) -> Option<ProvenanceRef<'_>> {
        match (&self.source, &self.provenance) {
            (Some(s), _) => Some(ProvenanceRef::Quote(s)),
            (None, Some(p)) => Some(p.as_ref()),
            (None, None) => None,
        }
    }

    pub fn is_quoted(&self) -> bool {
        self.source.is_some()
    }

    // Whether the quote source sits in the named section.
    pub fn anchored_at(&self, doc: &str, section: &str) -> bool {
        self.source
            .as_ref()
            .is_some_and(|s| s.doc == doc && s.section == section)
    }
}

// One direction-and-type group of a relationship, with the requirements behind it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contribution {
    pub a: String,
    pub b: String,
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cardinality: Option<String>,
    pub requirements: Vec<String>,
}

// Derived: recomputed on every commit from requirement edges. Never written directly.
// One node per unordered entity pair. Mirrors docs/compiler/model/relationship.md.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub members: Vec<String>,
    #[serde(default)]
    pub contributions: Vec<Contribution>,
}

impl Relationship {
    // The summary type across groups: the strongest ranked type, instantiation only
    // when it is the sole group. What a reader wanting one arrow shows.
    pub fn strongest(&self) -> &str {
        let ranked = self
            .contributions
            .iter()
            .filter(|c| c.r#type != INSTANTIATION)
            .min_by_key(|c| rel_rank(&c.r#type));
        match ranked {
            Some(c) => &c.r#type,
            None if self.contributions.is_empty() => "dependency",
            None => INSTANTIATION,
        }
    }

    // The union of contributing requirements across groups.
    pub fn requirements(&self) -> BTreeSet<&str> {
        self.contributions
            .iter()
            .flat_map(|c| c.requirements.iter().map(String::as_str))
            .collect()
    }
}

// One transition of a derived state machine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StateTransition {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<String>,
    pub requirement: String,
}

// Derived: one per entity any transition names as subject, recomputed on every commit.
// Mirrors docs/compiler/model/state-machine.md.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StateMachine {
    pub subject: String,
    #[serde(default)]
    pub states: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial: Option<String>,
    #[serde(default)]
    pub transitions: Vec<StateTransition>,
}

// Membership by rule instead of list. Mirrors docs/compiler/model/view.md#fields.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stereotype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
}

// A node kept out of a view on purpose.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Exclusion {
    pub id: String,
    pub note: String,
}

// The stored half of a diagram: what it includes, never how it looks.
// Mirrors docs/compiler/model/view.md.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct View {
    pub kind: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<Exclusion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<ViewQuery>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collapse: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    // True on a view the recompute owns. Any mutation on the view clears it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub default: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub limits: BTreeMap<String, LimitBump>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
}

// The renderable view kinds, in catalog order. Mirrors docs/compiler/model/view.md#kinds.
pub const VIEW_KINDS: [&str; 13] = [
    "class",
    "object",
    "package",
    "component",
    "composite",
    "deployment",
    "use-case",
    "activity",
    "state",
    "sequence",
    "communication",
    "timing",
    "overview",
];

// The kind segment of a view id: the catalog kind with its hyphens removed.
pub fn view_kind_slug(kind: &str) -> String {
    kind.replace('-', "")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub rule: String,
    pub severity: String,
    pub subjects: Vec<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default = "default_lifecycle", skip_serializing_if = "is_open")]
    pub lifecycle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triage: Option<String>,
    // A question attached to the finding, with suggested resolutions. Optional;
    // most diagnostics carry none. Mirrors docs/compiler/model/diagnostic.md#prompts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<DiagnosticPrompt>,
    // The human response. Like triage: set by frontends, never by the compiler,
    // and it survives rebuilds. Mirrors docs/compiler/model/diagnostic.md#answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<DiagnosticAnswer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticPrompt {
    pub question: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<PromptOption>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub freeform: bool,
}

// One choice: a label plus exactly one of `edit` (deterministic to apply) or
// `answer` (a prefilled reply the model handles).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PromptOption {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit: Option<SuggestedEdit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

// The same shape the dual-write tools use (Op::EditDocProse), so applying one is
// the existing absorb path.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SuggestedEdit {
    pub doc: String,
    pub section: String,
    pub old_text: String,
    pub new_text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DiagnosticAnswer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<usize>,
    pub text: String,
    // applied | handling | handled | failed
    pub status: String,
}

// One section of a parsed document. `raw` is verbatim; `hash` is the content hash of raw.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Section {
    pub title: String,
    pub kind: String,
    pub order: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub raw: String,
    pub hash: String,
    pub lines: [usize; 2],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Coverage {
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(rename = "claimedBy", default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
}

// One file under docs/ in the out dir, mirroring one source document.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DocRecord {
    #[serde(rename = "contentHash")]
    pub content_hash: String,
    #[serde(default)]
    pub sections: BTreeMap<String, Section>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub coverage: BTreeMap<String, Coverage>,
}

// One scheduled unit of work in the wave loop: a task type and its target. Converted to
// a goal at the seams (the journal, the parked list) until the goal board replaces the
// loop. Mirrors docs/compiler/reconciler.md#goal-derivation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkItem {
    pub task: String,
    pub target: String,
    #[serde(
        rename = "dirtySections",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub dirty_sections: Vec<String>,
    #[serde(
        rename = "staleAnchors",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub stale_anchors: Vec<String>,
    // Anchor ids a place-anchors item must decide.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposals: Vec<String>,
}

// Task names of the wave loop and the goal kinds that replace them, side by side.
const TASK_GOAL_KINDS: [(&str, &str, &str); 8] = [
    ("reconcile-doc", "reconcile-section", "section"),
    ("review-requirement", "rejudge-pair", "pair"),
    ("review-entity", "review-entity", "entity"),
    ("align-doc", "place-anchors", "document"),
    ("bind-requirement", "bind", "requirement"),
    ("generate-entity", "generate", "entity"),
    ("verify-requirement", "verify", "ledger row"),
    ("answer-diagnostic", "answer", "diagnostic"),
];

impl WorkItem {
    pub fn new(task: &str, target: &str) -> WorkItem {
        WorkItem {
            task: task.to_string(),
            target: target.to_string(),
            dirty_sections: Vec::new(),
            stale_anchors: Vec::new(),
            proposals: Vec::new(),
        }
    }

    // The goal kind that replaces this task. Store-level tasks keep their name.
    pub fn goal_kind(&self) -> &str {
        TASK_GOAL_KINDS
            .iter()
            .find(|(task, _, _)| *task == self.task)
            .map(|(_, kind, _)| *kind)
            .unwrap_or(self.task.as_str())
    }

    pub fn goal_id(&self) -> String {
        format!("g:{}:{}", self.goal_kind(), self.target)
    }

    // The journal entry kind this item commits as: `session` for model-executed work,
    // the store-level name otherwise. Mirrors docs/compiler/graph.md#journal.
    pub fn journal_kind(&self) -> &'static str {
        match self.task.as_str() {
            "align" => "align",
            "gc" => "gc",
            "settle-diagnostics" => "settle-diagnostics",
            "answer-diagnostic" => "answer",
            "chat" => "dual-write",
            "triage" => "triage",
            "decree" => "decree",
            "edit" => "edit",
            _ => "session",
        }
    }

    // The commit this item lands as: a session entry naming its goal, or the
    // store-level kind. Mirrors docs/compiler/graph.md#journal.
    pub fn commit(&self, rounds: u32, tokens: u64) -> crate::store::Commit {
        let kind = self.journal_kind();
        crate::store::Commit {
            kind: kind.to_string(),
            batch: if kind == "session" {
                vec![self.goal_id()]
            } else {
                Vec::new()
            },
            resolved: Vec::new(),
            rounds,
            tokens,
        }
    }

    pub fn to_goal(&self, state: GoalState) -> Goal {
        let unit = TASK_GOAL_KINDS
            .iter()
            .find(|(task, _, _)| *task == self.task)
            .map(|(_, _, unit)| *unit)
            .unwrap_or("target");
        Goal {
            id: self.goal_id(),
            kind: self.goal_kind().to_string(),
            class: "compile".to_string(),
            mandatory: true,
            target: self.target.clone(),
            unit: unit.to_string(),
            change: serde_json::json!({
                "dirtySections": self.dirty_sections,
                "staleAnchors": self.stale_anchors,
                "proposals": self.proposals,
            }),
            cause: None,
            state,
            hints: Vec::new(),
        }
    }

    // The wave-loop item behind a goal: the task name of the goal's kind, the target,
    // and the item fields carried in the goal's change payload.
    pub fn from_goal(goal: &Goal) -> WorkItem {
        let task = TASK_GOAL_KINDS
            .iter()
            .find(|(_, kind, _)| *kind == goal.kind)
            .map(|(task, _, _)| *task)
            .unwrap_or(goal.kind.as_str());
        let list = |key: &str| -> Vec<String> {
            goal.change[key]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        WorkItem {
            task: task.to_string(),
            target: goal.target.clone(),
            dirty_sections: list("dirtySections"),
            stale_anchors: list("staleAnchors"),
            proposals: list("proposals"),
        }
    }
}

// One computed section change. Mirrors docs/compiler/alignment.md#phases.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SectionOp {
    pub op: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub from: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f64>,
}

// One candidate section for a proposed anchor.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AnchorCandidate {
    pub section: String,
    pub similarity: f64,
    #[serde(rename = "quoteLocates")]
    pub quote_locates: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nearest: Option<String>,
    #[serde(default)]
    pub excerpt: String,
}

// One anchor alignment could not place with certainty.
// Mirrors docs/compiler/alignment.md#anchor-relocation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AnchorProposal {
    pub anchor: String,
    pub from: String,
    pub quote: String,
    #[serde(default)]
    pub excerpt: String,
    pub candidates: Vec<AnchorCandidate>,
}

// The pending proposals of one target document, persisted in status.yaml.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DocAlignment {
    pub doc: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<SectionOp>,
    pub proposals: Vec<AnchorProposal>,
}

// The typed dirtiness one commit caused, the input of goal derivation.
// Mirrors docs/compiler/graph.md#change-records.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChangeRecord {
    pub id: String,
    pub generation: u64,
    pub mutation: usize,
    pub kind: String,
    pub subject: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub via: String,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub detail: serde_json::Value,
}

impl ChangeRecord {
    // `index` numbers the record within its generation (the id is `c<generation>-<index>`);
    // `mutation` is the 1-based index of the journal mutation that caused it, 0 for a
    // store-level cause.
    pub fn new(
        generation: u64,
        index: usize,
        mutation: usize,
        kind: &str,
        subject: &str,
        via: &str,
    ) -> ChangeRecord {
        ChangeRecord {
            id: format!("c{}-{}", generation, index),
            generation,
            mutation,
            kind: kind.to_string(),
            subject: subject.to_string(),
            via: via.to_string(),
            detail: serde_json::Value::Null,
        }
    }

    pub fn with_detail(mut self, detail: serde_json::Value) -> ChangeRecord {
        self.detail = detail;
        self
    }

    pub fn cause(&self) -> Cause {
        Cause {
            generation: self.generation,
            mutation: self.mutation,
            via: self.via.clone(),
        }
    }
}

// The committed change that spawned a goal.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cause {
    pub generation: u64,
    pub mutation: usize,
    #[serde(default)]
    pub via: String,
}

// Where a goal stands. Mirrors docs/compiler/reconciler.md#parked-and-failed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalState {
    #[default]
    Open,
    Blocked {
        on: String,
    },
    Parked,
    Failed {
        reason: String,
    },
}

// A unit of work the harness derives and a session resolves. Never stored as a node;
// parked and failed goals persist whole in status.yaml so a re-derivation keeps them.
// Mirrors docs/compiler/reconciler.md#goal-derivation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub kind: String,
    // compile | gc
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub mandatory: bool,
    pub target: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unit: String,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub change: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<Cause>,
    #[serde(default)]
    pub state: GoalState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
}

// A goal a session marked failed, with the reason.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FailedGoal {
    pub goal: Goal,
    pub reason: String,
}

// A goal a changeset resolved, with its one-line justification.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Resolved {
    pub goal: String,
    pub justification: String,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub evidence: serde_json::Value,
}

// A goal a changeset opened, with its cause.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OpenedGoal {
    pub goal: String,
    pub cause: Cause,
}

// One file under journal/, one per generation. Mirrors docs/compiler/graph.md#journal.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct JournalEntry {
    pub build: String,
    #[serde(default)]
    pub generation: u64,
    // session | edit | align | gc | settle-diagnostics | decree | dual-write | ratify | triage | answer
    #[serde(default)]
    pub kind: String,
    // The goals the session was given. Empty for store-level kinds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub batch: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    // Full section references a human save dirtied or removed. Edit entries only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dirtied: Vec<String>,
    #[serde(default)]
    pub mutations: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_goals: Vec<Resolved>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opened_goals: Vec<OpenedGoal>,
    #[serde(default)]
    pub rounds: u32,
    #[serde(default)]
    pub tokens: u64,
}

// The budgets the last build used.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Spent {
    #[serde(default)]
    pub sessions: u64,
    #[serde(default)]
    pub rounds: u64,
    #[serde(default)]
    pub tokens: u64,
}

// Sessions and tokens for one slice of the cost accounting.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CostLine {
    #[serde(default)]
    pub sessions: u64,
    #[serde(default)]
    pub tokens: u64,
}

// Cost accounting for the last build, by goal kind and class.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Costs {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sessions: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tokens: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub by_kind: BTreeMap<String, CostLine>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub by_class: BTreeMap<String, CostLine>,
}

impl Costs {
    pub fn is_empty(&self) -> bool {
        self.sessions == 0
            && self.tokens == 0
            && self.by_kind.is_empty()
            && self.by_class.is_empty()
    }

    // Charge one session to a goal kind and its class.
    pub fn charge(&mut self, kind: &str, class: &str, tokens: u64) {
        self.sessions += 1;
        self.tokens += tokens;
        for (map, key) in [(&mut self.by_kind, kind), (&mut self.by_class, class)] {
            let line = map.entry(key.to_string()).or_default();
            line.sessions += 1;
            line.tokens += tokens;
        }
    }
}

// The convergence verdict of the last build with its counts.
// Mirrors docs/compiler/compilation.md#convergence.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    // converged | incomplete; empty before the first build
    #[serde(default)]
    pub state: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub open: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub failed: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub blocked: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub optional: u64,
}

impl Verdict {
    pub fn converged(&self) -> bool {
        self.state == "converged"
    }

    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.state.is_empty() {
            return write!(f, "(no build yet)");
        }
        if self.converged() {
            write!(f, "converged")?;
            if self.blocked > 0 {
                write!(f, ", {} blocked", self.blocked)?;
            }
            if self.optional > 0 {
                write!(f, ", {} optional advised", self.optional)?;
            }
            return Ok(());
        }
        write!(
            f,
            "{}: {} open, {} failed, {} blocked, {} optional advised",
            self.state, self.open, self.failed, self.blocked, self.optional
        )
    }
}

// The whole of status.yaml. Mirrors docs/compiler/graph.md#storage-layout.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    // The store version. Missing on disk reads as 0, which never matches.
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub generation: u64,
    #[serde(default, skip_serializing_if = "Verdict::is_empty")]
    pub verdict: Verdict,
    // The open change records: the input of goal derivation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<ChangeRecord>,
    // Goals left open when a budget ran out, whole, so a re-derivation keeps them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parked: Vec<Goal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<FailedGoal>,
    #[serde(default, skip_serializing_if = "Costs::is_empty")]
    pub costs: Costs,
    #[serde(default)]
    pub spent: Spent,
    // Open diagnostic counts by severity (suppressed excluded), refreshed by the
    // deterministic tail. The verdict speaks to work completion; this line speaks to
    // document health. Mirrors docs/compiler/compilation.md#convergence.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub diagnostics: BTreeMap<String, u64>,
    // Alignment proposals awaiting a place-anchors session, one block per target document.
    // Mirrors docs/compiler/alignment.md#what-applies-and-what-is-proposed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alignment: Vec<DocAlignment>,
    // Anchors a place-anchors session placed with `reevaluate`: listed as stale anchors
    // on their document's reconcile item until that session addresses them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reevaluate: Vec<String>,
    // The documents matching the project roots, stamped by the build at sync. Commits
    // outside a build read it to order documents by link level.
    // Mirrors docs/compiler/graph.md#storage-layout and reconciler.md#link-levels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<String>,
}

impl Default for Status {
    fn default() -> Self {
        Status {
            version: crate::store::STORE_VERSION,
            generation: 0,
            verdict: Verdict::default(),
            changes: Vec::new(),
            parked: Vec::new(),
            failed: Vec::new(),
            costs: Costs::default(),
            spent: Spent::default(),
            diagnostics: BTreeMap::new(),
            alignment: Vec::new(),
            reevaluate: Vec::new(),
            roots: Vec::new(),
        }
    }
}

impl Status {
    // Subjects of the open change records of the given kinds, in record order, deduplicated.
    pub fn changed_subjects(&self, kinds: &[&str]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in &self.changes {
            if kinds.contains(&c.kind.as_str()) && !out.contains(&c.subject) {
                out.push(c.subject.clone());
            }
        }
        out
    }

    pub fn has_change(&self, kind: &str, subject: &str) -> bool {
        self.changes
            .iter()
            .any(|c| c.kind == kind && c.subject == subject)
    }

    // Record one change. A newer record of the same kind on the same subject supersedes
    // the older one. Mirrors docs/compiler/graph.md#change-records.
    pub fn record_change(&mut self, record: ChangeRecord) {
        self.changes
            .retain(|c| !(c.kind == record.kind && c.subject == record.subject));
        self.changes.push(record);
    }

    // Clear the records of the given kinds on one subject: a goal resolved.
    pub fn clear_changes(&mut self, kinds: &[&str], subject: &str) {
        self.changes
            .retain(|c| !(kinds.contains(&c.kind.as_str()) && c.subject == subject));
    }

    // The ids of the records of the given kinds on one subject.
    pub fn change_ids(&self, kinds: &[&str], subject: &str) -> Vec<String> {
        self.changes
            .iter()
            .filter(|c| kinds.contains(&c.kind.as_str()) && c.subject == subject)
            .map(|c| c.id.clone())
            .collect()
    }

    // The parked goal for a wave-loop item, if any.
    pub fn parked_items(&self) -> Vec<WorkItem> {
        self.parked.iter().map(WorkItem::from_goal).collect()
    }
}

// The in-memory graph: the contents of the graph/ shard files.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub entities: BTreeMap<String, Entity>,
    pub requirements: BTreeMap<String, Requirement>,
    pub views: BTreeMap<String, View>,
    pub relationships: BTreeMap<String, Relationship>,
    pub state_machines: BTreeMap<String, StateMachine>,
    pub diagnostics: BTreeMap<String, Diagnostic>,
    pub redirects: BTreeMap<String, String>,
}

// Relationship types. The first six rank, strongest first: a collapsed or lifted arrow
// shows the strongest ranked type beneath it. Instantiation stands outside the ranking
// and never promotes. Mirrors docs/compiler/model/relationship.md#types.
pub const REL_TYPES: [&str; 7] = [
    "generalization",
    "realization",
    "composition",
    "aggregation",
    "association",
    "dependency",
    "instantiation",
];

pub const INSTANTIATION: &str = "instantiation";

// The type an edge without one contributes: the weakest structural claim.
pub const DEFAULT_REL_TYPE: &str = "dependency";

// Rank among the six ranked types; instantiation and unknown types rank last.
pub fn rel_rank(t: &str) -> usize {
    REL_TYPES
        .iter()
        .take(6)
        .position(|r| *r == t)
        .unwrap_or(REL_TYPES.len())
}

// Process-stable content hash (SipHash with fixed keys), hex-encoded.
pub fn hash_hex(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

// Split a full section reference "doc/path.md#/internal/ref" into (doc, internal ref).
pub fn split_section_ref(full: &str) -> Option<(String, String)> {
    let (doc, sec) = full.split_once('#')?;
    if doc.is_empty() || !sec.starts_with('/') {
        return None;
    }
    Some((doc.to_string(), sec.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(doc: &str, sec: &str, q: &str) -> SourceRef {
        SourceRef {
            doc: doc.into(),
            section: sec.into(),
            quote: q.into(),
        }
    }

    #[test]
    fn requirement_provenance_accessor_reads_whichever_field_carries_it() {
        let quoted = Requirement {
            statement: "The cart holds items.".into(),
            entities: vec!["ent:cart".into()],
            source: Some(quote("t.md", "/t", "The cart holds items.")),
            ..Default::default()
        };
        assert!(matches!(quoted.provenance(), Some(ProvenanceRef::Quote(s)) if s.section == "/t"));
        assert!(quoted.is_quoted());

        let derived = Requirement {
            statement: "The cart is split by category.".into(),
            entities: vec!["ent:cart".into()],
            provenance: Some(Provenance::Derived {
                from: vec!["ent:cart".into()],
                reasoning: "too dense".into(),
            }),
            ..Default::default()
        };
        match derived.provenance() {
            Some(ProvenanceRef::Derived { from, reasoning }) => {
                assert_eq!(from, ["ent:cart".to_string()]);
                assert_eq!(reasoning, "too dense");
            }
            other => panic!("unexpected {:?}", other),
        }
        assert!(!derived.is_quoted());

        let decreed = Requirement {
            provenance: Some(Provenance::Decree {
                author: "owner".into(),
                at: "2026-08-29".into(),
                note: None,
            }),
            ..Default::default()
        };
        assert!(matches!(
            decreed.provenance(),
            Some(ProvenanceRef::Decree {
                author: "owner",
                note: None,
                ..
            })
        ));
        assert!(Requirement::default().provenance().is_none());
    }

    #[test]
    fn verdict_renders_its_counts() {
        let v = Verdict {
            state: "converged".into(),
            ..Default::default()
        };
        assert_eq!(v.to_string(), "converged");
        let v = Verdict {
            state: "converged".into(),
            blocked: 2,
            optional: 1,
            ..Default::default()
        };
        assert_eq!(v.to_string(), "converged, 2 blocked, 1 optional advised");
        let v = Verdict {
            state: "incomplete".into(),
            open: 3,
            failed: 1,
            blocked: 2,
            optional: 5,
        };
        assert_eq!(
            v.to_string(),
            "incomplete: 3 open, 1 failed, 2 blocked, 5 optional advised"
        );
        assert_eq!(Verdict::default().to_string(), "(no build yet)");
    }

    #[test]
    fn provenance_serializes_externally_tagged() {
        let p = Provenance::Derived {
            from: vec!["ent:a".into()],
            reasoning: "why".into(),
        };
        let y = serde_norway::to_string(&p).unwrap();
        assert!(y.starts_with("derived:"), "{}", y);
        assert_eq!(serde_norway::from_str::<Provenance>(&y).unwrap(), p);
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            serde_json::json!({"derived": {"from": ["ent:a"], "reasoning": "why"}})
        );
        let p = Provenance::Quote(quote("d.md", "/d", "q"));
        let y = serde_norway::to_string(&p).unwrap();
        assert!(y.starts_with("quote:"), "{}", y);
        let back: Provenance = serde_norway::from_str("decree: {author: me, at: now}").unwrap();
        assert_eq!(
            back,
            Provenance::Decree {
                author: "me".into(),
                at: "now".into(),
                note: None
            }
        );
        assert!(serde_norway::from_str::<Provenance>(
            "{quote: {doc: d, section: /d, quote: q}, decree: {author: me, at: now}}"
        )
        .is_err());
        assert!(serde_norway::from_str::<Provenance>("{}").is_err());
    }

    #[test]
    fn nodes_round_trip_through_yaml() {
        let src = quote(
            "docs/shop.md",
            "/shop/orders",
            "An order carries a total and a currency.",
        );
        let entity = Entity {
            name: "Order".into(),
            scope: "commerce".into(),
            stereotype: Some("table".into()),
            parent: Some("ent:order-service".into()),
            attributes: vec![
                Attribute {
                    name: "total".into(),
                    r#type: Some("money".into()),
                    value: None,
                    provenance: Provenance::Quote(src.clone()),
                },
                Attribute {
                    name: "tier".into(),
                    r#type: None,
                    value: Some("gold".into()),
                    provenance: Provenance::Decree {
                        author: "owner".into(),
                        at: "g3".into(),
                        note: Some("worked example".into()),
                    },
                },
            ],
            mentions: vec![src.clone()],
            limits: [(
                "requirements-per-entity".to_string(),
                LimitBump { value: 70 },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let y = serde_norway::to_string(&entity).unwrap();
        assert!(y.contains("requirements-per-entity: 70"), "{}", y);
        assert!(y.contains("stereotype: table"), "{}", y);
        let back: Entity = serde_norway::from_str(&y).unwrap();
        assert_eq!(back.attributes, entity.attributes);
        assert_eq!(back.limits, entity.limits);
        assert_eq!(back.parent.as_deref(), Some("ent:order-service"));

        let req = Requirement {
            statement: "When payment succeeds, the order becomes paid.".into(),
            entities: vec!["ent:order".into(), "ent:payment".into()],
            edges: vec![ReqEdge {
                a: "ent:order".into(),
                b: "ent:payment".into(),
                rel_type: Some("dependency".into()),
                cardinality: Some("1".into()),
            }],
            transition: Some(Transition {
                subject: "ent:order".into(),
                from: "placed".into(),
                to: "paid".into(),
                trigger: Some("payment succeeds".into()),
                guard: None,
            }),
            facets: vec![
                Facet {
                    facet: "behavior".into(),
                    reasoning: "a step on an event".into(),
                    measure: None,
                },
                Facet {
                    facet: "quality".into(),
                    reasoning: "bounded".into(),
                    measure: Some("2 seconds".into()),
                },
            ],
            source: Some(src.clone()),
            ..Default::default()
        };
        let y = serde_norway::to_string(&req).unwrap();
        assert!(y.contains("statement:"), "{}", y);
        assert!(!y.contains("provenance"), "{}", y);
        let back: Requirement = serde_norway::from_str(&y).unwrap();
        assert_eq!(back.transition, req.transition);
        assert_eq!(back.facets, req.facets);
        assert_eq!(back.edges, req.edges);
        assert_eq!(back.source, req.source);

        let view = View {
            kind: "sequence".into(),
            title: "Checkout".into(),
            members: vec!["req:shop-1".into(), "req:shop-3".into()],
            excluded: vec![Exclusion {
                id: "req:shop-9".into(),
                note: "example, not flow".into(),
            }],
            query: Some(ViewQuery {
                scope: Some("commerce".into()),
                depth: Some(1),
                ..Default::default()
            }),
            collapse: vec!["ent:order".into()],
            provenance: Some(Provenance::Derived {
                from: vec!["ent:order".into()],
                reasoning: "default view: sequence per flow cluster".into(),
            }),
            default: true,
            ..Default::default()
        };
        let y = serde_norway::to_string(&view).unwrap();
        assert!(y.contains("default: true"), "{}", y);
        let back: View = serde_norway::from_str(&y).unwrap();
        assert_eq!(back.members, view.members);
        assert_eq!(back.excluded, view.excluded);
        assert_eq!(back.query, view.query);
        assert!(back.default);
        let curated = View {
            default: false,
            ..view.clone()
        };
        assert!(!serde_norway::to_string(&curated)
            .unwrap()
            .contains("default: "));
    }

    #[test]
    fn relationship_summary_excludes_instantiation_unless_alone() {
        let group = |a: &str, b: &str, t: &str| Contribution {
            a: a.into(),
            b: b.into(),
            r#type: t.into(),
            cardinality: None,
            requirements: vec![format!("req:{}", t)],
        };
        let rel = Relationship {
            members: vec!["ent:a".into(), "ent:b".into()],
            contributions: vec![
                group("ent:a", "ent:b", "instantiation"),
                group("ent:a", "ent:b", "dependency"),
                group("ent:b", "ent:a", "association"),
            ],
        };
        assert_eq!(rel.strongest(), "association");
        assert_eq!(rel.requirements().len(), 3);
        let only = Relationship {
            members: vec![],
            contributions: vec![group("ent:a", "ent:b", "instantiation")],
        };
        assert_eq!(only.strongest(), "instantiation");
        assert!(rel_rank("instantiation") > rel_rank("dependency"));
    }

    #[test]
    fn work_item_converts_to_a_goal_and_back() {
        let item = WorkItem {
            task: "reconcile-doc".into(),
            target: "docs/shop.md".into(),
            dirty_sections: vec!["/shop".into()],
            stale_anchors: vec!["req:shop-1".into()],
            proposals: Vec::new(),
        };
        let goal = item.to_goal(GoalState::Parked);
        assert_eq!(goal.id, "g:reconcile-section:docs/shop.md");
        assert_eq!(goal.kind, "reconcile-section");
        assert_eq!(goal.state, GoalState::Parked);
        let back = WorkItem::from_goal(&goal);
        assert_eq!(back.task, "reconcile-doc");
        assert_eq!(back.dirty_sections, item.dirty_sections);
        assert_eq!(back.stale_anchors, item.stale_anchors);
        assert_eq!(WorkItem::new("gc", "graph").journal_kind(), "gc");
        assert_eq!(item.journal_kind(), "session");
    }

    #[test]
    fn status_change_records_supersede_and_clear() {
        let mut s = Status::default();
        assert_eq!(s.version, crate::store::STORE_VERSION);
        s.record_change(ChangeRecord::new(
            3,
            1,
            1,
            "entity-changed",
            "ent:a",
            "entities",
        ));
        s.record_change(ChangeRecord::new(
            4,
            2,
            2,
            "entity-changed",
            "ent:a",
            "entities",
        ));
        s.record_change(ChangeRecord::new(
            4,
            3,
            3,
            "requirement-created",
            "req:x-1",
            "section",
        ));
        assert_eq!(s.changes.len(), 2);
        assert_eq!(s.changes[0].id, "c4-2");
        assert_eq!(
            s.changed_subjects(&["entity-changed"]),
            vec!["ent:a".to_string()]
        );
        assert_eq!(
            s.change_ids(&["entity-changed"], "ent:a"),
            vec!["c4-2".to_string()]
        );
        s.clear_changes(&["entity-changed"], "ent:a");
        assert!(!s.has_change("entity-changed", "ent:a"));
        assert_eq!(s.changes.len(), 1);
        // A status.yaml without a version reads as version 0.
        let old: Status = serde_norway::from_str("generation: 4\n").unwrap();
        assert_eq!(old.version, 0);
    }
}
