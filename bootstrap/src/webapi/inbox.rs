// The Inbox, served by both transports (docs/frontends/wasm.md#the-inbox): the
// rollup the top bar, the navigator, and the Inbox's pages read, the standing
// questions, the human triage decision, and an answer to a prompted diagnostic.
// Mirrors docs/frontends/gui.md#inbox.
use crate::model::Diagnostic;
use crate::project::Project;
use crate::store::{Op, Store};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

const PROPOSAL_RULES: [&str; 2] = ["ratification-pending", "invented-choice"];
const DRAFT_RULE: &str = "unratified";

// The one classification every Inbox surface reads: the section an open diagnostic
// lists under, or none when it lists nowhere (resolved, or hidden).
pub fn section_of(d: &Diagnostic) -> Option<&'static str> {
    if d.lifecycle != "open" || d.triage.as_deref() == Some("suppressed") {
        return None;
    }
    if d.rule == DRAFT_RULE {
        return Some("drafts");
    }
    // Absent, handling, or failed: the question still stands. An answer that
    // landed (applied, handled) on a finding that still stands leaves a problem.
    let asking = d
        .answer
        .as_ref()
        .is_none_or(|a| a.status == "handling" || a.status == "failed");
    if PROPOSAL_RULES.contains(&d.rule.as_str()) && asking {
        return Some("proposals");
    }
    if d.prompt.is_some() && asking {
        return Some("questions");
    }
    Some("problems")
}

// Whether a listed question or proposal waits on a person: a `handling` answer is
// listed with its progress and not counted.
fn waiting(d: &Diagnostic) -> bool {
    d.answer.as_ref().is_none_or(|a| a.status == "failed")
}

pub fn inbox_value(proj: &Project, gs: &crate::gen::GenSettings, out: &Path, store: &Store) -> Value {
    let mut questions = 0usize;
    let mut proposals = 0usize;
    let mut handling = 0usize;
    let mut problems: BTreeMap<String, usize> = BTreeMap::new();
    let mut sections: BTreeMap<&str, Vec<&str>> = ["questions", "proposals", "problems", "drafts"]
        .into_iter()
        .map(|s| (s, Vec::new()))
        .collect();
    for (id, d) in &store.graph.diagnostics {
        let Some(section) = section_of(d) else {
            continue;
        };
        sections.entry(section).or_default().push(id);
        match section {
            "questions" | "proposals" => {
                if !waiting(d) {
                    handling += 1;
                } else if section == "questions" {
                    questions += 1;
                } else {
                    proposals += 1;
                }
            }
            "problems" => *problems.entry(d.severity.clone()).or_default() += 1,
            _ => {}
        }
    }
    let drafts = sections["drafts"].len();
    let control = crate::control::Control::load(proj, out);
    let mut pending_docs: Vec<String> = Vec::new();
    if control.compile == "manual" {
        for (doc, hash) in crate::control::current_doc_hashes(proj) {
            if control.released.compile.get(&doc) != Some(&hash) {
                pending_docs.push(doc);
            }
        }
    }
    let mut pending_entities = 0usize;
    if control.generate == "manual" && control.released.generate != store.status.generation {
        pending_entities = crate::gen::pending(store, gs).len();
    }
    let errors = problems.get("error").copied().unwrap_or(0);
    json!({
        "generation": store.status.generation,
        "questions": questions,
        "proposals": proposals,
        "handling": handling,
        "problems": problems,
        "pending": { "docs": pending_docs, "entities": pending_entities },
        "drafts": drafts,
        "sections": sections,
        "modes": { "compile": control.compile, "generate": control.generate },
        "total": questions + proposals + pending_docs.len() + errors,
    })
}

// The standing questions: open, unsuppressed, prompted diagnostics, each with any
// recorded answer. Mirrors docs/frontends/gui.md#api (diagnostics).
pub fn questions_value(store: &Store) -> Value {
    let items: Vec<Value> = store
        .graph
        .diagnostics
        .iter()
        .filter(|(_, d)| {
            d.lifecycle == "open" && d.triage.as_deref() != Some("suppressed") && d.prompt.is_some()
        })
        .map(|(id, d)| {
            json!({
                "id": id, "rule": d.rule, "severity": d.severity, "message": d.message,
                "subjects": d.subjects, "prompt": d.prompt, "answer": d.answer,
            })
        })
        .collect();
    json!({ "questions": items })
}

