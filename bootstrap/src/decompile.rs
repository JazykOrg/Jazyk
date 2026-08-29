// Decompilation: code to prose. Draft documents describing what the code under a
// scope does; the normal compile runs on them, and binding self-checks the result
// against the same code. Produces documentation files, never graph mutations.
// Mirrors docs/consumers/decompile.md and docs/compiler/tools.md#decompilation-tools.
use crate::gen::GenSettings;
use crate::model::hash_hex;
use crate::project::Project;
use crate::store::Store;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

// The drafting contract, served as begin_decompile instructions and as the internal
// worker's system prompt. Mirrors docs/consumers/decompile.md#draft-tasks.
pub fn instructions() -> String {
    include_str!("../../docs/compiler/goals/prompts/decompile-contract.md").into()
}

// ---------------------------------------------------------------------------
// drafts.yaml: each submitted draft's path and content hash, what ratification reads.

pub fn drafts_path(out: &Path) -> std::path::PathBuf {
    out.join("decompile").join("drafts.yaml")
}

pub fn drafts(out: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(drafts_path(out))
        .ok()
        .and_then(|t| serde_norway::from_str(&t).ok())
        .unwrap_or_default()
}

fn record_draft(out: &Path, doc: &str, hash: &str) {
    let mut d = drafts(out);
    d.insert(doc.to_string(), hash.to_string());
    std::fs::create_dir_all(out.join("decompile")).ok();
    if let Ok(text) = serde_norway::to_string(&d) {
        std::fs::write(drafts_path(out), text).ok();
    }
}

