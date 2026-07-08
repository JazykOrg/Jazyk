// The tool registry: the graph's only interface for models. One registry, served
// in-process to the turn harness and over stdio as the MCP server.
// Mirrors docs2/compiler/tools.md.
use crate::context::{self, Focus};
use crate::model::*;
use crate::store::{Op, Store};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

// A rejection: names the violated rule and how to repair the call, because the caller is a
// model that will read it and try again.
#[derive(Debug)]
pub struct ToolError {
    pub rule: String,
    pub message: String,
}

impl ToolError {
    fn new(rule: &str, message: String) -> ToolError {
        ToolError { rule: rule.to_string(), message }
    }
    pub fn to_value(&self) -> Value {
        json!({"error": {"rule": self.rule, "message": self.message}})
    }
}

pub fn catalog() -> Vec<ToolDef> {
    fn obj(props: Value, required: &[&str]) -> Value {
        json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
    }
    vec![
        ToolDef {
            name: "context",
            description: "Load a bounded context pack around a target: an entity id (ent:...), a requirement id (req:...), or a section reference (doc.md#/ref). Returns the pack plus expansion handles for what the budget cut off.",
            parameters: obj(
                json!({
                    "target": {"type": "string"},
                    "focus": {"type": "object", "properties": {"parents": {"type": "integer"}, "mentions": {"type": "integer"}, "requirements": {"type": "integer"}}},
                    "budget": {"type": "integer"}
                }),
                &["target"],
            ),
        },
        ToolDef {
            name: "expand",
            description: "Load the frontier behind an expansion handle returned by a previous context call.",
            parameters: obj(json!({"handle": {"type": "string"}, "budget": {"type": "integer"}}), &["handle"]),
        },
        ToolDef {
            name: "search",
            description: "Look up entities by name or alias before creating one. Deterministic: exact match, then substring, then token overlap. Returns up to 8 results.",
            parameters: obj(json!({"query": {"type": "string"}}), &["query"]),
        },
        ToolDef {
            name: "read_section",
            description: "Read one section's raw body and its child section titles. ref is a full section reference (doc.md#/ref).",
            parameters: obj(json!({"ref": {"type": "string"}}), &["ref"]),
        },
        ToolDef {
            name: "get_entity",
            description: "One entity with its definition, mentions, requirements, and relationships.",
            parameters: obj(json!({"id": {"type": "string"}}), &["id"]),
        },
        ToolDef {
            name: "upsert_entity",
            description: "Create a domain-concept entity, or update it if the name already exists. Entities are concepts, never file paths, CLI flags, or markdown terms. mention cites the section and the verbatim quote that talks about it. Omit scope unless the documents explicitly name a bounded context; an invented scope splits one concept into two.",
            parameters: obj(
                json!({
                    "name": {"type": "string"},
                    "definition": {"type": "string"},
                    "aliases": {"type": "array", "items": {"type": "string"}},
                    "scope": {"type": "string"},
                    "mention": {"type": "object", "properties": {"section": {"type": "string"}, "quote": {"type": "string"}}, "required": ["section", "quote"]},
                    "note": {"type": "string"}
                }),
                &["name", "mention"],
            ),
        },
        ToolDef {
            name: "update_entity",
            description: "Update an existing entity. A rename keeps the id.",
            parameters: obj(
                json!({
                    "id": {"type": "string"},
                    "name": {"type": "string"},
                    "definition": {"type": "string"},
                    "add_aliases": {"type": "array", "items": {"type": "string"}}
                }),
                &["id"],
            ),
        },
        ToolDef {
            name: "delete_entity",
            description: "Delete an entity that no requirement references.",
            parameters: obj(json!({"id": {"type": "string"}, "reason": {"type": "string"}}), &["id", "reason"]),
        },
        ToolDef {
            name: "merge_entities",
            description: "Merge two entities that are the same concept. keep survives; absorb's aliases, mentions, and references are rewired onto it.",
            parameters: obj(json!({"keep": {"type": "string"}, "absorb": {"type": "string"}, "reason": {"type": "string"}}), &["keep", "absorb", "reason"]),
        },
        ToolDef {
            name: "upsert_requirement",
            description: "Record one EARS requirement (a single testable statement using 'shall'). The store mints the id; calls are idempotent by the statement's natural key. entities are the entity ids the statement is about. quote is the verbatim source sentence copied from the section. edges optionally tie two of the entities with a relationship type.",
            parameters: obj(
                json!({
                    "ears": {"type": "string"},
                    "entities": {"type": "array", "items": {"type": "string"}},
                    "section": {"type": "string"},
                    "quote": {"type": "string"},
                    "edges": {"type": "array", "items": {"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "string"}, "type": {"type": "string"}}, "required": ["a", "b"]}}
                }),
                &["ears", "entities", "section", "quote"],
            ),
        },
        ToolDef {
            name: "update_requirement",
            description: "Update an existing requirement's statement, entities, or edges.",
            parameters: obj(
                json!({
                    "id": {"type": "string"},
                    "ears": {"type": "string"},
                    "entities": {"type": "array", "items": {"type": "string"}},
                    "edges": {"type": "array", "items": {"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "string"}, "type": {"type": "string"}}, "required": ["a", "b"]}}
                }),
                &["id"],
            ),
        },
        ToolDef {
            name: "delete_requirement",
            description: "Delete a requirement.",
            parameters: obj(json!({"id": {"type": "string"}, "reason": {"type": "string"}}), &["id", "reason"]),
        },
        ToolDef {
            name: "report_diagnostic",
            description: "Record a judgment about the graph or documents. rule is one of: contradiction, duplicate-entity, missing-link, ambiguity. severity: error, warning, info, or none. Severity error only when two statements cannot both hold.",
            parameters: obj(
                json!({
                    "rule": {"type": "string"},
                    "severity": {"type": "string", "enum": ["error", "warning", "info", "none"]},
                    "subjects": {"type": "array", "items": {"type": "string"}},
                    "message": {"type": "string"},
                    "reasoning": {"type": "string"}
                }),
                &["rule", "severity", "subjects", "message"],
            ),
        },
        ToolDef {
            name: "resolve_diagnostic",
            description: "Mark a diagnostic resolved: its condition no longer holds.",
            parameters: obj(json!({"id": {"type": "string"}, "reason": {"type": "string"}}), &["id", "reason"]),
        },
        ToolDef {
            name: "set_coverage",
            description: "Mark a section covered (its content is reflected in the graph) or non-normative (it states no requirements; a note is required saying why).",
            parameters: obj(
                json!({
                    "section": {"type": "string"},
                    "state": {"type": "string", "enum": ["covered", "non-normative"]},
                    "note": {"type": "string"}
                }),
                &["section", "state"],
            ),
        },
        ToolDef {
            name: "gen_instructions",
            description: "The generation contract every worker follows: one task per entity producing the deliverable files and the tests for its requirements, traceability markers, the two test kinds, the parts protocol for dense entities. The medium derives from the context; the contract never names one.",
            parameters: obj(json!({}), &[]),
        },
        ToolDef {
            name: "gen_pending",
            description: "Entities whose facts differ from the ledger, each with the requirement ids added, removed, or reworded since the entity was last generated.",
            parameters: obj(json!({}), &[]),
        },
        ToolDef {
            name: "gen_task",
            description: "The full generation package for one entity: instructions, context pack, requirement groups (with suggested test names), change diff, the deliverable directory, factHash, and the manifest of already generated files. The worker writes the files itself and chooses layout, names, and run commands.",
            parameters: obj(json!({"entity": {"type": "string"}}), &["entity"]),
        },
        ToolDef {
            name: "gen_mark",
            description: "Record the task done. manifest.files lists every deliverable-relative file written; manifest.tests binds each requirement to its test: {requirement, kind: programmatic|llm, label, artifact, name, run, cwd?, files?}. Pass the factHash from the gen_task package.",
            parameters: obj(
                json!({
                    "entity": {"type": "string"},
                    "factHash": {"type": "string"},
                    "manifest": {"type": "object", "properties": {
                        "files": {"type": "array", "items": {"type": "string"}},
                        "tests": {"type": "array", "items": {"type": "object"}}
                    }}
                }),
                &["entity", "factHash", "manifest"],
            ),
        },
        ToolDef {
            name: "verify_pending",
            description: "Ledger rows needing action, with derived status (missing, stale-requirement, stale-test, stale-code, failing, unverified) and reason. Deterministic; no model involved.",
            parameters: obj(json!({"filter": {"type": "string", "enum": ["stale", "failing", "all"]}, "entity": {"type": "string"}}), &[]),
        },
        ToolDef {
            name: "verify_task",
            description: "The verification package for one requirement: statement, quote, factHash, context pack, implementing files, and either the run command (programmatic) or the criteria (llm).",
            parameters: obj(json!({"requirement": {"type": "string"}}), &["requirement"]),
        },
        ToolDef {
            name: "verify_mark",
            description: "Record a pass or fail verdict with evidence. Pass the factHash from the verify_task package; if the graph moved meanwhile the verdict is recorded but the row stays pending.",
            parameters: obj(
                json!({
                    "requirement": {"type": "string"},
                    "verdict": {"type": "string", "enum": ["pass", "fail"]},
                    "factHash": {"type": "string"},
                    "evidence": {"type": "string"}
                }),
                &["requirement", "verdict"],
            ),
        },
        ToolDef {
            name: "done",
            description: "End the turn and request commit of the staged mutations. summary says what was done.",
            parameters: obj(json!({"summary": {"type": "string"}}), &["summary"]),
        },
    ]
}

pub const READ_TOOLS: [&str; 5] = ["context", "expand", "search", "read_section", "get_entity"];
pub const GEN_TOOLS: [&str; 4] = ["gen_instructions", "gen_pending", "gen_task", "gen_mark"];
pub const VERIFY_TOOLS: [&str; 3] = ["verify_pending", "verify_task", "verify_mark"];

pub fn toolset(task: &str) -> Vec<&'static str> {
    match task {
        "reconcile-doc" => vec![
            "context", "expand", "search", "read_section", "upsert_entity", "update_entity", "delete_entity",
            "upsert_requirement", "update_requirement", "delete_requirement", "set_coverage", "done",
        ],
        "review-entity" => vec![
            "context", "expand", "search", "get_entity", "update_entity", "merge_entities", "update_requirement",
            "delete_requirement", "report_diagnostic", "resolve_diagnostic", "done",
        ],
        "mcp-read" => {
            let mut v = READ_TOOLS.to_vec();
            v.extend(GEN_TOOLS);
            v.extend(VERIFY_TOOLS);
            v
        }
        "mcp-write" => catalog().iter().map(|t| t.name).filter(|n| *n != "done").collect(),
        _ => READ_TOOLS.to_vec(),
    }
}

// A "built with X and Y" style list inside one statement: several atomic facts bundled
// into one sentence. Returns the offending clause for the repair message.
fn bundled_tech_list(ears: &str) -> Option<String> {
    let lower = ears.to_lowercase();
    for marker in ["built with ", "built using ", "implemented with ", "implemented using ", "written in ", "composed of "] {
        if let Some(pos) = lower.find(marker) {
            let tail = &ears[pos..];
            let clause: String = tail.chars().take_while(|c| *c != '.' && *c != ';').collect();
            if clause.to_lowercase().contains(" and ") || clause.contains(',') {
                return Some(clause.trim().to_string());
            }
        }
    }
    None
}

// The lenient EARS shape gate, shared by upsert_requirement and update_requirement so a
// revision cannot land a statement a fresh upsert would reject. Mirrors
// docs2/compiler/concepts/ears.md#shape-check.
fn ears_shape(ears: &str) -> Result<(), ToolError> {
    if !ears.to_lowercase().contains("shall") {
        return Err(ToolError::new(
            "not-ears",
            "the statement must be a single testable EARS sentence using 'shall' (e.g. 'When X, the system shall Y.')".into(),
        ));
    }
    if ears.len() > 400 {
        return Err(ToolError::new("not-ears", "the statement is too long; one testable sentence, not a paragraph".into()));
    }
    // Atomicity: a technology list bundled into one statement is several requirements
    // wearing one sentence.
    if let Some(bundle) = bundled_tech_list(ears) {
        return Err(ToolError::new(
            "not-ears",
            format!("the statement bundles several facts ({}); record one requirement per fact, all quoting the same source sentence", bundle),
        ));
    }
    Ok(())
}

// Names that look like syntax rather than a concept. Rejected without an explaining note.
fn junk_name(name: &str) -> Option<&'static str> {
    let n = name.trim();
    let lower = n.to_lowercase();
    if n.starts_with('-') {
        return Some("looks like a CLI flag");
    }
    if n.contains('/') || n.contains('\\') {
        return Some("looks like a file path");
    }
    for ext in [".md", ".rs", ".yaml", ".yml", ".toml", ".json", ".html"] {
        if lower.ends_with(ext) {
            return Some("looks like a file name");
        }
    }
    if n.contains('`') || n.contains('#') {
        return Some("contains markup");
    }
    // A single camelCase token is a code identifier: an operation or accessor named in
    // the docs. Operations are requirement detail, never entities.
    if !n.contains(' ')
        && n.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && n.chars().any(|c| c.is_ascii_uppercase())
    {
        return Some("looks like a code identifier (an operation or function); operations belong in the requirement statement, not the entity list");
    }
    const MD_TERMS: [&str; 12] = [
        "heading", "headings", "code block", "code blocks", "blockquote", "blockquotes", "list item",
        "list items", "markdown", "table", "link", "bullet",
    ];
    if MD_TERMS.contains(&lower.as_str()) {
        return Some("is a markdown construct, not a domain concept");
    }
    for article in ["the ", "a ", "an "] {
        if lower.starts_with(article) {
            return Some("starts with an article; name the concept itself");
        }
    }
    if n.len() > 60 {
        return Some("too long for a concept name");
    }
    if n.is_empty() {
        return Some("empty");
    }
    None
}

// The scope a turn works in: which task, which document, which sections it may claim.
#[derive(Clone, Default)]
pub struct WorkScope {
    pub task: String,
    pub doc: Option<String>,
    pub target_sections: Vec<String>,
    // Requirement ids whose quote stopped locating; the done gate holds the turn to
    // addressing each one. See docs2/compiler/graph.md#validation-gates.
    pub stale_anchors: Vec<String>,
}

// One turn's tool session: reads answer from the snapshot, writes stage into the changeset.
pub struct ToolSession {
    pub snapshot: Store,
    pub scope: WorkScope,
    pub staged: Vec<Op>,
    pub done: Option<String>,
    // Resolved [gen] settings for the generation and verification tools.
    pub gen: crate::gen::GenSettings,
    mutation_limit: usize,
    default_budget: usize,
    // Staged entities (id -> entity) so lookup-before-create sees this turn's own creates.
    staged_entities: std::collections::BTreeMap<String, Entity>,
    staged_reqs: BTreeSet<String>,
    taken_ids: BTreeSet<String>,
}

impl ToolSession {
    pub fn new(snapshot: Store, scope: WorkScope, mutation_limit: usize, default_budget: usize) -> ToolSession {
        let gen = crate::gen::GenSettings::from_out(&snapshot.out);
        ToolSession {
            snapshot,
            scope,
            staged: Vec::new(),
            done: None,
            gen,
            mutation_limit,
            default_budget,
            staged_entities: Default::default(),
            staged_reqs: Default::default(),
            taken_ids: Default::default(),
        }
    }

    fn gen_settings(&self) -> crate::gen::GenSettings {
        self.gen.clone()
    }

    // An existing or staged entity whose name tokens contain, or are contained by, the
    // candidate's tokens (same scope). "backend" vs "backend system" is one concept;
    // single generic tokens are exempt to keep "id" from matching "user id".
    fn near_name(&self, name: &str, scope: &str) -> Option<(String, String)> {
        let tokens = |s: &str| -> BTreeSet<String> {
            s.to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .filter(|t| !t.is_empty())
                .map(String::from)
                .collect()
        };
        let cand = tokens(name);
        if cand.is_empty() {
            return None;
        }
        let check = |ename: &str| -> bool {
            let ex = tokens(ename);
            if ex == cand {
                return false; // exact natural-key match is the upsert path, not a twin
            }
            let (small, big) = if ex.len() <= cand.len() { (&ex, &cand) } else { (&cand, &ex) };
            // Containment of a multi-token name, or of a single specific (long) token.
            small.is_subset(big) && (small.len() > 1 || small.iter().next().map(|t| t.len() >= 5).unwrap_or(false))
        };
        for (id, e) in &self.snapshot.graph.entities {
            if e.scope == scope && check(&e.name) {
                return Some((id.clone(), e.name.clone()));
            }
        }
        for (id, e) in &self.staged_entities {
            if e.scope == scope && check(&e.name) {
                return Some((id.clone(), e.name.clone()));
            }
        }
        None
    }

    fn known_entity(&self, id: &str) -> bool {
        let rid = self.snapshot.resolve_id(id);
        self.snapshot.graph.entities.contains_key(rid) || self.staged_entities.contains_key(id)
    }

    // Lenient reference resolution. Models, small ones especially, drop the `ent:`
    // prefix or pass the display name; when exactly one node matches, the intent is
    // unambiguous, so resolve it instead of bouncing the call. Mirrors
    // docs2/compiler/graph.md#validation-gates.
    fn canon_entity_id(&self, raw: &str) -> Option<String> {
        if self.known_entity(raw) {
            return Some(self.snapshot.resolve_id(raw).to_string());
        }
        let raw = raw.trim();
        if !raw.starts_with("ent:") {
            let prefixed = format!("ent:{}", raw);
            if self.known_entity(&prefixed) {
                return Some(self.snapshot.resolve_id(&prefixed).to_string());
            }
            let slug = format!("ent:{}", raw.to_lowercase().split_whitespace().collect::<Vec<_>>().join("-"));
            if self.known_entity(&slug) {
                return Some(self.snapshot.resolve_id(&slug).to_string());
            }
        } else if let Some(rest) = raw.strip_prefix("ent:") {
            // A case or spacing variant of an existing id (`ent:factHash`) resolves to it.
            let slug = format!("ent:{}", rest.to_lowercase().split_whitespace().collect::<Vec<_>>().join("-"));
            if self.known_entity(&slug) {
                return Some(self.snapshot.resolve_id(&slug).to_string());
            }
        }
        // Exact display name or alias, snapshot plus staged; unique match only.
        let want = raw.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        if want.is_empty() {
            return None;
        }
        let norm = |n: &str| n.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        let mut hits: Vec<String> = Vec::new();
        let all = self
            .snapshot
            .graph
            .entities
            .iter()
            .map(|(i, e)| (i.clone(), e))
            .chain(self.staged_entities.iter().map(|(i, e)| (i.clone(), e)));
        for (id, e) in all {
            if (norm(&e.name) == want || e.aliases.iter().any(|a| norm(a) == want)) && !hits.contains(&id) {
                hits.push(id);
            }
        }
        if hits.len() == 1 {
            return Some(hits.remove(0));
        }
        None
    }

    // Requirement-id counterpart of canon_entity_id: forgive a missing `req:` prefix.
    fn canon_req_id(&self, raw: &str) -> Result<String, ToolError> {
        let known = |id: &str| self.snapshot.graph.requirements.contains_key(id) || self.staged_reqs.contains(id);
        if known(raw) {
            return Ok(raw.to_string());
        }
        if !raw.starts_with("req:") {
            let prefixed = format!("req:{}", raw.trim());
            if known(&prefixed) {
                return Ok(prefixed);
            }
        }
        Err(ToolError::new("unknown-id", format!("unknown requirement id `{}`", raw)))
    }

    fn unknown_entity_error(&self, id: &str) -> ToolError {
        let bare = id.strip_prefix("ent:").unwrap_or(id).replace('-', " ");
        let hits = self.search_all(&bare);
        let hint = if hits.is_empty() {
            "search for it, or create it with upsert_entity first".to_string()
        } else {
            format!("nearest existing: {}; use one of those, or create it with upsert_entity first", hits
                .iter()
                .take(3)
                .map(|(id, _, _)| id.as_str())
                .collect::<Vec<_>>()
                .join(", "))
        };
        ToolError::new("unknown-id", format!("unknown entity id `{}`; {}", id, hint))
    }

    // Search across the snapshot plus this turn's staged creates.
    fn search_all(&self, query: &str) -> Vec<(String, String, String)> {
        let mut hits: Vec<(String, String, String)> = Vec::new();
        let q = query.trim().to_lowercase();
        for (id, e) in &self.staged_entities {
            if e.name.to_lowercase().contains(&q) || q.contains(&e.name.to_lowercase()) {
                hits.push((id.clone(), e.name.clone(), e.definition.clone().unwrap_or_default()));
            }
        }
        hits.extend(self.snapshot.search(query));
        hits.truncate(8);
        hits
    }

    // Resolve a section argument: either "doc.md#/ref" or a bare "/ref" against the work doc.
    fn resolve_section(&self, section: &str) -> Result<(String, String), ToolError> {
        let full = if section.starts_with('/') {
            match &self.scope.doc {
                Some(d) => format!("{}#{}", d, section),
                None => {
                    return Err(ToolError::new(
                        "bad-section",
                        format!("bare section reference `{}` needs a document; use doc.md#{}", section, section),
                    ))
                }
            }
        } else {
            section.to_string()
        };
        let (doc, sec) = split_section_ref(&full).ok_or_else(|| {
            // Repair-oriented: name the sections this turn is actually working on.
            let hint = if self.scope.target_sections.is_empty() {
                String::new()
            } else {
                let doc = self.scope.doc.as_deref().unwrap_or_default();
                format!(
                    "; this turn's sections: {}",
                    self.scope.target_sections.iter().map(|s| format!("{}#{}", doc, s)).collect::<Vec<_>>().join(", ")
                )
            };
            ToolError::new("bad-section", format!("bad section reference `{}`; expected doc.md#/ref{}", section, hint))
        })?;
        if !self
            .snapshot
            .docs
            .get(&doc)
            .map(|d| d.sections.contains_key(&sec))
            .unwrap_or(false)
        {
            return Err(ToolError::new("unknown-section", format!("unknown section `{}#{}`", doc, sec)));
        }
        Ok((doc, sec))
    }

    // Validates the quote and returns the form that locates in the source, so the
    // stored provenance stays verbatim to the document. A text-codec model often
    // backslash-escapes markdown inside JSON (\` for `); the source never carries
    // the backslashes, so the unescaped form is tried as a fallback and stored.
    // Mirrors docs2/compiler/graph.md#validation-gates.
    fn check_quote(&self, doc: &str, sec: &str, quote: &str) -> Result<String, ToolError> {
        let q = quote.trim();
        if q.is_empty() {
            return Err(ToolError::new("bad-quote", "quote is empty; copy the sentence verbatim from the section".into()));
        }
        if self.snapshot.quote_locates(doc, sec, q) {
            return Ok(q.to_string());
        }
        let mut unescaped = String::with_capacity(q.len());
        let mut chars = q.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(&n) = chars.peek() {
                    if n.is_ascii_punctuation() {
                        continue;
                    }
                }
            }
            unescaped.push(c);
        }
        if unescaped != q && self.snapshot.quote_locates(doc, sec, &unescaped) {
            return Ok(unescaped);
        }
        Err(ToolError::new(
            "quote-not-found",
            format!("quote not found in {}#{}; copy the sentence verbatim from the section", doc, sec),
        ))
    }

    // Implicit done: commit valid staged work when the model forgot the finish
    // contract or ran out of rounds. One dishonest `covered` claim must not sink the
    // rest of the turn's work: drop the offending coverage marks (those sections stay
    // unprocessed; the next build resumes them) and try once more. Mirrors
    // docs2/compiler/turns.md#budgets.
    pub fn finish_implicit(&mut self, summary: &str) -> bool {
        if self.staged.is_empty() {
            return false;
        }
        if self.dispatch("done", &json!({"summary": summary})).is_ok() {
            return true;
        }
        let staged_req_sources: Vec<(String, String)> = self
            .staged
            .iter()
            .filter_map(|o| match o {
                Op::CreateRequirement { requirement, .. } => {
                    Some((requirement.source.doc.clone(), requirement.source.section.clone()))
                }
                _ => None,
            })
            .collect();
        let snap = &self.snapshot;
        self.staged.retain(|op| match op {
            Op::SetCoverage { doc, section, state, .. } if state == "covered" => {
                snap.graph
                    .requirements
                    .values()
                    .any(|r| &r.source.doc == doc && &r.source.section == section)
                    || staged_req_sources.iter().any(|(d, s)| d == doc && s == section)
            }
            _ => true,
        });
        !self.staged.is_empty() && self.dispatch("done", &json!({"summary": summary})).is_ok()
    }

    fn stage(&mut self, op: Op) -> Result<(), ToolError> {
        if self.staged.len() >= self.mutation_limit {
            return Err(ToolError::new(
                "mutation-budget",
                format!("turn mutation budget ({}) exhausted; call done", self.mutation_limit),
            ));
        }
        self.staged.push(op);
        Ok(())
    }

    fn str_arg(args: &Value, key: &str) -> Result<String, ToolError> {
        args[key]
            .as_str()
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| ToolError::new("bad-args", format!("missing required string argument `{}`", key)))
    }

    fn opt_str(args: &Value, key: &str) -> Option<String> {
        args[key].as_str().map(|s| s.to_string()).filter(|s| !s.trim().is_empty())
    }

    fn str_list(args: &Value, key: &str) -> Vec<String> {
        args[key]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default()
    }

    pub fn dispatch(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        match name {
            "context" => {
                let target = Self::str_arg(args, "target")?;
                let focus = if args["focus"].is_object() {
                    Focus {
                        parents: args["focus"]["parents"].as_u64().unwrap_or(2) as u32,
                        mentions: args["focus"]["mentions"].as_u64().unwrap_or(1) as u32,
                        requirements: args["focus"]["requirements"].as_u64().unwrap_or(2) as u32,
                    }
                } else {
                    Focus::default()
                };
                let budget = args["budget"].as_u64().map(|b| b as usize).unwrap_or(self.default_budget / 2);
                let pack = context::assemble(&self.snapshot, &target, &focus, budget)
                    .map_err(|e| ToolError::new("bad-target", e))?;
                Ok(json!({"pack": pack.pack, "handles": pack.handles}))
            }
            "expand" => {
                let handle = Self::str_arg(args, "handle")?;
                let budget = args["budget"].as_u64().map(|b| b as usize).unwrap_or(self.default_budget / 2);
                let pack = context::expand(&self.snapshot, &handle, budget)
                    .map_err(|e| ToolError::new("bad-handle", e))?;
                Ok(json!({"pack": pack.pack, "handles": pack.handles}))
            }
            "search" => {
                let query = Self::str_arg(args, "query")?;
                let hits = self.search_all(&query);
                Ok(json!(hits
                    .iter()
                    .map(|(id, name, def)| json!({"id": id, "name": name, "definition": def}))
                    .collect::<Vec<_>>()))
            }
            "read_section" => {
                let r = Self::str_arg(args, "ref")?;
                let (doc, sec) = self.resolve_section(&r)?;
                let rec = &self.snapshot.docs[&doc];
                let s = &rec.sections[&sec];
                let children: Vec<String> = rec
                    .sections
                    .iter()
                    .filter(|(_, c)| c.parent.as_deref() == Some(sec.as_str()))
                    .map(|(r, c)| format!("{}#{} ({})", doc, r, c.title))
                    .collect();
                Ok(json!({"title": s.title, "raw": s.raw, "children": children}))
            }
            "get_entity" => {
                let id = Self::str_arg(args, "id")?;
                let rid = self.canon_entity_id(&id).ok_or_else(|| self.unknown_entity_error(&id))?;
                let e = self
                    .snapshot
                    .graph
                    .entities
                    .get(&rid)
                    .ok_or_else(|| self.unknown_entity_error(&id))?;
                let reqs: Vec<Value> = self
                    .snapshot
                    .requirements_referencing(&rid)
                    .iter()
                    .filter_map(|r| self.snapshot.graph.requirements.get(r).map(|req| json!({"id": r, "ears": req.ears})))
                    .collect();
                let rels: Vec<Value> = self
                    .snapshot
                    .graph
                    .relationships
                    .iter()
                    .filter(|(_, rel)| rel.members.contains(&rid))
                    .map(|(id, rel)| json!({"id": id, "type": rel.rel_type, "members": rel.members}))
                    .collect();
                Ok(json!({
                    "id": rid, "name": e.name, "definition": e.definition, "aliases": e.aliases,
                    "scope": e.scope, "mentions": e.mentions.iter().map(|m| json!({"doc": m.doc, "section": m.section, "quote": m.quote})).collect::<Vec<_>>(),
                    "requirements": reqs, "relationships": rels
                }))
            }
            "upsert_entity" => {
                let name_arg = Self::str_arg(args, "name")?;
                let scope = Self::opt_str(args, "scope").unwrap_or_else(|| "public".to_string());
                let note = Self::opt_str(args, "note");
                if let Some(why) = junk_name(&name_arg) {
                    if note.is_none() {
                        return Err(ToolError::new(
                            "junk-name",
                            format!(
                                "`{}` {}; entities are domain concepts. If it truly is one, repeat the call with a `note` explaining why",
                                name_arg, why
                            ),
                        ));
                    }
                }
                // Near-name gate: a qualifier variant of an existing entity is almost
                // always the same concept. Reuse it and record the wording as an alias
                // instead of minting a twin; a note overrides when genuinely distinct.
                // Mirrors docs2/compiler/model/entity.md#what-is-an-entity.
                if note.is_none() {
                    if let Some((eid, ename)) = self.near_name(&name_arg, &scope) {
                        return Err(ToolError::new(
                            "near-duplicate",
                            format!(
                                "`{}` is a name variant of existing `{}` ({}); reuse that id and add your wording with update_entity add_aliases. If it truly is a different concept, repeat the call with a `note` saying how they differ",
                                name_arg, eid, ename
                            ),
                        ));
                    }
                }
                let mention = &args["mention"];
                let section = mention["section"].as_str().unwrap_or_default();
                let quote = mention["quote"].as_str().unwrap_or_default();
                let (doc, sec) = self.resolve_section(section)?;
                if let (Some(wd), "reconcile-doc") = (&self.scope.doc, self.scope.task.as_str()) {
                    if &doc != wd {
                        return Err(ToolError::new(
                            "wrong-document",
                            format!(
                                "mention cites {} but this turn reconciles {}; quote a sentence from {}'s own sections (text this document merely links to cannot anchor a mention here)",
                                doc, wd, wd
                            ),
                        ));
                    }
                }
                let quote = self.check_quote(&doc, &sec, quote)?;
                let mention_ref = SourceRef { doc, section: sec, quote };

                // Lookup before create: the natural key may already exist in the graph or in
                // this turn's own staged creates.
                let existing = self
                    .snapshot
                    .find_natural(&name_arg, &scope)
                    .or_else(|| {
                        self.staged_entities
                            .iter()
                            .find(|(_, e)| {
                                e.scope == scope && e.name.trim().to_lowercase() == name_arg.trim().to_lowercase()
                            })
                            .map(|(id, _)| id.clone())
                    });
                if let Some(id) = existing {
                    self.stage(Op::UpdateEntity {
                        id: id.clone(),
                        name: None,
                        definition: Self::opt_str(args, "definition"),
                        add_aliases: Self::str_list(args, "aliases"),
                        add_mention: Some(mention_ref),
                    })?;
                    return Ok(json!({"id": id, "created": false}));
                }
                let id = self.snapshot.mint_entity_id(&name_arg, &self.taken_ids);
                self.taken_ids.insert(id.clone());
                let entity = Entity {
                    name: name_arg,
                    aliases: Self::str_list(args, "aliases"),
                    definition: Self::opt_str(args, "definition"),
                    scope,
                    mentions: vec![mention_ref],
                    confidence: None,
                    reasoning: note,
                    created: None,
                    updated: None,
                };
                self.staged_entities.insert(id.clone(), entity.clone());
                self.stage(Op::CreateEntity { id: id.clone(), entity })?;
                Ok(json!({"id": id, "created": true}))
            }
            "update_entity" => {
                let id = Self::str_arg(args, "id")?;
                let Some(rid) = self.canon_entity_id(&id) else {
                    return Err(self.unknown_entity_error(&id));
                };
                let name = Self::opt_str(args, "name");
                if let Some(n) = &name {
                    if let Some(why) = junk_name(n) {
                        return Err(ToolError::new("junk-name", format!("`{}` {}", n, why)));
                    }
                }
                self.stage(Op::UpdateEntity {
                    id: rid.clone(),
                    name,
                    definition: Self::opt_str(args, "definition"),
                    add_aliases: Self::str_list(args, "add_aliases"),
                    add_mention: None,
                })?;
                Ok(json!({"id": rid, "updated": true}))
            }
            "delete_entity" => {
                let id = Self::str_arg(args, "id")?;
                let reason = Self::str_arg(args, "reason")?;
                let Some(rid) = self.canon_entity_id(&id) else {
                    return Err(self.unknown_entity_error(&id));
                };
                let mut refs = self.snapshot.requirements_referencing(&rid);
                for op in &self.staged {
                    if let Op::CreateRequirement { id: qid, requirement } = op {
                        if requirement.entities.contains(&rid) || requirement.entities.contains(&id) {
                            refs.push(qid.clone());
                        }
                    }
                }
                if !refs.is_empty() {
                    return Err(ToolError::new(
                        "still-referenced",
                        format!("cannot delete {}; requirements still reference it: {}", rid, refs.join(", ")),
                    ));
                }
                self.stage(Op::DeleteEntity { id: rid, reason })?;
                Ok(json!({"deleted": true}))
            }
            "merge_entities" => {
                let keep_arg = Self::str_arg(args, "keep")?;
                let absorb_arg = Self::str_arg(args, "absorb")?;
                let reason = Self::str_arg(args, "reason")?;
                let Some(keep) = self.canon_entity_id(&keep_arg) else {
                    return Err(self.unknown_entity_error(&keep_arg));
                };
                let Some(absorb) = self.canon_entity_id(&absorb_arg) else {
                    return Err(self.unknown_entity_error(&absorb_arg));
                };
                if keep == absorb {
                    return Err(ToolError::new("bad-merge", "keep and absorb are the same entity".into()));
                }
                self.stage(Op::MergeEntities { keep: keep.clone(), absorb, reason })?;
                Ok(json!({"kept": keep}))
            }
            "upsert_requirement" => {
                let ears = Self::str_arg(args, "ears")?;
                ears_shape(&ears)?;
                // Provenance is validated first: a quote that does not locate is the
                // clearest signal a statement was invented, and it must not be masked
                // by an entity-id error the model would keep retrying around.
                let section = Self::str_arg(args, "section")?;
                let quote = Self::str_arg(args, "quote")?;
                let (doc, sec) = self.resolve_section(&section)?;
                if let (Some(wd), "reconcile-doc") = (&self.scope.doc, self.scope.task.as_str()) {
                    if &doc != wd {
                        return Err(ToolError::new(
                            "wrong-document",
                            format!(
                                "source cites {} but this turn reconciles {}; quote the sentence from {}'s own sections (text this document merely links to cannot anchor a requirement here)",
                                doc, wd, wd
                            ),
                        ));
                    }
                }
                let quote = self.check_quote(&doc, &sec, &quote)?;
                let raw_entities = Self::str_list(args, "entities");
                if raw_entities.is_empty() {
                    return Err(ToolError::new("no-entities", "a requirement must reference at least one entity id".into()));
                }
                let mut entities: Vec<String> = Vec::new();
                for e in &raw_entities {
                    match self.canon_entity_id(e) {
                        Some(id) => {
                            if !entities.contains(&id) {
                                entities.push(id);
                            }
                        }
                        None => return Err(self.unknown_entity_error(e)),
                    }
                }
                let mut edges = Vec::new();
                if let Some(arr) = args["edges"].as_array() {
                    for e in arr {
                        let raw_a = e["a"].as_str().unwrap_or_default();
                        let raw_b = e["b"].as_str().unwrap_or_default();
                        let a = self.canon_entity_id(raw_a).unwrap_or_else(|| raw_a.to_string());
                        let b = self.canon_entity_id(raw_b).unwrap_or_else(|| raw_b.to_string());
                        if !entities.contains(&a) || !entities.contains(&b) {
                            return Err(ToolError::new(
                                "bad-edge",
                                format!("edge {}~{} may only tie entities the requirement itself references", a, b),
                            ));
                        }
                        let t = e["type"].as_str().map(|s| s.to_string());
                        if let Some(t) = &t {
                            if !REL_TYPES.contains(&t.as_str()) {
                                return Err(ToolError::new(
                                    "bad-edge",
                                    format!("unknown relationship type `{}`; one of: {}", t, REL_TYPES.join(", ")),
                                ));
                            }
                        }
                        edges.push(ReqEdge { a, b, rel_type: t });
                    }
                }
                let source = SourceRef { doc: doc.clone(), section: sec, quote: quote.trim().to_string() };
                // The store mints ids; a supplied id is ignored. Idempotency comes from
                // the natural key at commit, and update_requirement handles revisions.
                let mut taken = self.taken_ids.clone();
                taken.extend(self.staged_reqs.iter().cloned());
                let id = self.snapshot.mint_req_id(&doc, &taken);
                self.staged_reqs.insert(id.clone());
                self.taken_ids.insert(id.clone());
                self.stage(Op::CreateRequirement {
                    id: id.clone(),
                    requirement: Requirement {
                        ears,
                        entities,
                        edges,
                        source,
                        confidence: None,
                        reasoning: Self::opt_str(args, "reasoning"),
                        created: None,
                        updated: None,
                    },
                })?;
                Ok(json!({"id": id, "created": true}))
            }
            "update_requirement" => {
                let id = self.canon_req_id(&Self::str_arg(args, "id")?)?;
                let ears = Self::opt_str(args, "ears");
                if let Some(e) = &ears {
                    ears_shape(e)?;
                }
                let entities = match args["entities"].as_array() {
                    Some(_) => {
                        let mut canon: Vec<String> = Vec::new();
                        for e in Self::str_list(args, "entities") {
                            match self.canon_entity_id(&e) {
                                Some(id) => {
                                    if !canon.contains(&id) {
                                        canon.push(id);
                                    }
                                }
                                None => return Err(self.unknown_entity_error(&e)),
                            }
                        }
                        Some(canon)
                    }
                    None => None,
                };
                let mut edges: Option<Vec<ReqEdge>> = None;
                if let Some(arr) = args["edges"].as_array() {
                    let mut v = Vec::new();
                    for e in arr {
                        let t = e["type"].as_str().map(|s| s.to_string());
                        if let Some(t) = &t {
                            if !REL_TYPES.contains(&t.as_str()) {
                                return Err(ToolError::new(
                                    "bad-edge",
                                    format!("unknown relationship type `{}`; one of: {}", t, REL_TYPES.join(", ")),
                                ));
                            }
                        }
                        let raw_a = e["a"].as_str().unwrap_or_default();
                        let raw_b = e["b"].as_str().unwrap_or_default();
                        v.push(ReqEdge {
                            a: self.canon_entity_id(raw_a).unwrap_or_else(|| raw_a.to_string()),
                            b: self.canon_entity_id(raw_b).unwrap_or_else(|| raw_b.to_string()),
                            rel_type: t,
                        });
                    }
                    edges = Some(v);
                }
                self.stage(Op::UpdateRequirement { id: id.clone(), ears, entities, edges })?;
                Ok(json!({"id": id, "updated": true}))
            }
            "delete_requirement" => {
                let id = self.canon_req_id(&Self::str_arg(args, "id")?)?;
                let reason = Self::str_arg(args, "reason")?;
                self.stage(Op::DeleteRequirement { id, reason })?;
                Ok(json!({"deleted": true}))
            }
            "report_diagnostic" => {
                let rule = Self::str_arg(args, "rule")?;
                const REVIEW_RULES: [&str; 5] = ["contradiction", "duplicate-entity", "missing-link", "ambiguity", "lint"];
                if !REVIEW_RULES.contains(&rule.as_str()) {
                    return Err(ToolError::new(
                        "bad-rule",
                        format!("rule `{}` is not in the catalog; use one of: {}", rule, REVIEW_RULES.join(", ")),
                    ));
                }
                let severity = Self::str_arg(args, "severity")?;
                if !["error", "warning", "info", "none"].contains(&severity.as_str()) {
                    return Err(ToolError::new("bad-severity", format!("severity `{}` must be error, warning, info, or none", severity)));
                }
                let subjects = Self::str_list(args, "subjects");
                if subjects.is_empty() {
                    return Err(ToolError::new("no-subjects", "a diagnostic needs at least one subject node id".into()));
                }
                for s in &subjects {
                    let ok = self.known_entity(s)
                        || self.snapshot.graph.requirements.contains_key(s)
                        || self.staged_reqs.contains(s)
                        || split_section_ref(s)
                            .map(|(d, r)| self.snapshot.docs.get(&d).map(|rec| rec.sections.contains_key(&r)).unwrap_or(false))
                            .unwrap_or(false);
                    if !ok {
                        return Err(ToolError::new("unknown-id", format!("diagnostic subject `{}` does not exist", s)));
                    }
                }
                let message = Self::str_arg(args, "message")?;
                self.stage(Op::ReportDiagnostic {
                    id: String::new(),
                    diagnostic: Diagnostic {
                        rule,
                        severity,
                        subjects,
                        message,
                        reasoning: Self::opt_str(args, "reasoning"),
                        lifecycle: "open".to_string(),
                        triage: None,
                        created: None,
                        updated: None,
                    },
                })?;
                Ok(json!({"reported": true}))
            }
            "resolve_diagnostic" => {
                let id = Self::str_arg(args, "id")?;
                let reason = Self::str_arg(args, "reason")?;
                if !self.snapshot.graph.diagnostics.contains_key(&id) {
                    return Err(ToolError::new("unknown-id", format!("unknown diagnostic id `{}`", id)));
                }
                self.stage(Op::ResolveDiagnostic { id, reason })?;
                Ok(json!({"resolved": true}))
            }
            "set_coverage" => {
                let section = Self::str_arg(args, "section")?;
                let state = Self::str_arg(args, "state")?;
                if !["covered", "non-normative"].contains(&state.as_str()) {
                    return Err(ToolError::new("bad-state", format!("state `{}` must be covered or non-normative", state)));
                }
                // A placeholder note counts as absent; weak models emit these literally.
                let note = Self::opt_str(args, "note")
                    .filter(|n| !matches!(n.trim().to_lowercase().as_str(), "<nil>" | "nil" | "null" | "none" | "n/a" | "na" | "-"));
                if state == "non-normative" && note.is_none() {
                    return Err(ToolError::new("note-required", "non-normative requires a note saying why the section states no requirements".into()));
                }
                let (doc, sec) = self.resolve_section(&section)?;
                if self.scope.task == "reconcile-doc" && !self.scope.target_sections.is_empty() && !self.scope.target_sections.contains(&sec) {
                    return Err(ToolError::new(
                        "wrong-section",
                        format!("{} is not one of this turn's dirty sections ({})", sec, self.scope.target_sections.join(", ")),
                    ));
                }
                // One coverage mark per section per changeset: restaging replaces the
                // earlier mark instead of journaling contradictory states.
                self.staged
                    .retain(|op| !matches!(op, Op::SetCoverage { doc: d, section: s, .. } if d == &doc && s == &sec));
                self.stage(Op::SetCoverage { doc, section: sec, state, note })?;
                Ok(json!({"set": true}))
            }
            "gen_instructions" => {
                Ok(json!({"instructions": crate::gen::instructions()}))
            }
            "gen_pending" => {
                let gs = self.gen_settings();
                Ok(json!(crate::gen::pending(&self.snapshot, &gs)))
            }
            "gen_task" => {
                let entity = Self::str_arg(args, "entity")?;
                let gs = self.gen_settings();
                let id = self.snapshot.resolve_id(&entity).to_string();
                crate::gen::task_package(&self.snapshot, &id, &gs)
                    .map_err(|e| ToolError::new("unknown-id", e))
            }
            "gen_mark" => {
                let entity = Self::str_arg(args, "entity")?;
                let gs = self.gen_settings();
                let id = self.snapshot.resolve_id(&entity).to_string();
                let Some(seen) = Self::opt_str(args, "factHash") else {
                    return Err(ToolError::new(
                        "bad-argument",
                        "factHash is required; pass the factHash from the gen_task package".into(),
                    ));
                };
                if !args["manifest"].is_object() {
                    return Err(ToolError::new(
                        "bad-argument",
                        "manifest is required: {files: [...], tests: [{requirement, kind, label, artifact, name, run}]}".into(),
                    ));
                }
                crate::gen::mark(&self.snapshot, &id, Some(seen.as_str()), &args["manifest"], &gs)
                    .map_err(|e| ToolError::new("unknown-id", e))
            }
            "verify_pending" => {
                let gs = self.gen_settings();
                let filter = Self::opt_str(args, "filter");
                let entity = Self::opt_str(args, "entity");
                Ok(json!(crate::verify::pending(&self.snapshot, &gs, filter.as_deref(), entity.as_deref())))
            }
            "verify_task" => {
                let rid = Self::str_arg(args, "requirement")?;
                let gs = self.gen_settings();
                crate::verify::task(&self.snapshot, &rid, &gs).map_err(|e| ToolError::new("unknown-id", e))
            }
            "verify_mark" => {
                let rid = Self::str_arg(args, "requirement")?;
                let verdict = Self::str_arg(args, "verdict")?;
                let gs = self.gen_settings();
                let seen = Self::opt_str(args, "factHash");
                let evidence = Self::opt_str(args, "evidence");
                crate::verify::mark(&self.snapshot, &rid, &verdict, seen.as_deref(), evidence.as_deref(), &gs)
                    .map_err(|e| ToolError::new("bad-argument", e))
            }
            "done" => {
                // Batch gate: stale anchors are a contract. Each must be re-anchored
                // (its quote locates again), re-recorded under its natural key, revised,
                // or deleted; a turn cannot mark coverage around them and walk away.
                let mut untouched: Vec<String> = Vec::new();
                for a in &self.scope.stale_anchors {
                    let Some(r) = self.snapshot.graph.requirements.get(a) else { continue };
                    if self.snapshot.quote_locates(&r.source.doc, &r.source.section, &r.source.quote) {
                        continue;
                    }
                    let addressed = self.staged.iter().any(|o| match o {
                        Op::UpdateRequirement { id, .. } | Op::DeleteRequirement { id, .. } => id == a,
                        Op::CreateRequirement { requirement, .. } => {
                            requirement.source.doc == r.source.doc
                                && requirement.source.section == r.source.section
                                && crate::store::normalize_statement(&requirement.ears)
                                    == crate::store::normalize_statement(&r.ears)
                        }
                        _ => false,
                    });
                    if !addressed {
                        untouched.push(a.clone());
                    }
                }
                if !untouched.is_empty() {
                    return Err(ToolError::new(
                        "stale-anchor",
                        format!(
                            "stale anchors left untouched: {}; for each, re-record the statement with upsert_requirement using a fresh verbatim quote (the same statement updates in place), or delete_requirement if the document no longer states it",
                            untouched.join(", ")
                        ),
                    ));
                }
                // Batch gate: a `covered` claim is honest only when a requirement is
                // sourced from that section; a section with nothing to extract is
                // non-normative with a note, never silently covered. This stops a turn
                // from dropping a rejected requirement and claiming the section anyway,
                // and from skimming past declarative prose without extracting.
                for op in &self.staged {
                    if let Op::SetCoverage { doc, section, state, .. } = op {
                        if state != "covered" {
                            continue;
                        }
                        let has_req = self
                            .snapshot
                            .graph
                            .requirements
                            .values()
                            .any(|r| &r.source.doc == doc && &r.source.section == section)
                            || self.staged.iter().any(|o| match o {
                                Op::CreateRequirement { requirement, .. } => {
                                    &requirement.source.doc == doc && &requirement.source.section == section
                                }
                                _ => false,
                            });
                        if !has_req {
                            return Err(ToolError::new(
                                "uncovered-claim",
                                format!(
                                    "{}#{} is claimed covered but no requirement is sourced from it; extract from its sentences (rephrase into a shall statement, keep the quote verbatim), or mark the section non-normative with a note",
                                    doc, section
                                ),
                            ));
                        }
                    }
                }
                let summary = Self::opt_str(args, "summary").unwrap_or_default();
                self.done = Some(summary);
                Ok(json!({"ok": true}))
            }
            other => Err(ToolError::new("unknown-tool", format!("unknown tool `{}`", other))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn session() -> ToolSession {
        let mut s = Store::default();
        let text = "# Shop\nintro text\n\n## Cart\nThe Shopping Cart holds items a Customer intends to buy.\n";
        s.docs.insert(
            "shop.md".into(),
            DocRecord { content_hash: hash_hex(text), sections: crate::md::parse_sections(text), coverage: BTreeMap::new() },
        );
        s.graph.entities.insert(
            "ent:customer".into(),
            Entity { name: "Customer".into(), definition: Some("a person who buys".into()), ..Default::default() },
        );
        ToolSession::new(
            s,
            WorkScope {
                task: "reconcile-doc".into(),
                doc: Some("shop.md".into()),
                target_sections: vec!["/shop".into(), "/shop/cart".into()],
                stale_anchors: Vec::new(),
            },
            64,
            24_000,
        )
    }

    #[test]
    fn junk_names_rejected_with_repair_hint() {
        let mut t = session();
        let err = t
            .dispatch("upsert_entity", &json!({"name": "--api-key", "mention": {"section": "/shop/cart", "quote": "holds items"}}))
            .unwrap_err();
        assert_eq!(err.rule, "junk-name");
        assert!(err.message.contains("note"));
        // Operation identifiers are requirement detail, not entities.
        let err2 = t
            .dispatch("upsert_entity", &json!({"name": "createUser", "mention": {"section": "/shop/cart", "quote": "holds items"}}))
            .unwrap_err();
        assert_eq!(err2.rule, "junk-name");
        assert!(err2.message.contains("operation"), "{}", err2.message);
    }

    #[test]
    fn quote_must_locate() {
        let mut t = session();
        let err = t
            .dispatch("upsert_entity", &json!({"name": "Shopping Cart", "mention": {"section": "/shop/cart", "quote": "this text is not there"}}))
            .unwrap_err();
        assert_eq!(err.rule, "quote-not-found");
    }

    #[test]
    fn upsert_reuses_existing_natural_key() {
        let mut t = session();
        let v = t
            .dispatch("upsert_entity", &json!({"name": "customer", "mention": {"section": "/shop/cart", "quote": "a Customer intends to buy"}}))
            .unwrap();
        assert_eq!(v["id"], "ent:customer");
        assert_eq!(v["created"], false);
    }

    #[test]
    fn requirement_needs_known_entities_and_shall() {
        let mut t = session();
        let err = t
            .dispatch("upsert_requirement", &json!({"ears": "The cart is nice.", "entities": ["ent:cart"], "section": "/shop/cart", "quote": "holds items"}))
            .unwrap_err();
        assert_eq!(err.rule, "not-ears");
        let err2 = t
            .dispatch("upsert_requirement", &json!({"ears": "The Cart shall hold items.", "entities": ["ent:cart"], "section": "/shop/cart", "quote": "holds items"}))
            .unwrap_err();
        assert_eq!(err2.rule, "unknown-id");
        assert!(err2.message.contains("upsert_entity"), "repair hint: {}", err2.message);
    }

    #[test]
    fn prefixed_case_variant_resolves() {
        let mut t = session();
        let v = t.dispatch("update_entity", &json!({"id": "ent:Customer", "add_aliases": ["Buyer"]})).unwrap();
        assert_eq!(v["id"], "ent:customer");
    }

    #[test]
    fn update_requirement_runs_the_shape_gate() {
        let mut t = session();
        t.dispatch(
            "upsert_entity",
            &json!({"name": "Shopping Cart", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}}),
        )
        .unwrap();
        let r = t
            .dispatch(
                "upsert_requirement",
                &json!({"ears": "The Shopping Cart shall hold items.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "holds items a Customer intends to buy"}),
            )
            .unwrap();
        let rid = r["id"].as_str().unwrap().to_string();
        // A revision is not a side door around the shape gate: junk that upsert would
        // bounce (the req:tools-15 corruption: an edges JSON array as the statement)
        // bounces here too.
        let err = t.dispatch("update_requirement", &json!({"id": rid, "ears": "[{\"a\": \"ent:task-type\"}]"})).unwrap_err();
        assert_eq!(err.rule, "not-ears");
    }

    #[test]
    fn implicit_done_drops_dishonest_coverage_and_commits_the_rest() {
        let mut t = session();
        t.dispatch(
            "upsert_entity",
            &json!({"name": "Shopping Cart", "definition": "holds items", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}}),
        )
        .unwrap();
        // A covered claim with no requirement sourced from the section is dishonest;
        // the explicit done bounces it.
        t.dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "covered"})).unwrap();
        assert!(t.dispatch("done", &json!({"summary": "covered"})).is_err());
        // The implicit done drops the offending mark and commits the rest.
        assert!(t.finish_implicit("(implicit: test)"));
        assert!(t.done.is_some());
        assert!(t.staged.iter().any(|op| matches!(op, Op::CreateEntity { .. })));
        assert!(!t.staged.iter().any(|op| matches!(op, Op::SetCoverage { state, .. } if state == "covered")));
    }

    #[test]
    fn full_flow_stages_ops_and_done() {
        let mut t = session();
        let e = t
            .dispatch("upsert_entity", &json!({"name": "Shopping Cart", "definition": "holds items", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}}))
            .unwrap();
        let id = e["id"].as_str().unwrap().to_string();
        assert_eq!(id, "ent:shopping-cart");
        t.dispatch(
            "upsert_requirement",
            &json!({"ears": "The Shopping Cart shall hold items a Customer intends to buy.", "entities": [id, "ent:customer"], "section": "/shop/cart", "quote": "holds items a Customer intends to buy", "edges": [{"a": "ent:shopping-cart", "b": "ent:customer", "type": "association"}]}),
        )
        .unwrap();
        t.dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "covered"})).unwrap();
        let err = t.dispatch("set_coverage", &json!({"section": "/nope", "state": "covered"})).unwrap_err();
        assert_eq!(err.rule, "unknown-section");
        t.dispatch("done", &json!({"summary": "reconciled cart"})).unwrap();
        assert!(t.done.is_some());
        assert_eq!(t.staged.len(), 3);
    }

    #[test]
    fn edges_must_be_subset_of_entities() {
        let mut t = session();
        t.dispatch("upsert_entity", &json!({"name": "Shopping Cart", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}})).unwrap();
        let err = t
            .dispatch(
                "upsert_requirement",
                &json!({"ears": "The Shopping Cart shall exist.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "holds items", "edges": [{"a": "ent:shopping-cart", "b": "ent:customer"}]}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "bad-edge");
    }

    #[test]
    fn lenient_entity_refs_resolve_unambiguously() {
        let mut t = session();
        // Prefix-less id and exact display name both resolve to ent:customer.
        let v = t
            .dispatch(
                "upsert_requirement",
                &json!({"ears": "The Customer shall buy items.", "entities": ["customer"], "section": "/shop/cart", "quote": "a Customer intends to buy"}),
            )
            .unwrap();
        assert_eq!(v["created"], true);
        let v2 = t
            .dispatch(
                "upsert_requirement",
                &json!({"ears": "The Customer shall intend to buy.", "entities": ["Customer"], "section": "/shop/cart", "quote": "intends to buy"}),
            )
            .unwrap();
        assert_eq!(v2["created"], true);
        match t.staged.iter().find(|o| matches!(o, Op::CreateRequirement { .. })).unwrap() {
            Op::CreateRequirement { requirement, .. } => assert_eq!(requirement.entities, vec!["ent:customer".to_string()]),
            _ => unreachable!(),
        }
    }

    #[test]
    fn escaped_quote_locates_and_stores_source_form() {
        let mut t = session();
        let v = t
            .dispatch(
                "upsert_requirement",
                &json!({"ears": "The Customer shall intend to buy.", "entities": ["ent:customer"], "section": "/shop/cart", "quote": "a Customer intends to buy\\."}),
            )
            .unwrap();
        assert_eq!(v["created"], true);
        match t.staged.iter().find(|o| matches!(o, Op::CreateRequirement { .. })).unwrap() {
            Op::CreateRequirement { requirement, .. } => assert_eq!(requirement.source.quote, "a Customer intends to buy."),
            _ => unreachable!(),
        }
    }

    #[test]
    fn coverage_restage_replaces_earlier_mark() {
        let mut t = session();
        t.dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "non-normative", "note": "just prose"})).unwrap();
        t.dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "covered"})).unwrap();
        let marks: Vec<&Op> = t.staged.iter().filter(|o| matches!(o, Op::SetCoverage { .. })).collect();
        assert_eq!(marks.len(), 1);
        match marks[0] {
            Op::SetCoverage { state, .. } => assert_eq!(state, "covered"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn placeholder_note_counts_as_absent() {
        let mut t = session();
        let err = t
            .dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "non-normative", "note": "<nil>"}))
            .unwrap_err();
        assert_eq!(err.rule, "note-required");
    }

    // A session whose snapshot holds a requirement quoting text the document no longer
    // contains, listed as a stale anchor in the work scope.
    fn session_with_stale_anchor() -> ToolSession {
        let mut s = Store::default();
        let text = "# Shop\nintro text\n\n## Cart\nThe Shopping Cart keeps items a Customer intends to buy.\n";
        s.docs.insert(
            "shop.md".into(),
            DocRecord { content_hash: hash_hex(text), sections: crate::md::parse_sections(text), coverage: BTreeMap::new() },
        );
        s.graph.entities.insert(
            "ent:shopping-cart".into(),
            Entity { name: "Shopping Cart".into(), ..Default::default() },
        );
        s.graph.requirements.insert(
            "req:shop-1".into(),
            Requirement {
                ears: "The Shopping Cart shall hold items a Customer intends to buy.".into(),
                entities: vec!["ent:shopping-cart".into()],
                edges: Vec::new(),
                source: SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop/cart".into(),
                    quote: "The Shopping Cart holds items a Customer intends to buy.".into(),
                },
                confidence: None,
                reasoning: None,
                created: None,
                updated: None,
            },
        );
        ToolSession::new(
            s,
            WorkScope {
                task: "reconcile-doc".into(),
                doc: Some("shop.md".into()),
                target_sections: vec!["/shop/cart".into()],
                stale_anchors: vec!["req:shop-1".into()],
            },
            64,
            24_000,
        )
    }

    #[test]
    fn done_rejects_untouched_stale_anchor() {
        let mut t = session_with_stale_anchor();
        t.dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "covered"})).unwrap();
        let err = t.dispatch("done", &json!({"summary": "covered around the anchor"})).unwrap_err();
        assert_eq!(err.rule, "stale-anchor");
        assert!(err.message.contains("req:shop-1"), "names the anchor: {}", err.message);
    }

    #[test]
    fn stale_anchor_satisfied_by_delete() {
        let mut t = session_with_stale_anchor();
        t.dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "covered"})).unwrap();
        t.dispatch("delete_requirement", &json!({"id": "req:shop-1", "reason": "the document dropped the statement"})).unwrap();
        t.dispatch("done", &json!({"summary": "anchor deleted"})).unwrap();
        assert!(t.done.is_some());
    }

    #[test]
    fn stale_anchor_satisfied_by_same_statement_reupsert() {
        let mut t = session_with_stale_anchor();
        // Same statement, fresh verbatim quote: the natural key updates it in place at commit.
        t.dispatch(
            "upsert_requirement",
            &json!({"ears": "The Shopping Cart shall hold items a Customer intends to buy.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "The Shopping Cart keeps items a Customer intends to buy."}),
        )
        .unwrap();
        t.dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "covered"})).unwrap();
        t.dispatch("done", &json!({"summary": "re-anchored"})).unwrap();
        assert!(t.done.is_some());
    }
}
