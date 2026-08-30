// Answering a diagnostic prompt: the one engine every frontend calls (LSP code
// actions, the GUI questions panel, answer_diagnostic in chat sessions).
// An edit option is deterministic: dual write plus resolve in one changeset. Any
// other reply records `handling` and a model acts on it over ACP.
// Mirrors docs/compiler/model/diagnostic.md#answers and
// docs/frontends/acp.md#answer-sessions.

use crate::model::{DiagnosticAnswer, Provenance, SourceRef};
use crate::project::Project;
use crate::store::{Commit, Op, ProseEdit, Store, WriteEdit};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub enum Reply {
    Choice(usize),
    Text(String),
}

// The rule of a ratification proposal. Mirrors docs/compiler/model/diagnostic.md#ratification-proposals.
const RATIFICATION: &str = "ratification-pending";

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
    let ratify = d.rule == RATIFICATION;
    let subjects = d.subjects.clone();
    let prompt = d.prompt.clone();
    let mut reworded = false;
    let (choice, text, edit, retract) = match reply {
        Reply::Choice(i) => {
            let Some(p) = &prompt else {
                return Err(format!(
                    "`{}` carries no prompt; there is nothing to choose",
                    rid
                ));
            };
            let Some(opt) = p.options.get(i) else {
                return Err(format!(
                    "option {} does not exist; the prompt has {} option(s)",
                    i,
                    p.options.len()
                ));
            };
            let text = opt.answer.clone().unwrap_or_else(|| opt.label.clone());
            let retract = ratify && opt.answer.as_deref() == Some("retract");
            (Some(i), text, opt.edit.clone(), retract)
        }
        Reply::Text(t) => {
            let t = t.trim().to_string();
            if t.is_empty() {
                return Err("an empty reply answers nothing".to_string());
            }
            // On a ratification proposal a freeform reply that rewords the sentence
            // is the accepted sentence: it lands where the proposal's edit would.
            let edit = if ratify {
                prompt
                    .as_ref()
                    .and_then(|p| p.options.iter().find_map(|o| o.edit.clone()))
                    .map(|mut e| {
                        reworded = true;
                        e.new_text = t.clone();
                        e
                    })
            } else {
                None
            };
            (None, t, edit, false)
        }
    };
    let label = choice
        .and_then(|i| {
            prompt
                .as_ref()
                .and_then(|p| p.options.get(i))
                .map(|o| o.label.clone())
        })
        .unwrap_or_default();
    drop(store);

    if let Some(e) = edit {
        let path = project.root.join(&e.doc);
        let old_full = std::fs::read_to_string(&path)
            .map_err(|err| format!("read {}: {}", path.display(), err))?;
        let (parsed, _) = crate::reconcile::parse_all(project);
        // The search is scoped to the option's own section, where the gate validated
        // it: the same phrase elsewhere in the document must not catch the edit.
        let section_raw = parsed
            .get(&e.doc)
            .and_then(|(_, secs)| secs.get(&e.section))
            .map(|sec| sec.raw.clone());
        let prose = ProseEdit::locate(
            &e.doc,
            &e.section,
            section_raw.as_deref(),
            &old_full,
            &e.old_text,
            &e.new_text,
        )
        .map_err(|_| {
            format!(
                "the suggested edit is stale: its old text no longer locates in {}#{}; update_diagnostic can re-author it",
                e.doc, e.section
            )
        })?;
        let mut s = Store::load(out);
        s.sync_docs(&parsed);
        let mut ops = Vec::new();
        if ratify {
            // Accepting the proposal flips the fact's provenance to the landed
            // sentence in the same changeset; the store resolves the proposal.
            // Mirrors docs/compiler/goals/ratify.md#accept.
            let source = SourceRef {
                doc: e.doc.clone(),
                section: e.section.clone(),
                quote: prose.new_text.trim().to_string(),
            };
            for sub in &subjects {
                ops.extend(ratify_ops(&s, &rid, sub, &source, reworded));
            }
        }
        ops.push(Op::AnswerDiagnostic {
            id: rid.clone(),
            answer: DiagnosticAnswer {
                choice,
                text: text.clone(),
                status: "applied".to_string(),
            },
        });
        if !ratify {
            ops.push(Op::ResolveDiagnostic {
                id: rid.clone(),
                reason: format!("suggested edit applied: {}", label),
            });
        }
        let kind = if ratify { "ratify" } else { "answer" };
        s.dual_write(&project.root, &prose, ops, &Commit::store(kind), write)
            .map_err(|e| format!("commit skipped: {}", e))?;
        return Ok(json!({
            "status": "applied", "resolved": true, "doc": e.doc,
            "note": if ratify {
                "the sentence landed and the fact is quoted from it; no recompile is owed for it"
            } else {
                "the edit landed as a dual write; no recompile is owed for it"
            }
        }));
    }

    if retract {
        // Retracting is deterministic: the decree (or derivation) is undone and the
        // proposal resolves with it. Mirrors docs/compiler/goals/ratify.md#retract.
        let mut s = Store::load(out);
        let mut ops = Vec::new();
        for sub in &subjects {
            ops.extend(retract_ops(&s, &rid, sub));
        }
        ops.push(Op::AnswerDiagnostic {
            id: rid.clone(),
            answer: DiagnosticAnswer {
                choice,
                text: text.clone(),
                status: "applied".to_string(),
            },
        });
        let report = s.apply(ops, &Commit::store("ratify"));
        if !report.skipped.is_empty() {
            return Err(report.skipped.join("; "));
        }
        return Ok(json!({
            "status": "applied", "resolved": true, "retracted": subjects,
            "note": "the decree is withdrawn; the proposal resolved with it"
        }));
    }

    let mut s = Store::load(out);
    let report = s.apply(
        vec![Op::AnswerDiagnostic {
            id: rid.clone(),
            answer: DiagnosticAnswer {
                choice,
                text: text.clone(),
                status: "handling".to_string(),
            },
        }],
        &Commit::store("answer"),
    );
    if !report.skipped.is_empty() {
        return Err(report.skipped.join("; "));
    }
    Ok(json!({"status": "handling", "id": rid, "reply": text}))
}