// Documents still carrying their drafted hash: nobody has touched them since the
// machine wrote them. The unratified diagnostic stands until an edit moves the hash.
// Mirrors docs/consumers/decompile.md#ratification.
pub fn unratified(store: &Store) -> Vec<String> {
    drafts(&store.out)
        .iter()
        .filter(|(doc, hash)| {
            store
                .docs
                .get(*doc)
                .map(|rec| &rec.content_hash == *hash)
                .unwrap_or(false)
        })
        .map(|(doc, _)| doc.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Scopes and tasks.

// Group the unclaimed report by top-level directory under the deliverable. A scope is
// the unit one draft task covers. Root-level files gather under ".".
pub fn scopes(proj: &Project, store: &Store, gs: &GenSettings) -> BTreeMap<String, Vec<String>> {
    let mut by_scope: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in crate::bind::unclaimed(proj, store, gs) {
        let scope = f
            .split('/')
            .next()
            .filter(|_| f.contains('/'))
            .unwrap_or(".")
            .to_string();
        by_scope.entry(scope).or_default().push(f);
    }
    by_scope
}

fn scope_released(control: &crate::control::Control, scope: &str) -> bool {
    control
        .released
        .decompile
        .iter()
        .any(|s| s == scope || s == "." || scope.starts_with(&format!("{}/", s)))
}

// Draft tasks, derived from the unclaimed report. Always gated until a decompile
// release names the scope; there is no auto mode.
// Mirrors docs/compiler/reconciler.md#the-task-queue.
pub fn pending(
    proj: &Project,
    store: &Store,
    gs: &GenSettings,
    control: &crate::control::Control,
) -> Vec<Value> {
    scopes(proj, store, gs)
        .into_iter()
        .map(|(scope, files)| {
            let released = scope_released(control, &scope);
            let mut t = json!({
                "kind": "draft-document",
                "scope": scope,
                "unclaimed": files.len(),
                "files": files.iter().take(40).collect::<Vec<_>>(),
                "ready": released,
            });
            if !released {
                t["gated"] = json!(true);
                t["blockedBy"] =
                    json!("awaiting a decompile release (`jazyk decompile` or the GUI)");
            }
            t
        })
        .collect()
}

// The package for one draft task: the inventory slice, the test files with their
// content, and the drafting contract. Mirrors docs/compiler/tools.md#decompilation-tools.
pub fn task(proj: &Project, store: &Store, gs: &GenSettings, scope: &str) -> Result<Value, String> {
    let by_scope = scopes(proj, store, gs);
    let Some(files) = by_scope.get(scope) else {
        return Err(format!(
            "no unclaimed files under scope `{}`; decompile_tasks lists the scopes",
            scope
        ));
    };
    let looks_like_test = |f: &str| {
        let lower = f.to_lowercase();
        lower.contains("test") || lower.contains("spec")
    };
    let mut inventory: Vec<Value> = Vec::new();
    let mut budget: usize = 60_000;
    // Tests first: they are the primary evidence.
    let mut ordered: Vec<&String> = files.iter().collect();
    ordered.sort_by_key(|f| (!looks_like_test(f), (*f).clone()));
    for f in ordered {
        let cap = if looks_like_test(f) { 6_000 } else { 3_000 };
        let content = std::fs::read_to_string(gs.deliverable.join(f)).unwrap_or_default();
        let shown = crate::llm::truncate(&content, cap.min(budget));
        budget = budget.saturating_sub(shown.len());
        inventory.push(json!({
            "path": f,
            "test": looks_like_test(f),
            "content": if budget == 0 { Value::Null } else { json!(shown) },
        }));
        if budget == 0 {
            break;
        }
    }
    let slug = if scope == "." {
        "root".to_string()
    } else {
        scope.replace('/', "-")
    };
    let mut lint: Vec<String> = proj.linting.warnings.clone();
    lint.extend(proj.linting.errors.iter().cloned());
    Ok(json!({
        "scope": scope,
        "deliverable": gs.deliverable.to_string_lossy(),
        "unclaimed": files,
        "inventory": inventory,
        "lintRules": lint,
        "suggestedPath": format!("docs/{}.md", slug),
        "instructions": instructions(),
    }))
}

// Validate and land a draft in the docs tree, record its hash for ratification, and
// consume the scope's release. The compiler picks the file up like any hand-written
// document. Mirrors docs/consumers/decompile.md#drafts-land-in-the-docs-tree.
pub fn submit(
    proj: &Project,
    out: &Path,
    path: &str,
    content: &str,
    scope: Option<&str>,
) -> Result<Value, String> {
    let path = path.trim().trim_start_matches("./");
    if path.is_empty() || path.contains("..") {
        return Err("path must be a project-relative documentation path".into());
    }
    let content = content.trim_end();
    if content.is_empty() {
        return Err("content is empty; a draft states what the code does".into());
    }
    if !content.lines().any(|l| l.starts_with("# ")) {
        return Err("a draft is a document: it needs one H1 heading".into());
    }
    if content.contains('\u{2014}') {
        return Err("the draft contains an em dash; the documentation voice uses commas, periods, parentheses, or colons".into());
    }
    // The draft must be compiler input, or it lands invisible: the last docs glob
    // pattern to match must be an inclusion.
    let mut included = false;
    for p in &proj.docs_glob {
        if let Some(neg) = p.strip_prefix('!') {
            if crate::project::glob_match(neg, path) {
                included = false;
            }
        } else if crate::project::glob_match(p, path) {
            included = true;
        }
    }
    if !included {
        return Err(format!(
            "`{}` does not match the docs glob {:?}; the compiler would never read it",
            path, proj.docs_glob
        ));
    }
    let abs = proj.root.join(path);
    if let Some(dir) = abs.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let mut text = content.to_string();
    text.push('\n');
    std::fs::write(&abs, &text).map_err(|e| format!("cannot write `{}`: {}", path, e))?;
    record_draft(out, path, &hash_hex(&text));
    if let Some(s) = scope {
        crate::control::consume_decompile(proj, out, s);
    }
    Ok(json!({
        "written": path,
        "note": "the draft is compiler input now: compile extracts its statements, binding self-checks them against the code, and the document carries an unratified diagnostic until a human edit moves its hash",
    }))
}

// The built-in draft worker: one scope per LLM reply, the file-reply protocol the
// pipeline generation worker uses. An attached agent over `jazyk mcp decompile` is
// the better worker; this one keeps the command usable without one.
// Mirrors docs/consumers/decompile.md#draft-tasks.
pub fn run_all(
    proj: &Project,
    store: &Store,
    runner: &crate::acp::runner::AcpRunner,
    gs: &GenSettings,
    scopes_wanted: &[String],
    trace: &crate::turn::Trace,
) -> Result<Value, String> {
    let control = crate::control::Control::load(proj, &store.out);
    let all = pending(proj, store, gs, &control);
    let targets: Vec<String> = all
        .iter()
        .filter(|t| t["gated"] != true)
        .filter(|t| {
            scopes_wanted.is_empty() || scopes_wanted.iter().any(|s| t["scope"] == s.as_str())
        })
        .filter_map(|t| t["scope"].as_str().map(String::from))
        .collect();
    if targets.is_empty() {
        return Ok(json!({"drafted": 0, "note": "no released scopes with unclaimed files"}));
    }
    let (mut drafted, mut failures) = (0u64, 0u64);
    for scope in &targets {
        if trace.is_cancelled() {
            break;
        }
        trace.line("decompile", &format!("drafting scope {}", scope));
        let pkg = task(proj, store, gs, scope)?;
        let system = format!(
            "You are the decompilation worker of jazyk, a natural language compiler. {} \
             Reply with exactly one file: a line `FILE: <docs-relative path>` (use the suggested path unless a better name exists), then the full markdown content.",
            instructions()
        );
        let user = format!(
            "# Scope: {}\nsuggested path: {}\nlint rules: {}\n\n## Inventory (tests first)\n{}\n",
            scope,
            pkg["suggestedPath"].as_str().unwrap_or(""),
            pkg["lintRules"],
            pkg["inventory"]
        );
        let mut last_err = String::new();
        let mut ok = false;
        for attempt in 0..2 {
            let prompt = if attempt == 0 || last_err.is_empty() {
                user.clone()
            } else {
                format!(
                    "{}\n\nYour previous draft was rejected: {}\nFix exactly that and resubmit.",
                    user, last_err
                )
            };
            let reply = match runner.ask_traced(
                &system,
                &prompt,
                &format!("decompile {}", scope),
                if attempt == 0 { "draft" } else { "draft retry" },
                Some(trace),
            ) {
                Ok(r) => r,
                Err(e) => {
                    last_err = e;
                    continue;
                }
            };
            let (path, content) = match crate::gen::parse_file_reply(&reply) {
                Ok(pc) => pc,
                Err(e) => {
                    last_err = e;
                    continue;
                }
            };
            match submit(proj, &store.out, &path, &content, Some(scope)) {
                Ok(v) => {
                    trace.line(
                        "decompile",
                        &format!("drafted {}", v["written"].as_str().unwrap_or("")),
                    );
                    ok = true;
                    break;
                }
                Err(e) => last_err = e,
            }
        }
        if ok {
            drafted += 1;
        } else {
            trace.line(
                "decompile",
                &format!("scope {} failed: {}", scope, last_err),
            );
            failures += 1;
        }
    }
    Ok(json!({"drafted": drafted, "failures": failures}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> (Project, Store, GenSettings) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("jazyk-decompile-test-{}-{}", std::process::id(), n));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(&root).unwrap();
        let mut proj = Project::default();
        proj.root = root.clone();
        proj.out = root.join("jazyk-out");
        let store = Store {
            out: proj.out.clone(),
            ..Default::default()
        };
        let gs = GenSettings {
            deliverable: root.clone(),
            worker: "agentic".into(),
            code: Vec::new(),
        };
        (proj, store, gs)
    }

    #[test]
    fn draft_tasks_gate_until_released() {
        let (proj, store, gs) = tmp();
        std::fs::create_dir_all(proj.root.join("src")).unwrap();
        std::fs::write(proj.root.join("src/lib.sh"), "echo hi\n").unwrap();
        let control = crate::control::Control::load(&proj, &store.out);
        let p = pending(&proj, &store, &gs, &control);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0]["scope"], "src");
        assert_eq!(p[0]["gated"], true);
        crate::control::release_decompile(&proj, &store.out, &["src".into()]);
        let control = crate::control::Control::load(&proj, &store.out);
        let p = pending(&proj, &store, &gs, &control);
        assert_eq!(p[0]["gated"], serde_json::Value::Null);
    }

    #[test]
    fn a_submitted_draft_lands_and_consumes_the_release() {
        let (proj, store, gs) = tmp();
        std::fs::create_dir_all(proj.root.join("src")).unwrap();
        std::fs::write(proj.root.join("src/lib.sh"), "echo hi\n").unwrap();
        crate::control::release_decompile(&proj, &store.out, &["src".into()]);
        let v = submit(
            &proj,
            &store.out,
            "docs/src.md",
            "# Src\n\nThe `lib.sh` script prints hi (observed: none, inferred: `src/lib.sh`).",
            Some("src"),
        )
        .unwrap();
        assert_eq!(v["written"], "docs/src.md");
        assert!(proj.root.join("docs/src.md").exists());
        assert_eq!(drafts(&store.out).len(), 1);
        let control = crate::control::Control::load(&proj, &store.out);
        assert!(control.released.decompile.is_empty());
        let _ = gs;
    }

    #[test]
    fn a_draft_outside_the_docs_glob_is_rejected() {
        let (proj, store, _gs) = tmp();
        assert!(submit(&proj, &store.out, "notes/x.md", "# X\n\nBody.", None).is_err());
    }

    #[test]
    fn an_em_dash_is_rejected() {
        let (proj, store, _gs) = tmp();
        assert!(submit(
            &proj,
            &store.out,
            "docs/x.md",
            "# X\n\nBody \u{2014} with a dash.",
            None
        )
        .is_err());
    }
}
