use super::*;

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

fn goal_claim_parameters() -> Value {
    obj(
        json!({"goal": {"type": "string"}, "justification": {"type": "string"}, "evidence": {}}),
        &["goal", "justification"],
    )
}

pub(crate) fn done_parameters(begin_next: bool) -> Value {
    let mut properties = json!({
        "summary": {"type": "string"},
        "goals": {"type": "array", "minItems": 1, "items": goal_claim_parameters()}
    });
    if begin_next {
        properties["beginNext"] = json!({"type": "boolean"});
    }
    obj(properties, &["summary"])
}

pub fn catalog() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "load",
            description: "Load a target and its immediate neighborhood into the loaded set: any node id (ent:..., req:..., view:..., diag:...), a section reference (doc.md#/ref), or alignment:<document> for pending anchor proposals and section changes. depth defaults to 1: the target in full, its edges, each neighbor as a stub; depth 2 loads the neighbors in full too. Whatever the budget cut off arrives as expansion handles (h:<target>:<axis>). Loading an already loaded target is a repeat; deepen a loaded node with expand instead.",
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
            description: "Look up entities by name or alias (kind entity, the default) or views by title (kind view) before creating one. Pass query for one name, or queries for every name a section introduces in one call (one result per query, in order). Returns up to 8 possible matches per name; hits can be false neighbors (substrings, shared words), so inspect each before reusing it.",
            parameters: obj(
                json!({"query": {"type": "string"}, "queries": {"type": "array", "items": {"type": "string"}, "minItems": 1}, "kind": {"type": "string", "enum": ["entity", "view"]}}),
                &[],
            ),
        },
        ToolDef {
            name: "read_section",
            description: "Read one section's raw body and its child section titles. ref is a full section reference (doc.md#/ref). The body is capped at 400 lines and 16 KiB from line (default 1); a cut body carries truncated, totalLines, and nextLine.",
            parameters: obj(json!({"ref": {"type": "string"}, "line": {"type": "integer", "minimum": 1}}), &["ref"]),
        },
        ToolDef {
            name: "get_entity",
            description: "One entity (ent:...) with its definition, mentions, requirements, and relationships. For requirements (req:...) and other node kinds use load with target instead.",
            parameters: obj(json!({"id": {"type": "string"}}), &["id"]),
        },
        ToolDef {
            name: "get_view",
            description: "One view with its members in order, its exclusions, query, and collapse list, the relationships among its members, and children: the members that hold a level of their own, each with the id of its level view (the drill-down links the rendering carries).",
            parameters: obj(json!({"id": {"type": "string"}}), &["id"]),
        },
        ToolDef {
            name: "diagnostics",
            description: "List diagnostics: id, rule, severity, lifecycle, triage, subjects, message. Open ones by default; lifecycle takes open, resolved, or all; rule and subject narrow further. At most 50 entries from offset (default 0); count is the number that match, and a cut listing carries truncated and nextOffset.",
            parameters: obj(
                json!({"lifecycle": {"type": "string", "enum": ["open", "resolved", "all"]}, "rule": {"type": "string"}, "subject": {"type": "string"}, "offset": {"type": "integer", "minimum": 0}}),
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
            description: "Update an existing entity. A rename keeps the id. add_aliases records a wording, remove_aliases takes one back (a wording the documents turn out not to use). remove_parent: true takes the entity out of the containment tree (a generic rule-carrier that should stand beside no parent). attributes upsert by name; attributes not named stand. parent obeys the same gates as on create.",
            parameters: obj(
                json!({
                    "id": {"type": "string"},
                    "name": {"type": "string"},
                    "definition": {"type": "string"},
                    "add_aliases": {"type": "array", "items": {"type": "string"}},
                    "remove_aliases": {"type": "array", "items": {"type": "string"}},
                    "remove_parent": {"type": "boolean"},
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
            name: "upsert_criterion",
            description: "Record one acceptance criterion of a requirement: the observable evidence it is met, one condition and its outcome with concrete values, written from outside the system (never a function, a table, or a file). requirement is the owning requirement id. kind is positive, negative, boundary, or quality (with measure). Exactly one of section plus quote (a criterion supported by existing prose, including the requirement's own description; quote the original verbatim even when the criterion uses observer wording) or provenance.derived {from, reasoning} (a criterion the documents do not state; from names the requirement). Refused when the statement restates the requirement (criterion-restates). Safe to retry: the same statement on the same requirement updates in place.",
            parameters: obj(
                json!({
                    "requirement": {"type": "string"},
                    "statement": {"type": "string"},
                    "kind": {"type": "string", "enum": crate::model::CRITERION_KINDS},
                    "measure": {"type": "string"},
                    "given": {"type": "string"},
                    "when": {"type": "string"},
                    "then": {"type": "string"},
                    "entities": {"type": "array", "items": {"type": "string"}},
                    "section": {"type": "string"},
                    "quote": {"type": "string"},
                    "provenance": derived_schema()
                }),
                &["requirement", "statement", "kind"],
            ),
        },
        ToolDef {
            name: "update_criterion",
            description: "Update an existing criterion's statement, kind, measure, given, when, then, or entities; pass section and quote together to link a pending derived criterion to existing supporting prose without editing the document, clearing its ratification proposal; a field given replaces the stored one. The id and the owning requirement stay. In chat without an open compiler batch, commits immediately; no begin_goals is needed. Within a batch, remains staged until done.",
            parameters: obj(
                json!({
                    "id": {"type": "string"},
                    "section": {"type": "string"},
                    "quote": {"type": "string"},
                    "statement": {"type": "string"},
                    "kind": {"type": "string", "enum": crate::model::CRITERION_KINDS},
                    "measure": {"type": "string"},
                    "given": {"type": "string"},
                    "when": {"type": "string"},
                    "then": {"type": "string"},
                    "entities": {"type": "array", "items": {"type": "string"}}
                }),
                &["id"],
            ),
        },
        ToolDef {
            name: "delete_criterion",
            description: "Delete a criterion the documents no longer support.",
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
            description: "Leave one proposed anchor homeless: no candidate section states it any more. While its old section still exists it stays a stale anchor there for the extraction turn, which will delete it unless the document still states it; an anchor whose section is gone is deleted by the sweep at commit.",
            parameters: obj(json!({"id": {"type": "string"}}), &["id"]),
        },
        ToolDef {
            name: "report_diagnostic",
            description: "Record a judgment about the graph or documents. Severity error only when two statements cannot both hold; warning for real but repairable issues; info for observations; none for a considered judgment recorded but not surfaced. prompt is optional and usually absent: attach one only when the diagnostic asks a person a real question, omit it entirely otherwise. When present, it carries up to 4 options, each a label with exactly one of edit (a suggested prose edit, applied without a model) or answer (a prefilled reply), plus freeform for typed replies. A decision (a choice the documents leave open) requires a prompt; nonconformant-instance is an instance whose values or links its type's statements rule out.",
            parameters: obj(
                json!({
                    "rule": {"type": "string", "enum": REVIEW_RULES},
                    "severity": {"type": "string", "enum": SEVERITIES},
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
                    "state": {"type": "string", "enum": SETTABLE_COVERAGE},
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
            description: "Update a view. members replaces the whole ordered list; add_members and remove_members edit it; exclude adds one {id, note} pair or a list of them (each member leaves the list and stays out). Any field on a default view makes it curated from then on.",
            parameters: obj(
                json!({
                    "id": {"type": "string"},
                    "title": {"type": "string"},
                    "members": {"type": "array", "items": {"type": "string"}},
                    "add_members": {"type": "array", "items": {"type": "string"}},
                    "remove_members": {"type": "array", "items": {"type": "string"}},
                    "query": query_schema(),
                    "collapse": {"type": "array", "items": {"type": "string"}},
                    "exclude": {"anyOf": [exclusion_schema(), {"type": "array", "items": exclusion_schema()}]},
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
            description: "Requires id, field, value. Criterion IDs use update_criterion instead. Set one authored field on one requirement, entity, or view: a requirement's statement, edges, transition, or facets; an entity's definition, stereotype, parent, or an attribute's type or value (field attributes.<name>.type or attributes.<name>.value); a view's members. On a quote-provenanced fact, note carries the sentence rewrite the person accepted in conversation: the prose replacement and the graph mutation commit as one dual write. Without an accepted sentence, or on a derived or decreed fact, the edit lands graph-only with decree provenance (note becomes the decree's note) and a ratification proposal follows. value is the field's new content: a string, or the array or object the write tools take.",
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
            name: "report_spend",
            description: "Record what YOUR run billed for the open batch, when you know it: tokens is the input plus output tokens your endpoint billed across every round of this batch, cached the input tokens among them served from its prompt cache. Your own figure, never an estimate; do not call it if you do not know your usage. A second call replaces the first (the batch's whole spend so far, not an increment). At done the batch's journal entry takes the figure as tokens and cachedTokens with tokensSource agent. Stages nothing and touches no graph state. Outside an open batch it is refused; `jazyk spend <generation> <tokens>` stamps a committed entry instead.",
            parameters: obj(
                json!({
                    "tokens": {"type": "integer", "minimum": 1},
                    "cached": {"type": "integer", "minimum": 0}
                }),
                &["tokens"],
            ),
        },
        ToolDef {
            name: "report_feedback",
            description: "Report that jazyk's own prompt, tool, schema, or error message is ambiguous, wrong, confusing, or missing something. It appends one line to the project's own .jazyk/feedback.jsonl, read later by jazyk's developers, not this project's authors; nothing is transmitted anywhere. It never touches the graph. Use it for what blocked YOU; problems in the documents are diagnostics, not feedback. kind: ambiguous, wrong, confusing, missing, or other. subject names the tool, argument, or instruction. message says what was unclear and what would have helped. Then continue the task with your best judgment.",
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
            description: "Generation work currently derived for entities. Develop implementation and tests together; a separate Bind pass is optional. An empty list does not prove that an artifact exists or its checks passed. Follow next and prerequisites, then begin_generation on one ready entity.",
            parameters: obj(json!({}), &[]),
        },
        ToolDef {
            name: "begin_generation",
            description: "The full generation package for one entity: instructions (the generation contract), context pack, requirement groups (with suggested test names), change diff, the deliverable directory, factHash, the manifest of already generated files with what each holds, the medium, and the build. Write the deliverable files with your own tools, then record_generation with the manifest.",
            parameters: obj(json!({"entity": {"type": "string"}}), &["entity"]),
        },
        ToolDef {
            name: "record_medium",
            description: "Record what the deliverable is made of when no session has decided it yet (the package says medium: null with a mediumNote): form in your words (a Rust crate, a PowerPoint deck), produced written (the files are the deliverable) or built (they are the source a tool turns into artifact), toolchain, and artifact when built. An existing deliverable tree and recorded run commands pin the decision. Replaces an earlier decision until an entity is generated; refused after.",
            parameters: obj(
                json!({
                    "form": {"type": "string"},
                    "produced": {"type": "string", "enum": ["written", "built"]},
                    "toolchain": {"type": "string"},
                    "artifact": {"type": "string"}
                }),
                &["form", "produced"],
            ),
        },
        ToolDef {
            name: "record_generation",
            description: "Record the task done. manifest.files lists every deliverable-relative file written for this entity; manifest.supportFiles lists deliverable-wide files ownership never applies to (Cargo.toml, a crate root such as src/lib.rs); manifest.tests declares each required test: {requirement, criterion?, kind: programmatic|llm, label, artifact, name, run, cwd?, files?}; use criterion for a criterion test. Current test artifacts and selectors stay unchanged, while run/cwd may consolidate to a shared suite task and require fresh evidence. A programmatic artifact is the test source containing name, never the generated JUnit report; report paths belong in the saved task. For an llm row, artifact is the criteria file you wrote (front matter with the requirement id and statement hash, the statement, the quote, the implementing paths, the confirm steps, the verdict contract), path relative to the deliverable. Recording an identical manifest keeps existing verdicts. manifest.build is the one command that produces the deliverable's artifact when its medium must be built (a slide deck, a PDF, a rendered site, a binary): {run, cwd?, produces: [paths]}, run before any test. A task:<id> reference uses the saved task's project-relative cwd; legacy commands use the deliverable directory. produces paths remain deliverable-relative; omit it when the written files are themselves the deliverable, and reuse the build the begin_generation package already carries rather than recording a second one. Pass the factHash from the begin_generation package. choices lists what you had to invent, each {choice, scope: product|behavior|detail, reasoning, requirements?}; every entry lands as an invented-choice diagnostic graded by scope. Next: complete the related source changes for the other assigned entities, then run_tests at a useful execution milestone. Recording one entity does not require an immediate test run.",
            parameters: obj(
                json!({
                    "entity": {"type": "string"},
                    "factHash": {"type": "string"},
                    "manifest": {"type": "object", "properties": {
                        "files": {"type": "array", "items": {"type": "string"}},
                        "supportFiles": {"type": "array", "items": {"type": "string"}},
                        "tests": {"type": "array", "items": {"type": "object", "properties": {
                            "requirement": {"type": "string"},
                            "criterion": {"type": "string", "description": "Criterion ID from requirementGroups.criteria; omit only for a requirement-level test."},
                            "kind": {"type": "string", "enum": ["programmatic", "llm"]},
                            "label": {"type": "string"},
                            "artifact": {"type": "string", "description": "Deliverable-relative test source containing name, or an LLM criteria file. Never a generated JUnit report."},
                            "name": {"type": "string"},
                            "run": {"type": "string"},
                            "cwd": {"type": "string"},
                            "files": {"type": "array", "items": {"type": "string"}}
                        }, "required": ["requirement", "kind", "label", "artifact", "name", "run"]}},
                        "build": {"type": "object", "properties": {
                            "run": {"type": "string"},
                            "cwd": {"type": "string"},
                            "produces": {"type": "array", "items": {"type": "string"}}
                        }},
                        "tasks": {"type": "array", "items": {"type": "string"}, "description": "Saved task ids this generation uses beyond its tests and build, such as an asset copy. An unlisted task a session added is removed once nothing uses it."}
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
            description: "Record the binding: files lists the deliverable-relative implementing files (an empty list is the honest record of an unimplemented requirement; a file that only restates the statement is not one, list the files a criterion test inspects); tests is one entry per criterion of the requirement, {criterion, kind: programmatic|llm, label, artifact, name, run, cwd?, verdict: pass|fail, evidence?} (the package lists the criteria with suggested test names; a requirement with criteria is refused without them); test is the requirement-level {kind, label, artifact, name, run, cwd?} for a requirement without criteria (with criteria it may be omitted, the first criterion's test stands in); verdict is the outcome (pass|fail) with evidence. A programmatic artifact is the test source containing name, never the generated JUnit report. The row's derived status classifies the requirement: verified (the deliverable already satisfies it), unimplemented (fail with no files: generation work, the tests are its acceptance gate), partial (files exist and the criterion tests split, some passing and some failing: generation work for the failing criteria), failing (every test fails with files present: the deliverable contradicts the statement, a finding the failing-binding check files for the author). Never rewrite implementation files during binding.",
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
                    "tests": {"type": "array", "items": {"type": "object", "properties": {
                        "criterion": {"type": "string"},
                        "kind": {"type": "string", "enum": ["programmatic", "llm"]},
                        "label": {"type": "string"},
                        "artifact": {"type": "string"},
                        "name": {"type": "string"},
                        "run": {"type": "string"},
                        "cwd": {"type": "string"},
                        "verdict": {"type": "string", "enum": ["pass", "fail"]},
                        "evidence": {"type": "string"}
                    }, "required": ["criterion", "kind", "artifact", "name", "verdict"]}},
                    "verdict": {"type": "string", "enum": ["pass", "fail"]},
                    "evidence": {"type": "string"}
                }),
                &["requirement", "files"],
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
            description: "Run recorded programmatic tests: the build once, then each referenced saved task once for the selected rows, importing separate exact JUnit case verdicts. Reuse ordinary project suite tasks across requirements and criteria. Each later call starts fresh task runs. requirements selects rows (requirement or entity ids); empty runs every non-verified row; all: true reruns every programmatic row, verified ones included. llm rows are skipped here; judge them and record_verdict.",
            parameters: obj(json!({"requirements": {"type": "array", "items": {"type": "string"}}, "all": {"type": "boolean"}}), &[]),
        },
        ToolDef {
            name: "record_verdict",
            description: "Record LLM judgments with evidence. Copy factHash from begin_verification. For multiple LLM criteria, criteria must include every LLM criterionTests entry with its own criterion id, factHash, verdict, and evidence. The top-level verdict is pass only when every LLM entry passes. Programmatic evidence comes from run_tests.",
            parameters: obj(
                json!({
                    "requirement": {"type": "string"},
                    "verdict": {"type": "string", "enum": ["pass", "fail"]},
                    "factHash": {"type": "string"},
                    "evidence": {"type": "string"},
                    "criteria": {"type":"array","items":{"type":"object","properties":{
                        "criterion":{"type":"string"},"verdict":{"type":"string","enum":["pass","fail"]},
                        "factHash":{"type":"string"},"evidence":{"type":"string"}
                    },"required":["criterion","verdict","factHash"],"additionalProperties":false}}
                }),
                &["requirement", "verdict"],
            ),
        },
        // The generation turn's file and command tools: in-process only, never served
        // over MCP (an external agent brings its own editor). Names and shapes track
        // the Agent Client Protocol's file-system and terminal methods.
        // Mirrors docs/compiler/sessions.md#generation-sessions.
        ToolDef {
            name: "read_text_file",
            description: "One eligible deliverable file's content, capped at 64 KiB including a long first line. Credentials and private compiler/Git state are excluded. path is relative to the deliverable directory; line (1-based) and limit select lines. hash is the whole file SHA-256, including unread content. truncatedLine identifies a line whose remainder was omitted.",
            parameters: obj(
                json!({"path": {"type": "string"}, "line": {"type": "integer"}, "limit": {"type": "integer"}}),
                &["path"],
            ),
        },
        ToolDef {
            name: "write_text_file",
            description: "Write one deliverable file atomically. For a small edit, pass old_text matching exactly once and expectedHash from the last read or write; content replaces that literal text. old_text requires expectedHash and cannot combine with append true. An optional expectedHash guards any write mode; a conflict changes nothing. With append true, content goes at the end. Replies include the whole-file hash. path is relative to the deliverable. Source and ownership gates apply to every mode; preserve unrelated content.",
            parameters: obj(json!({"path": {"type": "string"}, "content": {"type": "string"}, "append": {"type": "boolean"}, "old_text": {"type": "string"}, "expectedHash": {"type": "string"}}), &["path", "content"]),
        },
        ToolDef {
            name: "list_files",
            description: "The deliverable's file tree, paths relative to the deliverable directory. path narrows to a subdirectory.",
            parameters: obj(json!({"path": {"type": "string"}}), &[]),
        },
        ToolDef {
            name: "task_context",
            description: "Read existing reusable project tasks, descriptions, configuration revision, execution capabilities and recent runs. Inspect this and project runner configuration before defining a task. Criterion identities belong in JUnit cases, not one task per criterion. Pass next_cursor as before for older runs. task_ids limits full definitions to those tasks and indexes the rest by id, name and description; omit it before update_tasks.",
            parameters: obj(json!({"before":{"type":"string"},"task_ids":{"type":"array","items":{"type":"string"}}}), &[]),
        },
        ToolDef {
            name: "update_tasks",
            description: "Replace the flat task list using its expected revision. Preserves unrelated project settings.",
            parameters: obj(json!({"expected_revision": {"type": "string"}, "tasks": {"type": "array", "items": crate::task_runtime::task_schema()}}), &["expected_revision", "tasks"]),
        },
        ToolDef {
            name: "run_task",
            description: "Run one saved project task in a fresh isolated environment. Returns a durable run ID immediately; wait=true waits in the host and returns the terminal status, or the current status if interrupted (then use wait_run). No per-run args, inputs or environment overrides exist. Reuse a suite task for related checks; run_tests imports its independent JUnit case evidence. The command and fixed timeout cannot change mid-run.",
            parameters: obj(json!({"task_id": {"type": "string"}, "expected_revision": {"type": "string"}, "operation_key": {"type": "string"}, "size": {"type": "string", "enum": ["small", "medium", "large"]}, "timeout_seconds": {"type": "integer"}, "wait": {"type": "boolean"}}), &["task_id", "expected_revision", "operation_key"]),
        },
        ToolDef {
            name: "inspect_run",
            description: "Inspect lifecycle, command outcome, retained results and confirmed termination.",
            parameters: obj(json!({"run_id": {"type": "string"}}), &["run_id"]),
        },
        ToolDef {
            name: "wait_run",
            description: "Wait briefly for a run update, then return its durable status. Repeating does not extend its deadline.",
            parameters: obj(json!({"run_id": {"type": "string"}, "wait_ms": {"type": "integer"}}), &["run_id"]),
        },
        ToolDef {
            name: "cancel_run",
            description: "Cancel queued or executing work immediately. Stops children and the container. Idempotent; available after budget exhaustion. Cancellation cannot undo external writes.",
            parameters: obj(json!({"run_id": {"type": "string"}}), &["run_id"]),
        },
        ToolDef {
            name: "read_run_log",
            description: "Read a bounded run log slice, including after teardown. Offsets and limits count Unicode characters, not bytes or lines. Continue with next_offset; search match line numbers are not read offsets.",
            parameters: obj(json!({"run_id": {"type": "string"}, "offset": {"type": "integer", "description": "Zero-based Unicode character offset; default 0. Reuse next_offset from the preceding read."}, "limit": {"type": "integer", "description": "Maximum Unicode characters; default 16000, capped at 64000."}}), &["run_id"]),
        },
        ToolDef {
            name: "search_run_log",
            description: "Search retained run logs for literal failure text before reading more log context. Returns bounded matching lines with one-based line numbers, not read_run_log character offsets.",
            parameters: obj(json!({"run_id": {"type": "string"}, "query": {"type": "string"}, "offset": {"type": "integer", "description": "Zero-based number of log lines to skip; default 0."}, "limit": {"type": "integer", "description": "Maximum matching lines; default and cap 100."}}), &["run_id", "query"]),
        },
        ToolDef {
            name: "read_run_changes",
            description: "List retained file changes or read one changed path (version: changes, the default). version: input requires path and reads the frozen run input, including an unchanged file, explicitly labeled historical input. It does not read the current workspace. Missing contents and partial collection are explicit.",
            parameters: obj(json!({"run_id": {"type": "string"}, "path": {"type": "string"}, "offset": {"type": "integer"}, "version": {"type": "string", "enum": ["changes", "input"]}}), &["run_id"]),
        },
        ToolDef {
            name: "mark_goal_done",
            description: "Claim one goal of the batch resolved. justification is mandatory and concise: one or two sentences of why the gate holds, never an essay. evidence carries what the kind's gate reads, one shape per kind: rejudge-pair {verdict: duplicate|contradiction|consistent, carried_by?}; review-entity {definition: current|refreshed, lookalikes: [{id, verdict: merged|separate, reason}], unreferenced: [{id, verdict: referenced|word-match, reason}], diagnostics: [{id, verdict: stands|resolved|refiled}], parts: [{id, verdict: parented|edge-fixed|kept, reason}], lint: [{rule, verdict: clean|filed}]} (one entry per lookalike, unreferenced statement, open diagnostic, and composition part the goal's hints list, and one lint entry per numbered rule on the ## Project lint line; omit the lists when the hints and the project list none, and a diagnostic staged in this batch is its filing goal's evidence, not an entry here, while one committed by an earlier batch is listed and takes an entry); declare-edges \"declared\" or \"not-structural\"; dedupe-candidates \"merged\" or \"separate\"; curate-view [{id, verdict: added|excluded}] per matched node; conform-instance {attributes: [{name, verdict: conforms|missing|nonconformant}], links: [{to, verdict, carried_by?}]} (one per declared attribute, inherited included, and per link); derive-criteria {requirements: [{id, criteria: [ids], verdict: complete|stated-only|open-question}]} (an object, one entry per requirement the goal listed); other kinds read the staged work alone. The claim is validated against the goal kind's gate over the store plus the staged work: a listed item without a verdict is refused as *-unjudged, a verdict the staged work does not carry as *-unbacked, each naming the repair.",
            parameters: goal_claim_parameters(),
        },
        ToolDef {
            name: "settle_pairs",
            description: "Settle several rejudge-pair goals of the batch in one call: one entry per goal with its verdict and a one or two sentence justification. consistent stages nothing and marks the goal done; duplicate files a duplicate-requirement diagnostic on the pair (severity info, message optional) and marks; contradiction files a contradiction diagnostic with the entry's severity, message (both required), reasoning, and prompt, and marks, unless an open contradiction already names both sides (filed by an earlier batch, or by this session before the pair opened): then that one carries the verdict, nothing is filed, and the entry needs only the goal, the verdict, and the justification; an entry may name carried_by, an open contradiction on one side that states the same conflict, and that one carries it; resolved resolves the pair's open contradiction or duplicate-requirement diagnostic with the justification as the reason and marks. Each entry runs the gates of the single calls it stands for; a refused entry is reported with the rule and the repair, leaves nothing staged, and stops none of the others. Goals outside the batch or of another kind are refused. The same-document delete stays on delete_requirement plus mark_goal_done.",
            parameters: obj(
                json!({"verdicts": {"type": "array", "minItems": 1, "items": {
                    "type": "object",
                    "properties": {
                        "goal": {"type": "string"},
                        "verdict": {"type": "string", "enum": PAIR_VERDICTS},
                        "justification": {"type": "string"},
                        "severity": {"type": "string", "enum": SEVERITIES},
                        "message": {"type": "string"},
                        "reasoning": {"type": "string"},
                        "prompt": prompt_schema()
                    },
                    "required": ["goal", "verdict", "justification"],
                    "additionalProperties": false
                }}}),
                &["verdicts"],
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
            description: "Bring one skill into the session by its name from the skills index. The reply renders the payload; at most eight skills render in one session.",
            parameters: obj(json!({"name": {"type": "string"}}), &["name"]),
        },
        ToolDef {
            name: "done",
            description: "Resolve final goals and commit the staged mutations in one call. goals entries use {goal, justification, evidence?}, with the same evidence and gates as mark_goal_done. Earlier claims may already be marked. Every mandatory goal must end done or failed; all batch gates run. A rejected claim or gate leaves earlier work intact for repair. summary is one line.",
            parameters: done_parameters(false),
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

// The calls without side effects whose answer the repeated-call guard recomputes before
// it refuses a third identical call. Mirrors docs/compiler/sessions.md#repeated-calls.
pub(super) const REANSWERABLE_TOOLS: [&str; 10] = [
    "graph_status",
    "search",
    "read_section",
    "get_entity",
    "get_view",
    "diagnostics",
    "read_text_file",
    "list_files",
    "generation_tasks",
    "binding_tasks",
];

// The graph write tools, each all or nothing at staging.
// Mirrors docs/compiler/tools.md#write-tools.
pub const GRAPH_WRITE_TOOLS: [&str; 21] = [
    "upsert_entity",
    "update_entity",
    "delete_entity",
    "merge_entities",
    "group_entities",
    "dissolve_entity",
    "upsert_requirement",
    "update_requirement",
    "delete_requirement",
    "upsert_criterion",
    "update_criterion",
    "delete_criterion",
    "set_coverage",
    "report_diagnostic",
    "update_diagnostic",
    "resolve_diagnostic",
    "place_anchor",
    "orphan_anchor",
    "upsert_view",
    "update_view",
    "delete_view",
];

// The files one list_files reply names at most.
// Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
pub(super) const LIST_FILES_LIMIT: usize = 500;

// The caps of one read_section body and one diagnostics page.
// Mirrors docs/compiler/tools.md#read-tools.
pub(super) const READ_SECTION_LINES: usize = 400;

pub(super) const READ_SECTION_BYTES: usize = 16 * 1024;

pub(super) const DIAGNOSTICS_PAGE: usize = 50;

// The goal tools every session sees. Mirrors docs/compiler/tools.md#goal-tools.
pub const GOAL_TOOLS: [&str; 4] = ["mark_goal_done", "mark_goal_failed", "load_skill", "done"];

// The batch claim of a rejudge-pair session: a goal tool served in that kind's slice
// only. Mirrors docs/compiler/tools.md#goal-tools.
pub const PAIR_SETTLE_TOOL: &str = "settle_pairs";

// The verdicts one settle_pairs entry may carry. Mirrors docs/compiler/goals/rejudge-pair.md#gate.
pub const PAIR_VERDICTS: [&str; 4] = ["consistent", "duplicate", "contradiction", "resolved"];

pub const GEN_TOOLS: [&str; 4] = [
    "generation_tasks",
    "begin_generation",
    "record_generation",
    "record_medium",
];

pub const BIND_TOOLS: [&str; 3] = ["binding_tasks", "begin_binding", "record_binding"];

pub const VERIFY_TOOLS: [&str; 4] = [
    "verification_tasks",
    "begin_verification",
    "run_tests",
    "record_verdict",
];

// In-process only: a generation turn's file tools, never served over MCP.
pub const FILE_TOOLS: [&str; 3] = ["read_text_file", "write_text_file", "list_files"];

// The view tools, served beside the write tools. See docs/compiler/tools.md#view-tools.
pub const VIEW_TOOLS: [&str; 3] = ["upsert_view", "update_view", "delete_view"];

// The chat serving's human paths, in no session's toolset and never a raw write.
// See docs/compiler/tools.md#chat-tools.
pub const CHAT_TOOLS: [&str; 1] = ["edit_fact"];

// The judged rules a session may file. Mirrors docs/compiler/model/diagnostic.md#rules-catalog.
pub const REVIEW_RULES: [&str; 9] = [
    "contradiction",
    "duplicate-entity",
    "duplicate-requirement",
    "missing-link",
    "ambiguity",
    "lint",
    "decision",
    "nonconformant-instance",
    // A derive-criteria session's value the documents do not give
    // (docs/compiler/goals/derive-criteria.md#gate).
    "invented-choice",
];

// Feedback about jazyk itself, not about the graph: served in every toolset, read-only
// MCP included. See docs/compiler/tools.md#feedback-tool.
pub const FEEDBACK_TOOL: &str = "report_feedback";

// Feedback entries one session may record. Past it the call is acknowledged without a
// record, so a confused model cannot flood the log.
pub const FEEDBACK_LIMIT: usize = 5;

// An external agent's own report of what its run billed, recorded on the open batch's
// journal entry: served in every toolset, read-only MCP included.
// See docs/compiler/tools.md#spend-tool.
pub const SPEND_TOOL: &str = "report_spend";

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
    if kinds
        .iter()
        .any(|k| matches!(*k, "bind" | "generate" | "verify"))
    {
        v.extend(crate::task_runtime::TOOLS);
    }
    v.push(FEEDBACK_TOOL);
    v.push(SPEND_TOOL);
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
        // The in-process ledger workers get the file tools.
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
            v.extend(GRAPH_WRITE_TOOLS);
            v.push(PAIR_SETTLE_TOOL);
            v.extend(GOAL_TOOLS);
            v
        }
        // `jazyk mcp graph --write`: the read tools, the raw write tools, and the
        // view tools, plus the task runtime tools a writable serving carries.
        // Mirrors docs/compiler/tools.md#toolsets and docs/compiler/tools.md#task-runtime-tools.
        "mcp-write" => {
            let mut v = READ_TOOLS.to_vec();
            v.extend(GRAPH_WRITE_TOOLS);
            v.extend(crate::task_runtime::TOOLS);
            v
        }
        _ => READ_TOOLS.to_vec(),
    };
    if matches!(task, "mcp-generate" | "mcp-verify") {
        v.extend(crate::task_runtime::TOOLS);
    }
    if task == "mcp-read" {
        v.extend(
            crate::task_runtime::TOOLS
                .into_iter()
                .filter(|t| !matches!(*t, "update_tasks" | "run_task" | "cancel_run")),
        );
    }
    if !v.contains(&FEEDBACK_TOOL) {
        v.push(FEEDBACK_TOOL);
    }
    if !v.contains(&SPEND_TOOL) {
        v.push(SPEND_TOOL);
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
            "cardinality": {"type": "string", "pattern": "^(\\*|[0-9]+|[0-9]+\\.\\.([0-9]+|\\*))$", "description": "the multiplicity at b: how many b per a (Order o-- 1..* Product is one or more Products per Order); *, a count (1, 4), or a range (0..1, 1..*, 2..3)"}
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
                    "old_text": {"type": "string"}, "new_text": {"type": "string"},
                    "facts": {"type": "array", "description": "the fact changes the rewrite implies for the requirements quoted from this section (e.g. a transition's new `to`), applied with the prose",
                        "items": {"type": "object", "properties": {
                            "id": {"type": "string", "description": "a requirement quoted from the edit's section"},
                            "field": {"type": "string", "enum": ["statement", "entities", "edges", "transition", "facets"]},
                            "value": {"description": "the field's whole new value, in the shape update_requirement takes"}},
                            "required": ["id", "field", "value"]}}},
                    "required": ["doc", "section", "old_text", "new_text"]},
                "answer": {"type": "string"}
            }, "required": ["label"]}},
            "freeform": {"type": "boolean"}
        },
        "required": ["question"]
    })
}