// The attribute a proposal names: the entity is the subject and the message carries
// the attribute name in backticks. Mirrors docs/compiler/model/diagnostic.md#ratification-proposals.
fn named_attribute(s: &Store, did: &str) -> Option<String> {
    let message = &s.graph.diagnostics.get(did)?.message;
    let start = message.find('`')? + 1;
    let end = start + message[start..].find('`')?;
    Some(message[start..end].to_string())
}

// The mutations that ratify one subject onto the landed sentence: the fact's
// provenance flips to the quote, and a reworded sentence becomes a requirement's
// statement. An attribute proposal (its entity quoted through mentions) flips the
// attribute's own provenance and resolves the proposal itself, since the store's
// ratification touches node-level provenance only.
fn ratify_ops(s: &Store, did: &str, subject: &str, source: &SourceRef, reworded: bool) -> Vec<Op> {
    let sid = s.resolve_id(subject).to_string();
    let mut ops = Vec::new();
    if let Some(e) = s.graph.entities.get(&sid) {
        if e.provenance.is_none() {
            if let Some(a) = named_attribute(s, did)
                .and_then(|name| e.attributes.iter().find(|a| a.name == name))
            {
                let mut a = a.clone();
                a.provenance = Provenance::Quote(source.clone());
                ops.push(Op::UpdateEntity {
                    id: sid.clone(),
                    name: None,
                    definition: None,
                    add_aliases: Vec::new(),
                    add_mention: Some(source.clone()),
                    stereotype: None,
                    parent: None,
                    set_attributes: None,
                    add_attributes: vec![a],
                    provenance: None,
                });
                ops.push(Op::ResolveDiagnostic {
                    id: did.to_string(),
                    reason: "ratified".to_string(),
                });
            }
            return ops;
        }
    }
    if reworded && s.graph.requirements.contains_key(&sid) {
        ops.push(Op::UpdateRequirement {
            id: sid.clone(),
            statement: Some(source.quote.clone()),
            entities: None,
            edges: None,
            transition: None,
            facets: None,
            source: None,
            provenance: None,
        });
    }
    ops.push(Op::RatifyProvenance {
        id: sid,
        source: source.clone(),
    });
    ops
}

