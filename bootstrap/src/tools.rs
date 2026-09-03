// The tool registry: the graph's only interface for models. One registry, served
// in-process to the turn harness and over stdio as the MCP server.
// Mirrors docs/compiler/tools.md.
use crate::context::LoadedSet;
use crate::model::*;
use crate::session::{SkillLoad, SkillState};
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
        ToolError {
            rule: rule.to_string(),
            message,
        }
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
            name: "load",
            description: "Load a target and its immediate neighborhood into the loaded set: any node id (ent:..., req:..., view:..., diag:...) or a section reference (doc.md#/ref). depth defaults to 1: the target in full, its edges, each neighbor as a stub; depth 2 loads the neighbors in full too. Whatever the budget cut off arrives as expansion handles (h:<target>:<axis>). Loading an already loaded target is a repeat; deepen a loaded node with expand instead.",
            parameters: obj(
                json!({"target": {"type": "string"}, "depth": {"type": "integer", "minimum": 1}}),
                &["target"],
            ),
        },
        ToolDef {
            name: "expand",
            description: "Load the frontier behind an expansion handle (h:<target>:<axis>[:<start>]) from the loaded set's status block or an earlier reply.",
            parameters: obj(json!({"handle": {"type": "string"}}), &["handle"]),
        },
        ToolDef {
            name: "unload",
            description: "Drop an item from the loaded set. Its handles close and its budget frees for the rest of the session.",
            parameters: obj(json!({"target": {"type": "string"}}), &["target"]),
        },
        ToolDef {
            name: "graph_status",
            description: "Re-render the loaded set's status block in full: every loaded item, its handles, the skill index line, and the unload suggestions. A condensed form already rides on every mutating reply.",
            parameters: obj(json!({}), &[]),
        },
        ToolDef {
            name: "search",
            description: "Look up entities by name or alias (kind entity, the default) or views by title (kind view) before creating one. Returns up to 8 possible matches; hits can be false neighbors (substrings, shared words), so inspect each before reusing it.",
            parameters: obj(
                json!({"query": {"type": "string"}, "kind": {"type": "string", "enum": ["entity", "view"]}}),
                &["query"],
            ),
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
            name: "get_view",
            description: "One view with its members in order, its exclusions, query, and collapse list, the relationships among its members, and children: the members that hold a level of their own, each with the id of its level view (the drill-down links the rendering carries).",
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
            description: "Create a domain concept, or update it if the name already exists (name plus scope, and parent when given; several entities under that name need parent to say which). mention cites the section and the verbatim quote that talks about it; provenance.derived {from, reasoning} is the alternative for structure the documents do not state. stereotype is free-form judgment (actor, service, interface, table). parent must exist and keeps the containment tree acyclic. attributes are [{name, type?, value?, provenance?: {section, quote}}], keyed by name; one without its own provenance takes the call's quote. Not for file paths, CLI flags, or markdown terms. Leave scope unset unless the documents name a bounded context. A name that reads as a variant of an existing entity is rejected unless note names how the concepts differ (a field, part, or state of X is a different concept than X).",
            parameters: obj(
                json!({
                    "name": {"type": "string"},
                    "definition": {"type": "string"},
                    "aliases": {"type": "array", "items": {"type": "string"}},
                    "scope": {"type": "string"},
                    "stereotype": {"type": "string"},
                    "parent": {"type": "string"},
                    "attributes": {"type": "array", "items": attribute_schema()},
                    "mention": {"type": "object", "properties": {"section": {"type": "string"}, "quote": {"type": "string"}}, "required": ["section", "quote"]},
                    "provenance": derived_schema(),
                    "note": {"type": "string"}
                }),
                &["name"],
            ),
        },
        ToolDef {
            name: "update_entity",
            description: "Update an existing entity. A rename keeps the id. attributes upsert by name; attributes not named stand. parent obeys the same gates as on create.",
            parameters: obj(
                json!({
                    "id": {"type": "string"},
                    "name": {"type": "string"},
                    "definition": {"type": "string"},
                    "add_aliases": {"type": "array", "items": {"type": "string"}},
                    "stereotype": {"type": "string"},
                    "parent": {"type": "string"},
                    "attributes": {"type": "array", "items": attribute_schema()}
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
            name: "group_entities",
            description: "Build one level: stage a derived grouping entity from members (provenance derived from exactly them, reasoning why the domain would recognize them as one thing) and reparent every member under it, as one changeset. The grouping takes the members' shared current parent (none at the scope root) and their scope. At least two members; every member resolves; all members share one current parent (a grouping never crosses levels); the name passes the near-duplicate gate, so a lookalike of an existing area reuses that entity instead; definition is one sentence stating the grouping's responsibility. stereotype is from the existing vocabulary (system, component, module) or absent; there is no grouping stereotype.",
            parameters: obj(
                json!({
                    "name": {"type": "string"},
                    "definition": {"type": "string"},
                    "members": {"type": "array", "items": {"type": "string"}, "minItems": 2},
                    "stereotype": {"type": "string"},
                    "reasoning": {"type": "string"}
                }),
                &["name", "definition", "members", "reasoning"],
            ),
        },
        ToolDef {
            name: "dissolve_entity",
            description: "Unbuild one level: a grouping with derived provenance and no mentions dissolves; its children reparent to its parent (parentless when it was at the scope root) and it tombstones with a redirect to its parent. Refused on an entity a document states (stated-entity): revise the documents instead. Not a delete: a redirect stays.",
            parameters: obj(json!({"id": {"type": "string"}, "reason": {"type": "string"}}), &["id", "reason"]),
        },
        ToolDef {
            name: "upsert_requirement",
            description: "Record one requirement: a free-form statement of one atomic obligation (specific, testable, entity-anchored). entities are the entity ids the statement is about. Exactly one of section plus quote (the verbatim source sentence copied from the section) or provenance.derived {from, reasoning} (a statement the documents do not state). Safe to retry: re-recording the same statement updates it in place. edges optionally tie two of the entities directionally (a acts on b) with a relationship type and cardinality. transition {subject, from, to, trigger?, guard?} says the subject (one of the entities) enters a state. facets are [{facet, reasoning, measure?}] with facet one of behavior, constraint, failure-mode, quality; measure only on quality.",
            parameters: obj(
                json!({
                    "statement": {"type": "string"},
                    "entities": {"type": "array", "items": {"type": "string"}},
                    "section": {"type": "string"},
                    "quote": {"type": "string"},
                    "provenance": derived_schema(),
                    "edges": {"type": "array", "items": edge_schema()},
                    "transition": transition_schema(),
                    "facets": {"type": "array", "items": facet_schema()}
                }),
                &["statement", "entities"],
            ),
        },
        ToolDef {
            name: "update_requirement",
            description: "Update an existing requirement's statement, entities, edges, transition, or facets; a field given replaces the stored one whole. section plus quote (together) re-anchor the provenance; the quote must locate verbatim in the section. Omit both when only the other fields change.",
            parameters: obj(
                json!({
                    "id": {"type": "string"},
                    "statement": {"type": "string"},
                    "entities": {"type": "array", "items": {"type": "string"}},
                    "edges": {"type": "array", "items": edge_schema()},
                    "transition": transition_schema(),
                    "facets": {"type": "array", "items": facet_schema()},
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
            description: "Record a judgment about the graph or documents. Severity error only when two statements cannot both hold; warning for real but repairable issues; info for observations. prompt is optional and usually absent: attach one only when the diagnostic asks a person a real question, omit it entirely otherwise. When present, it carries up to 4 options, each a label with exactly one of edit (a suggested prose edit, applied without a model) or answer (a prefilled reply), plus freeform for typed replies. A decision (a choice the documents leave open) requires a prompt; nonconformant-instance is an instance whose values or links its type's statements rule out.",
            parameters: obj(
                json!({
                    "rule": {"type": "string", "enum": REVIEW_RULES},
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
            name: "upsert_view",
            description: "Create a view (the stored half of a diagram: what it includes, never how it looks), or refresh it when a view of that kind and title exists. members are ordered node ids: entities for structural kinds (class, object, package, component, deployment; composite and state take exactly one), requirements for flow kinds (use-case, activity, sequence, communication, overview; order is the flow order), one entity then requirements for timing. query {scope?, parent?, stereotype?, depth?} adds membership by rule at every commit. collapse lists members (or their ancestors) shown as one node despite their children. excluded is [{id, note}]. reasoning says why the view exists; every id must exist.",
            parameters: obj(
                json!({
                    "kind": {"type": "string", "enum": VIEW_KINDS},
                    "title": {"type": "string"},
                    "members": {"type": "array", "items": {"type": "string"}},
                    "query": query_schema(),
                    "collapse": {"type": "array", "items": {"type": "string"}},
                    "excluded": {"type": "array", "items": exclusion_schema()},
                    "reasoning": {"type": "string"}
                }),
                &["kind", "title", "reasoning"],
            ),
        },
        ToolDef {
            name: "update_view",
            description: "Update a view. members replaces the whole ordered list; add_members and remove_members edit it; exclude adds one {id, note} pair (the member leaves the list and stays out). Any field on a default view makes it curated from then on.",
            parameters: obj(
                json!({
                    "id": {"type": "string"},
                    "title": {"type": "string"},
                    "members": {"type": "array", "items": {"type": "string"}},
                    "add_members": {"type": "array", "items": {"type": "string"}},
                    "remove_members": {"type": "array", "items": {"type": "string"}},
                    "query": query_schema(),
                    "collapse": {"type": "array", "items": {"type": "string"}},
                    "exclude": exclusion_schema(),
                    "reasoning": {"type": "string"}
                }),
                &["id"],
            ),
        },
        ToolDef {
            name: "delete_view",
            description: "Delete a curated view. Refused on a default view, which the next commit would derive again: exclude its members or collapse them instead.",
            parameters: obj(json!({"id": {"type": "string"}, "reason": {"type": "string"}}), &["id", "reason"]),
        },
        ToolDef {
            name: "edit_fact",
            description: "Set one authored field on one node: a requirement's statement, edges, transition, or facets; an entity's definition, stereotype, parent, or an attribute's type or value (field attributes.<name>.type or attributes.<name>.value); a view's members. On a quote-provenanced fact, note carries the sentence rewrite the person accepted in conversation: the prose replacement and the graph mutation commit as one dual write. Without an accepted sentence, or on a derived or decreed fact, the edit lands graph-only with decree provenance (note becomes the decree's note) and a ratification proposal follows. value is the field's new content: a string, or the array or object the write tools take.",
            parameters: obj(
                json!({
                    "id": {"type": "string"},
                    "field": {"type": "string"},
                    "value": {},
                    "note": {"type": "string"}
                }),
                &["id", "field", "value"],
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
            description: "Record the task done. manifest.files lists every deliverable-relative file written; manifest.tests binds each requirement to its test: {requirement, kind: programmatic|llm, label, artifact, name, run, cwd?, files?}. For an llm row, artifact is the criteria file you wrote (front matter with the requirement id and statement hash, the statement, the quote, the implementing paths, the confirm steps, the verdict contract), path relative to the deliverable. Recording an identical manifest keeps existing verdicts. manifest.build is the one command that produces the deliverable's artifact when its medium must be built (a slide deck, a PDF, a rendered site, a binary): {run, cwd?, produces: [paths]}, run from the deliverable directory before any test; omit it when the written files are themselves the deliverable, and reuse the build the begin_generation package already carries rather than recording a second one. Pass the factHash from the begin_generation package. choices lists what you had to invent, each {choice, scope: product|behavior|detail, reasoning, requirements?}; every entry lands as an invented-choice diagnostic graded by scope. Next: run_tests to verify.",
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
                    }},
                    "choices": {"type": "array", "items": {"type": "object"}}
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
            name: "mark_goal_done",
            description: "Claim one goal of the batch resolved. justification is mandatory and concise: one or two sentences of why the gate holds, never an essay. evidence carries what the kind's gate reads (a verdict per neighbor for rejudge-pair, per attribute for conform-instance). The claim is validated against the goal kind's gate over the store plus the staged work, and a false one is rejected with the gate named.",
            parameters: obj(
                json!({"goal": {"type": "string"}, "justification": {"type": "string"}, "evidence": {}}),
                &["goal", "justification"],
            ),
        },
        ToolDef {
            name: "mark_goal_failed",
            description: "Record that one goal of the batch cannot honestly be accomplished, with a one or two sentence reason. Always accepted; a failed mandatory goal blocks convergence, a failed optional goal is recorded and stands.",
            parameters: obj(
                json!({"goal": {"type": "string"}, "reason": {"type": "string"}}),
                &["goal", "reason"],
            ),
        },
        ToolDef {
            name: "load_skill",
            description: "Bring one skill into the session by name (extraction, judgment, flow-views, structural-views, abstraction, conformance). The reply renders the payload; at most four skills render in one session.",
            parameters: obj(json!({"name": {"type": "string"}}), &["name"]),
        },
        ToolDef {
            name: "done",
            description: "End the session and request commit of the staged mutations. Every mandatory goal of the batch must be marked done or failed first; the batch gates run and a failure names the repair. summary is one line.",
            parameters: obj(json!({"summary": {"type": "string"}}), &["summary"]),
        },
    ]
}

pub const READ_TOOLS: [&str; 9] = [
    "load",
    "expand",
    "unload",
    "graph_status",
    "search",
    "read_section",
    "get_entity",
    "get_view",
    "diagnostics",
];
// The goal tools every session sees. Mirrors docs/compiler/tools.md#goal-tools.
pub const GOAL_TOOLS: [&str; 4] = ["mark_goal_done", "mark_goal_failed", "load_skill", "done"];
pub const GEN_TOOLS: [&str; 3] = ["generation_tasks", "begin_generation", "record_generation"];
pub const BIND_TOOLS: [&str; 3] = ["binding_tasks", "begin_binding", "record_binding"];
pub const VERIFY_TOOLS: [&str; 4] = [
    "verification_tasks",
    "begin_verification",
    "run_tests",
    "record_verdict",
];
// In-process only: a generation turn's file and command tools, never served over MCP.
pub const FILE_TOOLS: [&str; 4] = [
    "read_text_file",
    "write_text_file",
    "list_files",
    "run_command",
];
// The view tools, served beside the write tools. See docs/compiler/tools.md#view-tools.
pub const VIEW_TOOLS: [&str; 3] = ["upsert_view", "update_view", "delete_view"];
// The chat serving's human paths, in no session's toolset and never a raw write.
// See docs/compiler/tools.md#chat-tools.
pub const CHAT_TOOLS: [&str; 1] = ["edit_fact"];
// The judged rules a session may file. Mirrors docs/compiler/model/diagnostic.md#rules-catalog.
pub const REVIEW_RULES: [&str; 8] = [
    "contradiction",
    "duplicate-entity",
    "duplicate-requirement",
    "missing-link",
    "ambiguity",
    "lint",
    "decision",
    "nonconformant-instance",
];
// Feedback about jazyk itself, not about the graph: served in every toolset, read-only
// MCP included. See docs/compiler/tools.md#feedback-tool.
pub const FEEDBACK_TOOL: &str = "report_feedback";
// Feedback entries one session may record. Past it the call is acknowledged without a
// record, so a confused model cannot flood the log.
pub const FEEDBACK_LIMIT: usize = 5;

// The session toolset for a goal batch: the union of the kinds' write slices plus
// the always-on set (the read tools, the goal tools, report_feedback).
// Mirrors docs/compiler/tools.md#toolsets.
pub fn toolset_for_kinds(kinds: &[&str]) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = READ_TOOLS.to_vec();
    for k in kinds {
        if let Some(kind) = crate::goals::kind(k) {
            for t in kind.toolset() {
                if !v.contains(t) {
                    v.push(t);
                }
            }
        }
    }
    for t in GOAL_TOOLS {
        if !v.contains(&t) {
            v.push(t);
        }
    }
    v.push(FEEDBACK_TOOL);
    v
}

// The task names of the per-target serving, and the serving surfaces (`mcp-*`).
pub fn toolset(task: &str) -> Vec<&'static str> {
    // A legacy task name routes through its goal kind's slice.
    if let Some(kind) = match task {
        "align-doc" => Some("place-anchors"),
        "reconcile-doc" => Some("reconcile-section"),
        "review-requirement" => Some("rejudge-pair"),
        "review-entity" => Some("review-entity"),
        "bind-requirement" => Some("bind"),
        "generate-entity" => Some("generate"),
        "verify-requirement" => Some("verify"),
        _ => None,
    } {
        let mut v = toolset_for_kinds(&[kind]);
        // The in-process ledger workers get the file and command tools.
        if matches!(task, "generate-entity" | "bind-requirement") {
            for t in FILE_TOOLS {
                if !v.contains(&t) {
                    v.push(t);
                }
            }
        }
        return v;
    }
    // A goal kind's own name (the task of a WorkItem built from a goal whose kind has
    // no legacy task, every GC kind among them) is its kind's slice.
    if crate::goals::kind(task).is_some() {
        return toolset_for_kinds(&[task]);
    }
    let mut v = match task {
        // MCP servings. Mirrors docs/compiler/tools.md#toolsets.
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
        "mcp-compile" => {
            let mut v = READ_TOOLS.to_vec();
            v.extend([
                "upsert_entity",
                "update_entity",
                "delete_entity",
                "merge_entities",
                "group_entities",
                "dissolve_entity",
                "upsert_requirement",
                "update_requirement",
                "delete_requirement",
                "set_coverage",
                "report_diagnostic",
                "update_diagnostic",
                "resolve_diagnostic",
                "place_anchor",
                "orphan_anchor",
            ]);
            v.extend(VIEW_TOOLS);
            v.extend(GOAL_TOOLS);
            v
        }
        "mcp-write" => catalog()
            .iter()
            .map(|t| t.name)
            .filter(|n| {
                !GOAL_TOOLS.contains(n) && !FILE_TOOLS.contains(n) && !CHAT_TOOLS.contains(n)
            })
            .collect(),
        _ => READ_TOOLS.to_vec(),
    };
    if !v.contains(&FEEDBACK_TOOL) {
        v.push(FEEDBACK_TOOL);
    }
    v
}

// The JSON schema of one requirement edge: directional, typed from the relationship
// catalog, with an optional cardinality. Mirrors docs/compiler/model/requirement.md#edges.
fn edge_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "a": {"type": "string"},
            "b": {"type": "string"},
            "type": {"type": "string", "enum": REL_TYPES},
            "cardinality": {"type": "string", "enum": crate::model::CARDINALITIES}
        },
        "required": ["a", "b"]
    })
}

// The derived provenance a session may stage: the upstream nodes and the reasoning.
// Mirrors docs/compiler/model.md#provenance.
fn derived_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "derived": {"type": "object", "properties": {
                "from": {"type": "array", "items": {"type": "string"}},
                "reasoning": {"type": "string"}
            }, "required": ["from", "reasoning"]}
        },
        "required": ["derived"]
    })
}

// One attribute of an entity. Mirrors docs/compiler/model/entity.md#fields.
fn attribute_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "type": {"type": "string"},
            "value": {"type": "string"},
            "provenance": {"type": "object", "properties": {"section": {"type": "string"}, "quote": {"type": "string"}}, "required": ["section", "quote"]}
        },
        "required": ["name"]
    })
}

// The state change a requirement describes. Mirrors docs/compiler/model/requirement.md#transition.
fn transition_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "subject": {"type": "string"},
            "from": {"type": "string"},
            "to": {"type": "string"},
            "trigger": {"type": "string"},
            "guard": {"type": "string"}
        },
        "required": ["subject", "from", "to"]
    })
}

// One judged facet. Mirrors docs/compiler/model/requirement.md#facets.
fn facet_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "facet": {"type": "string", "enum": FACETS},
            "reasoning": {"type": "string"},
            "measure": {"type": "string"}
        },
        "required": ["facet", "reasoning"]
    })
}

// Membership by rule. Mirrors docs/compiler/model/view.md#fields.
fn query_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "scope": {"type": "string"},
            "parent": {"type": "string"},
            "stereotype": {"type": "string"},
            "depth": {"type": "integer", "minimum": 0}
        }
    })
}

fn exclusion_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"id": {"type": "string"}, "note": {"type": "string"}},
        "required": ["id", "note"]
    })
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

// A statement is free-form; the only shape gate is that it says something.
fn statement_present(statement: &str) -> Result<(), ToolError> {
    if statement.trim().is_empty() {
        return Err(ToolError::new(
            "bad-args",
            "statement is empty; state the one obligation the sentence carries".into(),
        ));
    }
    Ok(())
}

// Parse the edges argument of a requirement call: directional, typed from the catalog,
// cardinality one of the four forms. `entities` restricts the ends when given.
fn parse_edges(
    session: &ToolSession,
    arr: &[Value],
    entities: Option<&[String]>,
) -> Result<Vec<ReqEdge>, ToolError> {
    let mut edges = Vec::new();
    for e in arr {
        // Empty means absent (docs/compiler/tools.md#validation-and-errors): an
        // all-empty item is a filled-in blank, not an edge.
        if !ToolSession::present(e) {
            continue;
        }
        let raw_a = e["a"].as_str().unwrap_or_default();
        let raw_b = e["b"].as_str().unwrap_or_default();
        let a = session
            .canon_entity_id(raw_a)
            .unwrap_or_else(|| raw_a.to_string());
        let b = session
            .canon_entity_id(raw_b)
            .unwrap_or_else(|| raw_b.to_string());
        if let Some(listed) = entities {
            if !listed.contains(&a) || !listed.contains(&b) {
                return Err(ToolError::new(
                    "bad-edge",
                    format!(
                        "edge {}~{} may only tie entities the requirement itself references",
                        a, b
                    ),
                ));
            }
        }
        let t = e["type"]
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(t) = &t {
            if !REL_TYPES.contains(&t.as_str()) {
                return Err(ToolError::new(
                    "bad-edge",
                    format!(
                        "unknown relationship type `{}`; one of: {}",
                        t,
                        REL_TYPES.join(", ")
                    ),
                ));
            }
        }
        let cardinality = e["cardinality"]
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(c) = &cardinality {
            if !crate::model::CARDINALITIES.contains(&c.as_str()) {
                return Err(ToolError::new(
                    "bad-cardinality",
                    format!(
                        "unknown cardinality `{}`; one of: {}",
                        c,
                        crate::model::CARDINALITIES.join(", ")
                    ),
                ));
            }
        }
        if a == b {
            return Err(ToolError::new(
                "bad-edge",
                format!(
                    "edge {}~{} ties an entity to itself; a and b are distinct",
                    a, b
                ),
            ));
        }
        // Edges are directional: the same pair in the other direction, or under
        // another type, is another edge. The same (a, b, type) again is the same one.
        if edges
            .iter()
            .any(|x: &ReqEdge| x.a == a && x.b == b && x.rel_type == t)
        {
            continue;
        }
        edges.push(ReqEdge {
            a,
            b,
            rel_type: t,
            cardinality,
        });
    }
    Ok(edges)
}

// Parse the facets argument: each names a facet from the catalog with its reasoning;
// `measure` is accepted only on `quality`. Mirrors docs/compiler/model/requirement.md#facets.
fn parse_facets(v: &Value) -> Result<Vec<Facet>, ToolError> {
    let Some(arr) = v.as_array() else {
        return Err(ToolError::new(
            "bad-facet",
            "facets is a list of {facet, reasoning, measure?}".into(),
        ));
    };
    let mut out: Vec<Facet> = Vec::new();
    for f in arr {
        // Empty means absent (docs/compiler/tools.md#validation-and-errors): an
        // all-empty item is a filled-in blank, not a facet.
        if !ToolSession::present(f) {
            continue;
        }
        let facet = f["facet"].as_str().unwrap_or_default().trim().to_string();
        if !FACETS.contains(&facet.as_str()) {
            return Err(ToolError::new(
                "bad-facet",
                format!("unknown facet `{}`; one of: {}", facet, FACETS.join(", ")),
            ));
        }
        let reasoning = f["reasoning"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        if reasoning.is_empty() {
            return Err(ToolError::new(
                "bad-facet",
                format!("facet `{}` needs its reasoning", facet),
            ));
        }
        let measure = f["measure"]
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if measure.is_some() && facet != "quality" {
            return Err(ToolError::new(
                "bad-facet",
                format!(
                    "measure is stated only on a quality facet, not on `{}`",
                    facet
                ),
            ));
        }
        if let Some(mine) = out.iter_mut().find(|x| x.facet == facet) {
            mine.reasoning = reasoning;
            mine.measure = measure;
            continue;
        }
        out.push(Facet {
            facet,
            reasoning,
            measure,
        });
    }
    Ok(out)
}

// The membership rule of a view kind: what its members are.
// Mirrors docs/compiler/model/view.md#kinds.
fn member_rule(kind: &str) -> &'static str {
    match kind {
        "class" | "object" | "package" | "component" | "deployment" => "entities",
        "composite" | "state" => "one entity",
        "timing" => "one entity then requirements",
        _ => "requirements",
    }
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
        "heading",
        "headings",
        "code block",
        "code blocks",
        "blockquote",
        "blockquotes",
        "list item",
        "list items",
        "markdown",
        "table",
        "link",
        "bullet",
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

// The scope of one goal in the batch: what the per-goal gates key on.
// Mirrors docs/compiler/sessions.md#anatomy.
#[derive(Clone, Debug, Default)]
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
        GoalScope {
            goal: g.id.clone(),
            kind: g.kind.clone(),
            mandatory: g.mandatory,
            target: g.target.clone(),
            doc,
            sections,
            stale_anchors: list("staleAnchors"),
            proposals,
        }
    }
}

