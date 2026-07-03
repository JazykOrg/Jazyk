// The semantic graph node types. Mirrors docs2/compiler/model.md and graph.schema.yaml.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

// A located quote: the verbatim text is found by string search inside the section's raw body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceRef {
    pub doc: String,
    pub section: String,
    pub quote: String,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<SourceRef>,
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
            mentions: Vec::new(),
            confidence: None,
            reasoning: None,
            created: None,
            updated: None,
        }
    }
}

// An entity pair a requirement ties together, with an optional relationship type.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReqEdge {
    pub a: String,
    pub b: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub rel_type: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Requirement {
    pub ears: String,
    pub entities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<ReqEdge>,
    pub source: SourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
}

// Derived: recomputed on every commit from requirement edges. Never written directly.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Relationship {
    #[serde(rename = "type")]
    pub rel_type: String,
    pub members: Vec<String>,
    pub requirements: Vec<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
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

// One scheduled unit of work: a task type and its target.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkItem {
    pub task: String,
    pub target: String,
    #[serde(rename = "dirtySections", default, skip_serializing_if = "Vec::is_empty")]
    pub dirty_sections: Vec<String>,
    #[serde(rename = "staleAnchors", default, skip_serializing_if = "Vec::is_empty")]
    pub stale_anchors: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalEntry {
    pub build: String,
    #[serde(rename = "workItem")]
    pub work_item: WorkItem,
    pub mutations: Vec<serde_json::Value>,
    pub rounds: u32,
    pub tokens: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Spent {
    #[serde(default)]
    pub turns: u64,
    #[serde(default)]
    pub rounds: u64,
    #[serde(default)]
    pub tokens: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    #[serde(default)]
    pub generation: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parked: Vec<WorkItem>,
    #[serde(default)]
    pub spent: Spent,
    #[serde(default)]
    pub verdict: String,
}

// The in-memory graph: the contents of the graph/ shard files.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub entities: BTreeMap<String, Entity>,
    pub requirements: BTreeMap<String, Requirement>,
    pub relationships: BTreeMap<String, Relationship>,
    pub diagnostics: BTreeMap<String, Diagnostic>,
    pub redirects: BTreeMap<String, String>,
}

// Relationship types, strongest first. An edge's type is promoted to the strongest
// implied across its contributing requirement edges.
pub const REL_TYPES: [&str; 7] = [
    "generalization",
    "realization",
    "composition",
    "aggregation",
    "association",
    "dependency",
    "reference",
];

pub fn rel_rank(t: &str) -> usize {
    REL_TYPES.iter().position(|r| *r == t).unwrap_or(REL_TYPES.len() - 1)
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