// The mutations that retract one subject: the decree node is undone through the
// store; an attribute decree drops the attribute and resolves the proposal itself.
fn retract_ops(s: &Store, did: &str, subject: &str) -> Vec<Op> {
    let sid = s.resolve_id(subject).to_string();
    if let Some(e) = s.graph.entities.get(&sid) {
        if e.provenance.is_none() {
            if let Some(name) = named_attribute(s, did) {
                let kept: Vec<_> = e
                    .attributes
                    .iter()
                    .filter(|a| a.name != name)
                    .cloned()
                    .collect();
                return vec![
                    Op::UpdateEntity {
                        id: sid,
                        name: None,
                        definition: None,
                        add_aliases: Vec::new(),
                        add_mention: None,
                        stereotype: None,
                        parent: None,
                        set_attributes: Some(kept),
                        add_attributes: Vec::new(),
                        provenance: None,
                    },
                    Op::ResolveDiagnostic {
                        id: did.to_string(),
                        reason: "retracted".to_string(),
                    },
                ];
            }
        }
    }
    vec![Op::RetractDecree {
        id: sid,
        reason: "retracted".to_string(),
    }]
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
            match r.source.as_ref() {
                Some(src) => s.push_str(&format!(
                    "- {}: {} (quoted from {}#{}: \"{}\")\n",
                    sid, r.statement, src.doc, src.section, src.quote
                )),
                None => s.push_str(&format!(
                    "- {}: {} ({})\n",
                    sid,
                    r.statement,
                    crate::session::provenance_line(r)
                )),
            }
        } else if let Some(e) = store.graph.entities.get(&sid) {
            s.push_str(&format!(
                "- {} ({}): {}\n",
                sid,
                e.name,
                e.definition.as_deref().unwrap_or("")
            ));
        } else {
            s.push_str(&format!("- {}\n", sub));
        }
    }
    s.push_str(&format!(
        "\nAct on the answer with the tools: revise_requirement moves a requirement's prose and graph \
         form together, update_requirement and update_entity record decisions, update_diagnostic \
         refines the question when the answer leaves it open. Scope: change only what the answer \
         names, with the smallest edit that satisfies it. Never rewrite sentences the answer does \
         not mention, and never fix unrelated problems in passing; they have their own findings. \
         When the finding is settled, call resolve_diagnostic with id `{}` and a reason. Finish \
         with a one-line summary.",
        rid
    ));
    Ok(s)
}

