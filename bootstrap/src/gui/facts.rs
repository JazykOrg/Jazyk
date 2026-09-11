// POST /api/facts/{id}/edit: the inspector's edit paths. A quote-provenanced fact
// edits in two phases: the first call answers with the proposed sentence rewrite
// and commits nothing; the same call with the proposal echoed back commits the
// dual write. `decree: true`, or a fact with no prose behind it, lands graph-only
// with decree provenance and a ratification proposal. `field: limits.<limit>`
// stages a per-node bump. Mirrors docs/frontends/gui.md#api (facts).
use super::state::SharedState;
use crate::project::Project;
use crate::store::{Commit, Op, ProseEdit, Store};
use crate::tools::{ToolSession, WorkScope};
use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use serde_json::{json, Value};
use std::path::Path;

type ApiError = (u16, String);

fn bad(msg: impl Into<String>) -> ApiError {
    (400, msg.into())
}

fn conflict(msg: impl Into<String>) -> ApiError {
    (409, msg.into())
}

// A tool session over a snapshot synced against the documents, the same gates the
// chat serving runs.
fn session_for(proj: &Project, out: &Path, target: &str) -> ToolSession {
    let mut snapshot = Store::load(out);
    let (parsed, _) = crate::reconcile::parse_all(proj);
    snapshot.sync_docs(&parsed);
    let mut scope = WorkScope::serving("mcp-write");
    scope.target = target.to_string();
    let mut session = ToolSession::new(
        snapshot,
        scope,
        crate::limits::SESSION_MUTATIONS,
        crate::limits::CONTEXT_BUDGET,
    );
    session.gen = crate::gen::GenSettings::resolve(proj);
    session.caller.source = "gui".into();
    session.caller.target = target.to_string();
    session
}

// The mechanical sentence a field edit implies, when one exists. The same templates
// the edit_fact tool uses; fields with no sentence form (edges, transition, facets,
// members) have no mechanical rewrite and take the decree path instead.
fn mechanical_sentence(store: &Store, id: &str, field: &str, value: &Value) -> Option<String> {
    let id = store.resolve_id(id);
    let text = value.as_str().map(str::trim).filter(|s| !s.is_empty());
    if store.graph.requirements.contains_key(id) {
        return match field {
            "statement" => text.map(String::from),
            _ => None,
        };
    }
    let e = store.graph.entities.get(id)?;
    let v = text?;
    match field {
        "definition" => Some(format!("{}: {}", e.name, v)),
        "stereotype" => Some(format!("{} is a {}.", e.name, v)),
        "parent" => {
            let pid = store.resolve_id(v);
            let pname = store
                .graph
                .entities
                .get(pid)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| v.to_string());
            Some(format!("{} is part of {}.", e.name, pname))
        }
        f if f.starts_with("attributes.") => {
            let rest = &f["attributes.".len()..];
            let (aname, sub) = rest.rsplit_once('.')?;
            match sub {
                "type" => Some(format!(
                    "{} has an attribute {} of type {}.",
                    e.name, aname, v
                )),
                "value" => Some(format!("{}'s {} is {}.", e.name, aname, v)),
                _ => None,
            }
        }
        _ => None,
    }
}

// Whether the fact behind (id, field) stands on a quote in the prose.
fn is_quoted(store: &Store, id: &str, field: &str) -> bool {
    let id = store.resolve_id(id);
    if let Some(r) = store.graph.requirements.get(id) {
        return r.source.is_some();
    }
    if let Some(e) = store.graph.entities.get(id) {
        if let Some(rest) = field.strip_prefix("attributes.") {
            let aname = rest.rsplit_once('.').map(|(n, _)| n).unwrap_or(rest);
            return e
                .attributes
                .iter()
                .find(|a| a.name == aname)
                .map(|a| matches!(a.provenance, crate::model::Provenance::Quote(_)))
                .unwrap_or(false);
        }
        return e.provenance.is_none() && !e.mentions.is_empty();
    }
    false
}