// The human triage decision on a diagnostic, committed through the store as a
// journaled `triage` changeset. The compiler never overwrites human triage.
pub fn triage_value(out: &Path, id: &str, body: &Value) -> Result<Value, super::Refusal> {
    let triage = match &body["triage"] {
        Value::Null => None,
        Value::String(s) if ["acknowledged", "suppressed", "wontfix"].contains(&s.as_str()) => {
            Some(s.clone())
        }
        _ => {
            return Err((
                400,
                "triage must be acknowledged, suppressed, wontfix, or null".to_string(),
            ))
        }
    };
    let mut store = Store::load(out);
    let rid = store.resolve_id(id).to_string();
    if !store.graph.diagnostics.contains_key(&rid) {
        return Err((404, format!("no diagnostic {}", rid)));
    }
    let item = crate::model::WorkItem {
        task: "triage".into(),
        target: rid.clone(),
        dirty_sections: vec![],
        stale_anchors: vec![],
        proposals: vec![],
    };
    let report = store.apply(
        vec![Op::TriageDiagnostic {
            id: rid.clone(),
            triage,
        }],
        &item.commit(0, None),
    );
    if !report.skipped.is_empty() {
        return Err((409, report.skipped.join("; ")));
    }
    Ok(json!({ "id": rid, "diagnostic": store.graph.diagnostics.get(&rid) }))
}

