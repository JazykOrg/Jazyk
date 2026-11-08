//! Set routing is request-local. Wire-qualified references never enter a store.
use crate::document_sets::{Result, Workspace};
use serde_json::{json, Value};
use std::path::Path;

const PATH_ESCAPE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');
const KINDS: &[&str] = &[
    "ent", "req", "crit", "view", "rel", "sm", "diag", "scope", "g",
];

pub fn qualify(id: &str, set: &str) -> String {
    if let Some((kind, tail)) = id.split_once(':') {
        if KINDS.contains(&kind)
            && !id.chars().any(char::is_whitespace)
            && unqualify(id).0.is_none()
        {
            return format!("{}:{}::{}", kind, set, tail);
        }
    }
    id.to_string()
}

pub fn unqualify(id: &str) -> (Option<String>, String) {
    if let Some((kind, tail)) = id.split_once(':') {
        if let Some((set, local)) = tail.split_once("::") {
            if KINDS.contains(&kind) && crate::document_sets::valid_id(set) {
                return (Some(set.to_string()), format!("{}:{}", kind, local));
            }
        }
    }
    (None, id.to_string())
}

pub fn qualify_value(value: Value, set: &str) -> Value {
    match value {
        Value::String(s) => {
            if s.trim_start().starts_with("<svg")
                || s.trim_start().starts_with("<?xml") && s.contains("<svg")
            {
                return json!(s.replacen("<svg", &format!("<svg data-jazyk-set=\"{}\"", set), 1));
            }
            if s.starts_with("/api/") && !s.contains("setId=") {
                return json!(format!(
                    "{}{}setId={}",
                    s,
                    if s.contains('?') { "&" } else { "?" },
                    set
                ));
            }
            json!(qualify(&s, set))
        }
        Value::Array(values) => {
            Value::Array(values.into_iter().map(|v| qualify_value(v, set)).collect())
        }
        Value::Object(values) => {
            let local_id = values.get("id").and_then(Value::as_str).map(str::to_string);
            let mut result: serde_json::Map<String, Value> = values
                .into_iter()
                .map(|(k, v)| (qualify(&k, set), qualify_value(v, set)))
                .collect();
            if let Some(id) = local_id {
                result.insert("setId".into(), json!(set));
                result.insert("localId".into(), json!(id));
            }
            Value::Object(result)
        }
        value => value,
    }
}

fn choose(selection: &mut Option<String>, candidate: Option<String>) -> Result<()> {
    if let Some(candidate) = candidate {
        if selection.as_ref().is_some_and(|s| s != &candidate) {
            return Err((
                400,
                "an operation cannot mix graph references from different document sets".into(),
            ));
        }
        *selection = Some(candidate);
    }
    Ok(())
}

fn decode_value(value: Value, selection: &mut Option<String>) -> Result<Value> {
    Ok(match value {
        Value::String(s) => {
            let (set, local) = unqualify(&s);
            choose(selection, set)?;
            json!(local)
        }
        Value::Array(a) => Value::Array(
            a.into_iter()
                .map(|v| decode_value(v, selection))
                .collect::<Result<_>>()?,
        ),
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, v)| Ok((k, decode_value(v, selection)?)))
                .collect::<Result<_>>()?,
        ),
        v => v,
    })
}

pub struct Route {
    pub path: String,
    pub selector: Option<String>,
    pub body: Value,
}