// The open ratification proposal a decree just filed on the subject, for the reply.
fn ratification_diag(store: &Store, subject: &str) -> Option<String> {
    store
        .graph
        .diagnostics
        .iter()
        .filter(|(_, d)| {
            d.rule == "ratification-pending"
                && d.lifecycle == "open"
                && d.subjects.iter().any(|s| s == subject)
        })
        .map(|(id, _)| id.clone())
        .next_back()
}

// A per-node limit bump: the board's dismiss action. Stages bump_limit with decree
// provenance, never edit_fact. Mirrors docs/compiler/graph.md#per-node-bumps.
fn bump_limit(
    proj: &Project,
    out: &Path,
    id: &str,
    field: &str,
    value: &Value,
) -> Result<Value, ApiError> {
    let limit = field.trim_start_matches("limits.").to_string();
    let value = value
        .as_u64()
        .filter(|v| *v > 0)
        .ok_or_else(|| bad("limits.<limit> takes a positive integer value"))?;
    let Some(l) = crate::limits::limit(&limit) else {
        return Err(bad(format!("`{}` is not a limit in the registry", limit)));
    };
    let mut store = Store::load(out);
    let (parsed, _) = crate::reconcile::parse_all(proj);
    store.sync_docs(&parsed);
    let rid = store.resolve_id(id).to_string();
    let is_entity = store.graph.entities.contains_key(&rid);
    let is_view = store.graph.views.contains_key(&rid);
    if !is_entity && !is_view {
        return Err((404, format!("no entity or view {}", rid)));
    }
    let applies = (is_entity && crate::limits::ENTITY_LIMITS.contains(&l.name))
        || (is_view && crate::limits::VIEW_LIMITS.contains(&l.name));
    if !applies {
        return Err(bad(format!("limit `{}` does not apply to {}", limit, rid)));
    }
    let provenance = crate::model::Provenance::Decree {
        author: "gui".into(),
        at: crate::verify::now_iso(),
        note: Some(format!("raised {} to {}", limit, value)),
    };
    let report = store.apply(
        vec![Op::BumpLimit {
            id: rid.clone(),
            limit: limit.clone(),
            value,
            provenance,
        }],
        &Commit::store("decree"),
    );
    if !report.skipped.is_empty() {
        return Err(bad(report.skipped.join("; ")));
    }
    Ok(json!({
        "id": rid, "field": field, "path": "bump",
        "limit": limit, "value": value, "generation": report.generation,
    }))
}

