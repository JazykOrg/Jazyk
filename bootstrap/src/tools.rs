// The tool registry: the graph's only interface for models. One registry, served
// in-process to the turn harness and over stdio as the MCP server.
// Mirrors docs/compiler/tools.md.
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
    pub fn new(rule: &str, message: String) -> ToolError {
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
            description: "Load a bounded context pack around a target: an entity id (ent:...), a requirement id (req:...), or a section reference (doc.md#/ref). Returns the pack plus expansion handles for what was cut off.",
            parameters: obj(json!({"target": {"type": "string"}}), &["target"]),
        },
        ToolDef {
            name: "expand",
            description: "Load the frontier behind an expansion handle returned by a previous context call.",
            parameters: obj(json!({"handle": {"type": "string"}}), &["handle"]),
        },
        ToolDef {
            name: "search",
            description: "Look up entities by name or alias before creating one. Returns up to 8 possible matches; hits can be false neighbors (substrings, shared words), so inspect each before reusing it.",
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
            name: "diagnostics",
            description: "List diagnostics: id, rule, severity, lifecycle, triage, subjects, message. Open ones by default; lifecycle takes open, resolved, or all; rule and subject narrow further.",
            parameters: obj(
                json!({"lifecycle": {"type": "string", "enum": ["open", "resolved", "all"]}, "rule": {"type": "string"}, "subject": {"type": "string"}}),
                &[],
            ),
        },
        ToolDef {
            name: "upsert_entity",
            description: "Create a domain concept, or update it if the name already exists. mention cites the section and the verbatim quote that talks about it. Not for file paths, CLI flags, or markdown terms. Leave scope unset unless the documents name a bounded context. A name that reads as a variant of an existing entity is rejected unless note names how the concepts differ (a field, part, or state of X is a different concept than X).",
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
            description: "Record one EARS requirement (a single testable statement using 'shall'). entities are the entity ids the statement is about. quote is the verbatim source sentence copied from the section. Safe to retry: re-recording the same statement updates it in place. edges optionally tie two of the entities with a relationship type, one of: generalization, realization, composition, aggregation, association, dependency, reference.",
            parameters: obj(
                json!({
                    "ears": {"type": "string"},
                    "entities": {"type": "array", "items": {"type": "string"}},
                    "section": {"type": "string"},
                    "quote": {"type": "string"},
                    "edges": {"type": "array", "items": {"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "string"}, "type": {"type": "string", "enum": ["generalization", "realization", "composition", "aggregation", "association", "dependency", "reference"]}}, "required": ["a", "b"]}}
                }),
                &["ears", "entities", "section", "quote"],
            ),
        },
        ToolDef {
            name: "update_requirement",
            description: "Update an existing requirement's statement, entities, or edges. section plus quote (together) re-anchor the provenance; the quote must locate verbatim in the section.",
            parameters: obj(
                json!({
                    "id": {"type": "string"},
                    "ears": {"type": "string"},
                    "entities": {"type": "array", "items": {"type": "string"}},
                    "edges": {"type": "array", "items": {"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "string"}, "type": {"type": "string", "enum": ["generalization", "realization", "composition", "aggregation", "association", "dependency", "reference"]}}, "required": ["a", "b"]}},
                    "section": {"type": "string"},
                    "quote": {"type": "string"}
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
            name: "place_anchor",
            description: "Move one proposed anchor (a requirement or an entity mention named in this align task) to the section that now holds its text. quote, when given, must locate verbatim there and replaces the stored quote. reevaluate true lists the anchor for the extraction turn to re-judge; false keeps it as an unchanged statement.",
            parameters: obj(
                json!({
                    "id": {"type": "string"},
                    "section": {"type": "string"},
                    "quote": {"type": "string"},
                    "reevaluate": {"type": "boolean"}
                }),
                &["id", "section", "reevaluate"],
            ),
        },
        ToolDef {
            name: "orphan_anchor",
            description: "Leave one proposed anchor homeless: no candidate section states it any more. It stays a stale anchor for the extraction turn, which will delete it unless the document still states it.",
            parameters: obj(json!({"id": {"type": "string"}}), &["id"]),
        },
        ToolDef {
            name: "report_diagnostic",
            description: "Record a judgment about the graph or documents. Severity error only when two statements cannot both hold; warning for real but repairable issues; info for observations. prompt optionally attaches a question for the document owner: up to 4 options, each a label with exactly one of edit (a suggested prose edit, applied without a model) or answer (a prefilled reply), plus freeform for typed replies.",
            parameters: obj(
                json!({
                    "rule": {"type": "string", "enum": ["contradiction", "duplicate-entity", "duplicate-requirement", "missing-link", "ambiguity", "lint"]},
                    "severity": {"type": "string", "enum": ["error", "warning", "info"]},
                    "subjects": {"type": "array", "items": {"type": "string"}},
                    "message": {"type": "string"},
                    "reasoning": {"type": "string"},
                    "prompt": prompt_schema()
                }),
                &["rule", "severity", "subjects", "message"],
            ),
        },
        ToolDef {
            name: "update_diagnostic",
            description: "Replace the question attached to an open diagnostic (null prompt removes it). The finding itself is edited by re-reporting it; this tool only maintains the prompt. Never touches a human answer or triage.",
            parameters: obj(
                json!({"id": {"type": "string"}, "prompt": prompt_schema()}),
                &["id"],
            ),
        },
        ToolDef {
            name: "resolve_diagnostic",
            description: "Mark a diagnostic resolved: its condition no longer holds.",
            parameters: obj(json!({"id": {"type": "string"}, "reason": {"type": "string"}}), &["id", "reason"]),
        },
        ToolDef {
            name: "set_coverage",
            description: "Mark a section covered (its content is reflected in the graph) or non-normative (it states no requirements). Setting state to non-normative requires a note saying why.",
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
            name: "report_feedback",
            description: "Report that jazyk's own prompt, tool, schema, or error message is ambiguous, wrong, confusing, or missing something. This reaches jazyk's developers, not this project's authors. It never touches the graph. Use it for what blocked YOU; problems in the documents are diagnostics, not feedback. kind: ambiguous, wrong, confusing, missing, or other. subject names the tool, argument, or instruction. message says what was unclear and what would have helped. Then continue the task with your best judgment.",
            parameters: obj(
                json!({
                    "kind": {"type": "string", "enum": ["ambiguous", "wrong", "confusing", "missing", "other"]},
                    "subject": {"type": "string"},
                    "message": {"type": "string"}
                }),
                &["kind", "message"],
            ),
        },
        ToolDef {
            name: "generation_tasks",
            description: "Entities whose facts differ from the ledger, each with the requirement ids added, removed, or reworded since the entity was last generated. Zero tasks means generation is current. Next: begin_generation on one entity.",
            parameters: obj(json!({}), &[]),
        },
        ToolDef {
            name: "begin_generation",
            description: "The full generation package for one entity: instructions (the generation contract), context pack, requirement groups (with suggested test names), change diff, the deliverable directory, factHash, the manifest of already generated files with what each holds, the medium, and the build. Write the deliverable files with your own tools, then record_generation with the manifest.",
            parameters: obj(json!({"entity": {"type": "string"}}), &["entity"]),
        },
        ToolDef {
            name: "record_generation",
            description: "Record the task done. manifest.files lists every deliverable-relative file written; manifest.tests binds each requirement to its test: {requirement, kind: programmatic|llm, label, artifact, name, run, cwd?, files?}. For an llm row, artifact is the criteria file you wrote (front matter with the requirement id and statement hash, the statement, the quote, the implementing paths, the confirm steps, the verdict contract), path relative to the deliverable. Recording an identical manifest keeps existing verdicts. manifest.build is the one command that produces the deliverable's artifact when its medium must be built (a slide deck, a PDF, a rendered site, a binary): {run, cwd?, produces: [paths]}, run from the deliverable directory before any test; omit it when the written files are themselves the deliverable, and reuse the build the begin_generation package already carries rather than recording a second one. Pass the factHash from the begin_generation package. Next: run_tests to verify.",
            parameters: obj(
                json!({
                    "entity": {"type": "string"},
                    "factHash": {"type": "string"},
                    "manifest": {"type": "object", "properties": {
                        "files": {"type": "array", "items": {"type": "string"}},
                        "tests": {"type": "array", "items": {"type": "object"}},
                        "build": {"type": "object", "properties": {
                            "run": {"type": "string"},
                            "cwd": {"type": "string"},
                            "produces": {"type": "array", "items": {"type": "string"}}
                        }}
                    }}
                }),
                &["entity", "factHash", "manifest"],
            ),
        },
        ToolDef {
            name: "binding_tasks",
            description: "Requirements owing a binding, with a reason (unbound, requirement-changed, artifact-gone). A binding ties one requirement to the deliverable: the implementing files (possibly none), the test that judges it, and the verdict. Deterministic; no model involved. Next: begin_binding on one requirement.",
            parameters: obj(json!({}), &[]),
        },
        ToolDef {
            name: "begin_binding",
            description: "The bind package for one requirement: instructions (the bind contract), statement, quote, factHash, context pack, the deliverable directory, the suggested test name, the decided medium and build when they exist, and the recorded test conventions. Search the deliverable with your own tools: find the implementing files (or none), find or write the test, run it, then record_binding.",
            parameters: obj(json!({"requirement": {"type": "string"}}), &["requirement"]),
        },
        ToolDef {
            name: "record_binding",
            description: "Record the binding: files lists the deliverable-relative implementing files (an empty list is the honest record of an unimplemented requirement); test is {kind: programmatic|llm, label, artifact, name, run, cwd?}; verdict is the test's outcome (pass|fail) with evidence. The row's derived status classifies the requirement: verified (the deliverable already satisfies it), unimplemented (fail with no files: generation work, the test is its acceptance gate), failing (fail with files: the deliverable contradicts the statement, a diagnostic for the author). Never rewrite implementation files during binding.",
            parameters: obj(
                json!({
                    "requirement": {"type": "string"},
                    "files": {"type": "array", "items": {"type": "string"}},
                    "test": {"type": "object", "properties": {
                        "kind": {"type": "string", "enum": ["programmatic", "llm"]},
                        "label": {"type": "string"},
                        "artifact": {"type": "string"},
                        "name": {"type": "string"},
                        "run": {"type": "string"},
                        "cwd": {"type": "string"}
                    }},
                    "verdict": {"type": "string", "enum": ["pass", "fail"]},
                    "evidence": {"type": "string"}
                }),
                &["requirement", "files", "test", "verdict"],
            ),
        },
        ToolDef {
            name: "verification_tasks",
            description: "Ledger rows needing action, with derived status (missing, stale-requirement, stale-test, stale-code, failing, unverified) and reason. Deterministic; no model involved. Next: run_tests for programmatic rows, begin_verification for llm rows.",
            parameters: obj(json!({"filter": {"type": "string", "enum": ["stale", "failing", "all"]}, "entity": {"type": "string"}}), &[]),
        },
        ToolDef {
            name: "begin_verification",
            description: "The verification package for one requirement: statement, quote, factHash, context pack, implementing files, and either the run command (programmatic) or the criteria (llm). For llm rows, judge the criteria against the deliverable and record_verdict.",
            parameters: obj(json!({"requirement": {"type": "string"}}), &["requirement"]),
        },
        ToolDef {
            name: "run_tests",
            description: "Run the recorded programmatic tests: the build once, then each selected row's command, verdicts recorded as a side effect. requirements selects rows (requirement or entity ids); empty runs every non-verified row. llm rows are skipped here; judge them and record_verdict.",
            parameters: obj(json!({"requirements": {"type": "array", "items": {"type": "string"}}}), &[]),
        },
        ToolDef {
            name: "record_verdict",
            description: "Record a pass or fail verdict with evidence, for a row you judged (llm rows; run_tests records programmatic rows itself). Pass the factHash from the begin_verification package; if the graph moved meanwhile the verdict is recorded but the row stays pending.",
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
        // The generation turn's file and command tools: in-process only, never served
        // over MCP (an external agent brings its own editor). Names and shapes track
        // the Agent Client Protocol's file-system and terminal methods.
        // Mirrors docs/compiler/turns.md#generation-turns.
        ToolDef {
            name: "read_text_file",
            description: "One deliverable file's content. path is relative to the deliverable directory; line (1-based) and limit select a slice of a large file.",
            parameters: obj(
                json!({"path": {"type": "string"}, "line": {"type": "integer"}, "limit": {"type": "integer"}}),
                &["path"],
            ),
        },
        ToolDef {
            name: "write_text_file",
            description: "Write one deliverable file, creating parent directories. path is relative to the deliverable directory. A path recorded for another entity is rejected with the owner named; reference other entities' files instead of rewriting them.",
            parameters: obj(json!({"path": {"type": "string"}, "content": {"type": "string"}}), &["path", "content"]),
        },
        ToolDef {
            name: "list_files",
            description: "The deliverable's file tree, paths relative to the deliverable directory. path narrows to a subdirectory.",
            parameters: obj(json!({"path": {"type": "string"}}), &[]),
        },
        ToolDef {
            name: "run_command",
            description: "Execute a shell command under the deliverable directory (cwd is deliverable-relative, default .). Returns the exit code and output tail. Use it to run the build you wrote and read its failures; record the final commands in the manifest.",
            parameters: obj(json!({"command": {"type": "string"}, "cwd": {"type": "string"}}), &["command"]),
        },
        ToolDef {
            name: "done",
            description: "End the turn and request commit of the staged mutations. summary says what was done.",
            parameters: obj(json!({"summary": {"type": "string"}}), &["summary"]),
        },
    ]
}

pub const READ_TOOLS: [&str; 6] = ["context", "expand", "search", "read_section", "get_entity", "diagnostics"];
pub const GEN_TOOLS: [&str; 3] = ["generation_tasks", "begin_generation", "record_generation"];
pub const BIND_TOOLS: [&str; 3] = ["binding_tasks", "begin_binding", "record_binding"];
pub const VERIFY_TOOLS: [&str; 4] = ["verification_tasks", "begin_verification", "run_tests", "record_verdict"];
// In-process only: a generation turn's file and command tools, never served over MCP.
pub const FILE_TOOLS: [&str; 4] = ["read_text_file", "write_text_file", "list_files", "run_command"];
// Feedback about jazyk itself, not about the graph: served in every toolset, read-only
// MCP included. See docs/compiler/tools.md#feedback-tool.
pub const FEEDBACK_TOOL: &str = "report_feedback";
// Feedback entries one session may record. Past it the call is acknowledged without a
// record, so a confused model cannot flood the log.
pub const FEEDBACK_LIMIT: usize = 5;

pub fn toolset(task: &str) -> Vec<&'static str> {
    let mut v = match task {
        "align-doc" => vec![
            "context", "expand", "search", "read_section", "get_entity", "place_anchor", "orphan_anchor", "done",
        ],
        "reconcile-doc" => vec![
            "context", "expand", "search", "read_section", "upsert_entity", "update_entity", "delete_entity",
            "upsert_requirement", "update_requirement", "delete_requirement", "set_coverage", "done",
        ],
        "review-requirement" => vec![
            "context", "expand", "search", "get_entity", "read_section", "diagnostics", "update_requirement",
            "delete_requirement", "report_diagnostic", "update_diagnostic", "resolve_diagnostic", "done",
        ],
        "review-entity" => vec![
            "context", "expand", "search", "get_entity", "diagnostics", "update_entity", "merge_entities",
            "update_requirement", "delete_requirement", "report_diagnostic", "update_diagnostic", "resolve_diagnostic", "done",
        ],
        // The in-process generation worker: the read tools, the file and command
        // tools, and the generation lifecycle. Mirrors docs/compiler/turns.md#generation-turns.
        "generate-entity" => {
            let mut v = READ_TOOLS.to_vec();
            v.extend(FILE_TOOLS);
            v.extend(["begin_generation", "record_generation", "run_tests", "done"]);
            v
        }
        // The in-process bind worker: search the deliverable, find or write the test,
        // record the row. Mirrors docs/consumers/bind.md#the-bind-task.
        "bind-requirement" => {
            let mut v = READ_TOOLS.to_vec();
            v.extend(FILE_TOOLS);
            v.extend(["record_binding", "done"]);
            v
        }
        // MCP servings. Mirrors docs/compiler/tools.md#task-toolsets.
        "mcp-generate" => {
            let mut v = READ_TOOLS.to_vec();
            v.extend(BIND_TOOLS);
            v.extend(GEN_TOOLS);
            v.push("run_tests");
            v
        }
        "mcp-verify" => {
            let mut v = READ_TOOLS.to_vec();
            v.extend(VERIFY_TOOLS);
            v
        }
        // The compile serving's write surface; the lifecycle tools live in the server.
        "mcp-compile" => vec![
            "context", "expand", "search", "read_section", "get_entity", "diagnostics", "upsert_entity",
            "update_entity", "delete_entity", "merge_entities", "upsert_requirement", "update_requirement",
            "delete_requirement", "set_coverage", "report_diagnostic", "update_diagnostic", "resolve_diagnostic",
            "place_anchor", "orphan_anchor",
        ],
        "mcp-write" => catalog()
            .iter()
            .map(|t| t.name)
            .filter(|n| *n != "done" && !FILE_TOOLS.contains(n))
            .collect(),
        _ => READ_TOOLS.to_vec(),
    };
    if !v.contains(&FEEDBACK_TOOL) {
        v.push(FEEDBACK_TOOL);
    }
    v
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

// The JSON schema of a diagnostic prompt argument, shared by report_diagnostic and
// update_diagnostic. Mirrors docs/compiler/model/diagnostic.md#prompts.
pub(crate) fn prompt_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "question": {"type": "string"},
            "options": {"type": "array", "maxItems": 4, "items": {"type": "object", "properties": {
                "label": {"type": "string"},
                "edit": {"type": "object", "properties": {
                    "doc": {"type": "string"},
                    "section": {"type": "string", "description": "the section, as /ref or the full doc.md#/ref"},
                    "old_text": {"type": "string"}, "new_text": {"type": "string"}},
                    "required": ["doc", "section", "old_text", "new_text"]},
                "answer": {"type": "string"}
            }, "required": ["label"]}},
            "freeform": {"type": "boolean"}
        },
        "required": ["question"]
    })
}

// The lenient EARS shape gate, shared by upsert_requirement and update_requirement so a
// revision cannot land a statement a fresh upsert would reject. Mirrors
// docs/compiler/concepts/ears.md#shape-check.
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
    // The work item's target: the entity id for a generate-entity turn (file
    // ownership checks name it), empty when the task has no single target.
    pub target: String,
    pub target_sections: Vec<String>,
    // Requirement ids whose quote stopped locating; the done gate holds the turn to
    // addressing each one. See docs/compiler/graph.md#validation-gates.
    pub stale_anchors: Vec<String>,
    // Anchor ids an align-doc turn must decide; place_anchor and orphan_anchor accept
    // no others, and done holds the turn to every one.
    pub proposals: Vec<String>,
}

// One turn's tool session: reads answer from the snapshot, writes stage into the changeset.
pub struct ToolSession {
    pub snapshot: Store,
    pub scope: WorkScope,
    pub staged: Vec<Op>,
    pub done: Option<String>,
    // Resolved [gen] settings for the generation and verification tools.
    pub gen: crate::gen::GenSettings,
    // Who is driving this session, recorded on every feedback entry so a record names
    // its caller. Set by the harness that owns the session.
    pub caller: crate::feedback::Caller,
    // Feedback entries this session already recorded; capped so a confused model
    // cannot flood the log (docs/compiler/tools.md#feedback-tool).
    feedback_count: usize,
    mutation_limit: usize,
    default_budget: usize,
    // Staged entities (id -> entity) so lookup-before-create sees this turn's own creates.
    staged_entities: std::collections::BTreeMap<String, Entity>,
    staged_reqs: BTreeSet<String>,
    taken_ids: BTreeSet<String>,
    // True only while finish_implicit drives `done`: the implicit path commits around
    // an unmarked dirty section instead of bouncing (docs/compiler/turns.md#budgets).
    implicit_done: bool,
}

impl ToolSession {
    pub fn new(snapshot: Store, scope: WorkScope, mutation_limit: usize, default_budget: usize) -> ToolSession {
        // Placeholder; sessions that reach the gen tools (MCP, run_turn) overwrite it
        // with the project-resolved settings.
        let gen = crate::gen::GenSettings::from_out(&snapshot.out);
        ToolSession {
            snapshot,
            scope,
            staged: Vec::new(),
            done: None,
            gen,
            caller: Default::default(),
            feedback_count: 0,
            mutation_limit,
            default_budget,
            staged_entities: Default::default(),
            staged_reqs: Default::default(),
            taken_ids: Default::default(),
            implicit_done: false,
        }
    }

    fn gen_settings(&self) -> crate::gen::GenSettings {
        self.gen.clone()
    }

    // The generation turn's file and command tools, sandboxed to the deliverable.
    // In-process only. Mirrors docs/compiler/turns.md#generation-turns.
    fn deliverable_path(&self, rel: &str) -> Result<std::path::PathBuf, ToolError> {
        let rel = rel.trim().trim_start_matches("./");
        if rel.is_empty() || rel.starts_with('/') || rel.split('/').any(|c| c == "..") {
            return Err(ToolError::new(
                "bad-path",
                format!("`{}` must be a relative path under the deliverable directory, without `..`", rel),
            ));
        }
        Ok(self.gen.deliverable.join(rel))
    }

    fn file_tool(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        match name {
            "read_text_file" => {
                let rel = Self::str_arg(args, "path")?;
                let path = self.deliverable_path(&rel)?;
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| ToolError::new("not-found", format!("cannot read `{}`: {}; list_files shows what exists", rel, e)))?;
                let start = args["line"].as_u64().map(|l| (l as usize).saturating_sub(1)).unwrap_or(0);
                let limit = args["limit"].as_u64().map(|l| l as usize).unwrap_or(usize::MAX);
                let lines: Vec<&str> = text.lines().skip(start).take(limit).collect();
                let total = text.lines().count();
                Ok(json!({"content": lines.join("\n"), "totalLines": total}))
            }
            "write_text_file" => {
                let rel = Self::str_arg(args, "path")?;
                let content = Self::str_arg(args, "content")?;
                let path = self.deliverable_path(&rel)?;
                // File ownership: a path recorded for another entity is off limits.
                // Mirrors docs/consumers/gen.md#file-ownership-and-conventions.
                let own = crate::gen::slug_of(&self.scope.target);
                let ledger = crate::gen::Ledger::load(&self.snapshot.out);
                if let Some((owner, _)) = ledger
                    .entities
                    .iter()
                    .find(|(slug, e)| slug.as_str() != own && e.files.iter().any(|f| f == &rel))
                {
                    return Err(ToolError::new(
                        "file-owned",
                        format!(
                            "`{}` belongs to entity `{}`; never write to another entity's file. Reference it (import, include, read) and pick a path of your own",
                            rel, owner
                        ),
                    ));
                }
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(&path, &content).map_err(|e| ToolError::new("io-error", format!("cannot write `{}`: {}", rel, e)))?;
                Ok(json!({"written": rel, "bytes": content.len()}))
            }
            "list_files" => {
                let rel = Self::opt_str(args, "path").unwrap_or_default();
                let root = if rel.is_empty() { self.gen.deliverable.clone() } else { self.deliverable_path(&rel)? };
                let mut out: Vec<String> = Vec::new();
                let mut stack = vec![root.clone()];
                while let Some(dir) = stack.pop() {
                    let Ok(entries) = std::fs::read_dir(&dir) else { continue };
                    for e in entries.flatten() {
                        let p = e.path();
                        let name = e.file_name().to_string_lossy().to_string();
                        if name.starts_with('.') || name == "target" || name == "node_modules" || name == "jazyk-out" {
                            continue;
                        }
                        if p.is_dir() {
                            stack.push(p);
                        } else if let Ok(r) = p.strip_prefix(&self.gen.deliverable) {
                            out.push(r.to_string_lossy().replace('\\', "/"));
                        }
                    }
                }
                out.sort();
                out.truncate(500);
                Ok(json!({"files": out}))
            }
            "run_command" => {
                let command = Self::str_arg(args, "command")?;
                let cwd_rel = Self::opt_str(args, "cwd").unwrap_or_else(|| ".".into());
                let cwd = if cwd_rel == "." { self.gen.deliverable.clone() } else { self.deliverable_path(&cwd_rel)? };
                let out = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&command)
                    .current_dir(&cwd)
                    .output()
                    .map_err(|e| ToolError::new("io-error", format!("cannot run `{}` in {}: {}", command, cwd.display(), e)))?;
                let mut text = String::from_utf8_lossy(&out.stdout).to_string();
                text.push_str(&String::from_utf8_lossy(&out.stderr));
                let tail: String = {
                    let lines: Vec<&str> = text.lines().collect();
                    let start = lines.len().saturating_sub(60);
                    lines[start..].join("\n")
                };
                Ok(json!({
                    "exitCode": out.status.code().unwrap_or(-1),
                    "output": crate::llm::truncate(&tail, 6000),
                }))
            }
            _ => unreachable!(),
        }
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
    // docs/compiler/graph.md#validation-gates.
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
    // Mirrors docs/compiler/graph.md#validation-gates.
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
    // docs/compiler/turns.md#budgets.
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

    // Parse and gate a diagnostic prompt argument: at most 4 options, label
    // required, exactly one of edit or answer per option, and an edit's old_text
    // must locate in its section. Mirrors docs/compiler/model/diagnostic.md#prompts.
    fn parse_prompt(&self, v: &Value) -> Result<Option<crate::model::DiagnosticPrompt>, ToolError> {
        use crate::model::{DiagnosticPrompt, PromptOption, SuggestedEdit};
        if v.is_null() {
            return Ok(None);
        }
        let Some(question) = v["question"].as_str().filter(|s| !s.trim().is_empty()) else {
            return Err(ToolError::new("bad-prompt", "prompt.question is required: one sentence addressed to a person".into()));
        };
        let mut options = Vec::new();
        if let Some(arr) = v["options"].as_array() {
            if arr.len() > 4 {
                return Err(ToolError::new("bad-prompt", "a prompt carries at most 4 options".into()));
            }
            for (i, o) in arr.iter().enumerate() {
                let Some(label) = o["label"].as_str().filter(|s| !s.trim().is_empty()) else {
                    return Err(ToolError::new("bad-prompt", format!("option {} needs a label", i)));
                };
                let has_edit = !o["edit"].is_null();
                let answer = o["answer"].as_str().filter(|s| !s.trim().is_empty());
                if has_edit == answer.is_some() {
                    return Err(ToolError::new(
                        "bad-prompt",
                        format!("option {} needs exactly one of edit or answer", i),
                    ));
                }
                let edit = if has_edit {
                    let e = &o["edit"];
                    let get = |k: &str| -> Result<String, ToolError> {
                        e[k].as_str()
                            .filter(|s| !s.trim().is_empty())
                            .map(|s| s.to_string())
                            .ok_or_else(|| ToolError::new("bad-prompt", format!("option {}: edit.{} is required", i, k)))
                    };
                    let (doc, mut section) = (get("doc")?, get("section")?);
                    // Both reference forms are accepted: `/ref` and the full
                    // `doc.md#/ref` the packs display.
                    if let Some((sd, sr)) = crate::model::split_section_ref(&section) {
                        if sd == doc {
                            section = sr;
                        } else {
                            return Err(ToolError::new(
                                "bad-prompt",
                                format!("option {}: edit.section names document `{}` but edit.doc says `{}`", i, sd, doc),
                            ));
                        }
                    }
                    let (old_text, new_text) = (get("old_text")?, get("new_text")?);
                    let Some(rec) = self.snapshot.docs.get(&doc) else {
                        return Err(ToolError::new("bad-prompt", format!("option {}: unknown document `{}`", i, doc)));
                    };
                    let Some(sec) = rec.sections.get(&section) else {
                        return Err(ToolError::new("bad-prompt", format!("option {}: unknown section `{}#{}`", i, doc, section)));
                    };
                    if crate::md::locate_bytes(&sec.raw, &old_text).is_none() {
                        return Err(ToolError::new(
                            "bad-prompt",
                            format!("option {}: old_text does not locate in {}#{}; copy it verbatim from the section", i, doc, section),
                        ));
                    }
                    Some(SuggestedEdit { doc, section, old_text, new_text })
                } else {
                    None
                };
                options.push(PromptOption { label: label.to_string(), edit, answer: answer.map(|s| s.to_string()) });
            }
        }
        Ok(Some(DiagnosticPrompt {
            question: question.to_string(),
            options,
            freeform: v["freeform"].as_bool().unwrap_or(false),
        }))
    }

    pub fn dispatch(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        // Reads see the turn's snapshot, not its staged mutations. Saying so on every
        // read while writes are staged stops the caller from concluding a staged
        // delete or update was lost.
        if READ_TOOLS.contains(&name) && !self.staged.is_empty() {
            return self.dispatch_inner(name, args).map(|mut v| {
                if v.is_object() {
                    v["note"] = json!("reads show the graph as the turn began; this turn's staged mutations apply at commit");
                }
                v
            });
        }
        self.dispatch_inner(name, args)
    }

    fn dispatch_inner(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
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
                if !hits.is_empty() {
                    return Ok(json!({
                        "hits": hits
                            .iter()
                            .map(|(id, name, def)| json!({"id": id, "name": name, "definition": def}))
                            .collect::<Vec<_>>()
                    }));
                }
                // A miss is an answer, not a dead end. A bare empty list reads as "ask
                // again"; models loop on it. Say what the graph holds and what to do next.
                let mut known: Vec<String> = self
                    .staged_entities
                    .iter()
                    .map(|(id, e)| format!("{} ({})", id, e.name))
                    .collect();
                for (id, e) in &self.snapshot.graph.entities {
                    if !self.staged_entities.contains_key(id) {
                        known.push(format!("{} ({})", id, e.name));
                    }
                }
                let total = known.len();
                known.truncate(10);
                let shown = known.len();
                Ok(json!({
                    "hits": [],
                    "entityCount": total,
                    "entities": known,
                    "next": if total == 0 {
                        format!(
                            "no match for `{}`: the graph holds no entities yet. Searching again will return this same answer. Create the entity with upsert_entity.",
                            query
                        )
                    } else {
                        format!(
                            "no match for `{}`. {} of the graph's {} entities are listed above; searching again will return this same answer. If one of them, or a known entity in your work pack, means the same concept under another name, use its id. Otherwise create the entity with upsert_entity.",
                            query, shown, total
                        )
                    }
                }))
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
                let mut v = json!({
                    "id": rid, "name": e.name, "definition": e.definition, "aliases": e.aliases,
                    "mentions": e.mentions.iter().map(|m| json!({"doc": m.doc, "section": m.section, "quote": m.quote})).collect::<Vec<_>>(),
                    "requirements": reqs, "relationships": rels
                });
                // The default scope is noise; showing it reads as a value someone set.
                if e.scope != "public" {
                    v["scope"] = json!(e.scope);
                }
                Ok(v)
            }
            "diagnostics" => {
                let lifecycle = Self::opt_str(args, "lifecycle").unwrap_or_else(|| "open".to_string());
                let rule = Self::opt_str(args, "rule");
                let subject = Self::opt_str(args, "subject");
                let list: Vec<Value> = self
                    .snapshot
                    .graph
                    .diagnostics
                    .iter()
                    .filter(|(_, d)| lifecycle == "all" || d.lifecycle == lifecycle)
                    .filter(|(_, d)| rule.as_deref().is_none_or(|r| d.rule == r))
                    .filter(|(_, d)| subject.as_deref().is_none_or(|s| d.subjects.iter().any(|x| x == s)))
                    .map(|(id, d)| {
                        let mut v = json!({
                            "id": id, "rule": d.rule, "severity": d.severity, "lifecycle": d.lifecycle,
                            "triage": d.triage, "subjects": d.subjects, "message": d.message,
                        });
                        // The standing question rides along, so verifying a prompt
                        // never requires reading the shard files.
                        if let Some(p) = &d.prompt {
                            v["question"] = json!(p.question);
                            v["options"] = json!(p.options.iter().map(|o| o.label.clone()).collect::<Vec<_>>());
                        }
                        if let Some(a) = &d.answer {
                            v["answered"] = json!(a.status);
                        }
                        v
                    })
                    .collect();
                Ok(json!({"diagnostics": list, "count": list.len()}))
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
                // Mirrors docs/compiler/model/entity.md#what-is-an-entity.
                if note.is_none() {
                    if let Some((eid, ename)) = self.near_name(&name_arg, &scope) {
                        return Err(ToolError::new(
                            "near-duplicate",
                            format!(
                                "`{}` looks like a name variant of existing `{}` ({}); if it is the same concept, reuse that id and add your wording with update_entity add_aliases. A field, part, state, or child concept of it IS a different concept: repeat the call with a `note` naming that relation",
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
                let requirement = Requirement {
                    ears,
                    entities,
                    edges,
                    source,
                    confidence: None,
                    reasoning: Self::opt_str(args, "reasoning"),
                    created: None,
                    updated: None,
                };
                // Stage-time natural-key resolution: the model sees the resolved id,
                // never a misleading fresh one. Same predicate as the commit-time fold.
                let norm_ears = crate::store::normalize_statement(&requirement.ears);
                let norm_quote = crate::store::normalize_statement(&requirement.source.quote);
                let same_key = |r: &Requirement| {
                    r.source.doc == requirement.source.doc
                        && r.source.section == requirement.source.section
                        && (crate::store::normalize_statement(&r.ears) == norm_ears
                            || (crate::store::normalize_statement(&r.source.quote) == norm_quote
                                && crate::store::statement_subsumes(&r.ears, &requirement.ears)))
                };
                // A statement this turn already staged: refresh that staged call in
                // place, so a repeated upsert is idempotent within the turn.
                let staged_pos = self.staged.iter().position(
                    |op| matches!(op, Op::CreateRequirement { requirement: r, .. } if same_key(r)),
                );
                if let Some(pos) = staged_pos {
                    let Op::CreateRequirement { id, requirement: r } = &mut self.staged[pos] else {
                        unreachable!()
                    };
                    for e in &requirement.entities {
                        if !r.entities.contains(e) {
                            r.entities.push(e.clone());
                        }
                    }
                    for edge in requirement.edges {
                        if !r.edges.iter().any(|x| (x.a == edge.a && x.b == edge.b) || (x.a == edge.b && x.b == edge.a)) {
                            r.edges.push(edge);
                        }
                    }
                    r.ears = requirement.ears;
                    r.source = requirement.source;
                    return Ok(json!({"id": id.clone(), "updated": true}));
                }
                // A statement the store already holds updates in place. A stale anchor
                // in the same section whose statement subsumes (or is subsumed by) the
                // new one is that statement reworded: it resolves to the anchor's id.
                let existing = self
                    .snapshot
                    .graph
                    .requirements
                    .iter()
                    .find(|(_, r)| same_key(r))
                    .map(|(rid, _)| rid.clone())
                    .or_else(|| {
                        self.scope
                            .stale_anchors
                            .iter()
                            .find(|a| {
                                self.snapshot.graph.requirements.get(*a).is_some_and(|r| {
                                    r.source.doc == requirement.source.doc
                                        && r.source.section == requirement.source.section
                                        && !self.snapshot.quote_locates(&r.source.doc, &r.source.section, &r.source.quote)
                                        && crate::store::statement_subsumes(&r.ears, &requirement.ears)
                                })
                            })
                            .cloned()
                    });
                if let Some(rid) = existing {
                    self.staged_reqs.insert(rid.clone());
                    self.taken_ids.insert(rid.clone());
                    self.stage(Op::CreateRequirement { id: rid.clone(), requirement })?;
                    return Ok(json!({"id": rid, "updated": true}));
                }
                // A new statement: the store mints the id; a supplied id is ignored.
                let mut taken = self.taken_ids.clone();
                taken.extend(self.staged_reqs.iter().cloned());
                let id = self.snapshot.mint_req_id(&doc, &taken);
                self.staged_reqs.insert(id.clone());
                self.taken_ids.insert(id.clone());
                self.stage(Op::CreateRequirement { id: id.clone(), requirement })?;
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
                // A revision may re-anchor its provenance: section plus quote, both or
                // neither. The quote must locate, same gate as a fresh upsert.
                // The common miscall is passing the statement as the quote while only
                // meaning to change `entities`; name the existing anchor so the repair
                // is to drop the two fields, not to guess another sentence.
                let anchor_hint = || match self.snapshot.graph.requirements.get(&id) {
                    Some(r) => format!(
                        "{} is anchored at {}#{} quoting \"{}\". `quote` is the document's own sentence, never the `ears` statement. To change only the entities or edges, omit `section` and `quote`",
                        id,
                        r.source.doc,
                        r.source.section,
                        crate::llm::truncate(&r.source.quote, 120)
                    ),
                    None => "`quote` is the document's own sentence, never the `ears` statement. To change only the entities or edges, omit `section` and `quote`".to_string(),
                };
                let source = match (Self::opt_str(args, "section"), Self::opt_str(args, "quote")) {
                    (Some(section), Some(q)) => {
                        let (doc, sec) = self
                            .resolve_section(&section)
                            .map_err(|e| ToolError::new(&e.rule, format!("{}; {}", e.message, anchor_hint())))?;
                        if let (Some(wd), "reconcile-doc") = (&self.scope.doc, self.scope.task.as_str()) {
                            if &doc != wd {
                                return Err(ToolError::new(
                                    "wrong-document",
                                    format!(
                                        "source cites {} but this turn reconciles {}; quote the sentence from {}'s own sections",
                                        doc, wd, wd
                                    ),
                                ));
                            }
                        }
                        let q = self
                            .check_quote(&doc, &sec, &q)
                            .map_err(|e| ToolError::new(&e.rule, format!("{}; {}", e.message, anchor_hint())))?;
                        Some(SourceRef { doc, section: sec, quote: q.trim().to_string() })
                    }
                    (None, None) => None,
                    _ => {
                        return Err(ToolError::new(
                            "bad-argument",
                            format!("re-anchoring needs both section and quote; pass the two together or neither. {}", anchor_hint()),
                        ))
                    }
                };
                self.stage(Op::UpdateRequirement { id: id.clone(), ears, entities, edges, source })?;
                Ok(json!({"id": id, "updated": true}))
            }
            "delete_requirement" => {
                let id = self.canon_req_id(&Self::str_arg(args, "id")?)?;
                let reason = Self::str_arg(args, "reason")?;
                self.stage(Op::DeleteRequirement { id, reason })?;
                Ok(json!({"deleted": true}))
            }
            "place_anchor" | "orphan_anchor" => {
                let raw = Self::str_arg(args, "id")?;
                let id = self
                    .canon_entity_id(&raw)
                    .filter(|i| self.scope.proposals.contains(i))
                    .or_else(|| self.canon_req_id(&raw).ok().filter(|i| self.scope.proposals.contains(i)))
                    .ok_or_else(|| {
                        ToolError::new(
                            "unknown-anchor",
                            format!(
                                "`{}` is not one of this task's proposals ({}); decide only the anchors the work pack lists",
                                raw,
                                self.scope.proposals.join(", ")
                            ),
                        )
                    })?;
                // The proposals name the anchor's old location and quote; the op carries
                // them so the store can tell one entity mention from another. An entity
                // with several proposed mentions is decided in one call: every one of
                // them goes to the same section.
                let proposals: Vec<AnchorProposal> = self
                    .snapshot
                    .status
                    .alignment
                    .iter()
                    .filter(|b| Some(&b.doc) == self.scope.doc.as_ref())
                    .flat_map(|b| b.proposals.iter())
                    .filter(|p| p.anchor == id)
                    .cloned()
                    .collect();
                if proposals.is_empty() {
                    return Err(ToolError::new("unknown-anchor", format!("no pending proposal for `{}`", id)));
                }
                let mut froms: Vec<SourceRef> = Vec::new();
                for p in &proposals {
                    let (from_doc, from_sec) = split_section_ref(&p.from)
                        .ok_or_else(|| ToolError::new("bad-section", format!("bad proposal location `{}`", p.from)))?;
                    froms.push(SourceRef { doc: from_doc, section: from_sec, quote: p.quote.clone() });
                }
                if name == "orphan_anchor" {
                    for from in froms {
                        self.stage(Op::OrphanAnchor { id: id.clone(), from })?;
                    }
                    return Ok(json!({"id": id, "orphaned": true}));
                }
                let (doc, sec) = self.resolve_section(&Self::str_arg(args, "section")?)?;
                let given = match Self::opt_str(args, "quote") {
                    Some(q) if froms.len() > 1 => {
                        return Err(ToolError::new(
                            "bad-argument",
                            format!(
                                "`{}` has {} proposed mentions; omit `quote` so each keeps its own, or decide them with the section alone",
                                id,
                                froms.len()
                            ),
                        ))
                    }
                    Some(q) => Some(self.check_quote(&doc, &sec, &q)?),
                    None => None,
                };
                let reevaluate = args["reevaluate"].as_bool().unwrap_or(false);
                let mut all_locate = true;
                for from in froms {
                    let quote = given.clone().unwrap_or_else(|| from.quote.clone());
                    all_locate &= self.snapshot.quote_locates(&doc, &sec, &quote);
                    self.stage(Op::PlaceAnchor {
                        id: id.clone(),
                        from,
                        to: SourceRef { doc: doc.clone(), section: sec.clone(), quote },
                        reevaluate,
                    })?;
                }
                Ok(json!({
                    "id": id,
                    "placed": true,
                    "reevaluate": reevaluate || !all_locate,
                    "note": if all_locate { "quote locates in the new section" } else { "the stored quote does not locate there; the extraction turn will re-anchor or delete it" },
                }))
            }
            "report_diagnostic" => {
                let rule = Self::str_arg(args, "rule")?;
                const REVIEW_RULES: [&str; 6] =
                    ["contradiction", "duplicate-entity", "duplicate-requirement", "missing-link", "ambiguity", "lint"];
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
                let prompt = self.parse_prompt(&args["prompt"])?;
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
                        prompt,
                        answer: None,
                        created: None,
                        updated: None,
                    },
                })?;
                Ok(json!({"reported": true}))
            }
            "update_diagnostic" => {
                let id = Self::str_arg(args, "id")?;
                if !self.snapshot.graph.diagnostics.contains_key(&id) {
                    return Err(ToolError::new("unknown-id", format!("unknown diagnostic id `{}`", id)));
                }
                let prompt = self.parse_prompt(&args["prompt"])?;
                self.stage(Op::UpdateDiagnosticPrompt { id, prompt })?;
                Ok(json!({"updated": true}))
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
                // A section that yielded a statement is not non-normative. The two claims
                // contradict, and the harness settles it deterministically rather than
                // journaling a mark the graph itself refutes.
                if state == "non-normative" {
                    let mut yielded: Vec<String> = self
                        .snapshot
                        .graph
                        .requirements
                        .iter()
                        .filter(|(_, r)| r.source.doc == doc && r.source.section == sec)
                        .map(|(id, _)| id.clone())
                        .collect();
                    for op in &self.staged {
                        if let Op::CreateRequirement { id, requirement } = op {
                            if requirement.source.doc == doc && requirement.source.section == sec {
                                yielded.push(id.clone());
                            }
                        }
                    }
                    if !yielded.is_empty() {
                        yielded.sort();
                        yielded.dedup();
                        return Err(ToolError::new(
                            "contradicts-extraction",
                            format!(
                                "{}#{} already yielded {}; a section that states a requirement is covered, not non-normative. Mark it covered, or delete the statement first if it was a mistake",
                                doc,
                                sec,
                                yielded.join(", ")
                            ),
                        ));
                    }
                }
                // One coverage mark per section per changeset: restaging replaces the
                // earlier mark instead of journaling contradictory states.
                self.staged
                    .retain(|op| !matches!(op, Op::SetCoverage { doc: d, section: s, .. } if d == &doc && s == &sec));
                self.stage(Op::SetCoverage { doc, section: sec, state, note })?;
                Ok(json!({"set": true}))
            }
            "generation_tasks" => {
                let gs = self.gen_settings();
                let tasks = crate::gen::pending(&self.snapshot, &gs);
                if tasks.is_empty() {
                    return Ok(json!({"tasks": [], "note": "generation is current; nothing to do"}));
                }
                Ok(json!({"tasks": tasks, "next": "begin_generation on one entity"}))
            }
            "begin_generation" => {
                let entity = Self::str_arg(args, "entity")?;
                let gs = self.gen_settings();
                let id = self.snapshot.resolve_id(&entity).to_string();
                crate::gen::task_package(&self.snapshot, &id, &gs)
                    .map_err(|e| ToolError::new("unknown-id", e))
            }
            "record_generation" => {
                let entity = Self::str_arg(args, "entity")?;
                let gs = self.gen_settings();
                let id = self.snapshot.resolve_id(&entity).to_string();
                let Some(seen) = Self::opt_str(args, "factHash") else {
                    return Err(ToolError::new(
                        "bad-argument",
                        "factHash is required; pass the factHash from the begin_generation package".into(),
                    ));
                };
                if !args["manifest"].is_object() {
                    return Err(ToolError::new(
                        "bad-argument",
                        "manifest is required: {files: [...], tests: [{requirement, kind, label, artifact, name, run}]}".into(),
                    ));
                }
                crate::gen::mark(&self.snapshot, &id, Some(seen.as_str()), &args["manifest"], &gs)
                    .map_err(|e| ToolError::new("bad-manifest", e))
            }
            "binding_tasks" => {
                let gs = self.gen_settings();
                let tasks = crate::bind::pending(&self.snapshot, &gs);
                if tasks.is_empty() {
                    return Ok(json!({"tasks": [], "note": "every requirement is bound; generation_tasks lists what binding left unimplemented"}));
                }
                Ok(json!({"tasks": tasks, "next": "begin_binding on one requirement"}))
            }
            "begin_binding" => {
                let rid = Self::str_arg(args, "requirement")?;
                let gs = self.gen_settings();
                crate::bind::task(&self.snapshot, &rid, &gs).map_err(|e| ToolError::new("unknown-id", e))
            }
            "record_binding" => {
                let rid = Self::str_arg(args, "requirement")?;
                let verdict = Self::str_arg(args, "verdict")?;
                let gs = self.gen_settings();
                let files = Self::str_list(args, "files");
                let evidence = Self::opt_str(args, "evidence");
                if !args["test"].is_object() {
                    return Err(ToolError::new(
                        "bad-argument",
                        "test is required: {kind: programmatic|llm, label, artifact, name, run, cwd?}".into(),
                    ));
                }
                crate::bind::record(&self.snapshot, &rid, &files, &args["test"], &verdict, evidence.as_deref(), &gs)
                    .map_err(|e| ToolError::new("bad-binding", e))
            }
            "verification_tasks" => {
                let gs = self.gen_settings();
                let filter = Self::opt_str(args, "filter");
                let entity = Self::opt_str(args, "entity");
                let tasks = crate::verify::pending(&self.snapshot, &gs, filter.as_deref(), entity.as_deref());
                if tasks.is_empty() {
                    return Ok(json!({"tasks": [], "note": "nothing pending; every targeted row is verified"}));
                }
                Ok(json!({"tasks": tasks, "next": "run_tests for programmatic rows; begin_verification then record_verdict for llm rows"}))
            }
            "begin_verification" => {
                let rid = Self::str_arg(args, "requirement")?;
                let gs = self.gen_settings();
                crate::verify::task(&self.snapshot, &rid, &gs).map_err(|e| ToolError::new("unknown-id", e))
            }
            "run_tests" => {
                let gs = self.gen_settings();
                let targets = Self::str_list(args, "requirements");
                crate::verify::run_selected(&self.snapshot, &gs, &targets)
                    .map_err(|e| ToolError::new("build-failed", e))
            }
            "record_verdict" => {
                let rid = Self::str_arg(args, "requirement")?;
                let verdict = Self::str_arg(args, "verdict")?;
                let gs = self.gen_settings();
                let seen = Self::opt_str(args, "factHash");
                let evidence = Self::opt_str(args, "evidence");
                crate::verify::mark(&self.snapshot, &rid, &verdict, seen.as_deref(), evidence.as_deref(), &gs)
                    .map_err(|e| ToolError::new("bad-argument", e))
            }
            "read_text_file" | "write_text_file" | "list_files" | "run_command" => self.file_tool(name, args),
            // Feedback about jazyk's own prompts and tools. It stages nothing, spends no
            // mutation budget, and passes no gate beyond a non-empty message: a model
            // asking for help is never bounced. Mirrors docs/compiler/tools.md#feedback-tool.
            "report_feedback" => {
                let message = Self::opt_str(args, "message").unwrap_or_default();
                let message = message.trim();
                if message.is_empty() {
                    return Err(ToolError::new(
                        "missing-argument",
                        "report_feedback needs a message saying what was unclear and what would have helped".into(),
                    ));
                }
                if self.feedback_count >= FEEDBACK_LIMIT {
                    return Ok(json!({
                        "recorded": false,
                        "note": format!("already recorded {} feedback entries for this session; continue the task", FEEDBACK_LIMIT)
                    }));
                }
                self.feedback_count += 1;
                let c = self.caller.clone();
                crate::feedback::append(
                    &self.snapshot.out,
                    &crate::feedback::Entry {
                        at: crate::verify::now_iso(),
                        kind: crate::feedback::normalize_kind(&Self::opt_str(args, "kind").unwrap_or_default()),
                        subject: Self::opt_str(args, "subject").unwrap_or_default().trim().to_string(),
                        message: crate::llm::truncate(message, 4_000).to_string(),
                        source: c.source,
                        task: if c.task.is_empty() { self.scope.task.clone() } else { c.task },
                        target: c.target,
                        model: c.model,
                        codec: c.codec,
                        generation: self.snapshot.status.generation,
                        run: c.run,
                        client: c.client,
                    },
                );
                Ok(json!({
                    "recorded": true,
                    "note": "logged for jazyk's developers; continue the task with your best judgment"
                }))
            }
            "done" => {
                // Batch gate for the align turn: every proposal is decided.
                // Mirrors docs/compiler/turns/align-doc.md#finish.
                let undecided: Vec<String> = self
                    .scope
                    .proposals
                    .iter()
                    .filter(|a| {
                        !self.staged.iter().any(|o| {
                            matches!(o, Op::PlaceAnchor { id, .. } | Op::OrphanAnchor { id, .. } if id == *a)
                        })
                    })
                    .cloned()
                    .collect();
                if !undecided.is_empty() {
                    return Err(ToolError::new(
                        "undecided-proposal",
                        format!(
                            "proposals left undecided: {}; for each, place_anchor with the section that now holds the statement (reevaluate true when its meaning may have changed), or orphan_anchor when no candidate states it",
                            undecided.join(", ")
                        ),
                    ));
                }
                // Batch gate: stale anchors are a contract. Each must be re-anchored
                // (its quote locates again), re-recorded under its natural key, revised,
                // or deleted; a turn cannot mark coverage around them and walk away. An
                // anchor the align turn flagged for re-evaluation owes a decision even
                // though its quote locates.
                let mut untouched: Vec<String> = Vec::new();
                for a in &self.scope.stale_anchors {
                    let Some(r) = self.snapshot.graph.requirements.get(a) else { continue };
                    if self.snapshot.quote_locates(&r.source.doc, &r.source.section, &r.source.quote)
                        && !self.snapshot.status.reevaluate.contains(a)
                    {
                        continue;
                    }
                    let addressed = self.staged.iter().any(|o| match o {
                        Op::UpdateRequirement { id, .. } | Op::DeleteRequirement { id, .. } => id == a,
                        // A staged create that resolved to the anchor carries its id;
                        // the statement-equality fallback covers pre-resolution stages.
                        Op::CreateRequirement { id, requirement } => {
                            id == a
                                || (requirement.source.doc == r.source.doc
                                    && requirement.source.section == r.source.section
                                    && crate::store::normalize_statement(&requirement.ears)
                                        == crate::store::normalize_statement(&r.ears))
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
                            "stale anchors left untouched: {}; for each: if the document still states the fact, re-record it with upsert_requirement quoting the new sentence verbatim (it resolves to the anchor and updates in place); if the statement changed meaning, revise it with update_requirement carrying the id, the new ears, and the new section plus quote; if the fact is gone, delete_requirement",
                            untouched.join(", ")
                        ),
                    ));
                }
                // Batch gate: an explicit done finishes the coverage contract. Every
                // dirty section carries a mark, staged or already recorded; the
                // implicit path commits without one and the section stays unprocessed.
                // Mirrors docs/compiler/turns.md#commit.
                if !self.implicit_done {
                    if let (Some(wd), "reconcile-doc") = (&self.scope.doc, self.scope.task.as_str()) {
                        let unmarked: Vec<String> = self
                            .scope
                            .target_sections
                            .iter()
                            .filter(|sec| {
                                let recorded = self
                                    .snapshot
                                    .docs
                                    .get(wd)
                                    .map(|d| d.coverage.contains_key(*sec))
                                    .unwrap_or(false);
                                let staged = self.staged.iter().any(|o| {
                                    matches!(o, Op::SetCoverage { doc, section, .. } if doc == wd && section == *sec)
                                });
                                !recorded && !staged
                            })
                            .cloned()
                            .collect();
                        if !unmarked.is_empty() {
                            return Err(ToolError::new(
                                "unmarked-section",
                                format!(
                                    "dirty section(s) without a coverage mark: {}; for each, set_coverage covered (a requirement must be sourced from it) or non-normative with a note",
                                    unmarked.join(", ")
                                ),
                            ));
                        }
                    }
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
                target: "shop.md".into(),
                target_sections: vec!["/shop".into(), "/shop/cart".into()],
                stale_anchors: Vec::new(),
                proposals: Vec::new(),
            },
            64,
            24_000,
        )
    }

    // A bare empty list reads as "ask again" and models loop on it; a miss must say what
    // the graph holds and what to do instead. Mirrors docs/compiler/tools.md#read-tools.
    #[test]
    fn search_miss_names_the_graph_and_the_next_step() {
        let mut t = session();
        let r = t.dispatch("search", &json!({"query": "slides"})).unwrap();
        assert_eq!(r["hits"].as_array().unwrap().len(), 0);
        assert_eq!(r["entityCount"], 1);
        assert!(r["entities"][0].as_str().unwrap().contains("ent:customer"), "{}", r);
        assert!(r["next"].as_str().unwrap().contains("upsert_entity"), "{}", r);
        // A hit answers under the same key, so the caller reads one shape.
        let hit = t.dispatch("search", &json!({"query": "Customer"})).unwrap();
        assert_eq!(hit["hits"][0]["id"], "ent:customer");
    }

    // A section that yielded a statement cannot also state none of them.
    #[test]
    fn non_normative_contradicts_an_extracted_statement() {
        let mut t = session();
        t.dispatch(
            "upsert_requirement",
            &json!({
                "ears": "The Shopping Cart shall hold items a Customer intends to buy.",
                "entities": ["ent:customer"],
                "section": "/shop/cart",
                "quote": "The Shopping Cart holds items a Customer intends to buy."
            }),
        )
        .unwrap();
        let err = t
            .dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "non-normative", "note": "just facts"}))
            .unwrap_err();
        assert_eq!(err.rule, "contradicts-extraction");
        assert!(err.message.contains("covered"), "{}", err.message);
        // The honest mark still goes through.
        t.dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "covered"})).unwrap();
        // A section that yielded nothing is still free to be non-normative.
        t.dispatch("set_coverage", &json!({"section": "/shop", "state": "non-normative", "note": "navigation only"}))
            .unwrap();
    }

    // Passing the ears statement as the quote is the common miscall; the rejection has to
    // point at the existing anchor, not just repeat that the quote is absent.
    #[test]
    fn reanchor_rejection_names_the_existing_anchor() {
        let mut t = session();
        let id = t
            .dispatch(
                "upsert_requirement",
                &json!({
                    "ears": "The Shopping Cart shall hold items a Customer intends to buy.",
                    "entities": ["ent:customer"],
                    "section": "/shop/cart",
                    "quote": "The Shopping Cart holds items a Customer intends to buy."
                }),
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let err = t
            .dispatch(
                "update_requirement",
                &json!({"id": id, "entities": ["ent:customer"], "section": "shop.md#/shop/cart",
                        "quote": "The Shopping Cart shall hold items a Customer intends to buy."}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "quote-not-found");
        assert!(err.message.contains("omit `section` and `quote`"), "{}", err.message);
        // Only-entities is the call that was meant, and it is accepted.
        t.dispatch("update_requirement", &json!({"id": id, "entities": ["ent:customer"]})).unwrap();
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
        t.dispatch("set_coverage", &json!({"section": "/shop", "state": "non-normative", "note": "intro text only"}))
            .unwrap();
        t.dispatch("done", &json!({"summary": "reconciled cart"})).unwrap();
        assert!(t.done.is_some());
        assert_eq!(t.staged.len(), 4);
    }

    #[test]
    fn explicit_done_requires_a_mark_per_dirty_section() {
        let mut t = session();
        t.dispatch(
            "upsert_entity",
            &json!({"name": "Shopping Cart", "definition": "holds items", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}}),
        )
        .unwrap();
        t.dispatch(
            "upsert_requirement",
            &json!({"ears": "The Shopping Cart shall hold items.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "holds items a Customer intends to buy"}),
        )
        .unwrap();
        t.dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "covered"})).unwrap();
        // Extracting from a section and walking away without marking the other dirty
        // section bounces the explicit done.
        let err = t.dispatch("done", &json!({"summary": "cart only"})).unwrap_err();
        assert_eq!(err.rule, "unmarked-section");
        assert!(err.message.contains("/shop"));
        t.dispatch("set_coverage", &json!({"section": "/shop", "state": "non-normative", "note": "intro text only"}))
            .unwrap();
        t.dispatch("done", &json!({"summary": "all sections marked"})).unwrap();
        assert!(t.done.is_some());
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
                target: "shop.md".into(),
                target_sections: vec!["/shop/cart".into()],
                stale_anchors: vec!["req:shop-1".into()],
                proposals: Vec::new(),
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
        // Same statement, fresh verbatim quote: the natural key resolves at stage time
        // and the model sees the anchor's id, not a misleading fresh one.
        let v = t
            .dispatch(
                "upsert_requirement",
                &json!({"ears": "The Shopping Cart shall hold items a Customer intends to buy.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "The Shopping Cart keeps items a Customer intends to buy."}),
            )
            .unwrap();
        assert_eq!(v["id"], "req:shop-1");
        assert_eq!(v["updated"], true);
        t.dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "covered"})).unwrap();
        t.dispatch("done", &json!({"summary": "re-anchored"})).unwrap();
        assert!(t.done.is_some());
    }

    #[test]
    fn repeated_upsert_resolves_to_one_staged_statement() {
        let mut t = session();
        t.dispatch(
            "upsert_entity",
            &json!({"name": "Shopping Cart", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}}),
        )
        .unwrap();
        let args = json!({"ears": "The Shopping Cart shall hold items.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "holds items a Customer intends to buy"});
        let v1 = t.dispatch("upsert_requirement", &args).unwrap();
        assert_eq!(v1["created"], true);
        // The identical call again is idempotent within the turn: same id, one staged op.
        let v2 = t.dispatch("upsert_requirement", &args).unwrap();
        assert_eq!(v2["updated"], true);
        assert_eq!(v1["id"], v2["id"]);
        assert_eq!(t.staged.iter().filter(|o| matches!(o, Op::CreateRequirement { .. })).count(), 1);
    }

    #[test]
    fn stale_anchor_reworded_statement_resolves_to_the_anchor() {
        let mut t = session_with_stale_anchor();
        // The statement was reworded (it subsumes the anchor's) and the old quote no
        // longer locates: the upsert lands on the anchor's id, updating in place.
        let v = t
            .dispatch(
                "upsert_requirement",
                &json!({"ears": "The Shopping Cart shall hold items.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "The Shopping Cart keeps items a Customer intends to buy."}),
            )
            .unwrap();
        assert_eq!(v["id"], "req:shop-1");
        assert_eq!(v["updated"], true);
        t.dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "covered"})).unwrap();
        t.dispatch("done", &json!({"summary": "re-anchored reworded"})).unwrap();
        assert!(t.done.is_some());
    }

    #[test]
    fn update_requirement_reanchors_with_section_and_quote() {
        let mut t = session_with_stale_anchor();
        // Re-anchoring needs the pair together, and the quote must locate.
        let err = t.dispatch("update_requirement", &json!({"id": "req:shop-1", "section": "/shop/cart"})).unwrap_err();
        assert_eq!(err.rule, "bad-argument");
        let err2 = t
            .dispatch("update_requirement", &json!({"id": "req:shop-1", "section": "/shop/cart", "quote": "not in the document"}))
            .unwrap_err();
        assert_eq!(err2.rule, "quote-not-found");
        t.dispatch(
            "update_requirement",
            &json!({"id": "req:shop-1", "ears": "The Shopping Cart shall keep items a Customer intends to buy.", "section": "/shop/cart", "quote": "The Shopping Cart keeps items a Customer intends to buy."}),
        )
        .unwrap();
        t.dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "covered"})).unwrap();
        t.dispatch("done", &json!({"summary": "revised and re-anchored"})).unwrap();
        assert!(t.done.is_some());
    }

    #[test]
    fn feedback_logs_with_its_caller_and_stages_nothing() {
        let dir = std::env::temp_dir().join(format!("jazyk-tool-fb-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        let mut t = session();
        t.snapshot.out = dir.clone();
        t.caller = crate::feedback::Caller {
            source: "turn".into(),
            target: "shop.md".into(),
            model: "test-model".into(),
            codec: "native".into(),
            run: Some("run-1".into()),
            ..Default::default()
        };
        let ok = t
            .dispatch("report_feedback", &json!({"kind": "ambiguous", "subject": "set_coverage", "message": "covered vs non-normative is unclear for a section of links"}))
            .unwrap();
        assert_eq!(ok["recorded"], json!(true));
        assert!(t.staged.is_empty(), "feedback stages no mutation");

        let logged = crate::feedback::read(&dir, 10);
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0]["kind"], "ambiguous");
        assert_eq!(logged[0]["task"], "reconcile-doc");
        assert_eq!(logged[0]["target"], "shop.md");
        assert_eq!(logged[0]["model"], "test-model");
        assert_eq!(logged[0]["run"], "run-1");

        // An empty message is the one rejection; an unknown kind is not.
        let err = t.dispatch("report_feedback", &json!({"kind": "wrong", "message": "  "})).unwrap_err();
        assert_eq!(err.rule, "missing-argument");
        t.dispatch("report_feedback", &json!({"kind": "puzzled", "message": "second"})).unwrap();
        assert_eq!(crate::feedback::read(&dir, 10)[0]["kind"], "other");

        // The cap holds: further calls are acknowledged without a record.
        for i in 0..5 {
            t.dispatch("report_feedback", &json!({"kind": "confusing", "message": format!("more {}", i)})).unwrap();
        }
        assert_eq!(crate::feedback::read(&dir, 99).len(), FEEDBACK_LIMIT);
        std::fs::remove_dir_all(&dir).ok();
    }

    // The per-task docs state what the model sees; a toolset change that forgets the
    // doc would let them drift. Every tool a task carries must appear on its page.
    // Mirrors docs/compiler/turns.md#task-types.
    #[test]
    fn task_docs_name_every_tool() {
        for (task, doc) in [
            ("align-doc", include_str!("../../docs/compiler/turns/align-doc.md")),
            ("reconcile-doc", include_str!("../../docs/compiler/turns/reconcile-doc.md")),
            ("review-requirement", include_str!("../../docs/compiler/turns/review-requirement.md")),
            ("review-entity", include_str!("../../docs/compiler/turns/review-entity.md")),
        ] {
            for tool in toolset(task) {
                assert!(doc.contains(tool), "the {} page misses tool `{}`", task, tool);
            }
        }
    }

    fn align_session() -> ToolSession {
        let mut s = Store::default();
        let text = "# Shop\nintro\n\n## Basket\nThe Basket keeps items a Customer intends to buy.\nItems stay until checkout.\n";
        s.docs.insert(
            "shop.md".into(),
            DocRecord { content_hash: hash_hex(text), sections: crate::md::parse_sections(text), coverage: BTreeMap::new() },
        );
        s.graph.entities.insert("ent:basket".into(), Entity { name: "Basket".into(), ..Default::default() });
        s.graph.requirements.insert(
            "req:shop-1".into(),
            Requirement {
                ears: "The Basket shall keep items until checkout.".into(),
                entities: vec!["ent:basket".into()],
                edges: Vec::new(),
                source: SourceRef { doc: "shop.md".into(), section: "/shop/cart".into(), quote: "Items stay until checkout.".into() },
                confidence: None,
                reasoning: None,
                created: None,
                updated: None,
            },
        );
        s.graph.requirements.insert(
            "req:shop-2".into(),
            Requirement {
                ears: "The Basket shall hold items a Customer intends to buy.".into(),
                entities: vec!["ent:basket".into()],
                edges: Vec::new(),
                source: SourceRef { doc: "shop.md".into(), section: "/shop/cart".into(), quote: "The Cart holds items a Customer intends to buy.".into() },
                confidence: None,
                reasoning: None,
                created: None,
                updated: None,
            },
        );
        let candidate = |locates: bool| crate::model::AnchorCandidate {
            section: "shop.md#/shop/basket".into(),
            similarity: if locates { 1.0 } else { 0.8 },
            quote_locates: locates,
            nearest: (!locates).then(|| "The Basket keeps items a Customer intends to buy.".to_string()),
            excerpt: String::new(),
        };
        s.status.alignment.push(crate::model::DocAlignment {
            doc: "shop.md".into(),
            changes: vec![],
            proposals: vec![
                crate::model::AnchorProposal { anchor: "req:shop-1".into(), from: "shop.md#/shop/cart".into(), quote: "Items stay until checkout.".into(), excerpt: String::new(), candidates: vec![candidate(true)] },
                crate::model::AnchorProposal { anchor: "req:shop-2".into(), from: "shop.md#/shop/cart".into(), quote: "The Cart holds items a Customer intends to buy.".into(), excerpt: String::new(), candidates: vec![candidate(false)] },
            ],
        });
        ToolSession::new(
            s,
            WorkScope {
                task: "align-doc".into(),
                doc: Some("shop.md".into()),
                target: "shop.md".into(),
                target_sections: Vec::new(),
                stale_anchors: Vec::new(),
                proposals: vec!["req:shop-1".into(), "req:shop-2".into()],
            },
            64,
            24_000,
        )
    }

    #[test]
    fn align_done_rejects_an_undecided_proposal() {
        let mut s = align_session();
        s.dispatch("place_anchor", &json!({"id": "req:shop-1", "section": "/shop/basket", "reevaluate": false})).unwrap();
        let err = s.dispatch("done", &json!({"summary": "x"})).unwrap_err();
        assert_eq!(err.rule, "undecided-proposal");
        assert!(err.message.contains("req:shop-2"));
        s.dispatch("orphan_anchor", &json!({"id": "req:shop-2"})).unwrap();
        assert!(s.dispatch("done", &json!({"summary": "x"})).is_ok());
        assert_eq!(s.staged.len(), 2);
    }

    #[test]
    fn place_anchor_gates_quote_and_section_and_rejects_strangers() {
        let mut s = align_session();
        let err = s.dispatch("place_anchor", &json!({"id": "ent:basket", "section": "/shop/basket", "reevaluate": false})).unwrap_err();
        assert_eq!(err.rule, "unknown-anchor");
        let err = s.dispatch("place_anchor", &json!({"id": "req:shop-2", "section": "/shop/nowhere", "reevaluate": false})).unwrap_err();
        assert_eq!(err.rule, "unknown-section");
        let err = s
            .dispatch("place_anchor", &json!({"id": "req:shop-2", "section": "/shop/basket", "quote": "not in the text", "reevaluate": false}))
            .unwrap_err();
        assert_eq!(err.rule, "quote-not-found");
        // Without a quote the old one rides along; the reply says it will not locate.
        let v = s.dispatch("place_anchor", &json!({"id": "req:shop-2", "section": "/shop/basket", "reevaluate": false})).unwrap();
        assert_eq!(v["reevaluate"], true);
        // With the new sentence verbatim it is a clean relocation.
        let v = s
            .dispatch("place_anchor", &json!({"id": "req:shop-1", "section": "shop.md#/shop/basket", "quote": "Items stay until checkout.", "reevaluate": false}))
            .unwrap();
        assert_eq!(v["reevaluate"], false);
        assert!(matches!(&s.staged[1], Op::PlaceAnchor { to, .. } if to.section == "/shop/basket"));
    }

    // A relocated anchor flagged for re-evaluation owes the reconcile turn a decision
    // even though its quote locates.
    #[test]
    fn reconcile_done_holds_a_flagged_anchor_whose_quote_locates() {
        let mut s = session();
        s.snapshot.graph.requirements.insert(
            "req:shop-9".into(),
            Requirement {
                ears: "The Shopping Cart shall hold items a Customer intends to buy.".into(),
                entities: vec!["ent:shopping-cart".into()],
                edges: Vec::new(),
                source: SourceRef { doc: "shop.md".into(), section: "/shop/cart".into(), quote: "The Shopping Cart holds items a Customer intends to buy.".into() },
                confidence: None,
                reasoning: None,
                created: None,
                updated: None,
            },
        );
        s.snapshot.status.reevaluate.push("req:shop-9".into());
        s.scope.stale_anchors.push("req:shop-9".into());
        s.scope.target_sections = vec!["/shop/cart".into()];
        s.dispatch("set_coverage", &json!({"section": "/shop/cart", "state": "covered"})).unwrap();
        let err = s.dispatch("done", &json!({"summary": "x"})).unwrap_err();
        assert_eq!(err.rule, "stale-anchor");
        s.dispatch("delete_requirement", &json!({"id": "req:shop-9", "reason": "meaning changed"})).unwrap();
        assert!(s.dispatch("done", &json!({"summary": "x"})).is_ok());
    }
}
