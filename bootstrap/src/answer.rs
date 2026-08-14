// Answering a diagnostic prompt: the one engine every frontend calls (LSP code
// actions, the GUI questions panel, answer_diagnostic in chat sessions).
// An edit option is deterministic: dual write plus resolve in one changeset. Any
// other reply records `handling` and a model acts on it over ACP.
// Mirrors docs/compiler/model/diagnostic.md#answers and
// docs/frontends/acp.md#answer-sessions.

use crate::model::{DiagnosticAnswer, WorkItem};
use crate::project::Project;
use crate::store::{Op, Store};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub enum Reply {
    Choice(usize),
    Text(String),
}

// A frontend that delegates file writes (the chat serving's edit sink) passes one;
// everyone else writes disk directly. Arguments: doc (relative), old_text,
// new_text, full new document text.
pub type WriteEdit<'a> = &'a dyn Fn(&str, &str, &str, &str) -> Result<(), String>;

pub fn answer(
    project: &Project,
    out: &Path,
    id: &str,
    reply: Reply,
    write: Option<WriteEdit>,
) -> Result<Value, String> {
    let store = Store::load(out);
    let rid = store.resolve_id(id).to_string();
    let Some(d) = store.graph.diagnostics.get(&rid) else {
        return Err(format!("unknown diagnostic `{}`", rid));
    };
    if d.lifecycle != "open" {
        return Err(format!("`{}` is already resolved; nothing to answer", rid));
    }
    if let Some(a) = &d.answer {
        // A failed handling attempt may be retried; anything else is final.
        if a.status != "failed" {
            return Err(format!("`{}` already has an answer ({})", rid, a.status));
        }
    }
    let prompt = d.prompt.clone();
    let (choice, text, edit) = match reply {
        Reply::Choice(i) => {
            let Some(p) = &prompt else {
                return Err(format!("`{}` carries no prompt; there is nothing to choose", rid));
            };
            let Some(opt) = p.options.get(i) else {
                return Err(format!("option {} does not exist; the prompt has {} option(s)", i, p.options.len()));
            };
            let text = opt.answer.clone().unwrap_or_else(|| opt.label.clone());
            (Some(i), text, opt.edit.clone())
        }
        Reply::Text(t) => {
            let t = t.trim().to_string();
            if t.is_empty() {
                return Err("an empty reply answers nothing".to_string());
            }
            (None, t, None)
        }
    };
    let label = choice
        .and_then(|i| prompt.as_ref().and_then(|p| p.options.get(i)).map(|o| o.label.clone()))
        .unwrap_or_default();
    drop(store);

    match edit {
        Some(e) => {
            let path = project.root.join(&e.doc);
            let old_full =
                std::fs::read_to_string(&path).map_err(|err| format!("read {}: {}", path.display(), err))?;
            let Some((b, end)) = crate::md::locate_bytes(&old_full, &e.old_text) else {
                return Err(format!(
                    "the suggested edit is stale: its old text no longer locates in {}; update_diagnostic can re-author it",
                    e.doc
                ));
            };
            let full = format!("{}{}{}", &old_full[..b], e.new_text, &old_full[end..]);
            match write {
                Some(w) => w(&e.doc, &e.old_text, &e.new_text, &full)?,
                None => std::fs::write(&path, &full).map_err(|err| format!("write {}: {}", path.display(), err))?,
            }
            let (parsed, _) = crate::reconcile::parse_all(project);
            let mut s = Store::load(out);
            s.sync_docs(&parsed);
            s.absorb_doc_edit(&e.doc, &full);
            let ops = vec![
                Op::EditDocProse {
                    doc: e.doc.clone(),
                    section: e.section.clone(),
                    old_text: e.old_text.clone(),
                    new_text: e.new_text.clone(),
                    text: full.clone(),
                },
                Op::AnswerDiagnostic {
                    id: rid.clone(),
                    answer: DiagnosticAnswer { choice, text: text.clone(), status: "applied".to_string() },
                },
                Op::ResolveDiagnostic { id: rid.clone(), reason: format!("suggested edit applied: {}", label) },
            ];
            let report = s.apply(ops, &work_item(&rid), 0, 0);
            if !report.skipped.is_empty() {
                // The graph side skipped: put the prose back so neither moved.
                let _ = std::fs::write(&path, &old_full);
                return Err(format!("commit skipped: {}", report.skipped.join("; ")));
            }
            Ok(json!({
                "status": "applied", "resolved": true, "doc": e.doc,
                "note": "the edit landed as a dual write; no recompile is owed for it"
            }))
        }
        None => {
            let mut s = Store::load(out);
            let report = s.apply(
                vec![Op::AnswerDiagnostic {
                    id: rid.clone(),
                    answer: DiagnosticAnswer { choice, text: text.clone(), status: "handling".to_string() },
                }],
                &work_item(&rid),
                0,
                0,
            );
            if !report.skipped.is_empty() {
                return Err(report.skipped.join("; "));
            }
            Ok(json!({"status": "handling", "id": rid, "reply": text}))
        }
    }
}