// The standing questions: open, unsuppressed, prompted, unanswered findings,
// rendered once for chat surfaces (the session-start summary and /questions).
// Mirrors docs/frontends/acp.md#questions-in-chat.
pub fn questions_summary(out: &Path) -> Option<String> {
    let store = Store::load(out);
    let mut lines: Vec<String> = Vec::new();
    for (id, d) in &store.graph.diagnostics {
        if d.lifecycle != "open" || d.triage.as_deref() == Some("suppressed") {
            continue;
        }
        let Some(p) = &d.prompt else { continue };
        if d.answer
            .as_ref()
            .map(|a| a.status != "failed")
            .unwrap_or(false)
        {
            continue;
        }
        let mut s = format!(
            "- {} ({}, {}): {}\n  Q: {}",
            id, d.rule, d.severity, d.message, p.question
        );
        for (i, o) in p.options.iter().enumerate() {
            s.push_str(&format!(
                "\n  {}. {}{}",
                i + 1,
                o.label,
                if o.edit.is_some() {
                    " (suggested edit)"
                } else {
                    ""
                }
            ));
        }
        if p.freeform {
            s.push_str("\n  (a freeform reply is accepted)");
        }
        lines.push(s);
    }
    if lines.is_empty() {
        return None;
    }
    let total = lines.len();
    lines.truncate(10);
    Some(format!(
        "{} standing question(s) on this project:\n{}\n\nAnswer in chat and the agent records it, or use the editor's quick fixes on the finding.",
        total,
        lines.join("\n")
    ))
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
            let _ = s.apply(
                vec![Op::AnswerDiagnostic {
                    id: rid.clone(),
                    answer: a,
                }],
                &Commit::store("answer"),
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Diagnostic, DiagnosticPrompt, PromptOption, SuggestedEdit};

    fn session() -> Commit {
        Commit::store("session")
    }

    // A project whose one quoted requirement was decreed over (the way edit_fact
    // stages it), with the ratification proposal the decree filed.
    fn seed_decree(label: &str) -> (Project, PathBuf, String) {
        let dir =
            std::env::temp_dir().join(format!("jazyk-answer-{}-{}", label, std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(
            dir.join("jazyk.toml"),
            "[docs]\nglob = [\"docs/**/*.md\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("docs/pay.md"),
            "# Pay\n\nAn Order is paid within 30 days.\n",
        )
        .unwrap();
        let project = Project::load(&dir);
        let out = project.out.clone();
        let (parsed, _) = crate::reconcile::parse_all(&project);
        let mut s = Store::load(&out);
        s.sync_docs(&parsed);
        let quote = "An Order is paid within 30 days.";
        s.apply(
            vec![
                Op::CreateEntity {
                    id: "ent:order".into(),
                    entity: crate::model::Entity {
                        name: "Order".into(),
                        ..Default::default()
                    },
                },
                Op::CreateRequirement {
                    id: "req:pay-1".into(),
                    requirement: crate::model::Requirement {
                        statement: quote.into(),
                        entities: vec!["ent:order".into()],
                        source: Some(SourceRef {
                            doc: "docs/pay.md".into(),
                            section: "/pay".into(),
                            quote: quote.into(),
                        }),
                        ..Default::default()
                    },
                },
            ],
            &session(),
        );
        let decree = Provenance::Decree {
            author: "owner".into(),
            at: "now".into(),
            note: Some("the bound moved".into()),
        };
        let r = s.graph.requirements["req:pay-1"].clone();
        let sentence = "An Order is paid within 21 days.";
        let proposal = s.ratification_proposal(
            "req:pay-1",
            sentence,
            &decree,
            r.source.as_ref(),
            &r.entities,
            None,
        );
        let report = s.apply(
            vec![
                Op::UpdateRequirement {
                    id: "req:pay-1".into(),
                    statement: Some(sentence.into()),
                    entities: None,
                    edges: None,
                    transition: None,
                    facets: None,
                    source: None,
                    provenance: Some(decree),
                },
                Op::ReportDiagnostic {
                    id: String::new(),
                    diagnostic: proposal,
                },
            ],
            &Commit::store("decree"),
        );
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        let did = s
            .graph
            .diagnostics
            .iter()
            .find(|(_, d)| d.rule == "ratification-pending")
            .map(|(id, _)| id.clone())
            .unwrap();
        (project, out, did)
    }

    fn journal_kind(out: &Path, generation: u64) -> crate::model::JournalEntry {
        let path = out.join("journal").join(format!("g{}.yaml", generation));
        serde_norway::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
    }

    // Accepting a ratification proposal lands the sentence, flips the fact to quote
    // provenance, and resolves the proposal, all in one generation.
    // Mirrors docs/compiler/goals/ratify.md#accept.
    #[test]
    fn ratification_accept_flips_provenance_in_one_generation() {
        let (project, out, did) = seed_decree("ratify");
        let s = Store::load(&out);
        assert!(s
            .status
            .has_change(crate::store::CHANGE_PROVENANCE_PENDING, "req:pay-1"));
        let p = s.graph.diagnostics[&did].prompt.clone().unwrap();
        let e = p.options[0].edit.as_ref().unwrap();
        assert_eq!(e.old_text, "An Order is paid within 30 days.");
        assert_eq!(e.new_text, "An Order is paid within 21 days.");
        assert_eq!(p.options[1].answer.as_deref(), Some("retract"));
        let before = s.status.generation;
        drop(s);

        let v = answer(&project, &out, &did, Reply::Choice(0), None).unwrap();
        assert_eq!(v["status"], "applied");
        let text = std::fs::read_to_string(project.root.join("docs/pay.md")).unwrap();
        assert!(
            text.contains("An Order is paid within 21 days."),
            "{}",
            text
        );
        assert!(!text.contains("30 days"), "{}", text);
        let s = Store::load(&out);
        assert_eq!(s.status.generation, before + 1, "one changeset");
        let r = &s.graph.requirements["req:pay-1"];
        assert_eq!(
            r.source.as_ref().unwrap().quote,
            "An Order is paid within 21 days."
        );
        assert!(r.provenance.is_none());
        let d = &s.graph.diagnostics[&did];
        assert_eq!(d.lifecycle, "resolved");
        assert_eq!(d.answer.as_ref().unwrap().status, "applied");
        assert!(!s
            .status
            .has_change(crate::store::CHANGE_PROVENANCE_PENDING, "req:pay-1"));
        let entry = journal_kind(&out, before + 1);
        assert_eq!(entry.kind, "ratify");
        assert!(entry.mutations.iter().any(|m| m["op"] == "edit_doc_prose"));
        assert!(entry
            .mutations
            .iter()
            .any(|m| m["op"] == "ratify_provenance"));
        // The absorbed hashes: no recompile is owed for the landed sentence.
        let (parsed, _) = crate::reconcile::parse_all(&project);
        assert_eq!(s.docs["docs/pay.md"].content_hash, parsed["docs/pay.md"].0);
        std::fs::remove_dir_all(&project.root).ok();
    }

    // A freeform reply on a proposal is the accepted sentence, and becomes the
    // requirement's statement too.
    #[test]
    fn ratification_freeform_reply_is_the_accepted_sentence() {
        let (project, out, did) = seed_decree("ratify-reword");
        let reworded = "Every Order is paid within 21 days.";
        answer(&project, &out, &did, Reply::Text(reworded.into()), None).unwrap();
        let text = std::fs::read_to_string(project.root.join("docs/pay.md")).unwrap();
        assert!(text.contains(reworded), "{}", text);
        let s = Store::load(&out);
        let r = &s.graph.requirements["req:pay-1"];
        assert_eq!(r.statement, reworded);
        assert_eq!(r.source.as_ref().unwrap().quote, reworded);
        assert_eq!(s.graph.diagnostics[&did].lifecycle, "resolved");
        std::fs::remove_dir_all(&project.root).ok();
    }

    // Retracting removes the decree deterministically and resolves the proposal.
    // Mirrors docs/compiler/goals/ratify.md#retract.
    #[test]
    fn ratification_retract_removes_the_decree() {
        let (project, out, did) = seed_decree("retract");
        let before = Store::load(&out).status.generation;
        let v = answer(&project, &out, &did, Reply::Choice(1), None).unwrap();
        assert_eq!(v["status"], "applied");
        let s = Store::load(&out);
        assert_eq!(s.status.generation, before + 1);
        assert!(!s.graph.requirements.contains_key("req:pay-1"));
        let d = &s.graph.diagnostics[&did];
        assert_eq!(d.lifecycle, "resolved");
        assert_eq!(d.answer.as_ref().unwrap().status, "applied");
        assert!(!s
            .status
            .has_change(crate::store::CHANGE_PROVENANCE_PENDING, "req:pay-1"));
        assert_eq!(journal_kind(&out, before + 1).kind, "ratify");
        // The document was never touched.
        let text = std::fs::read_to_string(project.root.join("docs/pay.md")).unwrap();
        assert!(text.contains("within 30 days"), "{}", text);
        std::fs::remove_dir_all(&project.root).ok();
    }

    // The edit-answer path end to end: the file changes on disk, the hashes are
    // absorbed (no recompile owed for the edited doc), the answer is recorded as
    // applied, and the diagnostic resolves in the same changeset.
    #[test]
    fn edit_answer_is_a_dual_write_that_resolves() {
        let dir = std::env::temp_dir().join(format!("jazyk-answer-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(
            dir.join("jazyk.toml"),
            "[docs]\nglob = [\"docs/**/*.md\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("docs/pay.md"),
            "# Pay\n\nAn Order shall be paid within 30 days.\n",
        )
        .unwrap();
        let project = Project::load(&dir);
        let out = project.out.clone();
        // Seed the graph: sync the doc, then file a prompted diagnostic on its section.
        let (parsed, _) = crate::reconcile::parse_all(&project);
        let mut s = Store::load(&out);
        s.sync_docs(&parsed);
        let sec = s.docs["docs/pay.md"]
            .sections
            .keys()
            .next()
            .unwrap()
            .clone();
        s.apply(
            vec![Op::ReportDiagnostic {
                id: String::new(),
                diagnostic: Diagnostic {
                    rule: "contradiction".into(),
                    severity: "warning".into(),
                    subjects: vec![format!("docs/pay.md#{}", sec)],
                    message: "21 vs 30 days".into(),
                    reasoning: None,
                    lifecycle: "open".into(),
                    triage: None,
                    prompt: Some(DiagnosticPrompt {
                        question: "which bound holds?".into(),
                        options: vec![PromptOption {
                            label: "21 days".into(),
                            edit: Some(SuggestedEdit {
                                doc: "docs/pay.md".into(),
                                section: sec.clone(),
                                old_text: "within 30 days".into(),
                                new_text: "within 21 days".into(),
                            }),
                            answer: None,
                        }],
                        freeform: true,
                    }),
                    answer: None,
                    created: None,
                    updated: None,
                },
            }],
            &session(),
        );
        let id = s.graph.diagnostics.keys().next().unwrap().clone();
        // A requirement anchored on the edited sentence: the answer re-anchors it
        // mechanically in the same changeset.
        s.apply(
            vec![Op::CreateRequirement {
                id: "req:pay-1".into(),
                requirement: crate::model::Requirement {
                    statement: "An Order shall be paid within 30 days.".into(),
                    entities: vec![],
                    edges: vec![],
                    source: Some(crate::model::SourceRef {
                        doc: "docs/pay.md".into(),
                        section: sec.clone(),
                        quote: "An Order shall be paid within 30 days.".into(),
                    }),
                    ..Default::default()
                },
            }],
            &session(),
        );
        drop(s);

        let v = answer(&project, &out, &id, Reply::Choice(0), None).unwrap();
        assert_eq!(v["status"], "applied");
        let text = std::fs::read_to_string(dir.join("docs/pay.md")).unwrap();
        assert!(text.contains("within 21 days"), "file rewritten: {}", text);
        let s = Store::load(&out);
        let d = &s.graph.diagnostics[&id];
        assert_eq!(d.lifecycle, "resolved");
        assert_eq!(d.answer.as_ref().unwrap().status, "applied");
        let r = s
            .graph
            .requirements
            .values()
            .next()
            .expect("requirement survives");
        assert!(
            r.source.as_ref().unwrap().quote.contains("within 21 days"),
            "re-anchored: {}",
            r.source.as_ref().unwrap().quote
        );
        assert!(
            r.statement.contains("within 21 days"),
            "statement updated: {}",
            r.statement
        );
        // The edit absorbed its own hashes: the doc is not dirty against the graph.
        let (parsed, _) = crate::reconcile::parse_all(&project);
        let on_disk = &parsed["docs/pay.md"];
        assert_eq!(
            s.docs["docs/pay.md"].content_hash, on_disk.0,
            "no recompile owed"
        );
        // A second answer is refused: the decision is final.
        assert!(answer(&project, &out, &id, Reply::Text("again".into()), None).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    // The same phrase in an earlier section must not catch the edit: the search is
    // scoped to the option's own section.
    #[test]
    fn edit_answer_lands_in_its_own_section() {
        let dir =
            std::env::temp_dir().join(format!("jazyk-answer-scope-test-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(
            dir.join("jazyk.toml"),
            "[docs]\nglob = [\"docs/**/*.md\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("docs/pay.md"),
            "# Pay\n\n## Invoices\n\nAn Invoice is due within 30 days.\n\n## Refunds\n\nA Refund is issued within 30 days.\n",
        )
        .unwrap();
        let project = Project::load(&dir);
        let out = project.out.clone();
        let (parsed, _) = crate::reconcile::parse_all(&project);
        let mut s = Store::load(&out);
        s.sync_docs(&parsed);
        let refunds = s.docs["docs/pay.md"]
            .sections
            .iter()
            .find(|(_, sec)| sec.title.contains("Refunds"))
            .map(|(r, _)| r.clone())
            .unwrap();
        s.apply(
            vec![Op::ReportDiagnostic {
                id: String::new(),
                diagnostic: Diagnostic {
                    rule: "ambiguity".into(),
                    severity: "warning".into(),
                    subjects: vec![format!("docs/pay.md#{}", refunds)],
                    message: "refund bound unclear".into(),
                    reasoning: None,
                    lifecycle: "open".into(),
                    triage: None,
                    prompt: Some(DiagnosticPrompt {
                        question: "which refund bound?".into(),
                        options: vec![PromptOption {
                            label: "14 days".into(),
                            edit: Some(SuggestedEdit {
                                doc: "docs/pay.md".into(),
                                section: refunds.clone(),
                                old_text: "within 30 days".into(),
                                new_text: "within 14 days".into(),
                            }),
                            answer: None,
                        }],
                        freeform: false,
                    }),
                    answer: None,
                    created: None,
                    updated: None,
                },
            }],
            &session(),
        );
        let id = s.graph.diagnostics.keys().next().unwrap().clone();
        drop(s);

        answer(&project, &out, &id, Reply::Choice(0), None).unwrap();
        let text = std::fs::read_to_string(dir.join("docs/pay.md")).unwrap();
        assert!(
            text.contains("An Invoice is due within 30 days."),
            "the earlier section is untouched: {}",
            text
        );
        assert!(
            text.contains("A Refund is issued within 14 days."),
            "the edit landed in Refunds: {}",
            text
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