// The core of the endpoint, callable without the server. Returns the reply or an
// (http status, message) pair.
pub(crate) fn edit_fact_core(
    proj: &Project,
    out: &Path,
    id: &str,
    body: &Value,
) -> Result<Value, ApiError> {
    let field = body["field"]
        .as_str()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .ok_or_else(|| bad("field is required"))?;
    let value = &body["value"];
    if field.starts_with("limits.") {
        return bump_limit(proj, out, id, field, value);
    }
    if value.is_null() {
        return Err(bad("value is required: the field's new content"));
    }
    let decree = body["decree"].as_bool().unwrap_or(false);
    let note = body["note"].as_str().map(str::trim).map(String::from);
    let proposal = body["proposal"].as_object().cloned();

    // Phase 1: a quote-provenanced fact without a proposal answers with the
    // proposed sentence rewrite; nothing commits.
    if !decree && proposal.is_none() {
        let store = Store::load(out);
        if is_quoted(&store, id, field) {
            let Some(sentence) = mechanical_sentence(&store, id, field, value) else {
                return Ok(json!({
                    "id": store.resolve_id(id), "field": field, "proposal": Value::Null,
                    "needsDecree": true,
                    "note": "no sentence rewrite follows mechanically from this field; \
                             pass decree: true to land it graph-only with a ratification proposal",
                }));
            };
            let mut session = session_for(proj, out, id);
            let reply = session
                .dispatch(
                    "edit_fact",
                    &json!({ "id": id, "field": field, "value": value, "note": sentence }),
                )
                .map_err(|e| {
                    bad(e.to_value()["error"]["message"]
                        .as_str()
                        .unwrap_or("edit refused")
                        .to_string())
                })?;
            // The staged ops are discarded: this phase only names the rewrite.
            let p = &reply["prose"];
            if p.is_object() {
                return Ok(json!({
                    "id": reply["id"], "field": field,
                    "proposal": {
                        "doc": p["doc"], "section": p["section"],
                        "old_text": p["old_text"], "new_text": p["new_text"],
                    },
                    "note": "echo the proposal back to commit the dual write; decree: true declines it",
                }));
            }
            // The tool decided the fact is not quoted after all: fall through to
            // the decree path below.
        }
    }

    // The committing phases share one session and one dispatch.
    let mut session = session_for(proj, out, id);
    let accepted = proposal
        .as_ref()
        .and_then(|p| p.get("new_text"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let mut args = json!({ "id": id, "field": field, "value": value });
    if let Some(n) = accepted.as_ref().filter(|_| !decree) {
        args["note"] = json!(n);
    } else if decree {
        // The decree note rides only where the tool reads it as one (an unquoted
        // fact); on a quoted fact a note would read as an accepted rewrite.
        if let Some(n) = note.as_ref() {
            if !is_quoted(&session.snapshot, id, field) {
                args["note"] = json!(n);
            }
        }
    } else if let Some(n) = note.as_ref() {
        args["note"] = json!(n);
    }
    let reply = session.dispatch("edit_fact", &args).map_err(|e| {
        bad(e.to_value()["error"]["message"]
            .as_str()
            .unwrap_or("edit refused")
            .to_string())
    })?;
    let ops = std::mem::take(&mut session.staged);

    if reply["prose"].is_object() && !decree {
        // Phase 2: the dual write. The document must still hold the proposal.
        let p = &reply["prose"];
        let (doc, section) = (
            p["doc"].as_str().unwrap_or_default().to_string(),
            p["section"].as_str().unwrap_or_default().to_string(),
        );
        if let Some(prop) = &proposal {
            let same = prop.get("doc").and_then(|v| v.as_str()) == Some(doc.as_str())
                && prop.get("section").and_then(|v| v.as_str()) == Some(section.as_str())
                && prop.get("old_text").and_then(|v| v.as_str())
                    == Some(p["old_text"].as_str().unwrap_or_default());
            if !same {
                return Err(conflict(
                    "the fact moved since the proposal was made; re-read and propose again",
                ));
            }
        }
        let path = proj.root.join(&doc);
        let old_full = std::fs::read_to_string(&path)
            .map_err(|e| bad(format!("cannot read {}: {}", doc, e)))?;
        let section_raw = Store::load(out)
            .docs
            .get(&doc)
            .and_then(|d| d.sections.get(&section))
            .map(|s| s.raw.clone());
        let edit = ProseEdit::locate(
            &doc,
            &section,
            section_raw.as_deref(),
            &old_full,
            p["old_text"].as_str().unwrap_or_default(),
            p["new_text"].as_str().unwrap_or_default(),
        )
        .map_err(conflict)?;
        let mut store = Store::load(out);
        let (parsed, _) = crate::reconcile::parse_all(proj);
        store.sync_docs(&parsed);
        let report = store
            .dual_write(&proj.root, &edit, ops, &Commit::store("dual-write"), None)
            .map_err(conflict)?;
        return Ok(json!({
            "id": reply["id"], "field": field, "path": "dual-write",
            "committed": true, "applied": report.applied, "doc": doc,
            "generation": report.generation,
            "note": "the prose and the graph moved together; the document is not re-dirtied",
        }));
    }

    // The decree path: graph-only, decree provenance, ratification proposal queued.
    if ops.is_empty() {
        return Err(bad("the edit staged no mutation"));
    }
    let mut store = Store::load(out);
    let (parsed, _) = crate::reconcile::parse_all(proj);
    store.sync_docs(&parsed);
    let report = store.apply(ops, &Commit::store("decree"));
    if !report.skipped.is_empty() {
        return Err(bad(report.skipped.join("; ")));
    }
    let subject = reply["id"].as_str().unwrap_or(id).to_string();
    let after = Store::load(out);
    Ok(json!({
        "id": subject, "field": field, "path": "decree",
        "committed": true, "generation": report.generation,
        "ratification": ratification_diag(&after, &subject),
        "note": reply["note"],
    }))
}

pub async fn edit_fact(
    State(st): State<SharedState>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> Response {
    let proj = st.proj();
    let out = st.out.clone();
    let result = tokio::task::spawn_blocking(move || edit_fact_core(&proj, &out, &id, &body))
        .await
        .expect("facts edit task panicked");
    match result {
        Ok(mut v) => {
            // A committed edit moves the board; the watcher also catches it, but the
            // click deserves an immediate refresh.
            if v["committed"].as_bool().unwrap_or(false) {
                super::events::emit_board_changed(&st);
            }
            v["ok"] = json!(true);
            Json(v).into_response()
        }
        Err((code, msg)) => super::api::err(
            StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST),
            msg,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A small project on disk: one document, one entity, one quoted requirement.
    fn fixture(name: &str) -> (Project, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("jazyk-gui-facts-{}-{}", name, std::process::id()));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(
            root.join("docs/shop.md"),
            "# Shop\n\nThe shop stores orders. The shop is small.\n",
        )
        .unwrap();
        let mut proj = Project::default();
        proj.root = root.clone();
        proj.docs_glob = vec!["docs/**/*.md".into()];
        let out = root.join("jazyk-out");
        proj.out = out.clone();
        // Seed the graph through the same tools a session uses.
        let mut session = session_for(&proj, &out, "seed");
        session
            .dispatch(
                "upsert_entity",
                &json!({
                    "name": "Shop", "definition": "the shop",
                    "mention": {"section": "docs/shop.md#/shop", "quote": "The shop stores orders."},
                }),
            )
            .expect("entity stages");
        session
            .dispatch(
                "upsert_requirement",
                &json!({
                    "statement": "The shop stores orders.",
                    "entities": ["ent:shop"],
                    "section": "docs/shop.md#/shop",
                    "quote": "The shop stores orders.",
                }),
            )
            .expect("requirement stages");
        let ops = std::mem::take(&mut session.staged);
        let mut store = Store::load(&out);
        let (parsed, _) = crate::reconcile::parse_all(&proj);
        store.sync_docs(&parsed);
        let report = store.apply(ops, &Commit::session(vec![], 1, 0));
        assert!(
            report.skipped.is_empty(),
            "seed commit: {:?}",
            report.skipped
        );
        (proj, out)
    }

    // Phase 1 on a quoted statement answers with the rewrite and commits nothing;
    // phase 2 with the proposal echoed commits prose and graph together.
    #[test]
    fn quoted_statement_edit_proposes_then_commits_the_dual_write() {
        let (proj, out) = fixture("dual");
        let gen_before = crate::store::read_generation(&out);
        let v = edit_fact_core(
            &proj,
            &out,
            "req:shop-1",
            &json!({ "field": "statement", "value": "The shop stores every order." }),
        )
        .expect("phase 1 succeeds");
        assert_eq!(v["proposal"]["old_text"], json!("The shop stores orders."));
        assert_eq!(
            v["proposal"]["new_text"],
            json!("The shop stores every order.")
        );
        assert_eq!(
            crate::store::read_generation(&out),
            gen_before,
            "phase 1 commits nothing"
        );
        let v2 = edit_fact_core(
            &proj,
            &out,
            "req:shop-1",
            &json!({
                "field": "statement", "value": "The shop stores every order.",
                "proposal": v["proposal"],
            }),
        )
        .expect("phase 2 succeeds");
        assert_eq!(v2["path"], json!("dual-write"));
        assert_eq!(v2["committed"], json!(true));
        let text = std::fs::read_to_string(proj.root.join("docs/shop.md")).unwrap();
        assert!(
            text.contains("The shop stores every order."),
            "the prose moved"
        );
        let store = Store::load(&out);
        let r = store.graph.requirements.get("req:shop-1").unwrap();
        assert_eq!(r.statement, "The shop stores every order.");
        assert_eq!(
            r.source.as_ref().unwrap().quote,
            "The shop stores every order."
        );
        // The commit absorbed the new hash: the document is not dirty against the graph.
        let rec = store.docs.get("docs/shop.md").unwrap();
        assert_eq!(rec.content_hash, crate::model::hash_hex(&text));
        std::fs::remove_dir_all(&proj.root).ok();
    }

    // A stale proposal (the document changed since) is refused with a conflict.
    #[test]
    fn a_stale_proposal_conflicts_instead_of_committing() {
        let (proj, out) = fixture("stale");
        let v = edit_fact_core(
            &proj,
            &out,
            "req:shop-1",
            &json!({ "field": "statement", "value": "The shop stores every order." }),
        )
        .unwrap();
        // The document changes under the proposal.
        std::fs::write(
            proj.root.join("docs/shop.md"),
            "# Shop\n\nThe shop keeps orders now. The shop is small.\n",
        )
        .unwrap();
        let err = edit_fact_core(
            &proj,
            &out,
            "req:shop-1",
            &json!({
                "field": "statement", "value": "The shop stores every order.",
                "proposal": v["proposal"],
            }),
        )
        .expect_err("a moved document conflicts");
        assert_eq!(err.0, 409);
        std::fs::remove_dir_all(&proj.root).ok();
    }

    // decree: true lands graph-only with decree provenance and queues the
    // ratification proposal.
    #[test]
    fn a_decree_lands_graph_only_and_queues_ratification() {
        let (proj, out) = fixture("decree");
        let doc_before = std::fs::read_to_string(proj.root.join("docs/shop.md")).unwrap();
        let v = edit_fact_core(
            &proj,
            &out,
            "req:shop-1",
            &json!({ "field": "statement", "value": "The shop archives orders.", "decree": true }),
        )
        .expect("decree succeeds");
        assert_eq!(v["path"], json!("decree"));
        assert!(v["ratification"].is_string(), "a proposal was filed");
        let store = Store::load(&out);
        let r = store.graph.requirements.get("req:shop-1").unwrap();
        assert!(r.source.is_none(), "the quote gave way to the decree");
        assert!(matches!(
            r.provenance,
            Some(crate::model::Provenance::Decree { .. })
        ));
        assert_eq!(
            std::fs::read_to_string(proj.root.join("docs/shop.md")).unwrap(),
            doc_before,
            "the prose never moved"
        );
        std::fs::remove_dir_all(&proj.root).ok();
    }

    // limits.<limit> stages a bump on the node, a decree in the journal.
    #[test]
    fn a_limit_bump_lands_on_the_entity() {
        let (proj, out) = fixture("bump");
        let v = edit_fact_core(
            &proj,
            &out,
            "ent:shop",
            &json!({ "field": "limits.requirements-per-entity", "value": 90 }),
        )
        .expect("bump succeeds");
        assert_eq!(v["path"], json!("bump"));
        let store = Store::load(&out);
        let e = store.graph.entities.get("ent:shop").unwrap();
        assert_eq!(
            e.limits.get("requirements-per-entity").map(|b| b.value),
            Some(90)
        );
        // A bogus limit name is refused.
        let err = edit_fact_core(
            &proj,
            &out,
            "ent:shop",
            &json!({ "field": "limits.no-such-limit", "value": 5 }),
        )
        .expect_err("unknown limit");
        assert_eq!(err.0, 400);
        std::fs::remove_dir_all(&proj.root).ok();
    }
}