// The contract handed to whichever agent acts on a non-edit answer: in a chat
// session it returns from answer_diagnostic; for sessionless frontends it becomes
// an answer session's prompt.
pub fn handling_prompt(out: &Path, id: &str) -> Result<String, String> {
    let store = Store::load(out);
    let rid = store.resolve_id(id).to_string();
    let Some(d) = store.graph.diagnostics.get(&rid) else {
        return Err(format!("unknown diagnostic `{}`", rid));
    };
    let mut s = String::new();
    s.push_str(&format!(
        "A human answered a standing question on diagnostic {} ({}, {}).\n\nFinding: {}\n",
        rid, d.rule, d.severity, d.message
    ));
    if let Some(p) = &d.prompt {
        s.push_str(&format!("Question: {}\n", p.question));
    }
    if let Some(a) = &d.answer {
        s.push_str(&format!("The human's answer: \"{}\"\n", a.text));
    }
    s.push_str("\nSubjects:\n");
    for sub in &d.subjects {
        let sid = store.resolve_id(sub).to_string();
        if let Some(r) = store.graph.requirements.get(&sid) {
            s.push_str(&format!(
                "- {}: {} (quoted from {}#{}: \"{}\")\n",
                sid, r.ears, r.source.doc, r.source.section, r.source.quote
            ));
        } else if let Some(e) = store.graph.entities.get(&sid) {
            s.push_str(&format!("- {} ({}): {}\n", sid, e.name, e.definition.as_deref().unwrap_or("")));
        } else {
            s.push_str(&format!("- {}\n", sub));
        }
    }
    s.push_str(&format!(
        "\nAct on the answer with the tools: revise_requirement moves a requirement's prose and graph \
         form together, update_requirement and update_entity record decisions, update_diagnostic \
         refines the question when the answer leaves it open. When the finding is settled, call \
         resolve_diagnostic with id `{}` and a reason. Finish with a one-line summary.",
        rid
    ));
    Ok(s)
}

// Frontends with no live session (LSP, GUI) hand a `handling` answer to one focused
// background session; `answer.status` moves to handled or failed when it lands, so
// every frontend shows the same progress from the store.
pub fn spawn_handler(project: Project, out: PathBuf, id: String) {
    std::thread::spawn(move || {
        let outcome = handle(&project, &out, &id);
        let status = match &outcome {
            Ok(()) => "handled",
            Err(e) => {
                eprintln!("jazyk: answer session for {} failed: {}", id, e);
                "failed"
            }
        };
        let mut s = Store::load(&out);
        let rid = s.resolve_id(&id).to_string();
        if let Some(a) = s.graph.diagnostics.get(&rid).and_then(|d| d.answer.clone()) {
            let mut a = a;
            a.status = status.to_string();
            let _ = s.apply(vec![Op::AnswerDiagnostic { id: rid.clone(), answer: a }], &work_item(&rid), 0, 0);
        }
    });
}

fn handle(project: &Project, out: &Path, id: &str) -> Result<(), String> {
    let llm = crate::cli::resolve_llm(
        &crate::cli::Options::default(),
        &project.llm,
        &crate::project::load_global_llm(),
        |n| std::env::var(n).ok(),
    );
    let runner = crate::acp::runner::AcpRunner::start(project, &llm, out)?;
    let prompt = handling_prompt(out, id)?;
    runner.run_answer(&prompt, &format!("answer {}", id))
}

fn work_item(target: &str) -> WorkItem {
    WorkItem {
        task: "answer-diagnostic".to_string(),
        target: target.to_string(),
        dirty_sections: Vec::new(),
        stale_anchors: Vec::new(),
    }
}
