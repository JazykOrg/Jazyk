//! Routes one host request to the shared web API or a hosted job.
use super::execution::execution;
use super::{llm, model_name, root, trace, Reply};
use serde_json::{json, Value};

pub(super) fn dispatch(method: &str, path: &str, body: &str) -> Result<Reply, String> {
    let routed =
        match crate::webapi::sets::route(path, serde_json::from_str(body).unwrap_or(Value::Null)) {
            Ok(r) => r,
            Err((code, e)) => return Ok(Reply::json(code, json!({"error":e}))),
        };
    let (route, query) = routed.path.split_once('?').unwrap_or((&routed.path, ""));
    // A configuration that does not load, or a project with no document set, is a
    // state of the project with its own status, never a server failure.
    // Mirrors docs/compiler/document-sets.md#discovery-and-opening.
    let workspace = match crate::document_sets::Workspace::load(&root()) {
        Ok(w) => w,
        Err((code, e)) => return Ok(Reply::json(code, json!({"error":e}))),
    };
    let selected = || workspace.resolve(routed.selector.as_deref());
    let route = route.strip_prefix("/api").unwrap_or(route);
    // Conversations live only in the host's record store; without one a request that
    // reads or writes a conversation is refused before it does any work.
    // Mirrors docs/compiler/hosted-runtime.md#host-records.
    let conversational = route.starts_with("/chat/")
        || (method == "POST"
            && route == "/jobs"
            && !routed.body.get("conversationId").is_none_or(Value::is_null));
    if conversational {
        if let Err(e) = super::records::store() {
            return Ok(Reply::json(501, json!({ "error": e })));
        }
    }
    if method == "GET"
        && matches!(
            route,
            "/out/file" | "/project/file" | "/project/raw" | "/deliverable/raw"
        )
    {
        let rel = url::form_urlencoded::parse(query.as_bytes())
            .find(|(k, _)| k == "path")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        let p = if matches!(route, "/project/file" | "/project/raw" | "/deliverable/raw")
            && routed.selector.is_none()
        {
            workspace.files_project()
        } else {
            match selected() {
                Ok(p) => p,
                Err((code, e)) => return Ok(Reply::json(code, json!({"error":e}))),
            }
        };
        let result = match route {
            "/out/file" => crate::webapi::outfiles::out_file(&p.out, &rel),
            "/project/file" | "/project/raw" => {
                crate::webapi::projfiles::project_file(&p.root, &rel)
            }
            _ => crate::webapi::deliverable::deliverable_raw(
                &crate::gen::GenSettings::resolve(&p),
                &rel,
            ),
        };
        return Ok(match result {
            Ok(body) => Reply {
                status: 200,
                content_type: crate::webapi::outfiles::content_type(&rel).into(),
                body,
            },
            Err((code, msg)) => Reply::json(code, json!({"error":msg})),
        });
    }
    if method == "POST" && route == "/jobs" {
        let request = routed.body.clone();
        let _conversation = match crate::conversation::BuildScope::enter(&request) {
            Ok(scope) => scope,
            Err((status, error)) => return Ok(Reply::json(status, json!({"error":error}))),
        };
        let kind = request["kind"].as_str().unwrap_or("compile");
        if kind == "workflow" {
            let tasks_only = crate::workflow::parse(&request["steps"]).is_ok_and(|steps| {
                steps
                    .iter()
                    .all(|s| matches!(s, crate::workflow::Step::Task { .. }))
            });
            let p = match if tasks_only && routed.selector.is_none() {
                Ok(workspace.files_project())
            } else {
                selected()
            } {
                Ok(p) => p,
                Err((code, e)) => return Ok(Reply::json(code, json!({"error":e}))),
            };
            let steps = match crate::workflow::parse(&request["steps"]) {
                Ok(steps) => steps,
                Err(e) => return Ok(Reply::json(400, json!({"error":e}))),
            };
            let result = crate::workflow::run(&p, &llm(), &steps, &trace())?;
            return Ok(Reply::json(
                if result["state"] == "failed" {
                    422
                } else {
                    200
                },
                json!({"kind":"workflow","report":result}),
            ));
        }
        if kind == "verify" && request.get("testPaths").is_some() {
            let paths = match crate::verify::artifacts::parse_paths(&request) {
                Ok(Some(paths)) => paths,
                Ok(None) => unreachable!(),
                Err(error) => return Ok(Reply::json(400, json!({"error":error}))),
            };
            let result = crate::verify::artifacts::run_workspace(
                &root(),
                routed.selector.as_deref(),
                &paths,
                &trace(),
                |project| {
                    Ok(Box::new(crate::inprocess::InProcessRunner::start(
                        project,
                        &llm(),
                        &project.out,
                    )?))
                },
            );
            return Ok(match result {
                Ok(report) => Reply::json(200, json!({"kind":"verify","report":report})),
                Err(error) => Reply::json(422, json!({"error":error})),
            });
        }
        if matches!(kind, "verify" | "audit" | "gen") {
            let p = match selected() {
                Ok(p) => p,
                Err((code, e)) => return Ok(Reply::json(code, json!({"error":e}))),
            };
            let store = crate::store::Store::load(&p.out);
            let gs = crate::gen::GenSettings::resolve(&p);
            let result = if kind == "gen" {
                use crate::runner::SessionRunner;
                let _build = crate::control::begin_internal_build(&p, &p.out, "generate")?;
                let runner = crate::inprocess::InProcessRunner::start(&p, &llm(), &p.out)?;
                runner.set_build_token(Some(_build.token().to_owned()));
                let entities = request["entities"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                crate::gen::run_all(
                    &store,
                    &runner,
                    &gs,
                    &entities,
                    request["force"].as_bool().unwrap_or(false),
                    &trace(),
                )
            } else if kind == "audit" {
                Ok(crate::verify::audit(&store, &gs))
            } else {
                let runner = crate::inprocess::InProcessRunner::start(&p, &llm(), &p.out)?;
                let targets = request["targets"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                crate::verify::run_all(
                    &store,
                    &runner,
                    &gs,
                    &targets,
                    request["testKind"].as_str(),
                    request["force"].as_bool().unwrap_or(false),
                    &trace(),
                )
            };
            return Ok(match result {
                Ok(report) => Reply::json(
                    200,
                    json!({"id":1,"kind":kind,"state":"finished","report":report}),
                ),
                Err(error) => Reply::json(422, json!({"error":error})),
            });
        }
    }
    if method == "GET" && route == "/capabilities" {
        return Ok(Reply::json(
            200,
            json!({"protocol":crate::protocol::EDITOR_PROTOCOL_VERSION,"backend":"hosted","projects":"list","docs":"write","events":"pushed","lsp":false,"jobs":["workflow","compile","gen","verify","audit"],"agent":"embedded","chatInputs":true,"conversationJobs":true,"testPaths":true,"model":if execution().transport.is_some(){"configured"}else{"none"},"modelName":model_name(),"rendering":"server","persistence":"memory","shell":false,"dev":false}),
        ));
    }
    let envelope: Value =
        serde_json::from_str(&crate::webapi::dispatch::request(method, path, body))
            .map_err(|e| e.to_string())?;
    Ok(Reply::json(
        envelope["status"].as_u64().unwrap_or(500) as u16,
        envelope["body"].clone(),
    ))
}
