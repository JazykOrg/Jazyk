use super::*;

#[test]
fn log_tool_coordinates_match_unicode_paging_and_search_lines() {
    let tools = catalog();
    let read = tools
        .iter()
        .find(|tool| tool.name == "read_run_log")
        .unwrap();
    assert!(read.parameters["properties"]["offset"]["description"]
        .as_str()
        .unwrap()
        .contains("Unicode character"));
    let search = tools
        .iter()
        .find(|tool| tool.name == "search_run_log")
        .unwrap();
    assert!(search.parameters["properties"]["offset"]["description"]
        .as_str()
        .unwrap()
        .contains("lines to skip"));
    let state = json!({"terminated":true, "log":"α\nЖ💡\nFAIL: x\n"});
    let page = crate::task_runtime::present(
        &state,
        &Value::Null,
        &json!({"operation":"read_run_log", "offset":2, "limit":2}),
    )
    .unwrap();
    assert_eq!(page["text"], "Ж💡");
    assert_eq!(page["next_offset"], 4);
    let next = crate::task_runtime::present(
        &state,
        &Value::Null,
        &json!({"operation":"read_run_log", "offset":page["next_offset"], "limit":6}),
    )
    .unwrap();
    assert_eq!(next["text"], "\nFAIL:");
    let matches = crate::task_runtime::present(
        &state,
        &Value::Null,
        &json!({"operation":"search_run_log", "query":"FAIL", "offset":1, "limit":1}),
    )
    .unwrap();
    assert_eq!(matches["matches"], json!([{"line":3, "text":"FAIL: x"}]));
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
            "place-anchors" => include_str!("../../../../docs/compiler/goals/place-anchors.md"),
            "reconcile-section" => {
                include_str!("../../../../docs/compiler/goals/reconcile-section.md")
            }
            "rejudge-pair" => include_str!("../../../../docs/compiler/goals/rejudge-pair.md"),
            "review-entity" => include_str!("../../../../docs/compiler/goals/review-entity.md"),
            "derive-criteria" => {
                include_str!("../../../../docs/compiler/goals/derive-criteria.md")
            }
            "retrace" => include_str!("../../../../docs/compiler/goals/retrace.md"),
            "conform-instance" => {
                include_str!("../../../../docs/compiler/goals/conform-instance.md")
            }
            "bind" => include_str!("../../../../docs/compiler/goals/bind.md"),
            "generate" => include_str!("../../../../docs/compiler/goals/generate.md"),
            "verify" => include_str!("../../../../docs/compiler/goals/verify.md"),
            "declare-edges" => include_str!("../../../../docs/compiler/goals/declare-edges.md"),
            "dedupe-candidates" => {
                include_str!("../../../../docs/compiler/goals/dedupe-candidates.md")
            }
            "curate-view" => include_str!("../../../../docs/compiler/goals/curate-view.md"),
            "split-view" => include_str!("../../../../docs/compiler/goals/split-view.md"),
            "abstract-entity" => {
                include_str!("../../../../docs/compiler/goals/abstract-entity.md")
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

#[test]
fn toolset_of_a_goal_kind_task_is_its_slice() {
    let names = toolset("abstract-entity");
    for t in [
        "group_entities",
        "dissolve_entity",
        "update_entity",
        "mark_goal_done",
        "done",
    ] {
        assert!(names.contains(&t), "abstract-entity misses `{}`", t);
    }
    assert!(toolset("curate-view").contains(&"update_view"));
    assert!(!toolset("no-such-task").contains(&"mark_goal_done"));
}

// docs/compiler/tools.schema.yaml specifies every catalog tool's request: one
// definition per tool (camelCase, `Request` suffix optional) with the same
// property names and the same required fields.
#[test]
fn the_tool_schema_matches_the_catalog() {
    let text = include_str!("../../../../docs/compiler/tools.schema.yaml");
    let schema: Value = serde_norway::from_str(text).expect("tools.schema.yaml parses");
    let defs = schema["definitions"].as_object().expect("definitions");
    let camel = |name: &str| {
        let mut out = String::new();
        let mut up = false;
        for c in name.chars() {
            if c == '_' {
                up = true;
            } else if up {
                out.extend(c.to_uppercase());
                up = false;
            } else {
                out.push(c);
            }
        }
        out
    };
    let names = |v: &Value, key: &str| -> BTreeSet<String> {
        match key {
            "required" => v["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str().map(String::from))
                .collect(),
            _ => v["properties"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(k, _)| k.clone())
                .collect(),
        }
    };
    let mut drift: Vec<String> = Vec::new();
    for mut tool in catalog() {
        // The catalog's done is a worker's; the schema's adds the MCP beginNext.
        if tool.name == "done" {
            tool.parameters = done_parameters(true);
        }
        let base = camel(tool.name);
        let Some(def) = defs
            .get(&format!("{}Request", base))
            .or_else(|| defs.get(&base))
        else {
            drift.push(format!(
                "{}: no {}Request or {} definition",
                tool.name, base, base
            ));
            continue;
        };
        for key in ["properties", "required"] {
            let (want, have) = (names(&tool.parameters, key), names(def, key));
            if want != have {
                drift.push(format!(
                    "{} {}: catalog {:?}, schema {:?}",
                    tool.name, key, want, have
                ));
            }
        }
    }
    assert!(
        drift.is_empty(),
        "tools.schema.yaml drifted from the catalog:\n{}",
        drift.join("\n")
    );
}

// The graph --write serving is what tools.md#toolsets names: the read tools, the
// raw write tools and view tools, the task runtime tools, feedback, and spend.
#[test]
fn the_write_serving_is_the_documented_set() {
    let served = toolset("mcp-write");
    let mut expected: Vec<&str> = READ_TOOLS.to_vec();
    expected.extend(GRAPH_WRITE_TOOLS);
    expected.extend(crate::task_runtime::TOOLS);
    expected.extend([FEEDBACK_TOOL, SPEND_TOOL]);
    let mut a = served.clone();
    a.sort();
    expected.sort();
    assert_eq!(a, expected);
    for absent in [
        "record_generation",
        "begin_binding",
        "record_verdict",
        PAIR_SETTLE_TOOL,
        "mark_goal_done",
    ] {
        assert!(!served.contains(&absent), "{absent}");
    }
}

// The batch claim rides in the pair slice only, and the compile serving lists it
// so an MCP client sees it. Mirrors docs/compiler/tools.md#toolsets.
// Mirrors docs/frontends/mcp.md#compilation-over-mcp: the compile serving exposes
// every tool a compile or GC goal kind's slice names, or a batch lists writeTools
// the server does not have (a sort session found upsert_criterion missing).
#[test]
fn the_compile_serving_exposes_every_goal_kinds_slice() {
    let served = toolset("mcp-compile");
    for k in crate::goals::REGISTRY.iter() {
        if matches!(k.kind(), "bind" | "generate" | "verify") {
            continue;
        }
        for t in k.toolset() {
            assert!(
                served.contains(t),
                "mcp compile lacks `{}` of {}",
                t,
                k.kind()
            );
        }
    }
}

#[test]
fn settle_pairs_is_in_the_pair_toolset_only() {
    assert!(toolset("rejudge-pair").contains(&PAIR_SETTLE_TOOL));
    assert!(toolset("review-requirement").contains(&PAIR_SETTLE_TOOL));
    assert!(toolset("mcp-compile").contains(&PAIR_SETTLE_TOOL));
    for kind in [
        "reconcile-section",
        "review-entity",
        "abstract-entity",
        "mcp-read",
    ] {
        assert!(
            !toolset(kind).contains(&PAIR_SETTLE_TOOL),
            "{} should not serve settle_pairs",
            kind
        );
    }
    assert!(catalog().iter().any(|t| t.name == PAIR_SETTLE_TOOL));
    assert!(toolset_for_kinds(&["rejudge-pair", "review-entity"]).contains(&PAIR_SETTLE_TOOL));
}