pub fn route(path: &str, body: Value) -> Result<Route> {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let mut path = path.strip_prefix("/api").unwrap_or(path).to_string();
    let mut selector = body
        .get("setId")
        .and_then(Value::as_str)
        .map(str::to_string);
    if let Some(tail) = path.strip_prefix("/sets/") {
        if let Some((set, rest)) = tail.split_once('/') {
            choose(&mut selector, Some(set.to_string()))?;
            path = format!("/{}", rest);
        }
    }
    let segments = path
        .split('/')
        .map(|segment| {
            let segment = percent_encoding::percent_decode_str(segment).decode_utf8_lossy();
            let (set, local) = unqualify(&segment);
            choose(&mut selector, set)?;
            Ok(percent_encoding::utf8_percent_encode(&local, PATH_ESCAPE).to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    let mut query_out = form_urlencoded::Serializer::new(String::new());
    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        if key == "setId" {
            choose(&mut selector, Some(value.into_owned()))?;
            continue;
        }
        if key == "path" && path.starts_with("/out/") {
            if let Some((set, local)) = value.split_once('/') {
                if crate::document_sets::valid_id(set) {
                    choose(&mut selector, Some(set.to_string()))?;
                    query_out.append_pair(&key, local);
                    continue;
                }
            }
        }
        let (set, local) = unqualify(&value);
        choose(&mut selector, set)?;
        query_out.append_pair(&key, &local);
    }
    let query = query_out.finish();
    let body = decode_value(body, &mut selector)?;
    // Static route words need no escaping; restore their readable spelling.
    let path = segments.join("/");
    Ok(Route {
        path: format!(
            "{}{}{}",
            path,
            if query.is_empty() { "" } else { "?" },
            query
        ),
        selector,
        body,
    })
}

pub fn aggregate_route(path: &str) -> bool {
    matches!(
        path,
        "/graph"
            | "/tree"
            | "/views"
            | "/search"
            | "/status"
            | "/docs"
            | "/coverage"
            | "/overview"
            | "/inbox"
            | "/questions"
            | "/journal"
            | "/workers"
            | "/deliverable"
            | "/deliverable/file"
            | "/product/overview"
            | "/out/list"
            | "/verify/matrix"
            | "/gen/pending"
            | "/verify/pending"
            | "/board"
    )
}

fn merge(into: &mut Value, value: Value) {
    match (into, value) {
        (Value::Object(to), Value::Object(from)) => {
            for (k, v) in from {
                if k == "generation" {
                    to.insert(k, json!(0));
                    continue;
                }
                if let Some(old) = to.get_mut(&k) {
                    merge(old, v);
                } else {
                    to.insert(k, v);
                }
            }
        }
        (Value::Array(to), Value::Array(mut from)) => to.append(&mut from),
        (Value::Number(to), Value::Number(from)) => {
            if let (Some(a), Some(b)) = (to.as_u64(), from.as_u64()) {
                *to = (a + b).into();
            }
        }
        (to @ Value::Null, from) => *to = from,
        (Value::Bool(to), Value::Bool(from)) => *to |= from,
        _ => {}
    }
}

pub fn aggregate(root: &Path, workspace: &Workspace, path: &str) -> Value {
    let mut combined = Value::Null;
    for set in &workspace.sets {
        let context = workspace.resolve(Some(&set.id)).expect("validated set");
        let response = super::dispatch::request_scoped(root, "GET", path, &Value::Null, &context);
        #[cfg(target_arch = "wasm32")]
        let response = {
            use futures::FutureExt;
            response
                .now_or_never()
                .expect("aggregate GET routes are deterministic")
        };
        let envelope: Value = serde_json::from_str(&response).unwrap();
        if envelope["status"].as_u64().unwrap_or(500) >= 300 {
            continue;
        }
        let mut body = qualify_value(envelope["body"].clone(), &set.id);
        if path.split('?').next() == Some("/out/list") {
            if let Some(files) = body["files"].as_array_mut() {
                for file in files {
                    if let Some(local) = file["path"].as_str() {
                        file["path"] = json!(format!("{}/{}", set.id, local));
                    }
                }
            }
        }
        if let Some(roots) = body.get_mut("roots").and_then(Value::as_array_mut) {
            for node in roots {
                if let Some(scope) = node["scope"].as_str() {
                    node["scope"] = json!(format!("{} · {}", set.name, scope));
                }
                if let Some(name) = node["name"].as_str() {
                    node["name"] = json!(format!("{} · {}", set.name, name));
                }
            }
        }
        if let Some(entries) = body.get_mut("entries").and_then(Value::as_array_mut) {
            for entry in entries {
                entry["setId"] = json!(set.id);
                entry["setName"] = json!(set.name);
            }
        }
        merge(&mut combined, body);
    }
    if combined.is_null() {
        return json!({});
    }
    if path.split('?').next() == Some("/docs") {
        if let Some(docs) = combined["docs"].as_array_mut() {
            let mut physical = std::collections::BTreeMap::<String, Value>::new();
            for doc in docs.drain(..) {
                let path = doc["path"].as_str().unwrap_or_default().to_string();
                if let Some(previous) = physical.get_mut(&path) {
                    previous["stale"] = json!(previous["stale"] == true || doc["stale"] == true);
                    for field in ["diagnostics", "goals"] {
                        merge(&mut previous[field], doc[field].clone());
                    }
                } else {
                    physical.insert(path, doc);
                }
            }
            docs.extend(physical.into_values());
            for doc in docs {
                if let Some(path) = doc["path"].as_str() {
                    doc["memberships"] = json!(workspace.memberships(path));
                }
            }
        }
    }
    if path.split('?').next() == Some("/status") && workspace.sets.len() > 1 {
        let summary = workspace.summary();
        combined["setGenerations"] = json!(summary["sets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (
                s["id"].as_str().unwrap().to_string(),
                s["generation"].clone()
            ))
            .collect::<std::collections::BTreeMap<_, _>>());
        combined["verdict"]["state"] = json!(if summary["sets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["dirty"] == false && s["verdict"]["state"] == "converged")
        {
            "converged"
        } else {
            "incomplete"
        });
    }
    combined
}