// The scope a session works in: one goal batch, or a serving surface outside one.
#[derive(Clone, Default)]
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
#[derive(Clone, Debug)]
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
    outcomes: std::collections::BTreeMap<String, GoalOutcome>,
    // The repeated-call guard, keyed per open batch: the same call with the same
    // arguments has the same answer (docs/compiler/sessions.md#repeated-calls).
    repeats: std::collections::BTreeMap<String, u32>,
    refusals: u32,
    // Feedback entries this session already recorded; capped so a confused model
    // cannot flood the log (docs/compiler/tools.md#feedback-tool).
    feedback_count: usize,
    mutation_limit: usize,
    default_budget: usize,
    // Staged entities (id -> entity) so lookup-before-create sees this session's own creates.
    staged_entities: std::collections::BTreeMap<String, Entity>,
    // The session's own staged findings, by their stage-minted ids: reads see
    // them, and a re-report answers with the id it updates.
    staged_diags: std::collections::BTreeMap<String, Diagnostic>,
    staged_reqs: BTreeSet<String>,
    // Staged views (id -> view) so a repeated upsert lands on the staged one and
    // update_view sees this session's own creates.
    staged_views: std::collections::BTreeMap<String, View>,
    // Parents this session set on existing entities: the cycle gate reads them.
    staged_parents: std::collections::BTreeMap<String, String>,
    taken_ids: BTreeSet<String>,
    // True only while finish_implicit drives `done`: the implicit path commits around
    // an unmarked dirty section instead of bouncing (docs/compiler/sessions.md#budgets).
    implicit_done: bool,
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
            refusals: 0,
            feedback_count: 0,
            mutation_limit,
            default_budget,
            staged_entities: Default::default(),
            staged_diags: Default::default(),
            staged_reqs: Default::default(),
            staged_views: Default::default(),
            staged_parents: Default::default(),
            taken_ids: Default::default(),
            implicit_done: false,
        }
    }

    // The commit this session lands as: kind `session`, the batch's goal ids, and the
    // resolutions with their justifications. Mirrors docs/compiler/graph.md#journal.
    pub fn commit(&self, rounds: u32, tokens: u64) -> crate::store::Commit {
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
        crate::store::Commit {
            kind: "session".to_string(),
            batch: self.scope.goal_ids(),
            resolved,
            rounds,
            tokens,
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

    fn render_status_full(&self) -> String {
        self.loaded.render_status(
            &self.skills.index_line(),
            self.skills.rendered_chars(),
            &self.pinned_targets(),
        )
    }

    fn condensed_status(&self) -> String {
        self.loaded.render_condensed(self.skills.rendered_chars())
    }

    // Past the high-water mark, load and expand are refused with the unload
    // candidates named, until something is unloaded. Mirrors docs/compiler/context.md#policy.
    fn refuse_past_high_water(&self, what: &str) -> Result<(), ToolError> {
        if !self.loaded.over_high_water(self.skills.rendered_chars()) {
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

    fn undecided_proposals(&self, proposals: &[String]) -> Vec<String> {
        proposals
            .iter()
            .filter(|a| {
                !self.staged.iter().any(|o| {
                    matches!(o, Op::PlaceAnchor { id, .. } | Op::OrphanAnchor { id, .. } if id == *a)
                })
            })
            .cloned()
            .collect()
    }

    // Stale anchors are a contract. Each must be re-anchored (its quote locates
    // again), re-recorded under its natural key, revised, or deleted. An anchor
    // flagged for re-evaluation owes a decision even though its quote locates.
    fn untouched_stale(&self, anchors: &[String]) -> Vec<String> {
        let mut untouched: Vec<String> = Vec::new();
        for a in anchors {
            let Some(r) = self.snapshot.graph.requirements.get(a) else {
                continue;
            };
            let Some(src) = r.source.as_ref() else {
                continue;
            };
            if self
                .snapshot
                .quote_locates(&src.doc, &src.section, &src.quote)
                && !self.snapshot.status.reevaluate.contains(a)
            {
                continue;
            }
            let addressed = self.staged.iter().any(|o| match o {
                Op::UpdateRequirement { id, .. } | Op::DeleteRequirement { id, .. } => id == a,
                // A staged create that resolved to the anchor carries its id; the
                // statement-equality fallback covers pre-resolution stages.
                Op::CreateRequirement { id, requirement } => {
                    id == a
                        || (requirement.anchored_at(&src.doc, &src.section)
                            && crate::store::normalize_statement(&requirement.statement)
                                == crate::store::normalize_statement(&r.statement))
                }
                _ => false,
            });
            if !addressed {
                untouched.push(a.clone());
            }
        }
        untouched
    }

    fn unmarked_sections(&self, doc: &str, sections: &[String]) -> Vec<String> {
        sections
            .iter()
            .filter(|sec| {
                let recorded = self
                    .snapshot
                    .docs
                    .get(doc)
                    .map(|d| d.coverage.contains_key(*sec))
                    .unwrap_or(false);
                let staged = self.staged.iter().any(|o| {
                    matches!(o, Op::SetCoverage { doc: d, section, .. } if d == doc && section == *sec)
                });
                !recorded && !staged
            })
            .cloned()
            .collect()
    }

    // A `covered` claim is honest only when a requirement is sourced from that
    // section, staged or recorded.
    fn dishonest_covered(&self) -> Option<ToolError> {
        for op in &self.staged {
            if let Op::SetCoverage {
                doc,
                section,
                state,
                ..
            } = op
            {
                if state != "covered" {
                    continue;
                }
                let has_req = self
                    .snapshot
                    .graph
                    .requirements
                    .values()
                    .any(|r| r.anchored_at(doc, section))
                    || self.staged.iter().any(|o| match o {
                        Op::CreateRequirement { requirement, .. } => {
                            requirement.anchored_at(doc, section)
                        }
                        _ => false,
                    });
                if !has_req {
                    return Some(ToolError::new(
                        "uncovered-claim",
                        format!(
                            "{}#{} is claimed covered but no requirement is sourced from it; extract from its sentences (state the obligation, keep the quote verbatim), or mark the section non-normative with a note",
                            doc, section
                        ),
                    ));
                }
            }
        }
        None
    }

    // One goal's gate over the store plus the staged work: the kind's batch gate and
    // the scope gates that key on the owning goal. mark_goal_done validates against
    // this and done re-validates every resolution.
    // Mirrors docs/compiler/tools.md#goal-tools.
    fn goal_gate(&self, gs: &GoalScope) -> Result<(), ToolError> {
        if let Some(k) = crate::goals::kind(&gs.kind) {
            if let Some(v) = k.gates(&self.snapshot, &self.staged).into_iter().next() {
                return Err(ToolError::new(&v.rule, v.message));
            }
        }
        // The fan-out variant's own level faces its count even when the changeset left
        // it alone. Mirrors docs/compiler/goals/abstract-entity.md#the-fan-out-gate.
        if gs.kind == "abstract-entity" {
            if let Some(v) =
                crate::goals::fan_out_goal_gate(&self.snapshot, &self.staged, &gs.target)
            {
                return Err(ToolError::new(&v.rule, v.message));
            }
        }
        if gs.kind == "place-anchors" {
            let undecided = self.undecided_proposals(&gs.proposals);
            if !undecided.is_empty() {
                return Err(ToolError::new(
                    "undecided-proposal",
                    format!(
                        "{}: proposals left undecided: {}; place_anchor or orphan_anchor each",
                        gs.goal,
                        undecided.join(", ")
                    ),
                ));
            }
        }
        if gs.kind == "reconcile-section" {
            let untouched = self.untouched_stale(&gs.stale_anchors);
            if !untouched.is_empty() {
                return Err(ToolError::new(
                    "stale-anchor",
                    format!(
                        "{}: stale anchors left untouched: {}; re-record, revise, or delete each",
                        gs.goal,
                        untouched.join(", ")
                    ),
                ));
            }
            if let Some(doc) = &gs.doc {
                let unmarked = self.unmarked_sections(doc, &gs.sections);
                if !unmarked.is_empty() {
                    return Err(ToolError::new(
                        "unmarked-section",
                        format!(
                            "{}: section(s) without a coverage mark: {}; set_coverage covered (a requirement must be sourced from it) or non-normative with a note",
                            gs.goal,
                            unmarked.join(", ")
                        ),
                    ));
                }
            }
            if let Some(e) = self.dishonest_covered() {
                return Err(e);
            }
        }
        Ok(())
    }

    // A sentence boundary is [.!?] followed by the end of the text, or by whitespace
    // and a sentence opener (an uppercase letter, a digit, a quote, a bracket), so a
    // dot inside `customer.md` or a versioned id never counts a phantom sentence and
    // an abbreviation running into lowercase (`vs. its member`, `e.g. the cart`)
    // never ends one (a naive split trained models to strip document names and
    // plain English from justifications, three bounced claims for one `vs.`).
    fn sentence_count(text: &str) -> usize {
        let chars: Vec<char> = text.chars().collect();
        let mut count = 0usize;
        let mut has_content = false;
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if matches!(c, '.' | '!' | '?') {
                let mut j = i + 1;
                while j < chars.len() && matches!(chars[j], '.' | '!' | '?') {
                    j += 1;
                }
                let at_end = j >= chars.len();
                let boundary = at_end
                    || (chars[j].is_whitespace() && {
                        let mut k = j;
                        while k < chars.len() && chars[k].is_whitespace() {
                            k += 1;
                        }
                        k >= chars.len()
                            || chars[k].is_uppercase()
                            || chars[k].is_ascii_digit()
                            || matches!(chars[k], '"' | '\'' | '(' | '[' | '`' | '«')
                    });
                if boundary {
                    if has_content {
                        count += 1;
                        has_content = false;
                    }
                    i = j;
                    continue;
                }
            }
            if !c.is_whitespace() {
                has_content = true;
            }
            i += 1;
        }
        if has_content {
            count += 1;
        }
        count
    }

    fn mark_goal_done(&mut self, args: &Value) -> Result<Value, ToolError> {
        let goal = Self::str_arg(args, "goal")?;
        let Some(gs) = self.scope.goal(&goal).cloned() else {
            return Err(ToolError::new(
                "unknown-goal",
                format!(
                    "`{}` is not a goal of this batch ({}); only goals in the batch can be marked",
                    goal,
                    self.scope.goal_ids().join(", ")
                ),
            ));
        };
        let justification = Self::str_arg(args, "justification")?;
        let sentences = Self::sentence_count(&justification);
        if sentences > 2 {
            return Err(ToolError::new(
                "bad-justification",
                format!(
                    "the justification for {} is {} sentences; one or two saying why the gate holds, never an essay",
                    goal, sentences
                ),
            ));
        }
        self.goal_gate(&gs)?;
        self.outcomes.insert(
            goal.clone(),
            GoalOutcome::Done {
                justification,
                evidence: args["evidence"].clone(),
            },
        );
        let open: Vec<&str> = self
            .scope
            .goals
            .iter()
            .filter(|g| !self.outcomes.contains_key(&g.goal))
            .map(|g| g.goal.as_str())
            .collect();
        Ok(json!({"marked": goal, "open": open}))
    }

    // The author a decree records: the MCP client when one is known, else the user.
    fn decree_author(&self) -> String {
        self.caller
            .client
            .clone()
            .filter(|c| !c.trim().is_empty())
            .or_else(|| std::env::var("USER").ok().filter(|u| !u.is_empty()))
            .unwrap_or_else(|| "human".to_string())
    }

    fn decree(&self, note: Option<String>) -> Provenance {
        Provenance::Decree {
            author: self.decree_author(),
            at: crate::verify::now_iso(),
            note,
        }
    }

    // The parent of an entity as this session sees it: staged first, then the snapshot.
    // The parent an entity holds once this session's staged work applies: a staged
    // move wins, a staged create carries its own, and a child of a staged dissolve
    // lands on the dissolved grouping's parent (recursively, when that parent
    // dissolves in the same changeset). Mirrors docs/compiler/tools.md#grouping-tools.
    fn parent_of(&self, id: &str) -> Option<String> {
        if let Some(p) = self.staged_parents.get(id) {
            return Some(p.clone());
        }
        if let Some(e) = self.staged_entities.get(id) {
            return e.parent.clone();
        }
        let stored = self
            .snapshot
            .graph
            .entities
            .get(self.snapshot.resolve_id(id))
            .and_then(|e| e.parent.clone())?;
        if self.staged_dissolved(&stored) {
            return self.parent_of(&stored);
        }
        Some(stored)
    }

    // Whether this session staged a dissolve of the entity.
    fn staged_dissolved(&self, id: &str) -> bool {
        self.staged
            .iter()
            .any(|o| matches!(o, Op::DissolveEntity { id: d, .. } if d == id))
    }

    // Every entity id alive once the staged work applies: the snapshot's minus the
    // ones this session deletes, absorbs, or dissolves, plus this session's creates.
    fn entities_after(&self) -> Vec<String> {
        let gone: BTreeSet<&str> = self
            .staged
            .iter()
            .filter_map(|o| match o {
                Op::DeleteEntity { id, .. }
                | Op::MergeEntities { absorb: id, .. }
                | Op::DissolveEntity { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        self.snapshot
            .graph
            .entities
            .keys()
            .filter(|id| !gone.contains(id.as_str()))
            .chain(self.staged_entities.keys())
            .cloned()
            .collect()
    }

    // An entity's scope, snapshot or staged.
    fn scope_of(&self, id: &str) -> Option<String> {
        self.staged_entities
            .get(id)
            .or_else(|| {
                self.snapshot
                    .graph
                    .entities
                    .get(self.snapshot.resolve_id(id))
            })
            .map(|e| e.scope.clone())
    }

    // The level an entity sits in, as a target: its parent's id, or `scope:<scope>`
    // for a parentless entity. Mirrors docs/compiler/concepts/levels.md#the-scope-root.
    fn level_target_of(&self, id: &str) -> String {
        match self.parent_of(id) {
            Some(p) => p,
            None => crate::store::scope_root_target(
                &self.scope_of(id).unwrap_or_else(|| "public".to_string()),
            ),
        }
    }

    // The direct children of a level once the staged work applies, in id order: the
    // entities whose parent is the node, or the parentless entities of the scope for
    // `scope:<scope>`. Mirrors docs/compiler/concepts/levels.md#levels.
    fn level_after(&self, target: &str) -> Vec<String> {
        let mut ids = self.entities_after();
        ids.sort();
        ids.retain(|id| self.level_target_of(id) == target);
        ids
    }

    // The level view of one entity: `view:component/<slug>` or `view:class/<slug>`
    // when the snapshot or this session's staged views hold it, or the id the kind
    // rule mints at commit when the entity holds two or more children after the
    // staged work (component when the node or any child carries a structural
    // stereotype, class otherwise). Mirrors docs/compiler/model/view.md#level-views.
    fn level_view_of(&self, id: &str) -> Option<String> {
        let slug = crate::derive::entity_slug(id);
        for kind in ["component", "class"] {
            let vid = format!("view:{}/{}", kind, slug);
            if self.view_known(&vid).is_some() {
                return Some(vid);
            }
        }
        let children = self.level_after(id);
        if children.len() < 2 {
            return None;
        }
        let structural = |x: &str| {
            self.staged_entities
                .get(x)
                .or_else(|| self.snapshot.graph.entities.get(x))
                .and_then(|e| e.stereotype.as_deref())
                .is_some_and(|s| {
                    matches!(
                        s.trim().to_lowercase().as_str(),
                        "system" | "component" | "service" | "interface" | "actor"
                    )
                })
        };
        let kind = if structural(id) || children.iter().any(|c| structural(c)) {
            "component"
        } else {
            "class"
        };
        Some(format!("view:{}/{}", kind, slug))
    }

    // The entities a view draws: a structural view's entity members, a flow view's
    // participants (its member requirements' entities), in member order, deduped.
    fn drawn_entities(&self, v: &View) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for m in &v.members {
            let rid = self.snapshot.resolve_id(m).to_string();
            if self.snapshot.graph.entities.contains_key(&rid) {
                if !out.contains(&rid) {
                    out.push(rid);
                }
            } else if let Some(r) = self.snapshot.graph.requirements.get(&rid) {
                for e in &r.entities {
                    let e = self.snapshot.resolve_id(e).to_string();
                    if !out.contains(&e) {
                        out.push(e);
                    }
                }
            }
        }
        out
    }

    // The drill-down links of a view: for every entity it draws that has a level
    // view, `{member, view}`. Computed at call time from the containment tree and the
    // level views, never stored. Mirrors docs/compiler/model/view.md#fields.
    fn view_children(&self, v: &View) -> Vec<Value> {
        self.drawn_entities(v)
            .into_iter()
            .filter_map(|m| {
                self.level_view_of(&m)
                    .map(|view| json!({"member": m, "view": view}))
            })
            .collect()
    }

    // The fan-out goals a grouping or a dissolve will open at commit: every level the
    // staged ops touch (the parent a grouping joins, the parents a move leaves and
    // joins, the parent a dissolve's children land on, the grouping's own level) is
    // counted after the changeset against `children-per-entity`, and a level past
    // the soft or hard threshold previews `abstract-entity` on its target.
    // Mirrors docs/compiler/reconciler.md#bubbling and #fan-out.
    fn level_opens(&self, ops: &[Op]) -> Vec<String> {
        use crate::limits::{threshold, CHILDREN_PER_ENTITY};
        let mut targets: Vec<String> = Vec::new();
        let mut touch = |t: String| {
            if !targets.contains(&t) {
                targets.push(t);
            }
        };
        for op in ops {
            match op {
                Op::CreateEntity { id, entity } => {
                    touch(match &entity.parent {
                        Some(p) => p.clone(),
                        None => crate::store::scope_root_target(&entity.scope),
                    });
                    touch(id.clone());
                }
                Op::UpdateEntity {
                    id,
                    parent: Some(p),
                    ..
                } => {
                    touch(p.clone());
                    if let Some(prior) = self
                        .snapshot
                        .graph
                        .entities
                        .get(id)
                        .and_then(|e| e.parent.clone())
                    {
                        touch(prior);
                    }
                }
                Op::DissolveEntity { id, .. } => {
                    if let Some(e) = self.snapshot.graph.entities.get(id) {
                        touch(match &e.parent {
                            Some(p) => p.clone(),
                            None => crate::store::scope_root_target(&e.scope),
                        });
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for t in targets {
            if self.staged_dissolved(&t) {
                continue;
            }
            let count = self.level_after(&t).len() as u64;
            let bump = self
                .snapshot
                .graph
                .entities
                .get(&t)
                .and_then(|e| e.limits.get(CHILDREN_PER_ENTITY))
                .map(|b| b.value);
            let Some((soft, hard)) = threshold(CHILDREN_PER_ENTITY, bump) else {
                continue;
            };
            if count > hard {
                out.push(format!(
                    "abstract-entity {} (fan-out {} over hard {}, mandatory)",
                    t, count, hard
                ));
            } else if count > soft {
                out.push(format!(
                    "abstract-entity {} (fan-out {} over soft {}, optional)",
                    t, count, soft
                ));
            }
        }
        out
    }

    // Gate a `parent` argument: it must resolve, and the containment tree stays
    // acyclic (an entity is never its own ancestor). `child` is the entity being
    // parented, absent for a create. Mirrors docs/compiler/graph.md#validation-gates.
    fn check_parent(&self, child: Option<&str>, raw: &str) -> Result<String, ToolError> {
        let Some(p) = self.canon_entity_id(raw) else {
            let e = self.unknown_entity_error(raw);
            return Err(ToolError::new(
                "unknown-parent",
                format!("parent {}", e.message),
            ));
        };
        if let Some(c) = child {
            let mut chain = vec![p.clone()];
            let mut cur = p.clone();
            for _ in 0..64 {
                if cur == c {
                    return Err(ToolError::new(
                        "parent-cycle",
                        format!(
                            "{} cannot be under {}: the chain {} leads back to it; an entity is never its own ancestor",
                            c,
                            p,
                            chain.join(" > ")
                        ),
                    ));
                }
                match self.parent_of(&cur) {
                    Some(next) => {
                        chain.push(next.clone());
                        cur = next;
                    }
                    None => break,
                }
            }
        }
        Ok(p)
    }

    // Parse a transition argument: the subject resolves, is among the requirement's
    // entities, and the two states are named. Mirrors docs/compiler/model/requirement.md#transition.
    fn parse_transition(&self, v: &Value, entities: &[String]) -> Result<Transition, ToolError> {
        if !v.is_object() {
            return Err(ToolError::new(
                "bad-transition",
                "transition is {subject, from, to, trigger?, guard?}".into(),
            ));
        }
        let raw = v["subject"].as_str().unwrap_or_default().trim();
        if raw.is_empty() {
            return Err(ToolError::new(
                "bad-transition",
                "transition.subject is empty; name the subject entity, or omit `transition` entirely"
                    .into(),
            ));
        }
        let Some(subject) = self.canon_entity_id(raw) else {
            let e = self.unknown_entity_error(raw);
            return Err(ToolError::new(
                "unknown-id",
                format!("transition subject: {}", e.message),
            ));
        };
        if !entities.contains(&subject) {
            return Err(ToolError::new(
                "bad-transition",
                format!(
                    "transition subject {} is not among the requirement's entities ({}); list it there",
                    subject,
                    entities.join(", ")
                ),
            ));
        }
        let state = |k: &str| -> Result<String, ToolError> {
            v[k].as_str()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    let coach = if k == "from" {
                        " Name the state the documents place the subject in before the trigger; the event that creates the subject names its initial state. When no sentence states one, omit `transition` entirely."
                    } else {
                        ""
                    };
                    ToolError::new(
                        "bad-transition",
                        format!("transition.{} names a state; it is empty.{}", k, coach),
                    )
                })
        };
        let opt = |k: &str| {
            v[k].as_str()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };
        Ok(Transition {
            subject,
            from: state("from")?,
            to: state("to")?,
            trigger: opt("trigger"),
            guard: opt("guard"),
        })
    }

    // Parse an attributes argument: named, unique by name, each with a located quote
    // provenance or the call's own. Mirrors docs/compiler/graph.md#validation-gates.
    fn parse_attributes(
        &self,
        v: &Value,
        default: Option<&SourceRef>,
    ) -> Result<Vec<Attribute>, ToolError> {
        let Some(arr) = v.as_array() else {
            return Err(ToolError::new(
                "bad-attribute",
                "attributes is a list of {name, type?, value?, provenance?: {section, quote}}"
                    .into(),
            ));
        };
        let mut out: Vec<Attribute> = Vec::new();
        for a in arr {
            // Empty means absent (docs/compiler/tools.md#validation-and-errors): an
            // all-empty item is a filled-in blank, not an attribute.
            if !Self::present(a) {
                continue;
            }
            let name = a["name"].as_str().unwrap_or_default().trim().to_string();
            if name.is_empty() {
                return Err(ToolError::new(
                    "bad-attribute",
                    "an attribute needs a name".into(),
                ));
            }
            if out.iter().any(|x| x.name == name) {
                return Err(ToolError::new(
                    "bad-attribute",
                    format!(
                        "attribute `{}` is listed twice; attributes are unique by name",
                        name
                    ),
                ));
            }
            let opt = |k: &str| {
                a[k].as_str()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            };
            // A hollow provenance object counts as absent: the attribute falls
            // back to the call's own quote, as the schema promises.
            let provenance = if Self::present(&a["provenance"]) {
                let section = a["provenance"]["section"].as_str().unwrap_or_default();
                let quote = a["provenance"]["quote"].as_str().unwrap_or_default();
                let (doc, sec) = self.resolve_section(section)?;
                let quote = self.check_quote(&doc, &sec, quote)?;
                Provenance::Quote(SourceRef {
                    doc,
                    section: sec,
                    quote,
                })
            } else {
                match default {
                    Some(src) => Provenance::Quote(src.clone()),
                    None => {
                        return Err(ToolError::new(
                            "bad-attribute",
                            format!(
                                "attribute `{}` needs a provenance ({{section, quote}}); the call carries no quote it could take",
                                name
                            ),
                        ))
                    }
                }
            };
            out.push(Attribute {
                name,
                r#type: opt("type"),
                value: opt("value"),
                provenance,
            });
        }
        Ok(out)
    }

    // An optional argument that is empty counts as absent: a model that fills every
    // schema field with "" / [] / {} makes the same call as one that omits them.
    // Mirrors docs/compiler/tools.md#validation-and-errors.
    fn present(v: &Value) -> bool {
        match v {
            Value::Null => false,
            Value::String(s) => !s.trim().is_empty(),
            Value::Array(a) => a.iter().any(Self::present),
            Value::Object(o) => o.values().any(Self::present),
            _ => true,
        }
    }

    // Parse a provenance argument a session may stage: a derivation naming live nodes
    // with its reasoning. A decree is refused: only a human path stages one.
    fn parse_derived(&self, v: &Value) -> Result<Provenance, ToolError> {
        if v["decree"].is_object() {
            return Err(ToolError::new(
                "bad-provenance",
                "a session never stages a decree; pass provenance.derived {from, reasoning}, or a mention".into(),
            ));
        }
        let d = &v["derived"];
        if !d.is_object() {
            return Err(ToolError::new(
                "bad-provenance",
                "provenance is {derived: {from: [ids], reasoning}}".into(),
            ));
        }
        let mut from: Vec<String> = Vec::new();
        for raw in Self::str_list(d, "from") {
            let Some(id) = self.node_known(&raw) else {
                return Err(ToolError::new(
                    "unknown-id",
                    format!(
                        "provenance.derived.from names `{}`, which does not exist",
                        raw
                    ),
                ));
            };
            if !from.contains(&id) {
                from.push(id);
            }
        }
        if from.is_empty() {
            return Err(ToolError::new(
                "bad-provenance",
                "provenance.derived.from names at least one live node".into(),
            ));
        }
        let reasoning = d["reasoning"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        if reasoning.is_empty() {
            return Err(ToolError::new(
                "bad-provenance",
                "provenance.derived.reasoning says why the fact holds".into(),
            ));
        }
        Ok(Provenance::Derived { from, reasoning })
    }

    // An entity or requirement id, snapshot or staged, canonicalized.
    fn node_known(&self, raw: &str) -> Option<String> {
        if let Some(id) = self.canon_entity_id(raw) {
            return Some(id);
        }
        self.canon_req_id(raw).ok()
    }

    // A view, snapshot or staged, by id.
    fn view_known(&self, raw: &str) -> Option<(String, View)> {
        let id = raw.trim();
        if let Some(v) = self.staged_views.get(id) {
            return Some((id.to_string(), v.clone()));
        }
        self.snapshot
            .graph
            .views
            .get(id)
            .map(|v| (id.to_string(), v.clone()))
    }

    // The staged view a natural key lands on: kind plus normalized title.
    fn staged_view_by_key(&self, kind: &str, title: &str) -> Option<String> {
        let norm = |s: &str| {
            s.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        };
        let want = norm(title);
        self.staged_views
            .iter()
            .find(|(_, v)| v.kind == kind && norm(&v.title) == want)
            .map(|(id, _)| id.clone())
    }

    // Canonicalize a list of member ids, each an existing entity or requirement.
    fn canon_members(&self, raw: &[String], what: &str) -> Result<Vec<String>, ToolError> {
        let mut out = Vec::new();
        for r in raw {
            let Some(id) = self.node_known(r) else {
                return Err(ToolError::new(
                    "unknown-id",
                    format!(
                        "{} `{}` does not exist; every view member is an existing entity or requirement id",
                        what, r
                    ),
                ));
            };
            out.push(id);
        }
        Ok(out)
    }

    fn is_entity_id(&self, id: &str) -> bool {
        self.known_entity(id)
    }

    // Gate a view's membership: unique ordered members that follow the kind's rule,
    // and collapse ids that are members or ancestors of members.
    // Mirrors docs/compiler/graph.md#validation-gates.
    fn check_view_members(
        &self,
        kind: &str,
        members: &[String],
        collapse: &[String],
    ) -> Result<(), ToolError> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for m in members {
            if !seen.insert(m.as_str()) {
                return Err(ToolError::new(
                    "duplicate-member",
                    format!(
                        "member {} is listed twice; members are unique and ordered",
                        m
                    ),
                ));
            }
        }
        let entities = members.iter().filter(|m| self.is_entity_id(m)).count();
        let requirements = members.len() - entities;
        let rule = member_rule(kind);
        let ok = match rule {
            "entities" => requirements == 0,
            "one entity" => members.is_empty() || (entities == 1 && requirements == 0),
            "one entity then requirements" => {
                members.is_empty() || (entities == 1 && self.is_entity_id(&members[0]))
            }
            _ => entities == 0,
        };
        if !ok {
            return Err(ToolError::new(
                "bad-member",
                format!(
                    "a {} view's members are {}; got {} entities and {} requirements",
                    kind, rule, entities, requirements
                ),
            ));
        }
        for c in collapse {
            let covers = members.iter().any(|m| {
                if m == c {
                    return true;
                }
                let mut cur = m.clone();
                for _ in 0..64 {
                    match self.parent_of(&cur) {
                        Some(p) if &p == c => return true,
                        Some(p) => cur = p,
                        None => return false,
                    }
                }
                false
            });
            if !covers {
                return Err(ToolError::new(
                    "bad-collapse",
                    format!(
                        "collapse names {}, which is neither a member nor an ancestor of one",
                        c
                    ),
                ));
            }
        }
        Ok(())
    }

    // Parse a view query argument. Mirrors docs/compiler/model/view.md#fields.
    // An all-empty query object is a filled-in blank, not a match-everything rule:
    // empty means absent (docs/compiler/tools.md#validation-and-errors), so a query
    // naming no scope, parent, or stereotype parses as no query at all.
    fn parse_query(&self, v: &Value) -> Result<Option<ViewQuery>, ToolError> {
        if !v.is_object() {
            return Err(ToolError::new(
                "bad-args",
                "query is {scope?, parent?, stereotype?, depth?}".into(),
            ));
        }
        let parent = match Self::opt_str(v, "parent") {
            Some(p) => Some(self.canon_entity_id(&p).ok_or_else(|| {
                let e = self.unknown_entity_error(&p);
                ToolError::new("unknown-id", format!("query.parent: {}", e.message))
            })?),
            None => None,
        };
        let q = ViewQuery {
            scope: Self::opt_str(v, "scope"),
            parent,
            stereotype: Self::opt_str(v, "stereotype"),
            depth: v["depth"].as_u64().map(|d| d as u32),
        };
        if q.scope.is_none() && q.parent.is_none() && q.stereotype.is_none() {
            return Ok(None);
        }
        Ok(Some(q))
    }

    fn parse_exclusions(&self, v: &Value) -> Result<Vec<Exclusion>, ToolError> {
        let items: Vec<&Value> = match v {
            Value::Array(a) => a.iter().collect(),
            Value::Object(_) => vec![v],
            _ => Vec::new(),
        };
        let mut out = Vec::new();
        for x in items {
            // Empty means absent (docs/compiler/tools.md#validation-and-errors): a
            // hollow exclusion, item or singleton, is a filled-in blank.
            if !Self::present(x) {
                continue;
            }
            let raw = x["id"].as_str().unwrap_or_default().trim().to_string();
            let Some(id) = self.node_known(&raw) else {
                return Err(ToolError::new(
                    "unknown-id",
                    format!("excluded `{}` does not exist", raw),
                ));
            };
            let note = x["note"].as_str().unwrap_or_default().trim().to_string();
            if note.is_empty() {
                return Err(ToolError::new(
                    "bad-args",
                    format!("excluding {} needs a note saying why it stays out", id),
                ));
            }
            out.push(Exclusion { id, note });
        }
        Ok(out)
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
                format!(
                    "`{}` must be a relative path under the deliverable directory, without `..`",
                    rel
                ),
            ));
        }
        Ok(self.gen.deliverable.join(rel))
    }

    fn file_tool(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        match name {
            "read_text_file" => {
                let rel = Self::str_arg(args, "path")?;
                let path = self.deliverable_path(&rel)?;
                let text = std::fs::read_to_string(&path).map_err(|e| {
                    ToolError::new(
                        "not-found",
                        format!("cannot read `{}`: {}; list_files shows what exists", rel, e),
                    )
                })?;
                let start = args["line"]
                    .as_u64()
                    .map(|l| (l as usize).saturating_sub(1))
                    .unwrap_or(0);
                let limit = args["limit"]
                    .as_u64()
                    .map(|l| l as usize)
                    .unwrap_or(usize::MAX);
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
                let own = crate::gen::slug_of(&self.scope.gen_target());
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
                std::fs::write(&path, &content).map_err(|e| {
                    ToolError::new("io-error", format!("cannot write `{}`: {}", rel, e))
                })?;
                Ok(json!({"written": rel, "bytes": content.len()}))
            }
            "list_files" => {
                let rel = Self::opt_str(args, "path").unwrap_or_default();
                let root = if rel.is_empty() {
                    self.gen.deliverable.clone()
                } else {
                    self.deliverable_path(&rel)?
                };
                let mut out: Vec<String> = Vec::new();
                let mut stack = vec![root.clone()];
                while let Some(dir) = stack.pop() {
                    let Ok(entries) = std::fs::read_dir(&dir) else {
                        continue;
                    };
                    for e in entries.flatten() {
                        let p = e.path();
                        let name = e.file_name().to_string_lossy().to_string();
                        if name.starts_with('.')
                            || name == "target"
                            || name == "node_modules"
                            || name == "jazyk-out"
                        {
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
                let cwd = if cwd_rel == "." {
                    self.gen.deliverable.clone()
                } else {
                    self.deliverable_path(&cwd_rel)?
                };
                let out = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&command)
                    .current_dir(&cwd)
                    .output()
                    .map_err(|e| {
                        ToolError::new(
                            "io-error",
                            format!("cannot run `{}` in {}: {}", command, cwd.display(), e),
                        )
                    })?;
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
            let (small, big) = if ex.len() <= cand.len() {
                (&ex, &cand)
            } else {
                (&cand, &ex)
            };
            // Containment of a multi-token name, or of a single specific (long) token.
            small.is_subset(big)
                && (small.len() > 1 || small.iter().next().map(|t| t.len() >= 5).unwrap_or(false))
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
            let slug = format!(
                "ent:{}",
                raw.to_lowercase()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join("-")
            );
            if self.known_entity(&slug) {
                return Some(self.snapshot.resolve_id(&slug).to_string());
            }
        } else if let Some(rest) = raw.strip_prefix("ent:") {
            // A case or spacing variant of an existing id (`ent:factHash`) resolves to it.
            let slug = format!(
                "ent:{}",
                rest.to_lowercase()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join("-")
            );
            if self.known_entity(&slug) {
                return Some(self.snapshot.resolve_id(&slug).to_string());
            }
        }
        // Exact display name or alias, snapshot plus staged; unique match only.
        let want = raw
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if want.is_empty() {
            return None;
        }
        let norm = |n: &str| {
            n.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        };
        let mut hits: Vec<String> = Vec::new();
        let all = self
            .snapshot
            .graph
            .entities
            .iter()
            .map(|(i, e)| (i.clone(), e))
            .chain(self.staged_entities.iter().map(|(i, e)| (i.clone(), e)));
        for (id, e) in all {
            if (norm(&e.name) == want || e.aliases.iter().any(|a| norm(a) == want))
                && !hits.contains(&id)
            {
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
        let known = |id: &str| {
            self.snapshot.graph.requirements.contains_key(id) || self.staged_reqs.contains(id)
        };
        if known(raw) {
            return Ok(raw.to_string());
        }
        if !raw.starts_with("req:") {
            let prefixed = format!("req:{}", raw.trim());
            if known(&prefixed) {
                return Ok(prefixed);
            }
        }
        Err(ToolError::new(
            "unknown-id",
            format!("unknown requirement id `{}`", raw),
        ))
    }

    fn unknown_entity_error(&self, id: &str) -> ToolError {
        // A non-entity node id is a wrong-tool call, not a missing entity: say which
        // tool reads that kind instead of suggesting a create.
        for (prefix, kind) in [
            ("req:", "a requirement"),
            ("view:", "a view; get_view also reads it"),
            ("diag:", "a diagnostic; diagnostics lists it"),
            ("sm:", "a state machine"),
        ] {
            if id.starts_with(prefix) {
                return ToolError::new(
                    "unknown-id",
                    format!(
                        "`{}` is {}, not an entity; call load with target `{}`",
                        id, kind, id
                    ),
                );
            }
        }
        let bare = id.strip_prefix("ent:").unwrap_or(id).replace('-', " ");
        let hits = self.search_all(&bare);
        let hint = if hits.is_empty() {
            "search for it, or create it with upsert_entity first".to_string()
        } else {
            format!(
                "nearest existing: {}; use one of those, or create it with upsert_entity first",
                hits.iter()
                    .take(3)
                    .map(|(id, _, _)| id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        ToolError::new(
            "unknown-id",
            format!("unknown entity id `{}`; {}", id, hint),
        )
    }

    // Search across the snapshot plus this turn's staged creates.
    fn search_all(&self, query: &str) -> Vec<(String, String, String)> {
        let mut hits: Vec<(String, String, String)> = Vec::new();
        let q = query.trim().to_lowercase();
        for (id, e) in &self.staged_entities {
            if e.name.to_lowercase().contains(&q) || q.contains(&e.name.to_lowercase()) {
                hits.push((
                    id.clone(),
                    e.name.clone(),
                    e.definition.clone().unwrap_or_default(),
                ));
            }
        }
        hits.extend(self.snapshot.search(query));
        hits.truncate(8);
        hits
    }

    // Search view titles across the snapshot plus this turn's staged creates. Same
    // tiering as entity search (exact, then substring, then token overlap); views have
    // no aliases. Mirrors docs/compiler/tools.md#read-tools.
    fn search_views(&self, query: &str) -> Vec<(String, String)> {
        let q = crate::store::normalize(query);
        let q_tokens: std::collections::BTreeSet<&str> = q.split(' ').collect();
        let mut scored: Vec<(u32, String, String)> = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for (id, v) in self
            .staged_views
            .iter()
            .chain(self.snapshot.graph.views.iter())
        {
            if !seen.insert(id.clone()) {
                continue;
            }
            let n = crate::store::normalize(&v.title);
            let tier = if n == q {
                0
            } else if n.contains(&q) || q.contains(n.as_str()) {
                1
            } else {
                let n_tokens: std::collections::BTreeSet<&str> = n.split(' ').collect();
                if q_tokens.intersection(&n_tokens).count() > 0 {
                    2
                } else {
                    continue;
                }
            };
            scored.push((tier, id.clone(), v.title.clone()));
        }
        scored.sort();
        scored
            .into_iter()
            .take(8)
            .map(|(_, id, title)| (id, title))
            .collect()
    }

    // A miss is an answer, not a dead end. A bare empty list reads as "ask again";
    // models loop on it. Say what the graph holds and what to do next.
    fn search_miss(&self, query: &str) -> Value {
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
        known.truncate(25);
        let shown = known.len();
        json!({
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
        })
    }

    // Resolve a section argument: either "doc.md#/ref" or a bare "/ref" against the
    // batch's document.
    fn resolve_section(&self, section: &str) -> Result<(String, String), ToolError> {
        // A bare document name (`orders.md`, `orders.md#`) is its root section.
        // Mirrors docs/compiler/tools.md#read-tools.
        let bare = section
            .strip_suffix("#/")
            .or_else(|| section.strip_suffix('#'))
            .unwrap_or(section);
        if !bare.contains('#') {
            if let Some(rec) = self.snapshot.docs.get(bare) {
                let mut roots: Vec<&String> = rec
                    .sections
                    .iter()
                    .filter(|(_, c)| c.parent.is_none())
                    .map(|(r, _)| r)
                    .collect();
                roots.sort_by_key(|r| r.len());
                if let Some(root) = roots.first() {
                    return Ok((bare.to_string(), (*root).clone()));
                }
            }
        }
        let full = if section.starts_with('/') {
            match self.scope.doc() {
                Some(d) => format!("{}#{}", d, section),
                None => {
                    return Err(ToolError::new(
                        "bad-section",
                        format!(
                            "bare section reference `{}` needs a document; use doc.md#{}",
                            section, section
                        ),
                    ))
                }
            }
        } else {
            section.to_string()
        };
        let (doc, sec) = split_section_ref(&full).ok_or_else(|| {
            // Repair-oriented: name the sections this batch is actually working on.
            let owned: Vec<String> = self
                .scope
                .reconcile_scopes()
                .iter()
                .flat_map(|g| {
                    let d = g.doc.clone().unwrap_or_default();
                    g.sections
                        .iter()
                        .map(move |s| format!("{}#{}", d, s))
                        .collect::<Vec<_>>()
                })
                .collect();
            let hint = if owned.is_empty() {
                String::new()
            } else {
                format!("; this batch's sections: {}", owned.join(", "))
            };
            ToolError::new(
                "bad-section",
                format!(
                    "bad section reference `{}`; expected doc.md#/ref{}",
                    section, hint
                ),
            )
        })?;
        if !self
            .snapshot
            .docs
            .get(&doc)
            .map(|d| d.sections.contains_key(&sec))
            .unwrap_or(false)
        {
            return Err(ToolError::new(
                "unknown-section",
                format!("unknown section `{}#{}`", doc, sec),
            ));
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
            return Err(ToolError::new(
                "bad-quote",
                "quote is empty; copy the sentence verbatim from the section".into(),
            ));
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
            format!(
                "quote not found in {}#{}; copy the sentence verbatim from the section",
                doc, sec
            ),
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

    fn stage(&mut self, op: Op) -> Result<(), ToolError> {
        if self.staged.len() >= self.mutation_limit {
            return Err(ToolError::new(
                "mutation-budget",
                format!(
                    "turn mutation budget ({}) exhausted; call done",
                    self.mutation_limit
                ),
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
            .ok_or_else(|| {
                ToolError::new(
                    "bad-args",
                    format!("missing required string argument `{}`", key),
                )
            })
    }

    fn opt_str(args: &Value, key: &str) -> Option<String> {
        args[key]
            .as_str()
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty())
    }

    fn str_list(args: &Value, key: &str) -> Vec<String> {
        // Empty means absent (docs/compiler/tools.md#validation-and-errors): every
        // caller is an id or name list, where "" is a filled-in blank, never data.
        args[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .filter(|s| !s.trim().is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    // Parse and gate a diagnostic prompt argument: at most 4 options, label
    // required, exactly one of edit or answer per option, and an edit's old_text
    // must locate in its section. Mirrors docs/compiler/model/diagnostic.md#prompts.
    fn parse_prompt(&self, v: &Value) -> Result<Option<crate::model::DiagnosticPrompt>, ToolError> {
        use crate::model::{DiagnosticPrompt, PromptOption, SuggestedEdit};
        // Empty means absent (docs/compiler/tools.md#validation-and-errors): a
        // hollow prompt (no question, no options) was filled in, not asked. The
        // content fields decide; `freeform: false` is a filler's default, not content.
        if v.is_null() || (!Self::present(&v["question"]) && !Self::present(&v["options"])) {
            return Ok(None);
        }
        let Some(question) = v["question"].as_str().filter(|s| !s.trim().is_empty()) else {
            return Err(ToolError::new(
                "bad-prompt",
                "prompt.question is required: one sentence addressed to a person".into(),
            ));
        };
        let mut options = Vec::new();
        if let Some(arr) = v["options"].as_array() {
            if arr.len() > 4 {
                return Err(ToolError::new(
                    "bad-prompt",
                    "a prompt carries at most 4 options".into(),
                ));
            }
            for (i, o) in arr.iter().enumerate() {
                // An all-empty option item is a filled-in blank; drop it.
                if !Self::present(o) {
                    continue;
                }
                let Some(label) = o["label"].as_str().filter(|s| !s.trim().is_empty()) else {
                    return Err(ToolError::new(
                        "bad-prompt",
                        format!("option {} needs a label", i),
                    ));
                };
                let has_edit = Self::present(&o["edit"]);
                let answer = o["answer"].as_str().filter(|s| !s.trim().is_empty());
                if has_edit == answer.is_some() {
                    return Err(ToolError::new(
                        "bad-prompt",
                        format!("option {} needs exactly one of edit or answer; an option with neither is a filled-in blank, and a diagnostic that asks nothing of a human omits `prompt` entirely", i),
                    ));
                }
                let edit = if has_edit {
                    let e = &o["edit"];
                    let get = |k: &str| -> Result<String, ToolError> {
                        e[k].as_str()
                            .filter(|s| !s.trim().is_empty())
                            .map(|s| s.to_string())
                            .ok_or_else(|| {
                                ToolError::new(
                                    "bad-prompt",
                                    format!("option {}: edit.{} is required", i, k),
                                )
                            })
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
                    // An empty old_text appends new_text to the section's body.
                    let old_text = e["old_text"].as_str().unwrap_or_default().to_string();
                    let new_text = get("new_text")?;
                    let Some(rec) = self.snapshot.docs.get(&doc) else {
                        return Err(ToolError::new(
                            "bad-prompt",
                            format!("option {}: unknown document `{}`", i, doc),
                        ));
                    };
                    let Some(sec) = rec.sections.get(&section) else {
                        return Err(ToolError::new(
                            "bad-prompt",
                            format!("option {}: unknown section `{}#{}`", i, doc, section),
                        ));
                    };
                    if !old_text.trim().is_empty()
                        && crate::md::locate_bytes(&sec.raw, &old_text).is_none()
                    {
                        return Err(ToolError::new(
                            "bad-prompt",
                            format!("option {}: old_text does not locate in {}#{}; copy it verbatim from the section", i, doc, section),
                        ));
                    }
                    Some(SuggestedEdit {
                        doc,
                        section,
                        old_text,
                        new_text,
                    })
                } else {
                    None
                };
                options.push(PromptOption {
                    label: label.to_string(),
                    edit,
                    answer: answer.map(|s| s.to_string()),
                });
            }
        }
        Ok(Some(DiagnosticPrompt {
            question: question.to_string(),
            options,
            freeform: v["freeform"].as_bool().unwrap_or(false),
        }))
    }

    pub fn dispatch(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        self.loaded.next_round();
        // Targets a call names count as referenced, for the unload suggestions.
        for key in ["target", "id", "goal", "section", "ref"] {
            if let Some(v) = args[key].as_str() {
                self.loaded.note_reference(v);
            }
        }
        if let Some(h) = args["handle"].as_str() {
            if let Ok((t, _, _)) = crate::context::parse_handle(h) {
                self.loaded.note_reference(&t);
            }
        }
        // The repeated-call guard, keyed per open batch: the second identical call
        // answers with a repeat marker, the third is refused. done, mark_goal_done,
        // and mark_goal_failed are exempt: repairing a rejected claim legitimately
        // repeats it. A load of an already loaded target counts as a repeat whatever
        // its depth. Mirrors docs/compiler/sessions.md#repeated-calls.
        let exempt = matches!(name, "done" | "mark_goal_done" | "mark_goal_failed");
        if !exempt {
            let key = if name == "load" {
                format!("load|{}", args["target"].as_str().unwrap_or_default())
            } else {
                format!("{}|{}", name, args)
            };
            let seen = {
                let c = self.repeats.entry(key).or_insert(0);
                *c += 1;
                *c
            };
            if seen >= 3 {
                self.refusals += 1;
                // Past eight refusals in one batch, the serving finishes the session
                // implicitly: the staged work commits under the same gates the budget
                // path uses.
                if self.refusals > 8 && self.done.is_none() {
                    self.finish_implicit("(implicit: the session repeated itself past the guard)");
                }
                return Err(ToolError::new(
                    "repeated-call",
                    format!(
                        "this is call {} to `{}` with identical arguments; the answer has not changed. Act on the answer you already have, or finish with done. If this refusal seems wrong, say so with report_feedback.",
                        seen, name
                    ),
                ));
            }
            if seen == 2 {
                let mut v = self.dispatch_gated(name, args)?;
                if v.is_object() {
                    v["repeat"] = json!(
                        "you already made this exact call; the answer is unchanged. Act on it."
                    );
                }
                return Ok(v);
            }
        }
        self.dispatch_gated(name, args)
    }

    fn dispatch_gated(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        // Reads see the session's snapshot, not its staged mutations. Saying so on
        // every read while writes are staged stops the caller from concluding a
        // staged delete or update was lost.
        if READ_TOOLS.contains(&name) && !self.staged.is_empty() {
            return self.dispatch_inner(name, args).map(|mut v| {
                if v.is_object() {
                    v["note"] = json!("reads show the graph as the session began; this session's staged mutations apply at commit");
                }
                v
            });
        }
        let before = self.staged.len();
        let mut v = self.dispatch_inner(name, args)?;
        // A mutating reply previews the goals the mutation will open at commit and
        // re-renders the condensed status block.
        // Mirrors docs/compiler/reconciler.md#bubbling.
        if self.staged.len() > before && v.is_object() {
            let mut opens = crate::board::staged_opens(&self.snapshot, &self.staged[before..]);
            // A grouping or a dissolve moves children between levels, so the levels it
            // touches face the fan-out limit in the same preview.
            for line in self.level_opens(&self.staged[before..]) {
                if !opens.contains(&line) {
                    opens.push(line);
                }
            }
            if !opens.is_empty() {
                v["opens"] = json!(opens);
            }
            v["status"] = json!(self.condensed_status());
        }
        Ok(v)
    }

    fn dispatch_inner(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        match name {
            "load" => {
                let target = Self::str_arg(args, "target")?;
                let depth = args["depth"].as_u64().unwrap_or(1).max(1) as u32;
                self.refuse_past_high_water("load")?;
                let id = self.canon_entity_id(&target).unwrap_or(target);
                let text = self
                    .loaded
                    .load(&self.snapshot, &id, depth)
                    .map_err(|e| ToolError::new("bad-target", e))?;
                let mut v = json!({"pack": text});
                // In a serving, the first node of a kind loaded brings the kind's
                // skill, once; a batch works under its goal kinds' skills and reads
                // other nodes as reference. Mirrors docs/compiler/sessions.md#skills.
                if !self.skills.has_pinned() {
                    if let Some(skill) = crate::session::skill_for_target(&self.snapshot, &id) {
                        if let Ok(SkillLoad::Rendered(payload)) = self.skills.activate(skill) {
                            v["skill"] =
                                json!(format!("[skill: {} (active)]\n{}", skill, payload));
                        }
                    }
                }
                v["status"] = json!(self.condensed_status());
                Ok(v)
            }
            "expand" => {
                let handle = Self::str_arg(args, "handle")?;
                self.refuse_past_high_water("expand")?;
                let text = self
                    .loaded
                    .expand(&self.snapshot, &handle)
                    .map_err(|e| ToolError::new("unknown-handle", e))?;
                Ok(json!({"pack": text, "status": self.condensed_status()}))
            }
            "unload" => {
                let target = Self::str_arg(args, "target")?;
                if !self.loaded.unload(&target) {
                    return Err(ToolError::new(
                        "bad-target",
                        format!(
                            "`{}` is not loaded; graph_status lists the loaded set",
                            target
                        ),
                    ));
                }
                // A reload after an unload genuinely re-renders: clear the load count
                // for this target so the budget-forced unload->reload cycle is not
                // refused as a repeat.
                self.repeats.remove(&format!("load|{}", target));
                // Unloading the last node of a kind marks the kind's skill inactive.
                if let Some(skill) = crate::session::skill_for_target(&self.snapshot, &target) {
                    let another = self.loaded.items.iter().any(|i| {
                        crate::session::skill_for_target(&self.snapshot, &i.target) == Some(skill)
                    });
                    if !another {
                        self.skills.deactivate(skill);
                    }
                }
                Ok(json!({"unloaded": target, "status": self.condensed_status()}))
            }
            "graph_status" => Ok(json!({"status": self.render_status_full()})),
            "load_skill" => {
                let name_arg = Self::str_arg(args, "name")?;
                match self.skills.activate(&name_arg) {
                    Err(e) => Err(ToolError::new("unknown-skill", e)),
                    Ok(SkillLoad::CapReached) => Err(ToolError::new(
                        "skill-cap",
                        format!(
                            "at most {} skills render in one session; already rendered: {}",
                            self.skills.cap,
                            self.skills.rendered_names().join(", ")
                        ),
                    )),
                    Ok(SkillLoad::Rendered(payload)) => {
                        Ok(json!({"skill": name_arg, "payload": payload}))
                    }
                    Ok(_) => Ok(json!({
                        "skill": name_arg,
                        "note": "already rendered this session; its text stands in the conversation"
                    })),
                }
            }
            "get_view" => {
                let id = Self::str_arg(args, "id")?;
                let Some(v) = self.snapshot.graph.views.get(&id) else {
                    return Err(ToolError::new(
                        "unknown-id",
                        format!(
                            "unknown view id `{}`; search with kind view lists titles",
                            id
                        ),
                    ));
                };
                let slug = id.rsplit('/').next().unwrap_or_default();
                // The drill-down links: computed here from the containment tree and
                // the level views, never stored. Mirrors docs/compiler/tools.md#read-tools.
                let children = self.view_children(v);
                let reply = json!({
                    "id": id, "kind": v.kind, "title": v.title, "members": v.members,
                    "excluded": v.excluded.iter().map(|x| json!({"id": x.id, "note": x.note})).collect::<Vec<_>>(),
                    "collapse": v.collapse, "query": v.query, "default": v.default,
                    "rendering": format!("diagrams/{}/{}.svg", v.kind, slug),
                    "children": children,
                });
                self.loaded.load_stub(&self.snapshot, &id);
                Ok(reply)
            }
            "mark_goal_done" => self.mark_goal_done(args),
            "mark_goal_failed" => {
                let goal = Self::str_arg(args, "goal")?;
                if self.scope.goal(&goal).is_none() {
                    return Err(ToolError::new(
                        "unknown-goal",
                        format!(
                            "`{}` is not a goal of this batch ({})",
                            goal,
                            self.scope.goal_ids().join(", ")
                        ),
                    ));
                }
                let reason = Self::str_arg(args, "reason")?;
                self.outcomes
                    .insert(goal.clone(), GoalOutcome::Failed { reason });
                Ok(
                    json!({"failed": goal, "note": "recorded; a failed mandatory goal blocks convergence and stays visible on its target"}),
                )
            }
            "search" => {
                let query = Self::str_arg(args, "query")?;
                let kind = Self::opt_str(args, "kind").unwrap_or_else(|| "entity".to_string());
                if kind != "entity" && kind != "view" {
                    return Err(ToolError::new(
                        "bad-args",
                        format!("unknown search kind `{}`; expected entity or view", kind),
                    ));
                }
                if kind == "view" {
                    let hits = self.search_views(&query);
                    if !hits.is_empty() {
                        // A read's subject joins the loaded set as a stub.
                        for (id, _) in &hits {
                            self.loaded.load_stub(&self.snapshot, id);
                        }
                        return Ok(json!({
                            "hits": hits
                                .iter()
                                .map(|(id, title)| json!({"id": id, "name": title}))
                                .collect::<Vec<_>>()
                        }));
                    }
                    return Ok(self.search_miss(&query));
                }
                let hits = self.search_all(&query);
                if !hits.is_empty() {
                    // A read's subject joins the loaded set as a stub.
                    for (id, _, _) in &hits {
                        self.loaded.load_stub(&self.snapshot, id);
                    }
                    return Ok(json!({
                        "hits": hits
                            .iter()
                            .map(|(id, name, def)| json!({"id": id, "name": name, "definition": def}))
                            .collect::<Vec<_>>()
                    }));
                }
                Ok(self.search_miss(&query))
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
                let reply = json!({"title": s.title, "raw": s.raw, "children": children});
                self.loaded
                    .load_stub(&self.snapshot, &format!("{}#{}", doc, sec));
                Ok(reply)
            }
            "get_entity" => {
                let id = Self::str_arg(args, "id")?;
                let rid = self
                    .canon_entity_id(&id)
                    .ok_or_else(|| self.unknown_entity_error(&id))?;
                // Staged first, then the snapshot: canon_entity_id resolves the
                // session's own staged entities, so the lookup sees them too.
                // Read-your-writes: staged updates replay over the base, so the
                // session reads back what it just wrote.
                let mut e = self
                    .staged_entities
                    .get(&rid)
                    .or_else(|| self.snapshot.graph.entities.get(&rid))
                    .ok_or_else(|| self.unknown_entity_error(&id))?
                    .clone();
                for op in &self.staged {
                    let Op::UpdateEntity {
                        id: uid,
                        name,
                        definition,
                        add_aliases,
                        add_mention,
                        stereotype,
                        parent,
                        set_attributes,
                        add_attributes,
                        ..
                    } = op
                    else {
                        continue;
                    };
                    if *uid != rid && self.snapshot.resolve_id(uid) != rid {
                        continue;
                    }
                    if let Some(n) = name {
                        e.name = n.clone();
                    }
                    if let Some(d) = definition {
                        e.definition = Some(d.clone());
                    }
                    for a in add_aliases {
                        if !e.aliases.contains(a) {
                            e.aliases.push(a.clone());
                        }
                    }
                    if let Some(m) = add_mention {
                        e.mentions.push(m.clone());
                    }
                    if let Some(s) = stereotype {
                        e.stereotype = Some(s.clone());
                    }
                    if let Some(p) = parent {
                        e.parent = Some(p.clone());
                    }
                    if let Some(set) = set_attributes {
                        e.attributes = set.clone();
                    }
                    for a in add_attributes {
                        match e.attributes.iter_mut().find(|x| x.name == a.name) {
                            Some(x) => *x = a.clone(),
                            None => e.attributes.push(a.clone()),
                        }
                    }
                }
                let e = &e;
                let reqs: Vec<Value> = self
                    .snapshot
                    .requirements_referencing(&rid)
                    .iter()
                    .filter_map(|r| {
                        self.snapshot
                            .graph
                            .requirements
                            .get(r)
                            .map(|req| json!({"id": r, "statement": req.statement}))
                    })
                    .collect();
                let rels: Vec<Value> = self
                    .snapshot
                    .graph
                    .relationships
                    .iter()
                    .filter(|(_, rel)| rel.members.contains(&rid))
                    .map(|(id, rel)| json!({"id": id, "type": rel.strongest(), "members": rel.members, "contributions": rel.contributions}))
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
                self.loaded.load_stub(&self.snapshot, &rid);
                Ok(v)
            }
            "diagnostics" => {
                let lifecycle =
                    Self::opt_str(args, "lifecycle").unwrap_or_else(|| "open".to_string());
                let rule = Self::opt_str(args, "rule");
                let subject = Self::opt_str(args, "subject");
                // Read-your-writes: the session's own staged findings ride beside
                // the snapshot, staged content winning on a shared id.
                let mut merged: std::collections::BTreeMap<&String, &Diagnostic> =
                    self.snapshot.graph.diagnostics.iter().collect();
                for (i, d) in &self.staged_diags {
                    merged.insert(i, d);
                }
                let list: Vec<Value> = merged
                    .into_iter()
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
                // The subjects of the listed findings join the loaded set as stubs.
                let subjects: Vec<String> = list
                    .iter()
                    .flat_map(|d| d["subjects"].as_array().cloned().unwrap_or_default())
                    .filter_map(|s| s.as_str().map(String::from))
                    .take(8)
                    .collect();
                for s in subjects {
                    self.loaded.load_stub(&self.snapshot, &s);
                }
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
                // Exactly one provenance: a located mention, or a derivation. Empty
                // objects count as absent (docs/compiler/tools.md#validation-and-errors).
                let mention = &args["mention"];
                let has_mention = Self::present(mention);
                let has_derived = Self::present(&args["provenance"]);
                if has_mention == has_derived {
                    return Err(ToolError::new(
                        if has_mention { "bad-provenance" } else { "provenance-required" },
                        "an entity enters with exactly one provenance: mention {section, quote} (the sentence that talks about it), or provenance {derived: {from, reasoning}} for structure the documents do not state".into(),
                    ));
                }
                let mention_ref = if has_mention {
                    let section = mention["section"].as_str().unwrap_or_default();
                    let quote = mention["quote"].as_str().unwrap_or_default();
                    let (doc, sec) = self.resolve_section(section)?;
                    if let Some(wd) = self.scope.reconcile_doc() {
                        if doc != wd {
                            return Err(ToolError::new(
                                "wrong-document",
                                format!(
                                    "mention cites {} but this batch reconciles {}; quote a sentence from {}'s own sections (text this document merely links to cannot anchor a mention here)",
                                    doc, wd, wd
                                ),
                            ));
                        }
                    }
                    let quote = self.check_quote(&doc, &sec, quote)?;
                    Some(SourceRef {
                        doc,
                        section: sec,
                        quote,
                    })
                } else {
                    None
                };
                let provenance = if has_derived {
                    Some(self.parse_derived(&args["provenance"])?)
                } else {
                    None
                };
                let stereotype = Self::opt_str(args, "stereotype");
                let parent = match Self::opt_str(args, "parent") {
                    Some(p) => Some(self.check_parent(None, &p)?),
                    None => None,
                };
                let attributes = if args["attributes"].is_null() {
                    Vec::new()
                } else {
                    self.parse_attributes(&args["attributes"], mention_ref.as_ref())?
                };

                // Lookup before create: the natural key may already exist in the graph or in
                // this turn's own staged creates.
                let existing = match self
                    .snapshot
                    .find_natural(&name_arg, &scope, parent.as_deref())
                {
                    Ok(found) => found,
                    Err(candidates) => {
                        return Err(ToolError::new(
                            "ambiguous-name",
                            format!(
                                "`{}` names several entities ({}); pass `parent` to say which one you mean, or update_entity on the id",
                                name_arg,
                                candidates.join(", ")
                            ),
                        ))
                    }
                }
                .or_else(|| {
                    self.staged_entities
                        .iter()
                        .find(|(_, e)| {
                            e.scope == scope
                                && e.name.trim().to_lowercase() == name_arg.trim().to_lowercase()
                                && parent.as_ref().is_none_or(|p| e.parent.as_ref() == Some(p))
                        })
                        .map(|(id, _)| id.clone())
                });
                if let Some(id) = existing {
                    if let Some(p) = &parent {
                        self.staged_parents.insert(id.clone(), p.clone());
                    }
                    self.stage(Op::UpdateEntity {
                        id: id.clone(),
                        name: None,
                        definition: Self::opt_str(args, "definition"),
                        add_aliases: Self::str_list(args, "aliases"),
                        add_mention: mention_ref,
                        stereotype,
                        parent,
                        set_attributes: None,
                        add_attributes: attributes,
                        provenance: None,
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
                    stereotype,
                    parent,
                    attributes,
                    mentions: mention_ref.into_iter().collect(),
                    provenance,
                    reasoning: note,
                    ..Default::default()
                };
                self.staged_entities.insert(id.clone(), entity.clone());
                self.stage(Op::CreateEntity {
                    id: id.clone(),
                    entity,
                })?;
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
                let parent = match Self::opt_str(args, "parent") {
                    Some(p) => Some(self.check_parent(Some(&rid), &p)?),
                    None => None,
                };
                // An attribute without its own provenance takes the entity's first mention.
                let first_mention = self
                    .snapshot
                    .graph
                    .entities
                    .get(&rid)
                    .and_then(|e| e.mentions.first().cloned())
                    .or_else(|| {
                        self.staged_entities
                            .get(&rid)
                            .and_then(|e| e.mentions.first().cloned())
                    });
                let attributes = if args["attributes"].is_null() {
                    Vec::new()
                } else {
                    self.parse_attributes(&args["attributes"], first_mention.as_ref())?
                };
                if let Some(p) = &parent {
                    self.staged_parents.insert(rid.clone(), p.clone());
                }
                self.stage(Op::UpdateEntity {
                    id: rid.clone(),
                    name,
                    definition: Self::opt_str(args, "definition"),
                    add_aliases: Self::str_list(args, "add_aliases"),
                    add_mention: None,
                    stereotype: Self::opt_str(args, "stereotype"),
                    parent,
                    set_attributes: None,
                    add_attributes: attributes,
                    provenance: None,
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
                    if let Op::CreateRequirement {
                        id: qid,
                        requirement,
                    } = op
                    {
                        if requirement.entities.contains(&rid) || requirement.entities.contains(&id)
                        {
                            refs.push(qid.clone());
                        }
                    }
                }
                if !refs.is_empty() {
                    return Err(ToolError::new(
                        "still-referenced",
                        format!(
                            "cannot delete {}; requirements still reference it: {}",
                            rid,
                            refs.join(", ")
                        ),
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
                    return Err(ToolError::new(
                        "bad-merge",
                        "keep and absorb are the same entity".into(),
                    ));
                }
                self.stage(Op::MergeEntities {
                    keep: keep.clone(),
                    absorb,
                    reason,
                })?;
                Ok(json!({"kept": keep}))
            }
            // Build one level: one derived entity from the members, every member moved
            // under it, as one changeset. Composed from CreateEntity and one
            // UpdateEntity parent move per member, so the journal shows the create and
            // each move with its prior parent. Mirrors docs/compiler/tools.md#grouping-tools.
            "group_entities" => {
                let name_arg = Self::str_arg(args, "name")?;
                let definition = Self::opt_str(args, "definition").unwrap_or_default();
                if definition.trim().is_empty() {
                    return Err(ToolError::new(
                        "definition-required",
                        "definition is one sentence stating the grouping's responsibility; it is the sentence the documents should gain".into(),
                    ));
                }
                let reasoning = Self::opt_str(args, "reasoning").unwrap_or_default();
                if reasoning.trim().is_empty() {
                    return Err(ToolError::new(
                        "reasoning-required",
                        "reasoning says why the domain would recognize the members as one thing; it becomes the grouping's derived provenance".into(),
                    ));
                }
                // Every member resolves, once.
                let mut members: Vec<String> = Vec::new();
                for raw in Self::str_list(args, "members") {
                    let Some(id) = self.canon_entity_id(&raw) else {
                        let e = self.unknown_entity_error(&raw);
                        return Err(ToolError::new(
                            "unknown-id",
                            format!("member {}", e.message),
                        ));
                    };
                    if !members.contains(&id) {
                        members.push(id);
                    }
                }
                if members.len() < 2 {
                    return Err(ToolError::new(
                        "too-few-members",
                        format!(
                            "a grouping holds at least two members ({} given); below two there is nothing to judge. To move one child, use update_entity with parent",
                            members.len()
                        ),
                    ));
                }
                // All members share one current parent, staged moves counted: a
                // grouping never crosses levels, and it takes that parent.
                let parent = self.parent_of(&members[0]);
                if let Some(other) = members.iter().find(|m| self.parent_of(m) != parent) {
                    let level = |p: &Option<String>| {
                        p.clone().unwrap_or_else(|| "the scope root".to_string())
                    };
                    return Err(ToolError::new(
                        "cross-level",
                        format!(
                            "a grouping never crosses levels: {} sits under {} while {} sits under {}; group members that share one parent, or move one first with update_entity parent",
                            members[0],
                            level(&parent),
                            other,
                            level(&self.parent_of(other))
                        ),
                    ));
                }
                let scope = self
                    .scope_of(&members[0])
                    .unwrap_or_else(|| "public".to_string());
                if let Some(other) = members
                    .iter()
                    .find(|m| self.scope_of(m).as_deref() != Some(scope.as_str()))
                {
                    return Err(ToolError::new(
                        "scope-mismatch",
                        format!(
                            "a grouping never crosses a scope: {} is in {}, {} is not",
                            members[0], scope, other
                        ),
                    ));
                }
                // The near-duplicate gate, as upsert_entity: a lookalike of an existing
                // area reuses that entity, and an exact name is that entity already.
                // Mirrors docs/compiler/concepts/levels.md#naming.
                if let Ok(Some(existing)) = self.snapshot.find_natural(&name_arg, &scope, None) {
                    return Err(ToolError::new(
                        "near-duplicate",
                        format!(
                            "`{}` already exists as {}; a grouping's name is judged like an entity name, so reparent the members under it with update_entity parent instead of minting a twin",
                            name_arg, existing
                        ),
                    ));
                }
                if let Some((eid, ename)) = self.near_name(&name_arg, &scope) {
                    // A lookalike that is a peer of the members (a member of the
                    // same level) carries the area's word without being the area:
                    // the members never nest under it. Mirrors
                    // docs/compiler/concepts/levels.md#naming.
                    let sibling = !members.contains(&eid) && self.parent_of(&eid) == parent;
                    let holds_children = self
                        .snapshot
                        .graph
                        .entities
                        .keys()
                        .chain(self.staged_entities.keys())
                        .any(|c| c != &eid && self.parent_of(c).as_deref() == Some(eid.as_str()));
                    let message = if sibling && !holds_children {
                        format!(
                            "`{}` looks like a name variant of `{}` ({}), a peer of the members at this level; a peer that carries the area's word is not the area and never becomes the members' parent: name the grouping from the heading or document that lists the members (or qualify the name), and let {} join the grouping of its own heading",
                            name_arg, eid, ename, eid
                        )
                    } else if sibling {
                        format!(
                            "`{}` looks like a name variant of existing `{}` ({}), a sibling at this level that already holds children; a lookalike of an existing area reuses it: reparent the members under {} with update_entity parent, or pick the name the documents use for this area",
                            name_arg, eid, ename, eid
                        )
                    } else {
                        format!(
                            "`{}` looks like a name variant of existing `{}` ({}), which sits at another level of the tree and names that level's concept; a move under it would cross levels, so qualify the grouping's name (the heading or document that lists the members, plus what they are) instead",
                            name_arg, eid, ename
                        )
                    };
                    return Err(ToolError::new("near-duplicate", message));
                }
                // One changeset: the create and every move, or nothing.
                if self.staged.len() + members.len() + 1 > self.mutation_limit {
                    return Err(ToolError::new(
                        "mutation-budget",
                        format!(
                            "turn mutation budget ({}) cannot fit a grouping of {} members; call done",
                            self.mutation_limit,
                            members.len()
                        ),
                    ));
                }
                let id = self.snapshot.mint_entity_id(&name_arg, &self.taken_ids);
                self.taken_ids.insert(id.clone());
                let entity = Entity {
                    name: name_arg,
                    definition: Some(definition),
                    scope,
                    stereotype: Self::opt_str(args, "stereotype"),
                    parent,
                    provenance: Some(Provenance::Derived {
                        from: members.clone(),
                        reasoning,
                    }),
                    ..Default::default()
                };
                self.staged_entities.insert(id.clone(), entity.clone());
                self.stage(Op::CreateEntity {
                    id: id.clone(),
                    entity,
                })?;
                for m in &members {
                    self.staged_parents.insert(m.clone(), id.clone());
                    self.stage(Op::UpdateEntity {
                        id: m.clone(),
                        name: None,
                        definition: None,
                        add_aliases: Vec::new(),
                        add_mention: None,
                        stereotype: None,
                        parent: Some(id.clone()),
                        set_attributes: None,
                        add_attributes: Vec::new(),
                        provenance: None,
                    })?;
                }
                Ok(json!({"id": id, "moved": members}))
            }
            // Unbuild one level: a derived grouping's children reparent to its parent
            // and it tombstones with a redirect there. A stated entity is refused
            // toward the documents. Mirrors docs/compiler/tools.md#grouping-tools.
            "dissolve_entity" => {
                let id = Self::str_arg(args, "id")?;
                let reason = Self::str_arg(args, "reason")?;
                let Some(rid) = self.canon_entity_id(&id) else {
                    return Err(self.unknown_entity_error(&id));
                };
                if self.staged_entities.contains_key(&rid) {
                    return Err(ToolError::new(
                        "staged-entity",
                        format!(
                            "{} is staged in this session and not yet in the graph; it lands at commit, so dissolve it in a later session or leave it out now",
                            rid
                        ),
                    ));
                }
                if self.staged_dissolved(&rid) {
                    return Err(ToolError::new(
                        "already-dissolved",
                        format!("{} is already dissolved in this changeset", rid),
                    ));
                }
                let e = self.snapshot.graph.entities.get(&rid).cloned();
                let Some(e) = e else {
                    return Err(self.unknown_entity_error(&id));
                };
                let derived = matches!(e.provenance, Some(Provenance::Derived { .. }));
                if !e.mentions.is_empty() || !derived {
                    return Err(ToolError::new(
                        "stated-entity",
                        format!(
                            "{} is an entity the documents state ({}); it holds its children in role, not in provenance, and only a grouping with derived provenance and no mentions dissolves. Revise the documents instead",
                            rid,
                            if e.mentions.is_empty() {
                                "provenance is not derived".to_string()
                            } else {
                                format!("{} mention(s)", e.mentions.len())
                            }
                        ),
                    ));
                }
                let refs = self.snapshot.requirements_referencing(&rid);
                if !refs.is_empty() {
                    return Err(ToolError::new(
                        "not-a-grouping",
                        format!(
                            "{} carries requirements of its own ({}); higher levels carry none, so it is a sub-entity, not a grouping. Re-point its requirements first, or leave it",
                            rid,
                            refs.join(", ")
                        ),
                    ));
                }
                let parent = self.parent_of(&rid);
                let children = self.level_after(&rid);
                // The store fills parent and children as applied; this session's own
                // gates read the children's moves through parent_of.
                self.stage(Op::DissolveEntity {
                    id: rid.clone(),
                    reason,
                    parent: None,
                    children: Vec::new(),
                })?;
                Ok(json!({
                    "dissolved": rid,
                    "parent": parent,
                    "children": children,
                    "note": "a redirect to the parent stays; anything holding the old id resolves there",
                }))
            }
            "upsert_requirement" => {
                let statement = Self::str_arg(args, "statement")?;
                statement_present(&statement)?;
                // Exactly one provenance: the source sentence, or a derivation. Empty
                // fields count as absent (docs/compiler/tools.md#validation-and-errors).
                let has_source = Self::present(&args["section"]) || Self::present(&args["quote"]);
                let has_derived = Self::present(&args["provenance"]);
                if has_source == has_derived {
                    return Err(ToolError::new(
                        if has_source { "bad-provenance" } else { "provenance-required" },
                        "a requirement enters with exactly one provenance: section plus quote (the verbatim source sentence), or provenance {derived: {from, reasoning}} for a statement the documents do not state".into(),
                    ));
                }
                if has_derived {
                    return self.upsert_derived_requirement(args, statement);
                }
                // Provenance is validated first: a quote that does not locate is the
                // clearest signal a statement was invented, and it must not be masked
                // by an entity-id error the model would keep retrying around.
                let section = Self::str_arg(args, "section")?;
                let quote = Self::str_arg(args, "quote")?;
                let (doc, sec) = self.resolve_section(&section)?;
                if let Some(wd) = self.scope.reconcile_doc() {
                    if doc != wd {
                        return Err(ToolError::new(
                            "wrong-document",
                            format!(
                                "source cites {} but this batch reconciles {}; quote the sentence from {}'s own sections (text this document merely links to cannot anchor a requirement here)",
                                doc, wd, wd
                            ),
                        ));
                    }
                }
                let quote = self.check_quote(&doc, &sec, &quote)?;
                let raw_entities = Self::str_list(args, "entities");
                if raw_entities.is_empty() {
                    return Err(ToolError::new(
                        "no-entities",
                        "a requirement must reference at least one entity id".into(),
                    ));
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
                let edges = match args["edges"].as_array() {
                    Some(arr) => parse_edges(self, arr, Some(&entities))?,
                    None => Vec::new(),
                };
                let transition = if !Self::present(&args["transition"]) {
                    None
                } else {
                    Some(self.parse_transition(&args["transition"], &entities)?)
                };
                let facets = if !Self::present(&args["facets"]) {
                    Vec::new()
                } else {
                    parse_facets(&args["facets"])?
                };
                let source = SourceRef {
                    doc: doc.clone(),
                    section: sec.clone(),
                    quote: quote.trim().to_string(),
                };
                let requirement = Requirement {
                    statement,
                    entities,
                    edges,
                    transition,
                    facets,
                    source: Some(source.clone()),
                    reasoning: Self::opt_str(args, "reasoning"),
                    ..Default::default()
                };
                // Stage-time natural-key resolution: the model sees the resolved id,
                // never a misleading fresh one. Same predicate as the commit-time fold.
                let norm_statement = crate::store::normalize_statement(&requirement.statement);
                let norm_quote = crate::store::normalize_statement(&source.quote);
                let same_key = |r: &Requirement| {
                    let Some(rs) = r.source.as_ref() else {
                        return false;
                    };
                    rs.doc == source.doc
                        && rs.section == source.section
                        && (crate::store::normalize_statement(&r.statement) == norm_statement
                            || (crate::store::normalize_statement(&rs.quote) == norm_quote
                                && crate::store::statement_subsumes(
                                    &r.statement,
                                    &requirement.statement,
                                )))
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
                        if !r
                            .edges
                            .iter()
                            .any(|x| x.a == edge.a && x.b == edge.b && x.rel_type == edge.rel_type)
                        {
                            r.edges.push(edge);
                        }
                    }
                    if requirement.transition.is_some() {
                        r.transition = requirement.transition;
                    }
                    if !requirement.facets.is_empty() {
                        r.facets = requirement.facets;
                    }
                    r.statement = requirement.statement;
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
                            .stale_anchors()
                            .iter()
                            .find(|a| {
                                self.snapshot.graph.requirements.get(*a).is_some_and(|r| {
                                    r.source.as_ref().is_some_and(|rs| {
                                        rs.doc == source.doc
                                            && rs.section == source.section
                                            && !self.snapshot.quote_locates(
                                                &rs.doc,
                                                &rs.section,
                                                &rs.quote,
                                            )
                                    }) && crate::store::statement_subsumes(
                                        &r.statement,
                                        &requirement.statement,
                                    )
                                })
                            })
                            .cloned()
                    });
                if let Some(rid) = existing {
                    self.staged_reqs.insert(rid.clone());
                    self.taken_ids.insert(rid.clone());
                    self.stage(Op::CreateRequirement {
                        id: rid.clone(),
                        requirement,
                    })?;
                    return Ok(json!({"id": rid, "updated": true}));
                }
                // A new statement: the store mints the id; a supplied id is ignored.
                let mut taken = self.taken_ids.clone();
                taken.extend(self.staged_reqs.iter().cloned());
                let id = self.snapshot.mint_req_id(&doc, &taken);
                self.staged_reqs.insert(id.clone());
                self.taken_ids.insert(id.clone());
                self.stage(Op::CreateRequirement {
                    id: id.clone(),
                    requirement,
                })?;
                Ok(json!({"id": id, "created": true}))
            }
            "update_requirement" => {
                let id = self.canon_req_id(&Self::str_arg(args, "id")?)?;
                // An empty statement counts as absent: statement unchanged.
                let statement = Self::opt_str(args, "statement");
                // Empty means absent: [] (or [""]) leaves the entity list unchanged.
                let entities = if !Self::present(&args["entities"]) {
                    None
                } else {
                    match args["entities"].as_array() {
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
                    }
                };
                // The entities the edges and the transition are checked against: the
                // revised list, or the stored one.
                let listed: Vec<String> = entities.clone().unwrap_or_else(|| {
                    self.snapshot
                        .graph
                        .requirements
                        .get(&id)
                        .map(|r| r.entities.clone())
                        .unwrap_or_default()
                });
                // On the update path an empty edges list is a filled-in blank,
                // not "replace the judged edges with nothing".
                let edges = if !Self::present(&args["edges"]) {
                    None
                } else {
                    args["edges"]
                        .as_array()
                        .map(|arr| parse_edges(self, arr, Some(&listed)))
                        .transpose()?
                };
                let transition = if !Self::present(&args["transition"]) {
                    None
                } else {
                    Some(self.parse_transition(&args["transition"], &listed)?)
                };
                let facets = if !Self::present(&args["facets"]) {
                    None
                } else {
                    Some(parse_facets(&args["facets"])?)
                };
                // A revision may re-anchor its provenance: section plus quote, both or
                // neither. The quote must locate, same gate as a fresh upsert.
                // The common miscall is passing the statement as the quote while only
                // meaning to change `entities`; name the existing anchor so the repair
                // is to drop the two fields, not to guess another sentence.
                let anchor_hint = || {
                    match self.snapshot.graph.requirements.get(&id).and_then(|r| r.source.as_ref()) {
                    Some(src) => format!(
                        "{} is anchored at {}#{} quoting \"{}\". `quote` is the document's own sentence, never the statement. To change only the entities or edges, omit `section` and `quote`",
                        id,
                        src.doc,
                        src.section,
                        crate::llm::truncate(&src.quote, 120)
                    ),
                    None => "`quote` is the document's own sentence, never the statement. To change only the entities or edges, omit `section` and `quote`".to_string(),
                }
                };
                let source = match (Self::opt_str(args, "section"), Self::opt_str(args, "quote")) {
                    (Some(section), Some(q)) => {
                        let (doc, sec) = self
                            .resolve_section(&section)
                            .map_err(|e| ToolError::new(&e.rule, format!("{}; {}", e.message, anchor_hint())))?;
                        if let Some(wd) = self.scope.reconcile_doc() {
                            if doc != wd {
                                return Err(ToolError::new(
                                    "wrong-document",
                                    format!(
                                        "source cites {} but this batch reconciles {}; quote the sentence from {}'s own sections",
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
                self.stage(Op::UpdateRequirement {
                    id: id.clone(),
                    statement,
                    entities,
                    edges,
                    transition,
                    facets,
                    source,
                    provenance: None,
                })?;
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
                let scope_proposals = self.scope.proposals();
                let id = self
                    .canon_entity_id(&raw)
                    .filter(|i| scope_proposals.contains(i))
                    .or_else(|| {
                        self.canon_req_id(&raw)
                            .ok()
                            .filter(|i| scope_proposals.contains(i))
                    })
                    .ok_or_else(|| {
                        ToolError::new(
                            "unknown-anchor",
                            format!(
                                "`{}` is not one of this batch's proposals ({}); decide only the anchors the goal lists",
                                raw,
                                scope_proposals.join(", ")
                            ),
                        )
                    })?;
                // The proposals name the anchor's old location and quote; the op carries
                // them so the store can tell one entity mention from another. An entity
                // with several proposed mentions is decided in one call: every one of
                // them goes to the same section.
                let pdoc = self.scope.place_doc().or_else(|| self.scope.doc());
                let proposals: Vec<AnchorProposal> = self
                    .snapshot
                    .status
                    .alignment
                    .iter()
                    .filter(|b| Some(&b.doc) == pdoc.as_ref())
                    .flat_map(|b| b.proposals.iter())
                    .filter(|p| p.anchor == id)
                    .cloned()
                    .collect();
                if proposals.is_empty() {
                    return Err(ToolError::new(
                        "unknown-anchor",
                        format!("no pending proposal for `{}`", id),
                    ));
                }
                let mut froms: Vec<SourceRef> = Vec::new();
                for p in &proposals {
                    let (from_doc, from_sec) = split_section_ref(&p.from).ok_or_else(|| {
                        ToolError::new("bad-section", format!("bad proposal location `{}`", p.from))
                    })?;
                    froms.push(SourceRef {
                        doc: from_doc,
                        section: from_sec,
                        quote: p.quote.clone(),
                    });
                }
                if name == "orphan_anchor" {
                    for from in froms {
                        self.stage(Op::OrphanAnchor {
                            id: id.clone(),
                            from,
                        })?;
                    }
                    return Ok(json!({"id": id, "orphaned": true}));
                }
                let (doc, sec) = self.resolve_section(&Self::str_arg(args, "section")?)?;
                let given = match Self::opt_str(args, "quote") {
                    Some(_) if froms.len() > 1 => {
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
                        to: SourceRef {
                            doc: doc.clone(),
                            section: sec.clone(),
                            quote,
                        },
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
                if !REVIEW_RULES.contains(&rule.as_str()) {
                    return Err(ToolError::new(
                        "bad-rule",
                        format!(
                            "rule `{}` is not in the catalog; use one of: {}",
                            rule,
                            REVIEW_RULES.join(", ")
                        ),
                    ));
                }
                let severity = Self::str_arg(args, "severity")?;
                if !["error", "warning", "info", "none"].contains(&severity.as_str()) {
                    return Err(ToolError::new(
                        "bad-severity",
                        format!(
                            "severity `{}` must be error, warning, info, or none",
                            severity
                        ),
                    ));
                }
                let subjects = Self::str_list(args, "subjects");
                if subjects.is_empty() {
                    return Err(ToolError::new(
                        "no-subjects",
                        "a diagnostic needs at least one subject node id".into(),
                    ));
                }
                for s in &subjects {
                    let ok = self.known_entity(s)
                        || self.snapshot.graph.requirements.contains_key(s)
                        || self.staged_reqs.contains(s)
                        || split_section_ref(s)
                            .map(|(d, r)| {
                                self.snapshot
                                    .docs
                                    .get(&d)
                                    .map(|rec| rec.sections.contains_key(&r))
                                    .unwrap_or(false)
                            })
                            .unwrap_or(false);
                    if !ok {
                        return Err(ToolError::new(
                            "unknown-id",
                            format!("diagnostic subject `{}` does not exist", s),
                        ));
                    }
                }
                let message = Self::str_arg(args, "message")?;
                let prompt = self.parse_prompt(&args["prompt"])?;
                // A decision is a question for the owner: it carries the prompt that
                // asks it. Mirrors docs/compiler/model/diagnostic.md#rules-catalog.
                if rule == "decision" && prompt.is_none() {
                    return Err(ToolError::new(
                        "decision-needs-prompt",
                        "a decision diagnostic carries a prompt: the question and the options the documents leave open (each a label with an edit or an answer)".into(),
                    ));
                }
                // Stage-time natural-key resolution, same predicate as the commit
                // fold: the reply carries the finding's id, and a re-report answers
                // with the id it will update instead of a bare acknowledgement.
                let subject_set: BTreeSet<&str> = subjects.iter().map(String::as_str).collect();
                let same_key = |d: &Diagnostic| {
                    d.rule == rule
                        && d.lifecycle == "open"
                        && d.subjects
                            .iter()
                            .map(String::as_str)
                            .collect::<BTreeSet<&str>>()
                            == subject_set
                };
                let existing = self
                    .staged_diags
                    .iter()
                    .find(|(_, d)| same_key(d))
                    .map(|(i, _)| i.clone())
                    .or_else(|| {
                        self.snapshot
                            .graph
                            .diagnostics
                            .iter()
                            .find(|(_, d)| same_key(d))
                            .map(|(i, _)| i.clone())
                    });
                let (id, created) = match existing {
                    Some(i) => (i, false),
                    None => {
                        let taken: BTreeSet<String> = self.staged_diags.keys().cloned().collect();
                        (self.snapshot.mint_diag_id(&rule, &taken), true)
                    }
                };
                let diagnostic = Diagnostic {
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
                };
                self.stage(Op::ReportDiagnostic {
                    id: id.clone(),
                    diagnostic: diagnostic.clone(),
                })?;
                self.staged_diags.insert(id.clone(), diagnostic);
                Ok(json!({"reported": true, "id": id, "created": created}))
            }
            "update_diagnostic" => {
                let id = Self::str_arg(args, "id")?;
                if !self.snapshot.graph.diagnostics.contains_key(&id)
                    && !self.staged_diags.contains_key(&id)
                {
                    return Err(ToolError::new(
                        "unknown-id",
                        format!("unknown diagnostic id `{}`", id),
                    ));
                }
                let prompt = self.parse_prompt(&args["prompt"])?;
                self.stage(Op::UpdateDiagnosticPrompt { id, prompt })?;
                Ok(json!({"updated": true}))
            }
            "resolve_diagnostic" => {
                let id = Self::str_arg(args, "id")?;
                let reason = Self::str_arg(args, "reason")?;
                let Some(d) = self
                    .snapshot
                    .graph
                    .diagnostics
                    .get(&id)
                    .or_else(|| self.staged_diags.get(&id))
                else {
                    return Err(ToolError::new(
                        "unknown-id",
                        format!("unknown diagnostic id `{}`", id),
                    ));
                };
                // A ratification proposal closes only through accept or retract.
                // Mirrors docs/compiler/model/diagnostic.md#lifecycle-and-triage.
                if d.rule == "ratification-pending" {
                    return Err(ToolError::new(
                        "not-resolvable",
                        format!(
                            "{} is a ratification proposal; it resolves when its edit option is applied or the fact is retracted, never through resolve_diagnostic",
                            id
                        ),
                    ));
                }
                self.stage(Op::ResolveDiagnostic { id, reason })?;
                Ok(json!({"resolved": true}))
            }
            "upsert_view" | "update_view" | "delete_view" => self.view_tool(name, args),
            "edit_fact" => self.edit_fact(args),
            "set_coverage" => {
                let section = Self::str_arg(args, "section")?;
                let state = Self::str_arg(args, "state")?;
                if !["covered", "non-normative"].contains(&state.as_str()) {
                    return Err(ToolError::new(
                        "bad-state",
                        format!("state `{}` must be covered or non-normative", state),
                    ));
                }
                // A placeholder note counts as absent; weak models emit these literally.
                let note = Self::opt_str(args, "note").filter(|n| {
                    !matches!(
                        n.trim().to_lowercase().as_str(),
                        "<nil>" | "nil" | "null" | "none" | "n/a" | "na" | "-"
                    )
                });
                if state == "non-normative" && note.is_none() {
                    return Err(ToolError::new("note-required", "non-normative requires a note saying why the section states no requirements".into()));
                }
                let (doc, sec) = self.resolve_section(&section)?;
                // The mark must land on a section a reconcile-section goal of this
                // batch owns; the rejection names the owning goals' sections.
                let owned = self.scope.reconcile_scopes();
                if !owned.is_empty()
                    && !owned.iter().any(|g| {
                        g.doc.as_deref() == Some(doc.as_str()) && g.sections.contains(&sec)
                    })
                {
                    let listing: Vec<String> = owned
                        .iter()
                        .flat_map(|g| {
                            g.sections
                                .iter()
                                .map(|s| format!("{} ({})", s, g.goal))
                                .collect::<Vec<_>>()
                        })
                        .collect();
                    return Err(ToolError::new(
                        "wrong-section",
                        format!(
                            "{} is owned by no goal of this batch; the batch's sections: {}",
                            sec,
                            listing.join(", ")
                        ),
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
                        .filter(|(_, r)| r.anchored_at(&doc, &sec))
                        .map(|(id, _)| id.clone())
                        .collect();
                    for op in &self.staged {
                        if let Op::CreateRequirement { id, requirement } = op {
                            if requirement.anchored_at(&doc, &sec) {
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
                self.stage(Op::SetCoverage {
                    doc,
                    section: sec,
                    state,
                    note,
                })?;
                Ok(json!({"set": true}))
            }
            "generation_tasks" => {
                let gs = self.gen_settings();
                let tasks = crate::gen::pending(&self.snapshot, &gs);
                if tasks.is_empty() {
                    return Ok(
                        json!({"tasks": [], "note": "generation is current; nothing to do"}),
                    );
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
                        "factHash is required; pass the factHash from the begin_generation package"
                            .into(),
                    ));
                };
                if !args["manifest"].is_object() {
                    return Err(ToolError::new(
                        "bad-argument",
                        "manifest is required: {files: [...], tests: [{requirement, kind, label, artifact, name, run}]}".into(),
                    ));
                }
                // `choices` rides beside the manifest; fold it in so one validated
                // set feeds the record and the diagnostics.
                let mut manifest = args["manifest"].clone();
                if manifest["choices"].is_null() && args["choices"].is_array() {
                    manifest["choices"] = args["choices"].clone();
                }
                let choices = crate::gen::parse_choices(&self.snapshot, &manifest)
                    .map_err(|e| ToolError::new("bad-choices", e))?;
                let reply =
                    crate::gen::mark(&self.snapshot, &id, Some(seen.as_str()), &manifest, &gs)
                        .map_err(|e| ToolError::new("bad-manifest", e))?;
                // Invented choices land as diagnostics through the session path; the
                // ops also resolve open ones this record omits.
                // Mirrors docs/consumers/gen.md#invented-choices.
                let ledger = crate::gen::Ledger::load(&self.snapshot.out);
                let unattached = ledger
                    .entities
                    .get(&crate::gen::slug_of(&id))
                    .and_then(|e| e.unattached.clone());
                for op in crate::gen::choice_ops(&self.snapshot, &id, &choices, unattached.as_ref())
                {
                    self.stage(op)?;
                }
                Ok(reply)
            }
            "binding_tasks" => {
                let gs = self.gen_settings();
                let tasks = crate::bind::pending(&self.snapshot, &gs);
                if tasks.is_empty() {
                    return Ok(
                        json!({"tasks": [], "note": "every requirement is bound; generation_tasks lists what binding left unimplemented"}),
                    );
                }
                Ok(json!({"tasks": tasks, "next": "begin_binding on one requirement"}))
            }
            "begin_binding" => {
                let rid = Self::str_arg(args, "requirement")?;
                let gs = self.gen_settings();
                crate::bind::task(&self.snapshot, &rid, &gs)
                    .map_err(|e| ToolError::new("unknown-id", e))
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
                crate::bind::record(
                    &self.snapshot,
                    &rid,
                    &files,
                    &args["test"],
                    &verdict,
                    evidence.as_deref(),
                    &gs,
                )
                .map_err(|e| ToolError::new("bad-binding", e))
            }
            "verification_tasks" => {
                let gs = self.gen_settings();
                let filter = Self::opt_str(args, "filter");
                let entity = Self::opt_str(args, "entity");
                let tasks = crate::verify::pending(
                    &self.snapshot,
                    &gs,
                    filter.as_deref(),
                    entity.as_deref(),
                );
                if tasks.is_empty() {
                    return Ok(
                        json!({"tasks": [], "note": "nothing pending; every targeted row is verified"}),
                    );
                }
                Ok(
                    json!({"tasks": tasks, "next": "run_tests for programmatic rows; begin_verification then record_verdict for llm rows"}),
                )
            }
            "begin_verification" => {
                let rid = Self::str_arg(args, "requirement")?;
                let gs = self.gen_settings();
                crate::verify::task(&self.snapshot, &rid, &gs)
                    .map_err(|e| ToolError::new("unknown-id", e))
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
                crate::verify::mark(
                    &self.snapshot,
                    &rid,
                    &verdict,
                    seen.as_deref(),
                    evidence.as_deref(),
                    None,
                    &gs,
                )
                .map_err(|e| ToolError::new("bad-argument", e))
            }
            "read_text_file" | "write_text_file" | "list_files" | "run_command" => {
                self.file_tool(name, args)
            }
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
                        kind: crate::feedback::normalize_kind(
                            &Self::opt_str(args, "kind").unwrap_or_default(),
                        ),
                        subject: Self::opt_str(args, "subject")
                            .unwrap_or_default()
                            .trim()
                            .to_string(),
                        message: crate::llm::truncate(message, 4_000).to_string(),
                        source: c.source,
                        // The caller fields carry the goal kinds and the batch id.
                        task: if c.task.is_empty() {
                            self.scope.feedback_task()
                        } else {
                            c.task
                        },
                        target: if c.target.is_empty() {
                            self.scope.batch.clone()
                        } else {
                            c.target
                        },
                        batch: if c.batch.is_empty() {
                            self.scope.goal_ids()
                        } else {
                            c.batch
                        },
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
                // A done that leaves a mandatory goal neither done nor failed is
                // rejected naming it; the implicit path commits around it and the
                // goal stays open for its retry. Mirrors docs/compiler/sessions.md#commit.
                if !self.implicit_done {
                    let open: Vec<&str> = self
                        .scope
                        .goals
                        .iter()
                        .filter(|g| g.mandatory && !self.outcomes.contains_key(&g.goal))
                        .map(|g| g.goal.as_str())
                        .collect();
                    if !open.is_empty() {
                        return Err(ToolError::new(
                            "open-goal",
                            format!(
                                "goal(s) neither done nor failed: {}; mark_goal_done or mark_goal_failed each, then done",
                                open.join(", ")
                            ),
                        ));
                    }
                }
                // Batch gate: every proposal of the batch is decided.
                let undecided = self.undecided_proposals(&self.scope.proposals());
                if !undecided.is_empty() {
                    return Err(ToolError::new(
                        "undecided-proposal",
                        format!(
                            "proposals left undecided: {}; for each, place_anchor with the section that now holds the statement (reevaluate true when its meaning may have changed), or orphan_anchor when no candidate states it",
                            undecided.join(", ")
                        ),
                    ));
                }
                // Batch gate: stale anchors are a contract the harness never commits
                // around, implicit path included.
                let untouched = self.untouched_stale(&self.scope.stale_anchors());
                if !untouched.is_empty() {
                    return Err(ToolError::new(
                        "stale-anchor",
                        format!(
                            "stale anchors left untouched: {}; for each: if the document still states the fact, re-record it with upsert_requirement quoting the new sentence verbatim (it resolves to the anchor and updates in place); if the statement changed meaning, revise it with update_requirement carrying the id, the new statement, and the new section plus quote; if the fact is gone, delete_requirement",
                            untouched.join(", ")
                        ),
                    ));
                }
                // Batch gate: an explicit done finishes the coverage contract. Every
                // section of the batch carries a mark, staged or already recorded; the
                // implicit path commits without one and the section stays unprocessed.
                if !self.implicit_done {
                    for gs in self.scope.reconcile_scopes() {
                        if let Some(doc) = &gs.doc {
                            let unmarked = self.unmarked_sections(doc, &gs.sections);
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
                }
                // Batch gate: every covered claim is honest, implicit path included
                // (finish_implicit drops the offending marks and retries).
                if let Some(e) = self.dishonest_covered() {
                    return Err(e);
                }
                // Every resolution's gate still holds over the final changeset.
                let scopes: Vec<GoalScope> = self.scope.goals.clone();
                for gs in &scopes {
                    if matches!(self.outcomes.get(&gs.goal), Some(GoalOutcome::Done { .. })) {
                        self.goal_gate(gs)?;
                    }
                }
                // The union of the batch's kinds' gates over the whole changeset.
                for kind in self.scope.kinds() {
                    if let Some(k) = crate::goals::kind(&kind) {
                        if let Some(v) = k.gates(&self.snapshot, &self.staged).into_iter().next() {
                            return Err(ToolError::new(&v.rule, v.message));
                        }
                    }
                }
                let summary = Self::opt_str(args, "summary").unwrap_or_default();
                self.done = Some(summary);
                Ok(json!({"ok": true}))
            }
            other => Err(ToolError::new(
                "unknown-tool",
                format!("unknown tool `{}`", other),
            )),
        }
    }
}

// The view tools, the derived requirement path, and the chat edit path.
impl ToolSession {
    // upsert_requirement with derived provenance: a statement the documents do not
    // state, keyed on the statement within its `from` set, minted as `req:x-<n>`.
    // Mirrors docs/compiler/model/requirement.md#identity.
    fn upsert_derived_requirement(
        &mut self,
        args: &Value,
        statement: String,
    ) -> Result<Value, ToolError> {
        let raw_entities = Self::str_list(args, "entities");
        if raw_entities.is_empty() {
            return Err(ToolError::new(
                "no-entities",
                "a requirement must reference at least one entity id".into(),
            ));
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
        let edges = match args["edges"].as_array() {
            Some(arr) => parse_edges(self, arr, Some(&entities))?,
            None => Vec::new(),
        };
        let transition = if !Self::present(&args["transition"]) {
            None
        } else {
            Some(self.parse_transition(&args["transition"], &entities)?)
        };
        let facets = if !Self::present(&args["facets"]) {
            Vec::new()
        } else {
            parse_facets(&args["facets"])?
        };
        let provenance = self.parse_derived(&args["provenance"])?;
        let from_set: BTreeSet<String> = match &provenance {
            Provenance::Derived { from, .. } => from.iter().cloned().collect(),
            _ => BTreeSet::new(),
        };
        let norm = crate::store::normalize_statement(&statement);
        let same_key = |r: &Requirement| {
            r.source.is_none()
                && matches!(&r.provenance, Some(Provenance::Derived { from, .. })
                    if from.iter().cloned().collect::<BTreeSet<String>>() == from_set)
                && crate::store::normalize_statement(&r.statement) == norm
        };
        let requirement = Requirement {
            statement,
            entities,
            edges,
            transition,
            facets,
            provenance: Some(provenance),
            reasoning: Self::opt_str(args, "reasoning"),
            ..Default::default()
        };
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
                if !r
                    .edges
                    .iter()
                    .any(|x| x.a == edge.a && x.b == edge.b && x.rel_type == edge.rel_type)
                {
                    r.edges.push(edge);
                }
            }
            if requirement.transition.is_some() {
                r.transition = requirement.transition;
            }
            if !requirement.facets.is_empty() {
                r.facets = requirement.facets;
            }
            r.statement = requirement.statement;
            return Ok(json!({"id": id.clone(), "updated": true}));
        }
        if let Some(rid) = self
            .snapshot
            .graph
            .requirements
            .iter()
            .find(|(_, r)| same_key(r))
            .map(|(rid, _)| rid.clone())
        {
            self.staged_reqs.insert(rid.clone());
            self.taken_ids.insert(rid.clone());
            self.stage(Op::CreateRequirement {
                id: rid.clone(),
                requirement,
            })?;
            return Ok(json!({"id": rid, "updated": true}));
        }
        let mut taken = self.taken_ids.clone();
        taken.extend(self.staged_reqs.iter().cloned());
        let id = self.snapshot.mint_req_id("x", &taken);
        self.staged_reqs.insert(id.clone());
        self.taken_ids.insert(id.clone());
        self.stage(Op::CreateRequirement {
            id: id.clone(),
            requirement,
        })?;
        Ok(
            json!({"id": id, "created": true, "note": "a derived statement; a ratification proposal asks the owner to state it in the documents"}),
        )
    }

    // upsert_view, update_view, delete_view. Mirrors docs/compiler/tools.md#view-tools.
    fn view_tool(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        match name {
            "upsert_view" => {
                let kind = Self::str_arg(args, "kind")?;
                if !VIEW_KINDS.contains(&kind.as_str()) {
                    return Err(ToolError::new(
                        "bad-kind",
                        format!(
                            "unknown view kind `{}`; one of: {}",
                            kind,
                            VIEW_KINDS.join(", ")
                        ),
                    ));
                }
                let title = Self::str_arg(args, "title")?;
                let reasoning = Self::str_arg(args, "reasoning")?;
                let members = self.canon_members(&Self::str_list(args, "members"), "member")?;
                let collapse = self.canon_members(&Self::str_list(args, "collapse"), "collapse")?;
                let excluded = self.parse_exclusions(&args["excluded"])?;
                let query = if Self::present(&args["query"]) {
                    self.parse_query(&args["query"])?
                } else {
                    None
                };
                // A query matches entities; a flow view's members are requirements,
                // so a query on one would flood it with every entity in the graph.
                if query.is_some() && crate::derive::FLOW_KINDS.contains(&kind.as_str()) {
                    return Err(ToolError::new(
                        "bad-args",
                        format!(
                            "a {} view's members are requirements; an entity-matching query cannot drive it. Omit query and pick members",
                            kind
                        ),
                    ));
                }
                self.check_view_members(&kind, &members, &collapse)?;
                let mut from: Vec<String> = Vec::new();
                for id in members
                    .iter()
                    .chain(collapse.iter())
                    .chain(excluded.iter().map(|x| &x.id))
                    .chain(query.iter().filter_map(|q| q.parent.as_ref()))
                {
                    if !from.contains(id) {
                        from.push(id.clone());
                    }
                }
                let existing = self
                    .snapshot
                    .find_view(&kind, &title)
                    .or_else(|| self.staged_view_by_key(&kind, &title));
                if let Some(id) = existing {
                    if let Some(v) = self.staged_views.get_mut(&id) {
                        if !members.is_empty() {
                            v.members = members.clone();
                        }
                        if !collapse.is_empty() {
                            v.collapse = collapse.clone();
                        }
                    }
                    self.stage(Op::UpdateView {
                        id: id.clone(),
                        title: None,
                        members: (!members.is_empty()).then(|| members.clone()),
                        add_members: Vec::new(),
                        remove_members: Vec::new(),
                        query,
                        collapse: (!collapse.is_empty()).then(|| collapse.clone()),
                        exclude: excluded,
                        reasoning: Some(reasoning),
                    })?;
                    return Ok(json!({"id": id, "created": false}));
                }
                let id = self.snapshot.mint_view_id(&kind, &title, &self.taken_ids);
                self.taken_ids.insert(id.clone());
                let view = View {
                    kind,
                    title,
                    members,
                    excluded,
                    query,
                    collapse,
                    provenance: Some(Provenance::Derived { from, reasoning }),
                    default: false,
                    ..Default::default()
                };
                self.staged_views.insert(id.clone(), view.clone());
                self.stage(Op::CreateView {
                    id: id.clone(),
                    view,
                })?;
                Ok(json!({"id": id, "created": true}))
            }
            "update_view" => {
                let raw = Self::str_arg(args, "id")?;
                let Some((id, current)) = self.view_known(&raw) else {
                    return Err(ToolError::new(
                        "unknown-id",
                        format!(
                            "unknown view id `{}`; search with kind view, or create it with upsert_view",
                            raw
                        ),
                    ));
                };
                let title = Self::opt_str(args, "title");
                // Empty means absent: [] (or [""]) is a filled-in blank, not
                // "replace the membership with nothing".
                let members = if Self::present(&args["members"]) {
                    Some(self.canon_members(&Self::str_list(args, "members"), "member")?)
                } else {
                    None
                };
                let add_members =
                    self.canon_members(&Self::str_list(args, "add_members"), "member")?;
                let remove_members =
                    self.canon_members(&Self::str_list(args, "remove_members"), "member")?;
                let collapse = if Self::present(&args["collapse"]) {
                    Some(self.canon_members(&Self::str_list(args, "collapse"), "collapse")?)
                } else {
                    None
                };
                let exclude = self.parse_exclusions(&args["exclude"])?;
                let query = if Self::present(&args["query"]) {
                    self.parse_query(&args["query"])?
                } else {
                    None
                };
                let reasoning = Self::opt_str(args, "reasoning");
                // A query matches entities; a flow view's members are requirements,
                // so a query on one would flood it with every entity in the graph.
                if query.is_some() && crate::derive::FLOW_KINDS.contains(&current.kind.as_str()) {
                    return Err(ToolError::new(
                        "bad-args",
                        format!(
                            "a {} view's members are requirements; an entity-matching query cannot drive it. Omit query and pick members",
                            current.kind
                        ),
                    ));
                }
                // The membership the call leaves behind passes the kind's rule.
                let mut result = members.clone().unwrap_or_else(|| current.members.clone());
                for m in &add_members {
                    if !result.contains(m) {
                        result.push(m.clone());
                    }
                }
                result
                    .retain(|m| !remove_members.contains(m) && !exclude.iter().any(|x| &x.id == m));
                let coll = collapse.clone().unwrap_or_else(|| current.collapse.clone());
                self.check_view_members(&current.kind, &result, &coll)?;
                if let Some(v) = self.staged_views.get_mut(&id) {
                    v.members = result;
                    v.collapse = coll;
                    if let Some(t) = &title {
                        v.title = t.clone();
                    }
                }
                self.stage(Op::UpdateView {
                    id: id.clone(),
                    title,
                    members,
                    add_members,
                    remove_members,
                    query,
                    collapse,
                    exclude,
                    reasoning,
                })?;
                Ok(json!({"id": id, "updated": true}))
            }
            _ => {
                let raw = Self::str_arg(args, "id")?;
                let reason = Self::str_arg(args, "reason")?;
                let Some((id, current)) = self.view_known(&raw) else {
                    return Err(ToolError::new(
                        "unknown-id",
                        format!("unknown view id `{}`", raw),
                    ));
                };
                if current.default {
                    return Err(ToolError::new(
                        "default-view",
                        format!(
                            "{} is a default view; the next commit would derive it again. Exclude its members or collapse them (update_view), which makes it curated, then delete",
                            id
                        ),
                    ));
                }
                self.staged_views.remove(&id);
                self.stage(Op::DeleteView { id, reason })?;
                Ok(json!({"deleted": true}))
            }
        }
    }

    // The chat edit path: one authored field on one committed node. A quoted fact with
    // an accepted sentence rewrite (`note`) stages the mutation with its re-anchored
    // quote and reports the prose replacement for the serving to write; anything else
    // stages a decree with its ratification proposal.
    // Mirrors docs/compiler/tools.md#chat-tools and docs/compiler/compilation.md#edit-paths.
    fn edit_fact(&mut self, args: &Value) -> Result<Value, ToolError> {
        let raw_id = Self::str_arg(args, "id")?;
        let field = Self::str_arg(args, "field")?;
        let value = &args["value"];
        // A hollow value ({}, [], "") is as absent as null: the requiredness
        // bounce names the repair instead of a deeper parser's misleading error.
        if !Self::present(value) {
            return Err(ToolError::new(
                "bad-args",
                "value is required: the field's new content".into(),
            ));
        }
        let note = Self::opt_str(args, "note");
        let text_value = || -> Result<String, ToolError> {
            value
                .as_str()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    ToolError::new(
                        "bad-args",
                        format!("field `{}` takes a non-empty string value", field),
                    )
                })
        };
        let uncommitted = |id: &str| {
            ToolError::new(
                "unknown-id",
                format!(
                    "`{}` is staged in this session and not committed; edit_fact works on committed facts",
                    id
                ),
            )
        };
        let bad_field = |kind: &str, fields: &str| {
            ToolError::new(
                "bad-field",
                format!(
                    "field `{}` is not editable on {}; one of: {}. Ids, created, updated, mentions, and provenance itself are never edited directly",
                    field, kind, fields
                ),
            )
        };

        if let Ok(rid) = self.canon_req_id(&raw_id) {
            let Some(r) = self.snapshot.graph.requirements.get(&rid).cloned() else {
                return Err(uncommitted(&rid));
            };
            let (mut statement, mut edges, mut transition, mut facets) = (None, None, None, None);
            match field.as_str() {
                "statement" => statement = Some(text_value()?),
                "edges" => {
                    let Some(arr) = value.as_array() else {
                        return Err(ToolError::new(
                            "bad-args",
                            "edges takes a list of {a, b, type?, cardinality?}".into(),
                        ));
                    };
                    edges = Some(parse_edges(self, arr, Some(&r.entities))?);
                }
                "transition" => transition = Some(self.parse_transition(value, &r.entities)?),
                "facets" => facets = Some(parse_facets(value)?),
                _ => {
                    return Err(bad_field(
                        "a requirement",
                        "statement, edges, transition, facets",
                    ))
                }
            }
            let sentence = statement.clone().unwrap_or_else(|| r.statement.clone());
            return match (r.source.as_ref(), note.as_deref()) {
                (Some(src), Some(rewrite)) => {
                    let rewrite = rewrite.trim().to_string();
                    self.stage(Op::UpdateRequirement {
                        id: rid.clone(),
                        statement,
                        entities: None,
                        edges,
                        transition,
                        facets,
                        source: Some(SourceRef {
                            doc: src.doc.clone(),
                            section: src.section.clone(),
                            quote: rewrite.clone(),
                        }),
                        provenance: None,
                    })?;
                    Ok(json!({
                        "id": rid, "field": field, "path": "dual-write",
                        "prose": {"doc": src.doc, "section": src.section, "old_text": src.quote, "new_text": rewrite}
                    }))
                }
                _ => {
                    let decree = self.decree(note);
                    let proposal = self.snapshot.ratification_proposal(
                        &rid,
                        &sentence,
                        &decree,
                        r.source.as_ref(),
                        &r.entities,
                        None,
                    );
                    self.stage(Op::UpdateRequirement {
                        id: rid.clone(),
                        statement,
                        entities: None,
                        edges,
                        transition,
                        facets,
                        source: None,
                        provenance: Some(decree),
                    })?;
                    let question = proposal.prompt.as_ref().map(|p| p.question.clone());
                    self.stage(Op::ReportDiagnostic {
                        id: String::new(),
                        diagnostic: proposal,
                    })?;
                    Ok(json!({
                        "id": rid, "field": field, "path": "decree", "proposal": question,
                        "note": "the edit landed graph-only with decree provenance; a ratification proposal asks the owner to state it in the documents"
                    }))
                }
            };
        }

        if let Some(eid) = self.canon_entity_id(&raw_id) {
            let Some(e) = self.snapshot.graph.entities.get(&eid).cloned() else {
                return Err(uncommitted(&eid));
            };
            let quoted_mention = if e.provenance.is_none() {
                e.mentions.first().cloned()
            } else {
                None
            };
            let (mut definition, mut stereotype, mut parent) = (None, None, None);
            let mut attribute: Option<Attribute> = None;
            let (sentence, former): (String, Option<SourceRef>) = match field.as_str() {
                "definition" => {
                    let v = text_value()?;
                    let s = format!("{}: {}", e.name, v);
                    definition = Some(v);
                    (s, quoted_mention.clone())
                }
                "stereotype" => {
                    let v = text_value()?;
                    let s = format!("{} is a {}.", e.name, v);
                    stereotype = Some(v);
                    (s, quoted_mention.clone())
                }
                "parent" => {
                    let p = self.check_parent(Some(&eid), &text_value()?)?;
                    let pname = self
                        .snapshot
                        .graph
                        .entities
                        .get(&p)
                        .map(|x| x.name.clone())
                        .unwrap_or_else(|| p.clone());
                    let s = format!("{} is part of {}.", e.name, pname);
                    parent = Some(p);
                    (s, quoted_mention.clone())
                }
                f if f.starts_with("attributes.") => {
                    let rest = &f["attributes.".len()..];
                    let Some((aname, sub)) = rest.rsplit_once('.') else {
                        return Err(bad_field(
                            "an entity",
                            "definition, stereotype, parent, attributes.<name>.type, attributes.<name>.value",
                        ));
                    };
                    if sub != "type" && sub != "value" {
                        return Err(bad_field(
                            "an entity",
                            "definition, stereotype, parent, attributes.<name>.type, attributes.<name>.value",
                        ));
                    }
                    let Some(mut a) = e.attributes.iter().find(|a| a.name == aname).cloned() else {
                        return Err(ToolError::new(
                            "unknown-attribute",
                            format!(
                                "{} has no attribute `{}`; its attributes: {}",
                                eid,
                                aname,
                                e.attributes
                                    .iter()
                                    .map(|a| a.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        ));
                    };
                    let v = text_value()?;
                    let s = if sub == "type" {
                        a.r#type = Some(v.clone());
                        format!("{} has an attribute {} of type {}.", e.name, aname, v)
                    } else {
                        a.value = Some(v.clone());
                        format!("{}'s {} is {}.", e.name, aname, v)
                    };
                    let former = match &a.provenance {
                        Provenance::Quote(q) => Some(q.clone()),
                        _ => None,
                    };
                    attribute = Some(a);
                    (s, former)
                }
                _ => {
                    return Err(bad_field(
                        "an entity",
                        "definition, stereotype, parent, attributes.<name>.type, attributes.<name>.value",
                    ))
                }
            };
            if let Some(p) = &parent {
                self.staged_parents.insert(eid.clone(), p.clone());
            }
            return match (former.as_ref(), note.as_deref()) {
                (Some(src), Some(rewrite)) => {
                    let to = SourceRef {
                        doc: src.doc.clone(),
                        section: src.section.clone(),
                        quote: rewrite.trim().to_string(),
                    };
                    let mut add_attributes = Vec::new();
                    match attribute {
                        Some(mut a) => {
                            a.provenance = Provenance::Quote(to.clone());
                            add_attributes.push(a);
                        }
                        None => self.stage(Op::PlaceAnchor {
                            id: eid.clone(),
                            from: src.clone(),
                            to: to.clone(),
                            reevaluate: false,
                        })?,
                    }
                    self.stage(Op::UpdateEntity {
                        id: eid.clone(),
                        name: None,
                        definition,
                        add_aliases: Vec::new(),
                        add_mention: None,
                        stereotype,
                        parent,
                        set_attributes: None,
                        add_attributes,
                        provenance: None,
                    })?;
                    Ok(json!({
                        "id": eid, "field": field, "path": "dual-write",
                        "prose": {"doc": src.doc, "section": src.section, "old_text": src.quote, "new_text": to.quote}
                    }))
                }
                _ => {
                    let decree = self.decree(note);
                    let attr_name = attribute.as_ref().map(|a| a.name.clone());
                    let proposal = self.snapshot.ratification_proposal(
                        &eid,
                        &sentence,
                        &decree,
                        former.as_ref(),
                        &[eid.clone()],
                        attr_name.as_deref(),
                    );
                    let (add_attributes, provenance) = match attribute {
                        Some(mut a) => {
                            a.provenance = decree.clone();
                            (vec![a], None)
                        }
                        None => (Vec::new(), Some(decree)),
                    };
                    self.stage(Op::UpdateEntity {
                        id: eid.clone(),
                        name: None,
                        definition,
                        add_aliases: Vec::new(),
                        add_mention: None,
                        stereotype,
                        parent,
                        set_attributes: None,
                        add_attributes,
                        provenance,
                    })?;
                    let question = proposal.prompt.as_ref().map(|p| p.question.clone());
                    self.stage(Op::ReportDiagnostic {
                        id: String::new(),
                        diagnostic: proposal,
                    })?;
                    Ok(json!({
                        "id": eid, "field": field, "path": "decree", "proposal": question,
                        "note": "the edit landed graph-only with decree provenance; a ratification proposal asks the owner to state it in the documents"
                    }))
                }
            };
        }

        if let Some((vid, v)) = self.view_known(&raw_id) {
            if field != "members" {
                return Err(bad_field("a view", "members"));
            }
            let Some(arr) = value.as_array() else {
                return Err(ToolError::new(
                    "bad-args",
                    "members takes an ordered list of node ids".into(),
                ));
            };
            let raw: Vec<String> = arr
                .iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect();
            let members = self.canon_members(&raw, "member")?;
            self.check_view_members(&v.kind, &members, &v.collapse)?;
            self.stage(Op::UpdateView {
                id: vid.clone(),
                title: None,
                members: Some(members),
                add_members: Vec::new(),
                remove_members: Vec::new(),
                query: None,
                collapse: None,
                exclude: Vec::new(),
                reasoning: note,
            })?;
            return Ok(json!({
                "id": vid, "field": "members", "path": "decree",
                "note": "the view is curated from here on; views carry no ratification proposal"
            }));
        }

        Err(ToolError::new(
            "unknown-id",
            format!(
                "unknown id `{}`; edit_fact takes a requirement (req:), entity (ent:), or view (view:) id",
                raw_id
            ),
        ))
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
            DocRecord {
                content_hash: hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage: BTreeMap::new(),
            },
        );
        s.graph.entities.insert(
            "ent:customer".into(),
            Entity {
                name: "Customer".into(),
                definition: Some("a person who buys".into()),
                ..Default::default()
            },
        );
        ToolSession::new(
            s,
            reconcile_scope("shop.md", &["/shop", "/shop/cart"], &[]),
            64,
            24_000,
        )
    }

    // A batch of reconcile-section goals, one per section, with the stale anchors on
    // each section's goal.
    fn reconcile_scope(doc: &str, sections: &[&str], stale: &[&str]) -> WorkScope {
        let goals: Vec<Goal> = sections
            .iter()
            .map(|sec| Goal {
                id: format!("g:reconcile-section:{}#{}", doc, sec),
                kind: "reconcile-section".into(),
                mandatory: true,
                target: format!("{}#{}", doc, sec),
                change: json!({"staleAnchors": stale}),
                ..Default::default()
            })
            .collect();
        WorkScope::for_batch("b0-1", &goals)
    }

    fn plain(name: &str) -> Entity {
        Entity {
            name: name.into(),
            ..Default::default()
        }
    }

    fn under(name: &str, parent: &str) -> Entity {
        Entity {
            name: name.into(),
            parent: Some(parent.into()),
            ..Default::default()
        }
    }

    fn quoted_req(statement: &str, entities: &[&str], quote: &str) -> Requirement {
        Requirement {
            statement: statement.into(),
            entities: entities.iter().map(|e| e.to_string()).collect(),
            source: Some(SourceRef {
                doc: "shop.md".into(),
                section: "/shop/cart".into(),
                quote: quote.into(),
            }),
            ..Default::default()
        }
    }

    // Every staging gate names its rule and the repair.
    // Mirrors docs/compiler/graph.md#validation-gates.
    #[test]
    fn empty_optional_arguments_count_as_absent() {
        // gpt-class models fill every schema field; "" / [] / {} must read as
        // omitted (docs/compiler/tools.md#validation-and-errors). This is the
        // exact call shape a gpt-5.5 run staged, bounced by bad-provenance.
        let mut t = session();
        let v = t
            .dispatch(
                "upsert_entity",
                &json!({
                    "name": "Gadget", "definition": "A gadget.", "aliases": [],
                    "scope": "", "stereotype": "device", "parent": "",
                    "attributes": [],
                    "mention": {"section": "/shop/cart", "quote": "holds items"},
                    "provenance": {"derived": {"from": [], "reasoning": ""}},
                    "note": ""
                }),
            )
            .unwrap();
        assert_eq!(v["created"], true);
        let v = t
            .dispatch(
                "upsert_requirement",
                &json!({
                    "statement": "The gadget holds items.",
                    "entities": ["ent:gadget"],
                    "section": "/shop/cart", "quote": "holds items",
                    "edges": [], "facets": [],
                    "provenance": {"derived": {"from": [], "reasoning": ""}}
                }),
            )
            .unwrap();
        assert_eq!(v["created"], true);
        // The doctrine covers structured optionals too: an all-empty transition
        // object and all-empty items inside lists are filled-in blanks, not data.
        // This is the exact shape a gpt-5.5 run staged, bounced by unknown-id on
        // the empty transition subject.
        let v = t
            .dispatch(
                "upsert_requirement",
                &json!({
                    "statement": "A customer intends to buy the items.",
                    "entities": ["ent:customer", "ent:gadget"],
                    "section": "/shop/cart", "quote": "intends to buy",
                    "edges": [{}], "facets": [{}],
                    "transition": {"subject": "", "from": "", "to": "", "trigger": "", "guard": ""}
                }),
            )
            .unwrap();
        assert_eq!(v["created"], true);
        let spun = t
            .staged
            .iter()
            .find_map(|op| match op {
                Op::CreateRequirement { requirement, .. }
                    if requirement.statement == "A customer intends to buy the items." =>
                {
                    Some(requirement)
                }
                _ => None,
            })
            .expect("the requirement staged");
        assert!(spun.transition.is_none());
        assert!(spun.edges.is_empty());
        assert!(spun.facets.is_empty());
        // Hollow items inside otherwise-real arguments drop the same way: ""
        // aliases, blank attribute rows, a hollow per-attribute provenance
        // (falls back to the call's quote), "" entity ids, and empty edge
        // type/cardinality strings (an untyped edge, no cardinality).
        let v = t
            .dispatch(
                "upsert_entity",
                &json!({
                    "name": "Sprocket", "definition": "A sprocket.",
                    "aliases": [""],
                    "attributes": [{}, {"name": "teeth", "type": "int",
                                        "provenance": {"section": "", "quote": ""}}],
                    "mention": {"section": "/shop/cart", "quote": "intends to buy"}
                }),
            )
            .unwrap();
        assert_eq!(v["created"], true);
        let sprocket = t
            .staged_entities
            .get("ent:sprocket")
            .expect("staged entity");
        assert!(sprocket.aliases.is_empty());
        assert_eq!(sprocket.attributes.len(), 1);
        let v = t
            .dispatch(
                "upsert_requirement",
                &json!({
                    "statement": "The sprocket gadget pair holds.",
                    "entities": ["ent:sprocket", "ent:gadget", ""],
                    "section": "/shop/cart", "quote": "a Customer intends",
                    "edges": [{"a": "ent:sprocket", "b": "ent:gadget",
                               "type": "", "cardinality": ""}]
                }),
            )
            .unwrap();
        assert_eq!(v["created"], true);
        let pair = t
            .staged
            .iter()
            .find_map(|op| match op {
                Op::CreateRequirement { requirement, .. }
                    if requirement.statement == "The sprocket gadget pair holds." =>
                {
                    Some(requirement)
                }
                _ => None,
            })
            .expect("the pair requirement staged");
        assert_eq!(pair.entities.len(), 2);
        assert_eq!(pair.edges.len(), 1);
        // A hollow prompt object on a diagnostic was filled in, not asked.
        t.dispatch(
            "report_diagnostic",
            &json!({"rule": "lint", "severity": "warning",
                    "subjects": ["ent:customer"],
                    "message": "Sprocket is undefined in the glossary.",
                    "prompt": {"question": "", "options": [], "freeform": false}}),
        )
        .unwrap();
        // All-empty on both sides still reads as no provenance at all.
        let err = t
            .dispatch(
                "upsert_entity",
                &json!({"name": "Widget", "mention": {},
                        "provenance": {"derived": {"from": [], "reasoning": ""}}}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "provenance-required");
    }

    #[test]
    fn get_entity_sees_the_sessions_staged_entities() {
        // canon_entity_id resolves staged ids, so the lookup must too: a session
        // reading an entity it just staged gets it back, not `unknown-id` with
        // that very id listed as a near miss.
        let mut t = session();
        t.dispatch(
            "upsert_entity",
            &json!({"name": "Gadget", "definition": "A gadget.",
                    "mention": {"section": "/shop/cart", "quote": "holds items"}}),
        )
        .unwrap();
        let v = t
            .dispatch("get_entity", &json!({"id": "ent:gadget"}))
            .unwrap();
        assert_eq!(v["id"], "ent:gadget");
        assert_eq!(v["name"], "Gadget");
        // Read-your-writes: a staged update shows in the read, for a staged
        // create and for a committed entity alike.
        t.dispatch(
            "update_entity",
            &json!({"id": "ent:gadget", "definition": "A precise gadget.",
                    "add_aliases": ["widget"]}),
        )
        .unwrap();
        let v = t
            .dispatch("get_entity", &json!({"id": "ent:gadget"}))
            .unwrap();
        assert_eq!(v["definition"], "A precise gadget.");
        assert_eq!(v["aliases"][0], "widget");
        t.dispatch(
            "update_entity",
            &json!({"id": "ent:customer", "definition": "a person who buys things"}),
        )
        .unwrap();
        let v = t
            .dispatch("get_entity", &json!({"id": "ent:customer"}))
            .unwrap();
        assert_eq!(v["definition"], "a person who buys things");
    }

    #[test]
    fn report_diagnostic_answers_with_the_id_and_reads_see_it() {
        // The model's own feedback: a bare {"reported": true} left a just-filed
        // finding unaddressable. The reply carries the id, a re-report answers
        // with the id it updates, and the session's reads see staged findings.
        let mut t = session();
        let v = t
            .dispatch(
                "report_diagnostic",
                &json!({"rule": "ambiguity", "severity": "warning",
                        "subjects": ["ent:customer"], "message": "Which customer?"}),
            )
            .unwrap();
        let id = v["id"].as_str().expect("an id").to_string();
        assert!(id.starts_with("diag:ambiguity-"), "{}", id);
        assert_eq!(v["created"], true);
        let again = t
            .dispatch(
                "report_diagnostic",
                &json!({"rule": "ambiguity", "severity": "info",
                        "subjects": ["ent:customer"], "message": "Which customer, really?"}),
            )
            .unwrap();
        assert_eq!(again["id"], id.as_str());
        assert_eq!(again["created"], false);
        let list = t.dispatch("diagnostics", &json!({})).unwrap();
        assert!(
            list["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["id"] == id.as_str()),
            "{}",
            list
        );
        let r = t
            .dispatch(
                "resolve_diagnostic",
                &json!({"id": id, "reason": "resolved in review"}),
            )
            .unwrap();
        assert_eq!(r["resolved"], true);
    }

    #[test]
    fn entity_and_requirement_gates_name_their_rules() {
        let mut t = session();
        let ents = &mut t.snapshot.graph.entities;
        ents.insert("ent:cart".into(), plain("Cart"));
        ents.insert("ent:order".into(), plain("Order"));
        ents.insert("ent:item".into(), under("Item", "ent:cart"));
        ents.insert("ent:item-2".into(), under("Item", "ent:order"));
        let mention = json!({"section": "/shop/cart", "quote": "holds items"});
        let err = t
            .dispatch(
                "upsert_entity",
                &json!({"name": "Item", "mention": mention}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "ambiguous-name");
        assert!(
            err.message.contains("ent:item-2") && err.message.contains("parent"),
            "{}",
            err.message
        );
        let v = t
            .dispatch(
                "upsert_entity",
                &json!({"name": "Item", "parent": "ent:order", "mention": mention}),
            )
            .unwrap();
        assert_eq!(v["id"], "ent:item-2");
        assert_eq!(v["created"], false);
        let err = t
            .dispatch(
                "upsert_entity",
                &json!({"name": "Widget", "parent": "ent:nope", "mention": mention}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "unknown-parent");
        let err = t
            .dispatch(
                "update_entity",
                &json!({"id": "ent:cart", "parent": "ent:item"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "parent-cycle");
        assert!(err.message.contains("ent:item"), "{}", err.message);
        let err = t
            .dispatch("upsert_entity", &json!({"name": "Widget"}))
            .unwrap_err();
        assert_eq!(err.rule, "provenance-required");
        let v = t
            .dispatch("upsert_entity", &json!({"name": "Pricing", "parent": "ent:cart", "stereotype": "module", "provenance": {"derived": {"from": ["ent:cart"], "reasoning": "split out"}}}))
            .unwrap();
        assert_eq!(v["created"], true);
        assert!(matches!(
            t.staged.last(),
            Some(Op::CreateEntity { entity, .. })
                if entity.parent.as_deref() == Some("ent:cart")
                    && entity.stereotype.as_deref() == Some("module")
                    && matches!(entity.provenance, Some(Provenance::Derived { .. }))
        ));
        // A staged parent counts for the cycle gate: cart under pricing, which is under cart.
        let err = t
            .dispatch(
                "update_entity",
                &json!({"id": "ent:cart", "parent": "ent:pricing"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "parent-cycle");
        // Attributes are unique by name and take the call's quote.
        let err = t
            .dispatch(
                "update_entity",
                &json!({"id": "ent:cart", "attributes": [{"name": "total"}, {"name": "total"}]}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "bad-attribute");
        t.dispatch("upsert_entity", &json!({"name": "Cart", "mention": mention, "attributes": [{"name": "total", "type": "money"}]}))
            .unwrap();
        assert!(matches!(
            t.staged.last(),
            Some(Op::UpdateEntity { add_attributes, .. })
                if add_attributes.len() == 1
                    && matches!(add_attributes[0].provenance, Provenance::Quote(ref q) if q.quote == "holds items")
        ));

        let base = json!({"statement": "The Cart holds items.", "entities": ["ent:cart", "ent:order"], "section": "/shop/cart", "quote": "holds items"});
        let with = |extra: Value| {
            let mut b = base.clone();
            for (k, v) in extra.as_object().unwrap() {
                b[k] = v.clone();
            }
            b
        };
        let err = t
            .dispatch(
                "upsert_requirement",
                &with(
                    json!({"transition": {"subject": "ent:ghost", "from": "open", "to": "paid"}}),
                ),
            )
            .unwrap_err();
        assert_eq!(err.rule, "unknown-id");
        let err = t
            .dispatch("upsert_requirement", &with(json!({"transition": {"subject": "ent:customer", "from": "open", "to": "paid"}})))
            .unwrap_err();
        assert_eq!(err.rule, "bad-transition");
        let err = t
            .dispatch(
                "upsert_requirement",
                &with(
                    json!({"edges": [{"a": "ent:cart", "b": "ent:order", "cardinality": "many"}]}),
                ),
            )
            .unwrap_err();
        assert_eq!(err.rule, "bad-cardinality");
        let err = t
            .dispatch(
                "upsert_requirement",
                &with(json!({"facets": [{"facet": "speed", "reasoning": "x"}]})),
            )
            .unwrap_err();
        assert_eq!(err.rule, "bad-facet");
        let err = t
            .dispatch("upsert_requirement", &with(json!({"facets": [{"facet": "behavior", "reasoning": "x", "measure": "2 seconds"}]})))
            .unwrap_err();
        assert_eq!(err.rule, "bad-facet");
        let err = t
            .dispatch(
                "upsert_requirement",
                &json!({"statement": "The Cart holds items.", "entities": ["ent:cart"]}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "provenance-required");
        let err = t
            .dispatch("report_diagnostic", &json!({"rule": "decision", "severity": "warning", "subjects": ["ent:cart"], "message": "which bound?"}))
            .unwrap_err();
        assert_eq!(err.rule, "decision-needs-prompt");
        // The whole shape lands: edges deduplicated on (a, b, type), transition, facets.
        let v = t
            .dispatch("upsert_requirement", &with(json!({
                "edges": [{"a": "ent:cart", "b": "ent:order", "type": "composition", "cardinality": "1..*"}, {"a": "ent:cart", "b": "ent:order", "type": "composition"}],
                "transition": {"subject": "ent:cart", "from": "open", "to": "paid", "trigger": "payment"},
                "facets": [{"facet": "quality", "reasoning": "bounded", "measure": "2 seconds"}]
            })))
            .unwrap();
        assert_eq!(v["created"], true);
        match t.staged.last().unwrap() {
            Op::CreateRequirement { requirement, .. } => {
                assert_eq!(requirement.edges.len(), 1);
                assert_eq!(requirement.edges[0].cardinality.as_deref(), Some("1..*"));
                assert_eq!(requirement.transition.as_ref().unwrap().subject, "ent:cart");
                assert_eq!(requirement.facets[0].measure.as_deref(), Some("2 seconds"));
            }
            other => panic!("unexpected {:?}", other),
        }
        // A derived requirement mints req:x-<n> and keys on the statement within its from set.
        let derived = |s: &str| json!({"statement": s, "entities": ["ent:cart"], "provenance": {"derived": {"from": ["ent:cart"], "reasoning": "too dense"}}});
        let v = t
            .dispatch(
                "upsert_requirement",
                &derived("The Cart is split by category."),
            )
            .unwrap();
        assert_eq!(v["id"], "req:x-1");
        let again = t
            .dispatch(
                "upsert_requirement",
                &derived("The Cart is split by category"),
            )
            .unwrap();
        assert_eq!(again["id"], "req:x-1");
        assert_eq!(again["updated"], true);
    }

    // The view tools gate kind, membership, uniqueness, collapse, and defaults.
    // Mirrors docs/compiler/tools.md#view-tools.
    #[test]
    fn view_gates_name_their_rules() {
        let mut t = session();
        t.snapshot
            .graph
            .entities
            .insert("ent:cart".into(), plain("Cart"));
        t.snapshot
            .graph
            .entities
            .insert("ent:item".into(), under("Item", "ent:cart"));
        t.snapshot.graph.requirements.insert(
            "req:shop-1".into(),
            quoted_req("The Cart holds items.", &["ent:cart"], "holds items"),
        );
        t.snapshot.graph.views.insert(
            "view:class/public".into(),
            View {
                kind: "class".into(),
                title: "Public".into(),
                members: vec!["ent:cart".into()],
                default: true,
                ..Default::default()
            },
        );
        let call = |members: Value, collapse: Value| json!({"kind": "class", "title": "Cart parts", "members": members, "collapse": collapse, "reasoning": "the cart's parts"});
        let err = t
            .dispatch(
                "upsert_view",
                &json!({"kind": "blueprint", "title": "X", "reasoning": "r"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "bad-kind");
        let err = t
            .dispatch("upsert_view", &call(json!(["ent:ghost"]), json!([])))
            .unwrap_err();
        assert_eq!(err.rule, "unknown-id");
        let err = t
            .dispatch(
                "upsert_view",
                &call(json!(["ent:cart", "ent:cart"]), json!([])),
            )
            .unwrap_err();
        assert_eq!(err.rule, "duplicate-member");
        let err = t
            .dispatch("upsert_view", &call(json!(["req:shop-1"]), json!([])))
            .unwrap_err();
        assert_eq!(err.rule, "bad-member");
        let err = t
            .dispatch(
                "upsert_view",
                &call(json!(["ent:item"]), json!(["ent:customer"])),
            )
            .unwrap_err();
        assert_eq!(err.rule, "bad-collapse");
        // An ancestor of a member collapses; a repeated upsert lands on the staged view.
        let v = t
            .dispatch(
                "upsert_view",
                &call(json!(["ent:item"]), json!(["ent:cart"])),
            )
            .unwrap();
        assert_eq!(v["id"], "view:class/cart-parts");
        assert_eq!(v["created"], true);
        let again = t
            .dispatch(
                "upsert_view",
                &call(json!(["ent:item"]), json!(["ent:cart"])),
            )
            .unwrap();
        assert_eq!(again["id"], "view:class/cart-parts");
        assert_eq!(again["created"], false);
        t.dispatch(
            "update_view",
            &json!({"id": "view:class/cart-parts", "add_members": ["ent:cart"]}),
        )
        .unwrap();
        // Empty means absent: members [] is a filled-in blank, not "replace the
        // membership with nothing".
        t.dispatch(
            "update_view",
            &json!({"id": "view:class/cart-parts", "members": [], "add_members": ["ent:customer"]}),
        )
        .unwrap();
        let staged = t
            .staged_views
            .get("view:class/cart-parts")
            .expect("staged view");
        assert!(staged.members.contains(&"ent:item".to_string()));
        assert!(staged.members.contains(&"ent:customer".to_string()));
        let err = t
            .dispatch(
                "update_view",
                &json!({"id": "view:class/cart-parts", "add_members": ["req:shop-1"]}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "bad-member");
        let err = t
            .dispatch(
                "update_view",
                &json!({"id": "view:class/nope", "title": "x"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "unknown-id");
        let err = t
            .dispatch(
                "delete_view",
                &json!({"id": "view:class/public", "reason": "noise"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "default-view");
        // The default's natural key lands on it; the store clears default at commit.
        let v = t
            .dispatch("upsert_view", &json!({"kind": "class", "title": "Public", "members": ["ent:cart"], "reasoning": "curated"}))
            .unwrap();
        assert_eq!(v["id"], "view:class/public");
        assert_eq!(v["created"], false);
        let err = t
            .dispatch("upsert_view", &json!({"kind": "sequence", "title": "Flow", "members": ["ent:cart"], "reasoning": "r"}))
            .unwrap_err();
        assert_eq!(err.rule, "bad-member");
        let v = t
            .dispatch("upsert_view", &json!({"kind": "sequence", "title": "Flow", "members": ["req:shop-1"], "excluded": [{"id": "ent:cart", "note": "structure, not flow"}], "reasoning": "r"}))
            .unwrap();
        assert_eq!(v["id"], "view:sequence/flow");
        t.dispatch(
            "delete_view",
            &json!({"id": "view:sequence/flow", "reason": "no"}),
        )
        .unwrap();
        assert!(matches!(t.staged.last(), Some(Op::DeleteView { .. })));
    }

    // edit_fact stages a dual write when the fact is quoted and a sentence rewrite was
    // accepted, and a decree with its ratification proposal otherwise.
    // Mirrors docs/compiler/tools.md#chat-tools.
    #[test]
    fn edit_fact_dual_writes_a_quoted_fact_and_decrees_otherwise() {
        let mut t = session();
        let old = "The Shopping Cart holds items a Customer intends to buy.";
        t.snapshot
            .graph
            .requirements
            .insert("req:shop-1".into(), quoted_req(old, &["ent:customer"], old));
        let new = "The Shopping Cart holds items a Customer selected.";
        let v = t
            .dispatch(
                "edit_fact",
                &json!({"id": "req:shop-1", "field": "statement", "value": new, "note": new}),
            )
            .unwrap();
        assert_eq!(v["path"], "dual-write");
        assert_eq!(v["prose"]["old_text"], old);
        assert_eq!(v["prose"]["new_text"], new);
        assert!(matches!(
            t.staged.last(),
            Some(Op::UpdateRequirement { statement: Some(s), source: Some(src), provenance: None, .. })
                if s == new && src.quote == new
        ));
        let v = t
            .dispatch("edit_fact", &json!({"id": "req:shop-1", "field": "facets", "value": [{"facet": "behavior", "reasoning": "a step"}]}))
            .unwrap();
        assert_eq!(v["path"], "decree");
        let n = t.staged.len();
        assert!(matches!(
            &t.staged[n - 2],
            Op::UpdateRequirement { facets: Some(f), source: None, provenance: Some(Provenance::Decree { .. }), .. }
                if f.len() == 1
        ));
        match &t.staged[n - 1] {
            Op::ReportDiagnostic { diagnostic, .. } => {
                assert_eq!(diagnostic.rule, "ratification-pending");
                assert_eq!(diagnostic.subjects, vec!["req:shop-1".to_string()]);
                let p = diagnostic.prompt.as_ref().unwrap();
                let e = p.options[0].edit.as_ref().unwrap();
                assert_eq!(
                    (e.doc.as_str(), e.section.as_str()),
                    ("shop.md", "/shop/cart")
                );
                assert_eq!(e.old_text, old);
                assert_eq!(e.new_text, old, "the statement is the proposal");
                assert_eq!(p.options[1].answer.as_deref(), Some("retract"));
                assert!(p.freeform);
            }
            other => panic!("unexpected {:?}", other),
        }
        let err = t
            .dispatch(
                "edit_fact",
                &json!({"id": "req:shop-1", "field": "created", "value": "g9"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "bad-field");
        let err = t
            .dispatch(
                "edit_fact",
                &json!({"id": "req:nope", "field": "statement", "value": "x"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "unknown-id");
        // An entity's parent by decree, and a view's members.
        t.snapshot
            .graph
            .entities
            .insert("ent:cart".into(), plain("Cart"));
        let v = t
            .dispatch(
                "edit_fact",
                &json!({"id": "ent:customer", "field": "parent", "value": "ent:cart"}),
            )
            .unwrap();
        assert_eq!(v["path"], "decree");
        let n = t.staged.len();
        assert!(matches!(
            &t.staged[n - 2],
            Op::UpdateEntity { parent: Some(p), provenance: Some(Provenance::Decree { .. }), .. } if p == "ent:cart"
        ));
        t.snapshot.graph.views.insert(
            "view:class/public".into(),
            View {
                kind: "class".into(),
                title: "Public".into(),
                members: vec!["ent:customer".into()],
                default: true,
                ..Default::default()
            },
        );
        let v = t
            .dispatch("edit_fact", &json!({"id": "view:class/public", "field": "members", "value": ["ent:customer", "ent:cart"]}))
            .unwrap();
        assert_eq!(v["path"], "decree");
        assert!(matches!(
            t.staged.last(),
            Some(Op::UpdateView { members: Some(m), .. }) if m.len() == 2
        ));
        // A ratification proposal never closes through resolve_diagnostic.
        t.snapshot.graph.diagnostics.insert(
            "diag:ratification-pending-1".into(),
            Diagnostic {
                rule: "ratification-pending".into(),
                severity: "warning".into(),
                subjects: vec!["req:shop-1".into()],
                message: "x".into(),
                reasoning: None,
                lifecycle: "open".into(),
                triage: None,
                prompt: None,
                answer: None,
                created: None,
                updated: None,
            },
        );
        let err = t
            .dispatch(
                "resolve_diagnostic",
                &json!({"id": "diag:ratification-pending-1", "reason": "done"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "not-resolvable");
    }

    // A bare empty list reads as "ask again" and models loop on it; a miss must say what
    // the graph holds and what to do instead. Mirrors docs/compiler/tools.md#read-tools.
    #[test]
    fn search_miss_names_the_graph_and_the_next_step() {
        let mut t = session();
        let r = t.dispatch("search", &json!({"query": "slides"})).unwrap();
        assert_eq!(r["hits"].as_array().unwrap().len(), 0);
        assert_eq!(r["entityCount"], 1);
        assert!(
            r["entities"][0].as_str().unwrap().contains("ent:customer"),
            "{}",
            r
        );
        assert!(
            r["next"].as_str().unwrap().contains("upsert_entity"),
            "{}",
            r
        );
        // A hit answers under the same key, so the caller reads one shape.
        let hit = t.dispatch("search", &json!({"query": "Customer"})).unwrap();
        assert_eq!(hit["hits"][0]["id"], "ent:customer");
    }

    // kind narrows the search: view matches on title, entity stays the default, and an
    // unknown kind bounces. Mirrors docs/compiler/tools.md#read-tools.
    #[test]
    fn search_kind_view_matches_on_title() {
        let mut t = session();
        t.snapshot.graph.views.insert(
            "view:class/public".into(),
            View {
                kind: "class".into(),
                title: "Public surface".into(),
                ..Default::default()
            },
        );
        let r = t
            .dispatch("search", &json!({"query": "public", "kind": "view"}))
            .unwrap();
        assert_eq!(r["hits"][0]["id"], "view:class/public");
        assert_eq!(r["hits"][0]["name"], "Public surface");
        // The entity default never returns views.
        let r = t.dispatch("search", &json!({"query": "public"})).unwrap();
        assert_eq!(r["hits"].as_array().unwrap().len(), 0);
        // A view miss is the same documented miss body.
        let r = t
            .dispatch("search", &json!({"query": "slides", "kind": "view"}))
            .unwrap();
        assert_eq!(r["hits"].as_array().unwrap().len(), 0);
        assert_eq!(r["entityCount"], 1);
        // An unknown kind bounces.
        let err = t
            .dispatch("search", &json!({"query": "x", "kind": "diagram"}))
            .unwrap_err();
        assert_eq!(err.rule, "bad-args");
    }

    // A section that yielded a statement cannot also state none of them.
    #[test]
    fn non_normative_contradicts_an_extracted_statement() {
        let mut t = session();
        t.dispatch(
            "upsert_requirement",
            &json!({
                "statement": "The Shopping Cart shall hold items a Customer intends to buy.",
                "entities": ["ent:customer"],
                "section": "/shop/cart",
                "quote": "The Shopping Cart holds items a Customer intends to buy."
            }),
        )
        .unwrap();
        let err = t
            .dispatch(
                "set_coverage",
                &json!({"section": "/shop/cart", "state": "non-normative", "note": "just facts"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "contradicts-extraction");
        assert!(err.message.contains("covered"), "{}", err.message);
        // The honest mark still goes through.
        t.dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "covered"}),
        )
        .unwrap();
        // A section that yielded nothing is still free to be non-normative.
        t.dispatch(
            "set_coverage",
            &json!({"section": "/shop", "state": "non-normative", "note": "navigation only"}),
        )
        .unwrap();
    }

    // Passing the statement statement as the quote is the common miscall; the rejection has to
    // point at the existing anchor, not just repeat that the quote is absent.
    #[test]
    fn reanchor_rejection_names_the_existing_anchor() {
        let mut t = session();
        let id = t
            .dispatch(
                "upsert_requirement",
                &json!({
                    "statement": "The Shopping Cart shall hold items a Customer intends to buy.",
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
        assert!(
            err.message.contains("omit `section` and `quote`"),
            "{}",
            err.message
        );
        // Only-entities is the call that was meant, and it is accepted.
        t.dispatch(
            "update_requirement",
            &json!({"id": id, "entities": ["ent:customer"]}),
        )
        .unwrap();
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
    fn requirement_needs_a_statement_and_known_entities() {
        let mut t = session();
        let err = t
            .dispatch("upsert_requirement", &json!({"statement": "   ", "entities": ["ent:customer"], "section": "/shop/cart", "quote": "holds items"}))
            .unwrap_err();
        assert_eq!(err.rule, "bad-args");
        let err2 = t
            .dispatch("upsert_requirement", &json!({"statement": "The Cart holds items.", "entities": ["ent:cart"], "section": "/shop/cart", "quote": "holds items"}))
            .unwrap_err();
        assert_eq!(err2.rule, "unknown-id");
        assert!(
            err2.message.contains("upsert_entity"),
            "repair hint: {}",
            err2.message
        );
        // Free-form statements land: no shape gate stands between prose and the graph.
        let ok = t
            .dispatch("upsert_requirement", &json!({"statement": "The cart is nice.", "entities": ["ent:customer"], "section": "/shop/cart", "quote": "holds items"}))
            .unwrap();
        assert_eq!(ok["created"], true);
    }

    #[test]
    fn prefixed_case_variant_resolves() {
        let mut t = session();
        let v = t
            .dispatch(
                "update_entity",
                &json!({"id": "ent:Customer", "add_aliases": ["Buyer"]}),
            )
            .unwrap();
        assert_eq!(v["id"], "ent:customer");
    }

    #[test]
    fn update_requirement_takes_an_empty_statement_as_unchanged_and_rejects_a_bad_cardinality() {
        let mut t = session();
        t.dispatch(
            "upsert_entity",
            &json!({"name": "Shopping Cart", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}}),
        )
        .unwrap();
        let r = t
            .dispatch(
                "upsert_requirement",
                &json!({"statement": "The Shopping Cart holds items.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "holds items a Customer intends to buy"}),
            )
            .unwrap();
        let rid = r["id"].as_str().unwrap().to_string();
        // An empty statement counts as absent: the call lands, statement unchanged.
        let v = t
            .dispatch(
                "update_requirement",
                &json!({"id": rid, "statement": "", "entities": ["ent:shopping-cart"]}),
            )
            .unwrap();
        assert_eq!(v["id"], rid);
        let err = t
            .dispatch(
                "update_requirement",
                &json!({"id": rid, "entities": ["ent:shopping-cart", "ent:customer"], "edges": [{"a": "ent:shopping-cart", "b": "ent:customer", "type": "composition", "cardinality": "many"}]}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "bad-cardinality");
        assert!(err.message.contains("1..*"), "{}", err.message);
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
        t.dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "covered"}),
        )
        .unwrap();
        assert!(t.dispatch("done", &json!({"summary": "covered"})).is_err());
        // The implicit done drops the offending mark and commits the rest.
        assert!(t.finish_implicit("(implicit: test)"));
        assert!(t.done.is_some());
        assert!(t
            .staged
            .iter()
            .any(|op| matches!(op, Op::CreateEntity { .. })));
        assert!(!t
            .staged
            .iter()
            .any(|op| matches!(op, Op::SetCoverage { state, .. } if state == "covered")));
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
            &json!({"statement": "The Shopping Cart shall hold items a Customer intends to buy.", "entities": [id, "ent:customer"], "section": "/shop/cart", "quote": "holds items a Customer intends to buy", "edges": [{"a": "ent:shopping-cart", "b": "ent:customer", "type": "association"}]}),
        )
        .unwrap();
        t.dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "covered"}),
        )
        .unwrap();
        let err = t
            .dispatch(
                "set_coverage",
                &json!({"section": "/nope", "state": "covered"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "unknown-section");
        t.dispatch(
            "set_coverage",
            &json!({"section": "/shop", "state": "non-normative", "note": "intro text only"}),
        )
        .unwrap();
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:reconcile-section:shop.md#/shop/cart", "justification": "Extracted the cart statement and marked the section."}),
        )
        .unwrap();
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:reconcile-section:shop.md#/shop", "justification": "Intro text only."}),
        )
        .unwrap();
        t.dispatch("done", &json!({"summary": "reconciled cart"}))
            .unwrap();
        assert!(t.done.is_some());
        assert_eq!(t.staged.len(), 4);
    }

    // done refuses while a mandatory goal is unaddressed, and mark_goal_done rejects
    // a false claim: the reconcile-section goal whose section carries no mark.
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
            &json!({"statement": "The Shopping Cart shall hold items.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "holds items a Customer intends to buy"}),
        )
        .unwrap();
        t.dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "covered"}),
        )
        .unwrap();
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:reconcile-section:shop.md#/shop/cart", "justification": "Extracted and marked."}),
        )
        .unwrap();
        // The other section's goal owes its mark: the claim is rejected naming the gate.
        let err = t
            .dispatch(
                "mark_goal_done",
                &json!({"goal": "g:reconcile-section:shop.md#/shop", "justification": "Nothing here."}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "unmarked-section");
        assert!(err.message.contains("/shop"));
        // done with a mandatory goal neither done nor failed is rejected naming it.
        let err = t
            .dispatch("done", &json!({"summary": "cart only"}))
            .unwrap_err();
        assert_eq!(err.rule, "open-goal");
        assert!(err.message.contains("g:reconcile-section:shop.md#/shop"));
        t.dispatch(
            "set_coverage",
            &json!({"section": "/shop", "state": "non-normative", "note": "intro text only"}),
        )
        .unwrap();
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:reconcile-section:shop.md#/shop", "justification": "Intro text only."}),
        )
        .unwrap();
        t.dispatch("done", &json!({"summary": "all sections marked"}))
            .unwrap();
        assert!(t.done.is_some());
    }

    #[test]
    fn edges_must_be_subset_of_entities() {
        let mut t = session();
        t.dispatch("upsert_entity", &json!({"name": "Shopping Cart", "mention": {"section": "/shop/cart", "quote": "The Shopping Cart holds items"}})).unwrap();
        let err = t
            .dispatch(
                "upsert_requirement",
                &json!({"statement": "The Shopping Cart shall exist.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "holds items", "edges": [{"a": "ent:shopping-cart", "b": "ent:customer"}]}),
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
                &json!({"statement": "The Customer shall buy items.", "entities": ["customer"], "section": "/shop/cart", "quote": "a Customer intends to buy"}),
            )
            .unwrap();
        assert_eq!(v["created"], true);
        let v2 = t
            .dispatch(
                "upsert_requirement",
                &json!({"statement": "The Customer shall intend to buy.", "entities": ["Customer"], "section": "/shop/cart", "quote": "intends to buy"}),
            )
            .unwrap();
        assert_eq!(v2["created"], true);
        match t
            .staged
            .iter()
            .find(|o| matches!(o, Op::CreateRequirement { .. }))
            .unwrap()
        {
            Op::CreateRequirement { requirement, .. } => {
                assert_eq!(requirement.entities, vec!["ent:customer".to_string()])
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn escaped_quote_locates_and_stores_source_form() {
        let mut t = session();
        let v = t
            .dispatch(
                "upsert_requirement",
                &json!({"statement": "The Customer shall intend to buy.", "entities": ["ent:customer"], "section": "/shop/cart", "quote": "a Customer intends to buy\\."}),
            )
            .unwrap();
        assert_eq!(v["created"], true);
        match t
            .staged
            .iter()
            .find(|o| matches!(o, Op::CreateRequirement { .. }))
            .unwrap()
        {
            Op::CreateRequirement { requirement, .. } => {
                assert_eq!(
                    requirement.source.as_ref().unwrap().quote,
                    "a Customer intends to buy."
                )
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn coverage_restage_replaces_earlier_mark() {
        let mut t = session();
        t.dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "non-normative", "note": "just prose"}),
        )
        .unwrap();
        t.dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "covered"}),
        )
        .unwrap();
        let marks: Vec<&Op> = t
            .staged
            .iter()
            .filter(|o| matches!(o, Op::SetCoverage { .. }))
            .collect();
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
            .dispatch(
                "set_coverage",
                &json!({"section": "/shop/cart", "state": "non-normative", "note": "<nil>"}),
            )
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
            DocRecord {
                content_hash: hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage: BTreeMap::new(),
            },
        );
        s.graph.entities.insert(
            "ent:shopping-cart".into(),
            Entity {
                name: "Shopping Cart".into(),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:shop-1".into(),
            Requirement {
                statement: "The Shopping Cart shall hold items a Customer intends to buy.".into(),
                entities: vec!["ent:shopping-cart".into()],
                edges: Vec::new(),
                source: Some(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop/cart".into(),
                    quote: "The Shopping Cart holds items a Customer intends to buy.".into(),
                }),
                ..Default::default()
            },
        );
        ToolSession::new(
            s,
            reconcile_scope("shop.md", &["/shop/cart"], &["req:shop-1"]),
            64,
            24_000,
        )
    }

    #[test]
    fn done_rejects_untouched_stale_anchor() {
        let mut t = session_with_stale_anchor();
        t.dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "covered"}),
        )
        .unwrap();
        let goal = "g:reconcile-section:shop.md#/shop/cart";
        let err = t
            .dispatch(
                "mark_goal_done",
                &json!({"goal": goal, "justification": "Covered."}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "stale-anchor");
        assert!(
            err.message.contains("req:shop-1"),
            "names the anchor: {}",
            err.message
        );
        // Failing the goal does not let the batch commit around the anchor.
        t.dispatch(
            "mark_goal_failed",
            &json!({"goal": goal, "reason": "cannot decide the anchor"}),
        )
        .unwrap();
        let err = t
            .dispatch("done", &json!({"summary": "covered around the anchor"}))
            .unwrap_err();
        assert_eq!(err.rule, "stale-anchor");
    }

    #[test]
    fn stale_anchor_satisfied_by_delete() {
        let mut t = session_with_stale_anchor();
        t.dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "covered"}),
        )
        .unwrap();
        t.dispatch(
            "delete_requirement",
            &json!({"id": "req:shop-1", "reason": "the document dropped the statement"}),
        )
        .unwrap();
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:reconcile-section:shop.md#/shop/cart", "justification": "The fact is gone; deleted the anchor."}),
        )
        .unwrap();
        t.dispatch("done", &json!({"summary": "anchor deleted"}))
            .unwrap();
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
                &json!({"statement": "The Shopping Cart shall hold items a Customer intends to buy.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "The Shopping Cart keeps items a Customer intends to buy."}),
            )
            .unwrap();
        assert_eq!(v["id"], "req:shop-1");
        assert_eq!(v["updated"], true);
        t.dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "covered"}),
        )
        .unwrap();
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:reconcile-section:shop.md#/shop/cart", "justification": "Re-recorded on the anchor."}),
        )
        .unwrap();
        t.dispatch("done", &json!({"summary": "re-anchored"}))
            .unwrap();
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
        let args = json!({"statement": "The Shopping Cart shall hold items.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "holds items a Customer intends to buy"});
        let v1 = t.dispatch("upsert_requirement", &args).unwrap();
        assert_eq!(v1["created"], true);
        // The identical call again is idempotent within the turn: same id, one staged op.
        let v2 = t.dispatch("upsert_requirement", &args).unwrap();
        assert_eq!(v2["updated"], true);
        assert_eq!(v1["id"], v2["id"]);
        assert_eq!(
            t.staged
                .iter()
                .filter(|o| matches!(o, Op::CreateRequirement { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn stale_anchor_reworded_statement_resolves_to_the_anchor() {
        let mut t = session_with_stale_anchor();
        // The statement was reworded (it subsumes the anchor's) and the old quote no
        // longer locates: the upsert lands on the anchor's id, updating in place.
        let v = t
            .dispatch(
                "upsert_requirement",
                &json!({"statement": "The Shopping Cart shall hold items.", "entities": ["ent:shopping-cart"], "section": "/shop/cart", "quote": "The Shopping Cart keeps items a Customer intends to buy."}),
            )
            .unwrap();
        assert_eq!(v["id"], "req:shop-1");
        assert_eq!(v["updated"], true);
        t.dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "covered"}),
        )
        .unwrap();
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:reconcile-section:shop.md#/shop/cart", "justification": "Reworded onto the anchor."}),
        )
        .unwrap();
        t.dispatch("done", &json!({"summary": "re-anchored reworded"}))
            .unwrap();
        assert!(t.done.is_some());
    }

    #[test]
    fn update_requirement_reanchors_with_section_and_quote() {
        let mut t = session_with_stale_anchor();
        // Re-anchoring needs the pair together, and the quote must locate.
        let err = t
            .dispatch(
                "update_requirement",
                &json!({"id": "req:shop-1", "section": "/shop/cart"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "bad-argument");
        let err2 = t
            .dispatch("update_requirement", &json!({"id": "req:shop-1", "section": "/shop/cart", "quote": "not in the document"}))
            .unwrap_err();
        assert_eq!(err2.rule, "quote-not-found");
        t.dispatch(
            "update_requirement",
            &json!({"id": "req:shop-1", "statement": "The Shopping Cart shall keep items a Customer intends to buy.", "section": "/shop/cart", "quote": "The Shopping Cart keeps items a Customer intends to buy."}),
        )
        .unwrap();
        t.dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "covered"}),
        )
        .unwrap();
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:reconcile-section:shop.md#/shop/cart", "justification": "Revised and re-anchored."}),
        )
        .unwrap();
        t.dispatch("done", &json!({"summary": "revised and re-anchored"}))
            .unwrap();
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
        assert_eq!(logged[0]["task"], "reconcile-section");
        assert_eq!(logged[0]["target"], "shop.md");
        assert_eq!(logged[0]["model"], "test-model");
        assert_eq!(logged[0]["run"], "run-1");

        // An empty message is the one rejection; an unknown kind is not.
        let err = t
            .dispatch(
                "report_feedback",
                &json!({"kind": "wrong", "message": "  "}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "missing-argument");
        t.dispatch(
            "report_feedback",
            &json!({"kind": "puzzled", "message": "second"}),
        )
        .unwrap();
        assert_eq!(crate::feedback::read(&dir, 10)[0]["kind"], "other");

        // The cap holds: further calls are acknowledged without a record.
        for i in 0..5 {
            t.dispatch(
                "report_feedback",
                &json!({"kind": "confusing", "message": format!("more {}", i)}),
            )
            .unwrap();
        }
        assert_eq!(crate::feedback::read(&dir, 99).len(), FEEDBACK_LIMIT);
        std::fs::remove_dir_all(&dir).ok();
    }

    // The per-kind goal pages state what the model sees; a toolset change that
    // forgets the page would let them drift. Every name in a kind's toolset plus the
    // always-on set must appear on its page, and a new kind cannot ship without one.
    // Mirrors docs/compiler/goals/<kind>.md#tools.
    #[test]
    fn task_docs_name_every_tool() {
        for k in crate::goals::REGISTRY.iter() {
            if crate::goals::blocked_on_human(k.kind()) {
                continue;
            }
            let doc = match k.kind() {
                "place-anchors" => include_str!("../../docs/compiler/goals/place-anchors.md"),
                "reconcile-section" => {
                    include_str!("../../docs/compiler/goals/reconcile-section.md")
                }
                "rejudge-pair" => include_str!("../../docs/compiler/goals/rejudge-pair.md"),
                "review-entity" => include_str!("../../docs/compiler/goals/review-entity.md"),
                "retrace" => include_str!("../../docs/compiler/goals/retrace.md"),
                "conform-instance" => {
                    include_str!("../../docs/compiler/goals/conform-instance.md")
                }
                "bind" => include_str!("../../docs/compiler/goals/bind.md"),
                "generate" => include_str!("../../docs/compiler/goals/generate.md"),
                "verify" => include_str!("../../docs/compiler/goals/verify.md"),
                "declare-edges" => include_str!("../../docs/compiler/goals/declare-edges.md"),
                "dedupe-candidates" => {
                    include_str!("../../docs/compiler/goals/dedupe-candidates.md")
                }
                "curate-view" => include_str!("../../docs/compiler/goals/curate-view.md"),
                "split-view" => include_str!("../../docs/compiler/goals/split-view.md"),
                "abstract-entity" => {
                    include_str!("../../docs/compiler/goals/abstract-entity.md")
                }
                other => panic!("no page wired for kind {}", other),
            };
            let mut names: Vec<&str> = k.toolset().to_vec();
            names.extend(READ_TOOLS);
            names.extend(GOAL_TOOLS);
            names.push(FEEDBACK_TOOL);
            for tool in names {
                assert!(
                    doc.contains(tool),
                    "the {} page misses tool `{}`",
                    k.kind(),
                    tool
                );
            }
        }
    }

    // A WorkItem built from a GC goal carries the kind's name as its task; the
    // toolset is the kind's slice, never the read-only fallback (the snippet and
    // the MCP case path build their tool list from it).
    // The justification counter: document names, ids, and abbreviations running
    // into lowercase never end a sentence; an opener after the period does.
    #[test]
    fn sentence_count_ignores_dots_in_names_and_abbreviations() {
        let n = ToolSession::sentence_count;
        assert_eq!(n("The definition fits customer.md verbatim."), 1);
        assert_eq!(n("Order is a separate concept (the area vs. its member), so it stands."), 1);
        assert_eq!(n("It matches, e.g. the cart line. Nothing changed."), 2);
        assert_eq!(n("One. Two. Three."), 3);
        assert_eq!(n("Is it done? Yes! Done"), 3);
        assert_eq!(n("Ends with an id like ent:order. (Then a bracket.)"), 2);
        assert_eq!(n(""), 0);
    }

    #[test]
    fn a_bare_document_reference_reads_its_root_section() {
        let mut t = session();
        for r in ["shop.md", "shop.md#", "shop.md#/"] {
            let v = t.dispatch("read_section", &json!({"ref": r})).unwrap();
            assert_eq!(v["title"], "Shop", "{}", r);
            assert!(v["children"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c.as_str().unwrap().starts_with("shop.md#/shop/cart")));
        }
        let err = t
            .dispatch("read_section", &json!({"ref": "nope.md#"}))
            .unwrap_err();
        assert_eq!(err.rule, "bad-section");
    }

    #[test]
    fn toolset_of_a_goal_kind_task_is_its_slice() {
        let names = toolset("abstract-entity");
        for t in ["group_entities", "dissolve_entity", "update_entity", "mark_goal_done", "done"] {
            assert!(names.contains(&t), "abstract-entity misses `{}`", t);
        }
        assert!(toolset("curate-view").contains(&"update_view"));
        assert!(!toolset("no-such-task").contains(&"mark_goal_done"));
    }

    fn align_session() -> ToolSession {
        let mut s = Store::default();
        let text = "# Shop\nintro\n\n## Basket\nThe Basket keeps items a Customer intends to buy.\nItems stay until checkout.\n";
        s.docs.insert(
            "shop.md".into(),
            DocRecord {
                content_hash: hash_hex(text),
                sections: crate::md::parse_sections(text),
                coverage: BTreeMap::new(),
            },
        );
        s.graph.entities.insert(
            "ent:basket".into(),
            Entity {
                name: "Basket".into(),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:shop-1".into(),
            Requirement {
                statement: "The Basket shall keep items until checkout.".into(),
                entities: vec!["ent:basket".into()],
                edges: Vec::new(),
                source: Some(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop/cart".into(),
                    quote: "Items stay until checkout.".into(),
                }),
                ..Default::default()
            },
        );
        s.graph.requirements.insert(
            "req:shop-2".into(),
            Requirement {
                statement: "The Basket shall hold items a Customer intends to buy.".into(),
                entities: vec!["ent:basket".into()],
                edges: Vec::new(),
                source: Some(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop/cart".into(),
                    quote: "The Cart holds items a Customer intends to buy.".into(),
                }),
                ..Default::default()
            },
        );
        let candidate = |locates: bool| crate::model::AnchorCandidate {
            section: "shop.md#/shop/basket".into(),
            similarity: if locates { 1.0 } else { 0.8 },
            quote_locates: locates,
            nearest: (!locates)
                .then(|| "The Basket keeps items a Customer intends to buy.".to_string()),
            excerpt: String::new(),
        };
        s.status.alignment.push(crate::model::DocAlignment {
            doc: "shop.md".into(),
            changes: vec![],
            proposals: vec![
                crate::model::AnchorProposal {
                    anchor: "req:shop-1".into(),
                    from: "shop.md#/shop/cart".into(),
                    quote: "Items stay until checkout.".into(),
                    excerpt: String::new(),
                    candidates: vec![candidate(true)],
                },
                crate::model::AnchorProposal {
                    anchor: "req:shop-2".into(),
                    from: "shop.md#/shop/cart".into(),
                    quote: "The Cart holds items a Customer intends to buy.".into(),
                    excerpt: String::new(),
                    candidates: vec![candidate(false)],
                },
            ],
        });
        let goal = Goal {
            id: "g:place-anchors:shop.md".into(),
            kind: "place-anchors".into(),
            mandatory: true,
            target: "shop.md".into(),
            change: json!({"anchors": ["req:shop-1", "req:shop-2"]}),
            ..Default::default()
        };
        ToolSession::new(
            s,
            WorkScope::for_batch("b0-1", std::slice::from_ref(&goal)),
            64,
            24_000,
        )
    }

    #[test]
    fn align_done_rejects_an_undecided_proposal() {
        let mut s = align_session();
        s.dispatch(
            "place_anchor",
            &json!({"id": "req:shop-1", "section": "/shop/basket", "reevaluate": false}),
        )
        .unwrap();
        let err = s
            .dispatch(
                "mark_goal_done",
                &json!({"goal": "g:place-anchors:shop.md", "justification": "Both proposals decided."}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "undecided-proposal");
        assert!(err.message.contains("req:shop-2"));
        let err = s.dispatch("done", &json!({"summary": "x"})).unwrap_err();
        assert_eq!(err.rule, "open-goal");
        s.dispatch("orphan_anchor", &json!({"id": "req:shop-2"}))
            .unwrap();
        s.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:place-anchors:shop.md", "justification": "Both proposals decided."}),
        )
        .unwrap();
        assert!(s.dispatch("done", &json!({"summary": "x"})).is_ok());
        assert_eq!(s.staged.len(), 2);
    }

    #[test]
    fn place_anchor_gates_quote_and_section_and_rejects_strangers() {
        let mut s = align_session();
        let err = s
            .dispatch(
                "place_anchor",
                &json!({"id": "ent:basket", "section": "/shop/basket", "reevaluate": false}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "unknown-anchor");
        let err = s
            .dispatch(
                "place_anchor",
                &json!({"id": "req:shop-2", "section": "/shop/nowhere", "reevaluate": false}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "unknown-section");
        let err = s
            .dispatch("place_anchor", &json!({"id": "req:shop-2", "section": "/shop/basket", "quote": "not in the text", "reevaluate": false}))
            .unwrap_err();
        assert_eq!(err.rule, "quote-not-found");
        // Without a quote the old one rides along; the reply says it will not locate.
        let v = s
            .dispatch(
                "place_anchor",
                &json!({"id": "req:shop-2", "section": "/shop/basket", "reevaluate": false}),
            )
            .unwrap();
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
                statement: "The Shopping Cart shall hold items a Customer intends to buy.".into(),
                entities: vec!["ent:shopping-cart".into()],
                edges: Vec::new(),
                source: Some(SourceRef {
                    doc: "shop.md".into(),
                    section: "/shop/cart".into(),
                    quote: "The Shopping Cart holds items a Customer intends to buy.".into(),
                }),
                ..Default::default()
            },
        );
        s.snapshot.status.reevaluate.push("req:shop-9".into());
        s.scope
            .goals
            .retain(|g| g.sections == vec!["/shop/cart".to_string()]);
        s.scope.goals[0].stale_anchors.push("req:shop-9".into());
        s.dispatch(
            "set_coverage",
            &json!({"section": "/shop/cart", "state": "covered"}),
        )
        .unwrap();
        let goal = "g:reconcile-section:shop.md#/shop/cart";
        let err = s
            .dispatch(
                "mark_goal_done",
                &json!({"goal": goal, "justification": "Covered."}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "stale-anchor");
        s.dispatch(
            "delete_requirement",
            &json!({"id": "req:shop-9", "reason": "meaning changed"}),
        )
        .unwrap();
        s.dispatch(
            "mark_goal_done",
            &json!({"goal": goal, "justification": "Re-judged and removed."}),
        )
        .unwrap();
        assert!(s.dispatch("done", &json!({"summary": "x"})).is_ok());
    }

    // The guard: the second identical call is marked, the third refused; a load of a
    // loaded target counts; done and the goal claims are exempt.
    // Mirrors docs/compiler/sessions.md#repeated-calls.
    #[test]
    fn repeated_calls_are_marked_then_refused_with_exemptions() {
        let mut t = session();
        let args = json!({"query": "cart"});
        assert!(t.dispatch("search", &args).unwrap().get("repeat").is_none());
        assert!(t.dispatch("search", &args).unwrap()["repeat"].is_string());
        let err = t.dispatch("search", &args).unwrap_err();
        assert_eq!(err.rule, "repeated-call");
        // A load of an already loaded target is a repeat whatever its depth.
        t.dispatch("load", &json!({"target": "ent:customer"}))
            .unwrap();
        let v = t
            .dispatch("load", &json!({"target": "ent:customer", "depth": 2}))
            .unwrap();
        assert!(v["repeat"].is_string());
        let err = t
            .dispatch("load", &json!({"target": "ent:customer"}))
            .unwrap_err();
        assert_eq!(err.rule, "repeated-call");
        // done, mark_goal_done, and mark_goal_failed are exempt: a repaired claim
        // legitimately repeats.
        for _ in 0..4 {
            let err = t.dispatch("done", &json!({"summary": "x"})).unwrap_err();
            assert_ne!(err.rule, "repeated-call");
        }
    }

    // Past the high-water mark, load and expand refuse naming candidates; reads
    // still answer. Mirrors docs/compiler/context.md#policy.
    #[test]
    fn load_past_the_high_water_mark_names_unload_candidates() {
        let mut t = session();
        t.dispatch("load", &json!({"target": "ent:customer"}))
            .unwrap();
        t.loaded.high_water = 1;
        let err = t
            .dispatch("load", &json!({"target": "shop.md#/shop/cart"}))
            .unwrap_err();
        assert_eq!(err.rule, "context-full");
        assert!(err.message.contains("ent:customer"), "{}", err.message);
        assert!(
            t.dispatch("search", &json!({"query": "buy"})).is_ok(),
            "reads still answer past the mark"
        );
    }

    // Auto-load once, the per-session cap, and the inactive marking when the last
    // node of a kind unloads. Mirrors docs/compiler/sessions.md#skills.
    #[test]
    fn skills_auto_load_once_cap_and_go_inactive() {
        let mut t = session();
        assert!(
            t.skills.is_active("extraction"),
            "the goal kind's skill is active from the first round"
        );
        let v = t
            .dispatch("load", &json!({"target": "shop.md#/shop/cart"}))
            .unwrap();
        assert!(v.get("skill").is_none(), "a pinned skill never re-renders");
        // A batch reads a view as reference: no structural-views skill rides along
        // with the load (docs/compiler/sessions.md#skills).
        t.snapshot.graph.views.insert(
            "view:class/public".into(),
            View {
                kind: "class".into(),
                title: "Public".into(),
                members: vec!["ent:cart".into()],
                default: true,
                ..Default::default()
            },
        );
        let v = t
            .dispatch("load", &json!({"target": "view:class/public"}))
            .unwrap();
        assert!(v.get("skill").is_none(), "a batch never auto-loads a skill");
        assert!(!t.skills.is_rendered("structural-views"));
        t.dispatch("load_skill", &json!({"name": "judgment"}))
            .unwrap();
        t.dispatch("load_skill", &json!({"name": "flow-views"}))
            .unwrap();
        t.dispatch("load_skill", &json!({"name": "structural-views"}))
            .unwrap();
        let err = t
            .dispatch("load_skill", &json!({"name": "abstraction"}))
            .unwrap_err();
        assert_eq!(err.rule, "skill-cap");
        let err = t
            .dispatch("load_skill", &json!({"name": "no-such"}))
            .unwrap_err();
        assert_eq!(err.rule, "unknown-skill");
        // Outside a batch nothing pins: the first section auto-loads extraction once,
        // and unloading the last section marks it inactive without dropping its slot.
        let snap = session().snapshot;
        let mut t2 = ToolSession::new(snap, WorkScope::serving("mcp-read"), 64, 24_000);
        let v = t2
            .dispatch("load", &json!({"target": "shop.md#/shop/cart"}))
            .unwrap();
        assert!(v["skill"]
            .as_str()
            .unwrap()
            .contains("[skill: extraction (active)]"));
        t2.dispatch("unload", &json!({"target": "shop.md#/shop/cart"}))
            .unwrap();
        assert!(!t2.skills.is_active("extraction"));
        assert!(
            t2.skills.is_rendered("extraction"),
            "the rendered text keeps its cap slot"
        );
    }

    // The claim tools: an essay justification, a false claim, and a goal outside the
    // batch are rejected; a failure is always accepted.
    #[test]
    fn mark_goal_done_rejects_an_essay_and_a_false_claim() {
        let mut t = session();
        let goal = "g:reconcile-section:shop.md#/shop/cart";
        let essay = "First I read the section. Then I extracted everything. Then I checked the anchors. Finally I marked coverage.";
        let err = t
            .dispatch(
                "mark_goal_done",
                &json!({"goal": goal, "justification": essay}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "bad-justification");
        // A false claim: the section carries no coverage mark.
        let err = t
            .dispatch(
                "mark_goal_done",
                &json!({"goal": goal, "justification": "Extracted and marked."}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "unmarked-section");
        let err = t
            .dispatch(
                "mark_goal_done",
                &json!({"goal": "g:reconcile-section:other.md#/x", "justification": "Done."}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "unknown-goal");
        t.dispatch(
            "mark_goal_failed",
            &json!({"goal": goal, "reason": "the section contradicts itself"}),
        )
        .unwrap();
    }

    // The fan-out variant faces its level's count at mark_goal_done even when the
    // changeset left the level alone: a done claim over an untouched level over soft is
    // refused; a move under an existing sibling that brings the count to soft passes.
    // Mirrors docs/compiler/goals/abstract-entity.md#the-fan-out-gate.
    #[test]
    fn mark_goal_done_on_a_fan_out_goal_faces_the_untouched_level() {
        use crate::limits::{threshold, CHILDREN_PER_ENTITY};
        let (soft, hard) = threshold(CHILDREN_PER_ENTITY, None).unwrap();
        let mut s = Store::default();
        s.graph
            .entities
            .insert("ent:backend".into(), plain("Backend"));
        let n = soft as usize + 1;
        for i in 0..n {
            s.graph.entities.insert(
                format!("ent:c{}", i),
                under(&format!("C{}", i), "ent:backend"),
            );
        }
        s.status.changes.push(
            ChangeRecord::new(
                1,
                0,
                0,
                crate::store::CHANGE_THRESHOLD_CROSSED,
                "ent:backend",
                "limits",
            )
            .with_detail(json!({
                "limit": CHILDREN_PER_ENTITY, "count": n, "soft": soft, "hard": hard,
                "level": "soft", "goal": "abstract-entity",
            })),
        );
        let goal = Goal {
            id: "g:abstract-entity:ent:backend".into(),
            kind: "abstract-entity".into(),
            mandatory: false,
            target: "ent:backend".into(),
            change: json!({"fan_out": n, "limit": {"soft": soft, "hard": hard}, "candidates": []}),
            ..Default::default()
        };
        let mut t = ToolSession::new(s, WorkScope::for_batch("b0-1", &[goal]), 64, 24_000);
        let err = t
            .dispatch(
                "mark_goal_done",
                &json!({"goal": "g:abstract-entity:ent:backend", "justification": "The level is fine."}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "fan-out-over-limit");
        assert!(err.message.contains("ent:backend"), "{}", err.message);
        t.dispatch(
            "update_entity",
            &json!({"id": "ent:c1", "parent": "ent:c0"}),
        )
        .unwrap();
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:abstract-entity:ent:backend", "justification": "C1 belongs under C0, which already contains it conceptually."}),
        )
        .unwrap();
    }

    // A level under `ent:backend`: four stated children, a derived grouping
    // `ent:messaging` holding two of them, a stated sibling area whose name a grouping
    // could look like, and a parentless entity on another level. The session runs a
    // fan-out goal on the backend.
    fn level_session() -> ToolSession {
        let mut s = Store::default();
        s.graph
            .entities
            .insert("ent:backend".into(), plain("Backend"));
        for i in 0..4 {
            s.graph.entities.insert(
                format!("ent:c{}", i),
                under(&format!("C{}", i), "ent:backend"),
            );
        }
        s.graph.entities.insert(
            "ent:messaging".into(),
            Entity {
                name: "Messaging".into(),
                definition: Some("moves events between the services".into()),
                parent: Some("ent:backend".into()),
                provenance: Some(Provenance::Derived {
                    from: vec!["ent:c2".into(), "ent:c3".into()],
                    reasoning: "the documents treat the queue and the bus as one area".into(),
                }),
                ..Default::default()
            },
        );
        for c in ["ent:c2", "ent:c3"] {
            s.graph.entities.get_mut(c).unwrap().parent = Some("ent:messaging".into());
        }
        s.graph.entities.insert(
            "ent:storage-layer".into(),
            under("Storage Layer", "ent:backend"),
        );
        s.graph.entities.insert("ent:other".into(), plain("Other"));
        let goal = Goal {
            id: "g:abstract-entity:ent:backend".into(),
            kind: "abstract-entity".into(),
            mandatory: false,
            target: "ent:backend".into(),
            change: json!({"fan_out": 5, "limit": {"soft": 9, "hard": 15}, "candidates": []}),
            ..Default::default()
        };
        ToolSession::new(s, WorkScope::for_batch("b0-1", &[goal]), 64, 24_000)
    }

    // group_entities stages one derived entity from the members and one parent move
    // per member, and refuses a lone member, a cross-level set, and a lookalike name.
    // Mirrors docs/compiler/tools.md#grouping-tools.
    #[test]
    fn group_entities_accepts_a_valid_grouping_and_rejects_bad_ones() {
        let mut t = level_session();
        let grouping = json!({
            "name": "Compute",
            "definition": "Runs the request handlers.",
            "members": ["ent:c0", "c1"],
            "stereotype": "component",
            "reasoning": "the compute page describes both as one tier",
        });
        let r = t.dispatch("group_entities", &grouping).unwrap();
        assert_eq!(r["id"], "ent:compute");
        assert_eq!(r["moved"], json!(["ent:c0", "ent:c1"]));
        assert_eq!(t.staged.len(), 3, "{:?}", t.staged);
        match &t.staged[0] {
            Op::CreateEntity { id, entity } => {
                assert_eq!(id, "ent:compute");
                assert_eq!(entity.parent.as_deref(), Some("ent:backend"));
                assert_eq!(entity.stereotype.as_deref(), Some("component"));
                assert_eq!(
                    entity.provenance,
                    Some(Provenance::Derived {
                        from: vec!["ent:c0".into(), "ent:c1".into()],
                        reasoning: "the compute page describes both as one tier".into(),
                    })
                );
            }
            other => panic!("expected a create, got {:?}", other),
        }
        for (op, member) in t.staged[1..].iter().zip(["ent:c0", "ent:c1"]) {
            match op {
                Op::UpdateEntity { id, parent, .. } => {
                    assert_eq!(id, member);
                    assert_eq!(parent.as_deref(), Some("ent:compute"));
                }
                other => panic!("expected a move, got {:?}", other),
            }
        }
        // The session's own gates see the moves: the grouping now holds the level.
        assert_eq!(t.level_after("ent:compute"), vec!["ent:c0", "ent:c1"]);
        assert_eq!(
            t.level_view_of("ent:compute").as_deref(),
            Some("view:component/compute")
        );
        // The abstract-entity gate accepts the grouping as staged.
        t.dispatch(
            "mark_goal_done",
            &json!({"goal": "g:abstract-entity:ent:backend", "justification": "Compute groups the two handlers the compute page describes."}),
        )
        .unwrap();

        let mut t = level_session();
        let err = t
            .dispatch(
                "group_entities",
                &json!({"name": "Solo", "definition": "One.", "members": ["ent:c0"], "reasoning": "why"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "too-few-members");
        assert!(err.message.contains("update_entity"), "{}", err.message);
        let err = t
            .dispatch(
                "group_entities",
                &json!({"name": "Mixed", "definition": "Two levels.", "members": ["ent:c0", "ent:other"], "reasoning": "why"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "cross-level");
        assert!(
            err.message.contains("ent:other") && err.message.contains("the scope root"),
            "{}",
            err.message
        );
        // A staged move counts as the current parent: c2 moved beside c0 groups with it.
        let err = t
            .dispatch(
                "group_entities",
                &json!({"name": "Mixed", "definition": "Two levels.", "members": ["ent:c0", "ent:c2"], "reasoning": "why"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "cross-level");
        t.dispatch(
            "update_entity",
            &json!({"id": "ent:c2", "parent": "ent:backend"}),
        )
        .unwrap();
        let before = t.staged.len();
        t.dispatch(
            "group_entities",
            &json!({"name": "Pair", "definition": "Two handlers.", "members": ["ent:c0", "ent:c2"], "reasoning": "why"}),
        )
        .unwrap();
        assert_eq!(t.staged.len(), before + 3);
        // A lookalike of an existing area reuses it; an exact name is that entity.
        let err = t
            .dispatch(
                "group_entities",
                &json!({"name": "Storage", "definition": "Keeps the data.", "members": ["ent:c1", "ent:messaging"], "reasoning": "why"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "near-duplicate");
        assert!(err.message.contains("ent:storage-layer"), "{}", err.message);
        // The lookalike sits beside the members at this level: a peer carrying the
        // area's word, never their parent. Mirrors docs/compiler/concepts/levels.md#naming.
        assert!(
            err.message.contains("a peer of the members at this level")
                && err.message.contains("never becomes the members' parent"),
            "{}",
            err.message
        );
        // A sibling that already holds children is the area: reuse it.
        let err = t
            .dispatch(
                "group_entities",
                &json!({"name": "Messaging Hub", "definition": "Moves messages.", "members": ["ent:c1", "ent:storage-layer"], "reasoning": "why"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "near-duplicate");
        assert!(
            err.message.contains("ent:messaging") && err.message.contains("already holds children"),
            "{}",
            err.message
        );
        let err = t
            .dispatch(
                "group_entities",
                &json!({"name": "Storage Layer", "definition": "Keeps the data.", "members": ["ent:c1", "ent:storage-layer"], "reasoning": "why"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "near-duplicate");
        assert!(err.message.contains("already exists"), "{}", err.message);
        // A lookalike at another level names that level's concept: qualify the name.
        t.dispatch(
            "update_entity",
            &json!({"id": "ent:storage-layer", "parent": "ent:messaging"}),
        )
        .unwrap();
        let err = t
            .dispatch(
                "group_entities",
                &json!({"name": "Storage", "definition": "Keeps the data.", "members": ["ent:c1", "ent:messaging"], "reasoning": "why"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "near-duplicate");
        assert!(
            err.message.contains("another level") && err.message.contains("qualify"),
            "{}",
            err.message
        );
        // definition and reasoning are non-empty; a member must resolve.
        for (args, rule) in [
            (
                json!({"name": "Bare", "definition": " ", "members": ["ent:c1", "ent:messaging"], "reasoning": "why"}),
                "definition-required",
            ),
            (
                json!({"name": "Bare", "definition": "Keeps the data.", "members": ["ent:c1", "ent:messaging"], "reasoning": ""}),
                "reasoning-required",
            ),
            (
                json!({"name": "Bare", "definition": "Keeps the data.", "members": ["ent:c1", "ent:nope"], "reasoning": "why"}),
                "unknown-id",
            ),
        ] {
            assert_eq!(t.dispatch("group_entities", &args).unwrap_err().rule, rule);
        }
    }

    // dissolve_entity refuses an entity the documents state and stages the dissolve
    // of a derived grouping, its children landing on the grouping's parent for the
    // session's own gates. Mirrors docs/compiler/tools.md#grouping-tools.
    #[test]
    fn dissolve_entity_refuses_a_stated_entity_and_dissolves_a_grouping() {
        let mut t = level_session();
        let err = t
            .dispatch(
                "dissolve_entity",
                &json!({"id": "ent:backend", "reason": "flatten"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "stated-entity");
        assert!(
            err.message.contains("Revise the documents"),
            "{}",
            err.message
        );
        // A derived entity holding requirements is a sub-entity, not a grouping.
        t.snapshot.graph.requirements.insert(
            "req:shop-1".into(),
            quoted_req(
                "Messaging retries once.",
                &["ent:messaging"],
                "retries once",
            ),
        );
        let err = t
            .dispatch(
                "dissolve_entity",
                &json!({"id": "ent:messaging", "reason": "flatten"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "not-a-grouping");
        assert!(err.message.contains("req:shop-1"), "{}", err.message);
        t.snapshot.graph.requirements.clear();
        let r = t
            .dispatch(
                "dissolve_entity",
                &json!({"id": "messaging", "reason": "the two belong beside the cache"}),
            )
            .unwrap();
        assert_eq!(r["dissolved"], "ent:messaging");
        assert_eq!(r["parent"], "ent:backend");
        assert_eq!(r["children"], json!(["ent:c2", "ent:c3"]));
        assert!(matches!(
            &t.staged[0],
            Op::DissolveEntity { id, reason, parent: None, children }
                if id == "ent:messaging" && reason == "the two belong beside the cache" && children.is_empty()
        ));
        // The session reads the children's moves: they sit under the backend now,
        // and the dissolved grouping is no level.
        assert_eq!(t.parent_of("ent:c2").as_deref(), Some("ent:backend"));
        assert_eq!(
            t.level_after("ent:backend"),
            vec!["ent:c0", "ent:c1", "ent:c2", "ent:c3", "ent:storage-layer"]
        );
        assert!(t.level_after("ent:messaging").is_empty());
        let err = t
            .dispatch(
                "dissolve_entity",
                &json!({"id": "ent:messaging", "reason": "again"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "already-dissolved");
        // A grouping staged this session is not in the graph yet.
        t.dispatch(
            "group_entities",
            &json!({"name": "Compute", "definition": "Runs the handlers.", "members": ["ent:c0", "ent:c1"], "reasoning": "why"}),
        )
        .unwrap();
        let err = t
            .dispatch(
                "dissolve_entity",
                &json!({"id": "ent:compute", "reason": "undo"}),
            )
            .unwrap_err();
        assert_eq!(err.rule, "staged-entity");
    }

    // A dissolve that lands a level past the fan-out threshold previews the goal it
    // opens. Mirrors docs/compiler/reconciler.md#bubbling.
    #[test]
    fn dissolve_previews_the_fan_out_goal_it_opens() {
        use crate::limits::{threshold, CHILDREN_PER_ENTITY};
        let (soft, _) = threshold(CHILDREN_PER_ENTITY, None).unwrap();
        let mut t = level_session();
        // The backend holds five after the dissolve; pad it to the soft threshold first.
        for i in 0..(soft as usize - 4) {
            t.snapshot.graph.entities.insert(
                format!("ent:pad{}", i),
                under(&format!("Pad{}", i), "ent:backend"),
            );
        }
        let r = t
            .dispatch(
                "dissolve_entity",
                &json!({"id": "ent:messaging", "reason": "flatten"}),
            )
            .unwrap();
        let opens = r["opens"].as_array().cloned().unwrap_or_default();
        assert!(
            opens.iter().any(|o| {
                o.as_str()
                    .is_some_and(|s| s.starts_with("abstract-entity ent:backend (fan-out"))
            }),
            "{:?}",
            opens
        );
    }

    // get_view lists, for every member with a level view, the member and that view:
    // one present in the snapshot, one the containment tree yields at commit, none
    // for a member without a level. Mirrors docs/compiler/tools.md#read-tools.
    #[test]
    fn get_view_lists_the_members_level_views_as_children() {
        let mut s = Store::default();
        s.graph.entities.insert(
            "ent:backend".into(),
            Entity {
                name: "Backend".into(),
                stereotype: Some("system".into()),
                ..Default::default()
            },
        );
        for (id, name) in [
            ("ent:server", "Server"),
            ("ent:db", "Database"),
            ("ent:cache", "Cache"),
        ] {
            s.graph
                .entities
                .insert(id.into(), under(name, "ent:backend"));
        }
        for (id, name) in [("ent:model", "Model"), ("ent:controller", "Controller")] {
            s.graph
                .entities
                .insert(id.into(), under(name, "ent:server"));
        }
        for (id, name) in [
            ("ent:orders-table", "Orders Table"),
            ("ent:users-table", "Users Table"),
        ] {
            s.graph.entities.insert(id.into(), under(name, "ent:db"));
        }
        let view = |title: &str, kind: &str, members: &[&str]| View {
            kind: kind.into(),
            title: title.into(),
            members: members.iter().map(|m| m.to_string()).collect(),
            default: true,
            ..Default::default()
        };
        s.graph.views.insert(
            "view:component/backend".into(),
            view(
                "Backend",
                "component",
                &["ent:server", "ent:db", "ent:cache"],
            ),
        );
        s.graph.views.insert(
            "view:class/server".into(),
            view("Server", "class", &["ent:model", "ent:controller"]),
        );
        let mut t = ToolSession::new(s, WorkScope::serving("mcp-compile"), 64, 24_000);
        let r = t
            .dispatch("get_view", &json!({"id": "view:component/backend"}))
            .unwrap();
        assert_eq!(
            r["children"],
            json!([
                {"member": "ent:server", "view": "view:class/server"},
                {"member": "ent:db", "view": "view:class/db"},
            ])
        );
        // A flow view links through its participants.
        t.snapshot.graph.requirements.insert(
            "req:shop-1".into(),
            quoted_req(
                "The server reads the model.",
                &["ent:server", "ent:model"],
                "reads",
            ),
        );
        t.snapshot.graph.views.insert(
            "view:usecase/backend-server-shop".into(),
            view("Server: Shop", "use-case", &["req:shop-1"]),
        );
        let r = t
            .dispatch(
                "get_view",
                &json!({"id": "view:usecase/backend-server-shop"}),
            )
            .unwrap();
        assert_eq!(
            r["children"],
            json!([{"member": "ent:server", "view": "view:class/server"}])
        );
    }
}