// An answer to a prompted diagnostic: `{option}` (an index) or `{text}` (a freeform
// reply), through the engine every frontend calls. An edit option applies and
// resolves in the reply; any other answer records `handling`, and the transport
// decides what acts on it (a spawned answer session on a machine, the next build in
// a tab). Mirrors docs/compiler/model/diagnostic.md#answers.
pub fn answer_value(proj: &Project, out: &Path, id: &str, body: &Value) -> Result<Value, super::Refusal> {
    let reply = if let Some(i) = body["option"].as_u64() {
        crate::answer::Reply::Choice(i as usize)
    } else if let Some(t) = body["text"].as_str() {
        crate::answer::Reply::Text(t.to_string())
    } else {
        return Err((400, "pass option (an index) or text (a freeform reply)".to_string()));
    };
    crate::answer::answer(proj, out, id, reply, None).map_err(|e| (400, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DiagnosticAnswer, DiagnosticPrompt, PromptOption, SuggestedEdit};
    use crate::store::Commit;

    // A project with one document and three open diagnostics: a question with an
    // edit option and freeform, a plain problem, and a proposal.
    fn fixture(name: &str) -> (Project, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("jazyk-webapi-inbox-{}-{}", name, crate::sys::process_id()));
        crate::vfs::remove_dir_all(&root).ok();
        crate::vfs::create_dir_all(root.join("docs")).unwrap();
        crate::vfs::write({ let path = root.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path }, "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n").unwrap();
        crate::vfs::write(
            root.join("docs/arc.md"),
            "# Arc\n\nTheo is a rival at the midpoint.\n",
        )
        .unwrap();
        let proj = Project::load(&root);
        let out = proj.out.clone();
        let (parsed, _) = crate::reconcile::parse_all(&proj);
        let mut s = Store::load(&out);
        s.sync_docs(&parsed);
        let sec = s.docs["docs/arc.md"].sections.keys().next().unwrap().clone();
        let diag = |rule: &str, severity: &str, prompt: Option<DiagnosticPrompt>| Diagnostic {
            rule: rule.into(),
            severity: severity.into(),
            subjects: vec![format!("docs/arc.md#{}", sec)],
            message: format!("a {} finding", rule),
            reasoning: None,
            lifecycle: "open".into(),
            triage: None,
            prompt,
            answer: None,
            created: None,
            updated: None,
        };
        let question = DiagnosticPrompt {
            question: "Rival or ally?".into(),
            options: vec![
                PromptOption {
                    label: "Make it ally".into(),
                    edit: Some(SuggestedEdit {
                        doc: "docs/arc.md".into(),
                        section: sec.clone(),
                        old_text: "a rival".into(),
                        new_text: "an ally".into(),
                        facts: Vec::new(),
                    }),
                    answer: None,
                },
                PromptOption {
                    label: "Keep both".into(),
                    edit: None,
                    answer: Some("The two readings stand on purpose.".into()),
                },
            ],
            freeform: true,
        };
        let report = s.apply(
            vec![
                Op::ReportDiagnostic {
                    id: "diag:contradiction-1".into(),
                    diagnostic: diag("contradiction", "error", Some(question)),
                },
                Op::ReportDiagnostic {
                    id: "diag:gap-1".into(),
                    diagnostic: diag("gap", "warning", None),
                },
                Op::ReportDiagnostic {
                    id: "diag:invented-choice-1".into(),
                    diagnostic: diag(
                        "invented-choice",
                        "info",
                        Some(DiagnosticPrompt {
                            question: "State it?".into(),
                            options: vec![],
                            freeform: true,
                        }),
                    ),
                },
            ],
            &Commit::store("session"),
        );
        assert!(report.skipped.is_empty(), "seed: {:?}", report.skipped);
        (proj, out)
    }

    fn rollup(proj: &Project, out: &Path) -> Value {
        let store = Store::load(out);
        inbox_value(proj, &crate::gen::GenSettings::resolve(proj), out, &store)
    }

    #[test]
    fn the_rollup_counts_match_the_sections_it_lists() {
        let (proj, out) = fixture("rollup");
        let v = rollup(&proj, &out);
        assert_eq!(v["questions"], 1);
        assert_eq!(v["proposals"], 1);
        assert_eq!(v["problems"]["warning"], 1);
        assert_eq!(v["sections"]["questions"], json!(["diag:contradiction-1"]));
        assert_eq!(v["sections"]["proposals"], json!(["diag:invented-choice-1"]));
        assert_eq!(v["sections"]["problems"], json!(["diag:gap-1"]));
        assert_eq!(v["sections"]["drafts"], json!([]));
        // Questions and proposals, no pending documents (compile is auto by
        // default), no errors among the problems.
        assert_eq!(v["total"], 2 + v["pending"]["docs"].as_array().unwrap().len());
    }

    #[test]
    fn triage_commits_and_a_hidden_problem_leaves_the_rollup() {
        let (proj, out) = fixture("triage");
        let v = triage_value(&out, "diag:gap-1", &json!({"triage": "acknowledged"})).unwrap();
        assert_eq!(v["diagnostic"]["triage"], "acknowledged");
        assert_eq!(rollup(&proj, &out)["sections"]["problems"], json!(["diag:gap-1"]));
        triage_value(&out, "diag:gap-1", &json!({"triage": "suppressed"})).unwrap();
        assert_eq!(rollup(&proj, &out)["sections"]["problems"], json!([]));
        // Undo clears it.
        let v = triage_value(&out, "diag:gap-1", &json!({"triage": null})).unwrap();
        assert!(v["diagnostic"]["triage"].is_null());
        // Refusals name the rule.
        assert_eq!(triage_value(&out, "diag:gap-1", &json!({"triage": "maybe"})).unwrap_err().0, 400);
        assert_eq!(triage_value(&out, "diag:nope-1", &json!({"triage": null})).unwrap_err().0, 404);
    }

    #[test]
    fn an_edit_option_resolves_and_leaves_the_list() {
        let (proj, out) = fixture("edit");
        let v = answer_value(&proj, &out, "diag:contradiction-1", &json!({"option": 0})).unwrap();
        assert_eq!(v["status"], "applied");
        // The reply names the text that landed, for the editor's mark.
        assert_eq!(v["edit"]["doc"], "docs/arc.md");
        assert_eq!(v["edit"]["newText"], "an ally");
        let text = crate::vfs::read_to_string(proj.root.join("docs/arc.md")).unwrap();
        assert!(text.contains("Theo is an ally"), "{}", text);
        let r = rollup(&proj, &out);
        assert_eq!(r["questions"], 0);
        assert_eq!(r["sections"]["questions"], json!([]));
        assert!(!questions_value(&Store::load(&out))["questions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|q| q["id"] == "diag:contradiction-1"));
    }

    // A dual write moves the prose and the graph in one commit: in manual mode the
    // released document stays released, and no reconciliation derives for it
    // (docs/compiler/control-plane.md#modes-and-releases).
    #[test]
    fn an_edit_option_leaves_nothing_pending_and_nothing_to_reread() {
        let (proj, out) = fixture("pending");
        crate::control::release(&proj, &out, Some("compile"));
        assert_eq!(rollup(&proj, &out)["pending"]["docs"], json!([]));
        // The fixture never read its section, so the reading it owes stands; the edit
        // adds none.
        let reread = || -> Vec<String> {
            crate::board::Board::compute(&proj, &out)
                .goals
                .iter()
                .filter(|g| matches!(g.kind.as_str(), "reconcile-section" | "place-anchors"))
                .map(|g| g.id.clone())
                .collect()
        };
        let before = reread();
        answer_value(&proj, &out, "diag:contradiction-1", &json!({"option": 0})).unwrap();
        assert_eq!(rollup(&proj, &out)["pending"]["docs"], json!([]));
        assert_eq!(reread(), before);
        // A human edit after it is pending again, as any edit is.
        crate::vfs::write(proj.root.join("docs/arc.md"), "# Arc\n\nTheo is an ally now.\n").unwrap();
        assert_eq!(rollup(&proj, &out)["pending"]["docs"], json!(["docs/arc.md"]));
    }

    // A document that waited for a release before the write still waits after it.
    #[test]
    fn an_unreleased_document_stays_pending_after_an_edit_option() {
        let (proj, out) = fixture("unreleased");
        crate::control::release(&proj, &out, Some("compile"));
        crate::vfs::write(
            proj.root.join("docs/arc.md"),
            "# Arc\n\nTheo is a rival at the midpoint. He sulks.\n",
        )
        .unwrap();
        answer_value(&proj, &out, "diag:contradiction-1", &json!({"option": 0})).unwrap();
        assert_eq!(rollup(&proj, &out)["pending"]["docs"], json!(["docs/arc.md"]));
    }

    #[test]
    fn a_freeform_answer_records_handling_and_stays_listed_uncounted() {
        let (proj, out) = fixture("free");
        let v = answer_value(&proj, &out, "diag:contradiction-1", &json!({"text": "  keep the rival  "})).unwrap();
        assert_eq!(v["status"], "handling");
        let d = Store::load(&out).graph.diagnostics["diag:contradiction-1"].clone();
        assert_eq!(
            d.answer,
            Some(DiagnosticAnswer {
                choice: None,
                text: "keep the rival".into(),
                status: "handling".into(),
                error: None,
            })
        );
        let r = rollup(&proj, &out);
        assert_eq!(r["questions"], 0);
        assert_eq!(r["handling"], 1);
        assert_eq!(r["sections"]["questions"], json!(["diag:contradiction-1"]));
        assert_eq!(crate::answer::handling_ids(&out), vec!["diag:contradiction-1".to_string()]);
        // A second answer is refused while the first stands.
        let e = answer_value(&proj, &out, "diag:contradiction-1", &json!({"option": 1})).unwrap_err();
        assert_eq!(e.0, 400);
        assert!(e.1.contains("already has an answer"), "{}", e.1);
        // Neither an option nor text is refused.
        assert_eq!(answer_value(&proj, &out, "diag:contradiction-1", &json!({})).unwrap_err().0, 400);
    }

    #[test]
    fn a_recorded_outcome_moves_the_answer_on() {
        let (proj, out) = fixture("outcome");
        answer_value(&proj, &out, "diag:contradiction-1", &json!({"option": 1})).unwrap();
        crate::answer::record_outcome(&out, "diag:contradiction-1", &Err("no model".into()));
        let d = Store::load(&out).graph.diagnostics["diag:contradiction-1"].clone();
        let a = d.answer.unwrap();
        assert_eq!(a.status, "failed");
        // The card says why.
        assert_eq!(a.error.as_deref(), Some("no model"));
        // A failed answer waits on a person again, and nothing is left to handle.
        let r = rollup(&proj, &out);
        assert_eq!(r["questions"], 1);
        assert!(crate::answer::handling_ids(&out).is_empty());
    }
}
