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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/load-a-target-and-its-immediate-neighborhood-dac60.md")),
            parameters: obj(
                json!({"target": {"type": "string"}, "depth": {"type": "integer", "minimum": 1}}),
                &["target"],
            ),
        },
        ToolDef {
            name: "expand",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/load-the-frontier-behind-an-expansion-handle-ff841.md")),
            parameters: obj(json!({"handle": {"type": "string"}}), &["handle"]),
        },
        ToolDef {
            name: "unload",
            description: "Drop an item from the loaded set. Its handles close and its budget frees for the rest of the session.",
            parameters: obj(json!({"target": {"type": "string"}}), &["target"]),
        },
        ToolDef {
            name: "graph_status",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/re-render-the-loaded-set-s-status-896bf.md")),
            parameters: obj(json!({}), &[]),
        },
        ToolDef {
            name: "search",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/look-up-entities-by-name-or-alias-1b67f.md")),
            parameters: obj(
                json!({"query": {"type": "string"}, "queries": {"type": "array", "items": {"type": "string"}, "minItems": 1}, "kind": {"type": "string", "enum": ["entity", "view"]}}),
                &[],
            ),
        },
        ToolDef {
            name: "read_section",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/read-one-section-s-raw-body-and-405e0.md")),
            parameters: obj(json!({"ref": {"type": "string"}, "line": {"type": "integer", "minimum": 1}}), &["ref"]),
        },
        ToolDef {
            name: "get_entity",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/one-entity-ent-with-its-definition-mentions-4fbad.md")),
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/create-a-domain-concept-or-update-it-8b7e3.md")),
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/update-an-existing-entity-a-rename-keeps-71734.md")),
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
            name: "retract_entity_mention",
            description: "Remove one exact independent entity mention from the graph without editing prose. Preserve other mentions and facts. A live requirement's derived mention must be repaired through that requirement instead. Can share edit_graph with asset classification. Normal orphan cleanup still applies.",
            parameters: obj(json!({"id":{"type":"string"},"source":{"type":"object","properties":{"doc":{"type":"string"},"section":{"type":"string"},"quote":{"type":"string"}},"required":["doc","section","quote"],"additionalProperties":false},"reason":{"type":"string","minLength":1}}), &["id","source","reason"]),
        },
        ToolDef {
            name: "delete_entity",
            description: "Delete an entity that no requirement references.",
            parameters: obj(json!({"id": {"type": "string"}, "reason": {"type": "string"}}), &["id", "reason"]),
        },
        ToolDef {
            name: "merge_entities",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/merge-two-entities-that-are-the-same-3a53c.md")),
            parameters: obj(json!({"keep": {"type": "string"}, "absorb": {"type": "string"}, "reason": {"type": "string"}}), &["keep", "absorb", "reason"]),
        },
        ToolDef {
            name: "group_entities",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/build-one-level-stage-a-derived-grouping-71383.md")),
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/record-one-requirement-a-free-form-statement-65a84.md")),
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/update-an-existing-requirement-s-statement-entities-193fc.md")),
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/record-one-acceptance-criterion-of-a-requirement-9c0cc.md")),
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
            description: "Update an existing criterion's statement, kind, measure, given, when, then, or entities; pass section and quote together to link a pending derived criterion to existing supporting prose without editing the document, clearing its ratification proposal; a field given replaces the stored one. The id and the owning requirement stay. In chat without an open compiler batch, commits immediately without opening a compile batch. Within a batch, remains staged until done.",
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/move-one-proposed-anchor-a-requirement-or-65ff4.md")),
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/record-a-judgment-about-the-graph-or-e2ebc.md")),
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/replace-the-question-attached-to-an-open-b8e99.md")),
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
            name: "inspect_input",
            description: "Inspect a selected input without invoking its parser. Returns exact byte SHA-256, size, persisted processing decision, stored sectionRefs, and quoteReferences identifying facts to repair before asset classification. Entity mentions list their supplying requirement IDs. Use before classifying an input as an asset or a document, including for PDF inputs whose parsing fails.",
            parameters: obj(json!({"path":{"type":"string"}}), &["path"]),
        },
        ToolDef {
            name: "classify_input",
            description: "Classify a selected input as asset (one whole-file blob, no parsing) or document (normal format handler). In chat, commits immediately without an open batch or compile release; within an open batch, stages under its scope and gates. No user configuration is needed. Supply the exact original-byte SHA-256 and a specific reason. Existing quotes must be relocated or retracted first; inspect_input lists them in quoteReferences. Use edit_graph to combine delete_requirement with classification without editing prose. Requirement-derived mentions are pruned automatically. Byte and referring-prose changes reopen asset reconciliation; the mode persists.",
            parameters: obj(json!({"path":{"type":"string"},"mode":{"type":"string","enum":["asset","document"]},"sha256":{"type":"string"},"reason":{"type":"string"}}), &["path","mode","sha256","reason"]),
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/create-a-view-the-stored-half-of-b30f5.md")),
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
            name: "edit_graph",
            description: "Commit related authored graph mutations as one atomic chat transaction without build goals or a compile release. Supply ordered edits {tool, arguments} using the graph write tools or graph-only edit_fact. All edits validate before one commit; failures save none. This transaction is independent of an open compiler batch and makes its snapshot stale. Use derived provenance for model judgment and decision only for an explicit human choice. No lifecycle, file, task, nested transaction, or prose dual-write calls. Follow-up reconciliation stays pending.",
            parameters: obj(json!({"edits":{"type":"array","minItems":1,"items":{"type":"object","properties":{"tool":{"type":"string"},"arguments":{"type":"object"}},"required":["tool","arguments"],"additionalProperties":false}}}), &["edits"]),
        },
        ToolDef {
            name: "edit_fact",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/requires-id-field-value-criterion-ids-use-7b163.md")),
            parameters: obj(
                json!({
                    "id": {"type": "string"},
                    "field": {"type": "string"},
                    "value": {},
                    "note": {"type": "string"},
                    "provenance": derived_schema(),
                    "decision": {"type": "string", "description": "The explicit human decision being recorded. Never use for model inference."}
                }),
                &["id", "field", "value"],
            ),
        },
        ToolDef {
            name: "report_spend",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/record-what-your-run-billed-for-the-8a891.md")),
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/report-that-jazyk-s-own-prompt-tool-78bcb.md")),
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/generation-work-currently-derived-for-entities-develop-2a8e4.md")),
            parameters: obj(json!({"entity":{"type":"string"}}), &[]),
        },
        ToolDef {
            name: "begin_generation",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/the-full-generation-package-for-one-entity-bf3c8.md")),
            parameters: obj(json!({"entity": {"type": "string"}}), &["entity"]),
        },
        ToolDef {
            name: "record_medium",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/record-what-the-deliverable-is-made-of-034a1.md")),
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/record-the-task-done-manifest-files-lists-5b250.md")),
            parameters: obj(
                json!({
                    "entity": {"type": "string"},
                    "factHash": {"type": "string"},
                    "manifest": {"type": "object", "properties": {
                        "files": {"type": "array", "items": {"type": "string"}},
                        "supportFiles": {"type": "array", "items": {"type": "string"}},
                        "tests": {"type": "array", "items": {"type": "object", "properties": {
                            "requirement": {"type": "string"},
                            "criterion": {"type": "string", "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/criterion-id-from-requirementgroups-criteria-omit-only-29285.md"))},
                            "kind": {"type": "string", "enum": ["programmatic", "llm"]},
                            "label": {"type": "string"},
                            "artifact": {"type": "string", "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/deliverable-relative-test-source-containing-name-or-0744f.md"))},
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
                        "tasks": {"type": "array", "items": {"type": "string"}, "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/saved-task-ids-this-generation-uses-beyond-ead66.md"))}
                    }},
                    "choices": {"type": "array", "items": {"type": "object"}}
                }),
                &["entity", "factHash", "manifest"],
            ),
        },
        ToolDef {
            name: "binding_tasks",
            description: "Requirements owing a binding, with a reason (unbound, requirement-changed, artifact-gone). A binding ties one requirement to the deliverable: the implementing files (possibly none), the test that judges it, and the verdict. Deterministic; no model involved. Next: begin_binding on one requirement.",
            parameters: obj(json!({"entity":{"type":"string"}}), &[]),
        },
        ToolDef {
            name: "begin_binding",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/the-bind-package-for-one-requirement-instructions-1cb7e.md")),
            parameters: obj(json!({"requirement": {"type": "string"}}), &["requirement"]),
        },
        ToolDef {
            name: "record_binding",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/record-the-binding-files-lists-the-deliverable-587dd.md")),
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/the-verification-package-for-one-requirement-statement-35d15.md")),
            parameters: obj(json!({"requirement": {"type": "string"}}), &["requirement"]),
        },
        ToolDef {
            name: "run_tests",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/run-recorded-programmatic-tests-the-build-once-c44ea.md")),
            parameters: obj(json!({"requirements": {"type": "array", "items": {"type": "string"}}, "all": {"type": "boolean"}}), &[]),
        },
        ToolDef {
            name: "record_verdict",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/record-llm-judgments-with-evidence-copy-facthash-b07fb.md")),
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/one-eligible-deliverable-file-s-content-capped-a6c2c.md")),
            parameters: obj(
                json!({"path": {"type": "string"}, "line": {"type": "integer"}, "limit": {"type": "integer"}}),
                &["path"],
            ),
        },
        ToolDef {
            name: "link_asset",
            description: "Create a relative file symlink from selected Source path to selected asset target, both project-relative. Supply target sha256 from inspect_input. Refuses existing destinations, target links, unsafe paths and other entity owners. Record the destination in the generation file manifest.",
            parameters: obj(json!({"path":{"type":"string"},"target":{"type":"string"},"sha256":{"type":"string"}}), &["path","target","sha256"]),
        },
        ToolDef {
            name: "inspect_asset_link",
            description: "Inspect a project-relative asset symlink: relative target, resolved input and exact current byte hash. Broken or unsafe links return an explicit error.",
            parameters: obj(json!({"path":{"type":"string"}}), &["path"]),
        },
        ToolDef {
            name: "remove_asset_link",
            description: "Remove a Source asset symlink without deleting its target. expectedTarget must equal the stored relative target from inspection. Source and ownership gates apply.",
            parameters: obj(json!({"path":{"type":"string"},"expectedTarget":{"type":"string"}}), &["path","expectedTarget"]),
        },
        ToolDef {
            name: "write_text_file",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/write-one-deliverable-file-atomically-for-a-d2316.md")),
            parameters: obj(json!({"path": {"type": "string"}, "content": {"type": "string"}, "append": {"type": "boolean"}, "old_text": {"type": "string"}, "replaceAll": {"type": "boolean"}, "expectedHash": {"type": "string"}}), &["path", "content"]),
        },
        ToolDef {
            name: "list_files",
            description: "The deliverable's file tree, paths relative to the deliverable directory. path narrows to a subdirectory; pattern is a glob such as **/*.rs (without / it matches file names at any depth). At most limit files (default and maximum 500); a cut reply carries nextOffset, which offset takes with the same query to continue.",
            parameters: obj(json!({"path": {"type": "string"}, "pattern": {"type": "string"}, "offset": {"type": "integer", "minimum": 0}, "limit": {"type": "integer", "minimum": 1, "maximum": 500}}), &[]),
        },
        ToolDef {
            name: "search_files",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/search-eligible-file-contents-9a4d1.md")),
            parameters: obj(
                json!({
                    "pattern": {"type": "string"},
                    "path": {"type": "string"},
                    "glob": {"type": "string"},
                    "literal": {"type": "boolean"},
                    "ignoreCase": {"type": "boolean"},
                    "context": {"type": "integer", "minimum": 0, "maximum": 5},
                    "filesOnly": {"type": "boolean"},
                    "cursor": {"type": "object", "properties": {"path": {"type": "string"}, "line": {"type": "integer", "minimum": 1}}, "required": ["path"], "additionalProperties": false}
                }),
                &["pattern"],
            ),
        },
        ToolDef {
            name: "task_context",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/read-existing-reusable-project-tasks-descriptions-configuration-f39b5.md")),
            parameters: obj(json!({"before":{"type":"string"},"task_ids":{"type":"array","items":{"type":"string"}}}), &[]),
        },
        ToolDef {
            name: "update_tasks",
            description: "Replace the flat task list using its expected revision. Preserves unrelated project settings.",
            parameters: obj(json!({"expected_revision": {"type": "string"}, "tasks": {"type": "array", "items": crate::task_runtime::task_schema()}}), &["expected_revision", "tasks"]),
        },
        ToolDef {
            name: "run_task",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/run-one-saved-project-task-in-a-f3e1d.md")),
            parameters: obj(json!({"task_id": {"type": "string"}, "expected_revision": {"type": "string"}, "operation_key": {"type": "string"}, "size": {"type": "string", "enum": ["small", "medium", "large"]}, "timeout_seconds": {"type": "integer"}, "wait": {"type": "boolean"}}), &["task_id", "expected_revision", "operation_key"]),
        },
        ToolDef {
            name: "inspect_run",
            description: "Inspect lifecycle, command outcome, retained results and confirmed termination.",
            parameters: obj(json!({"run_id": {"type": "string"}}), &["run_id"]),
        },
        ToolDef {
            name: "wait_run",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/wait-briefly-for-a-run-update-then-1eddd.md")),
            parameters: obj(json!({"run_id": {"type": "string"}, "wait_ms": {"type": "integer"}}), &["run_id"]),
        },
        ToolDef {
            name: "cancel_run",
            description: "Cancel queued or executing work immediately. Stops children and the container. Idempotent; available after budget exhaustion. Cancellation cannot undo external writes.",
            parameters: obj(json!({"run_id": {"type": "string"}}), &["run_id"]),
        },
        ToolDef {
            name: "read_run_log",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/read-a-bounded-run-log-slice-including-52c66.md")),
            parameters: obj(json!({"run_id": {"type": "string"}, "offset": {"type": "integer", "description": include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/zero-based-unicode-character-offset-default-reuse-cbbd6.md"))}, "limit": {"type": "integer", "description": "Maximum Unicode characters; default 16000, capped at 64000."}}), &["run_id"]),
        },
        ToolDef {
            name: "search_run_log",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/search-retained-run-logs-for-literal-failure-a7589.md")),
            parameters: obj(json!({"run_id": {"type": "string"}, "query": {"type": "string"}, "offset": {"type": "integer", "description": "Zero-based number of log lines to skip; default 0."}, "limit": {"type": "integer", "description": "Maximum matching lines; default and cap 100."}}), &["run_id", "query"]),
        },
        ToolDef {
            name: "read_run_changes",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/list-retained-file-changes-or-read-one-11af1.md")),
            parameters: obj(json!({"run_id": {"type": "string"}, "path": {"type": "string"}, "offset": {"type": "integer"}, "version": {"type": "string", "enum": ["changes", "input"]}}), &["run_id"]),
        },
        ToolDef {
            name: "mark_goal_done",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/claim-one-goal-of-the-batch-resolved-617f9.md")),
            parameters: goal_claim_parameters(),
        },
        ToolDef {
            name: "settle_pairs",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/settle-several-rejudge-pair-goals-of-the-3cb89.md")),
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
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/record-that-one-goal-of-the-batch-a5ff6.md")),
            parameters: obj(
                json!({"goal": {"type": "string"}, "reason": {"type": "string"}}),
                &["goal", "reason"],
            ),
        },
        ToolDef {
            name: "load_skill",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/bring-one-skill-into-the-session-by-14eb3.md")),
            parameters: obj(json!({"name": {"type": "string"}}), &["name"]),
        },
        ToolDef {
            name: "done",
            description: include_str!(concat!(env!("OUT_DIR"), "/payloads/goals/prompts/inline/tools-catalog/resolve-final-goals-and-commit-the-staged-3db5b.md")),
            parameters: done_parameters(false),
        },
    ]
}

pub const READ_TOOLS: [&str; 10] = [
    "load",
    "expand",
    "unload",
    "graph_status",
    "search",
    "read_section",
    "get_entity",
    "get_view",
    "diagnostics",
    "inspect_input",
];

// The calls without side effects whose answer the repeated-call guard recomputes before
// it refuses a third identical call. Mirrors docs/compiler/sessions.md#repeated-calls.
pub(super) const REANSWERABLE_TOOLS: [&str; 11] = [
    "graph_status",
    "search",
    "read_section",
    "get_entity",
    "get_view",
    "diagnostics",
    "read_text_file",
    "list_files",
    "search_files",
    "generation_tasks",
    "binding_tasks",
];

// The graph write tools, each all or nothing at staging.
// Mirrors docs/compiler/tools.md#write-tools.
pub const GRAPH_WRITE_TOOLS: [&str; 23] = [
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
    "classify_input",
    "retract_entity_mention",
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

// The tools that only observe: a failed call earlier in the same model reply does
// not skip them. Mirrors docs/compiler/agent-runtime.md#models-and-tools.
pub fn read_only(name: &str) -> bool {
    READ_TOOLS.contains(&name)
        || REANSWERABLE_TOOLS.contains(&name)
        || matches!(
            name,
            "inspect_asset_link"
                | "read_document"
                | "list_document_sets"
                | "preview_document_set"
                | "project_context"
                | "list_jazyk_skills"
                | "read_jazyk_skill"
                | "load_skill"
                | "goals"
                | "recent_edits"
                | "verification_tasks"
                | "task_context"
                | "inspect_run"
                | "read_run_log"
                | "search_run_log"
                | "read_run_changes"
        )
}

// One search_files reply: matches, bytes, the context cap, and the longest line
// it quotes. Mirrors docs/compiler/goals/generate.md#file-and-command-tools.
pub(super) const SEARCH_FILES_MATCHES: usize = 100;

pub(super) const SEARCH_FILES_BYTES: usize = 32 * 1024;

pub(super) const SEARCH_FILES_CONTEXT: usize = 5;

pub(super) const SEARCH_FILES_LINE_CHARS: usize = 300;

pub(super) const SEARCH_FILES_FILE_BYTES: usize = 1024 * 1024;

pub(super) const SEARCH_FILES_SCAN_BYTES: usize = 8 * 1024 * 1024;

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
pub const FILE_TOOLS: [&str; 7] = [
    "read_text_file",
    "write_text_file",
    "list_files",
    "search_files",
    "link_asset",
    "inspect_asset_link",
    "remove_asset_link",
];

// The view tools, served beside the write tools. See docs/compiler/tools.md#view-tools.
pub const VIEW_TOOLS: [&str; 3] = ["upsert_view", "update_view", "delete_view"];

// The chat serving's human paths, in no session's toolset and never a raw write.
// See docs/compiler/tools.md#chat-tools.
pub const CHAT_TOOLS: [&str; 1] = ["edit_fact"];

pub const CHAT_GRAPH_DESCRIPTION: &str = "In writable chat, commits directly without a build goal or release when no batch is open. Within a batch, stages under its goal scope. Use edit_graph for related atomic edits or a separate request independent of that batch.";

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
