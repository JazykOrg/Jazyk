// Answering a diagnostic prompt: the one engine every frontend calls (LSP code
// actions, the GUI questions panel, answer_diagnostic in chat sessions).
// An edit option is deterministic: dual write plus resolve in one changeset. Any
// other reply records `handling` and a model acts on it over ACP.
// Mirrors docs/compiler/model/diagnostic.md#answers and
// docs/frontends/acp.md#answer-sessions.

use crate::model::{DiagnosticAnswer, Provenance, SourceRef, Transition};
use crate::project::Project;
use crate::store::{Commit, Op, ProseEdit, Store, WriteEdit};
use crate::tools::{ToolSession, WorkScope};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::ops::Range;
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

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
            // Freeform is an instruction for answer handling, never document prose.
            (None, t, None, false)
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
        let old_full = crate::vfs::read_to_string(&path)
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
        // The fact changes the rewrite implies: the option's listed facts through the
        // update_requirement gates, then the mechanical transition follow for every
        // touched requirement whose transition no fact sets. Mirrors
        // docs/compiler/model/diagnostic.md#answers.
        let mut followed = Vec::new();
        let mut review = Vec::new();
        if !ratify {
            let mut set_transition = BTreeSet::new();
            if !e.facts.is_empty() {
                let session = ToolSession::new(
                    s.clone(),
                    WorkScope::serving("mcp-write"),
                    crate::limits::SESSION_MUTATIONS,
                    crate::limits::CONTEXT_BUDGET,
                );
                let fact_ops = session
                    .fact_ops(&e.doc, &e.section, &e.facts)
                    .map_err(|err| {
                        format!(
                            "the option's fact change is refused ({}): {}; nothing was applied, and update_diagnostic can re-author the option",
                            err.rule, err.message
                        )
                    })?;
                for op in &fact_ops {
                    if let Op::UpdateRequirement {
                        id,
                        transition: Some(_),
                        ..
                    } = op
                    {
                        set_transition.insert(id.clone());
                    }
                }
                ops.extend(fact_ops);
            }
            for (qid, new_quote) in s.quote_rewrites(&prose) {
                if set_transition.contains(&qid) {
                    continue;
                }
                let r = &s.graph.requirements[&qid];
                let (Some(t), Some(src)) = (r.transition.as_ref(), r.source.as_ref()) else {
                    continue;
                };
                match follow_transition(&src.quote, &new_quote, t) {
                    Follow::Stays => {}
                    Follow::Moves(next) => {
                        followed.push(json!({"id": qid, "transition": next}));
                        ops.push(Op::UpdateRequirement {
                            id: qid.clone(),
                            statement: None,
                            entities: None,
                            edges: None,
                            transition: Some(next),
                            facets: None,
                            source: None,
                            provenance: None,
                        });
                    }
                    Follow::Ambiguous => review.push(qid.clone()),
                }
            }
        }
        if ratify {
            if subjects.len() > 1 {
                ops.push(Op::ResolveDiagnostic {
                    id: rid.clone(),
                    reason: "combined proposal accepted".into(),
                });
            }
            // Accepting the proposal flips the fact's provenance to the landed
            // sentence in the same changeset; the store resolves the proposal.
            // Mirrors docs/compiler/goals/ratify.md#accept.
            let source = SourceRef {
                doc: e.doc.clone(),
                section: e.section.clone(),
                quote: prose.new_text.trim().to_string(),
            };
            for sub in &subjects {
                let mut fact_source = source.clone();
                if subjects.len() > 1 {
                    let c = s
                        .graph
                        .criteria
                        .get(sub)
                        .ok_or("combined proposal must contain only criteria")?;
                    if !prose.new_text.contains(&c.statement) {
                        return Err(
                            "combined proposal is stale; a criterion's statement changed".into(),
                        );
                    }
                    fact_source.quote = c.statement.clone();
                }
                ops.extend(ratify_ops(&s, &rid, sub, &fact_source));
            }
        }
        ops.push(Op::AnswerDiagnostic {
            id: rid.clone(),
            answer: DiagnosticAnswer {
                choice,
                text: text.clone(),
                status: "applied".to_string(),
                error: None,
            },
        });
        if !ratify {
            ops.push(Op::ResolveDiagnostic {
                id: rid.clone(),
                reason: format!("suggested edit applied: {}", label),
            });
        }
        let kind = if ratify { "ratify" } else { "answer" };
        // An answer lands whole or not at all: a fact change the commit gates refuse
        // puts the prose back and records no answer.
        let commit = Commit {
            all_or_nothing: !ratify || subjects.len() > 1,
            ..Commit::store(kind)
        };
        s.dual_write(&project.root, &prose, ops, &commit, write)
            .map_err(|e| format!("commit skipped: {}", e))?;
        // The edit as it landed, so a frontend can mark the changed text in an open
        // editor (docs/frontends/gui.md#api, `POST /api/questions/{id}/answer`;
        // the marking is jazyk-cloud docs/editor/editor.md#questions).
        return Ok(json!({
            "status": "applied", "resolved": true, "doc": e.doc,
            "edit": {"doc": e.doc, "section": e.section, "newText": prose.new_text},
            "facts": e.facts, "followed": followed, "review": review,
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
        if subjects.len() > 1 {
            ops.push(Op::ResolveDiagnostic {
                id: rid.clone(),
                reason: "combined proposal retracted".into(),
            });
        }
        for sub in &subjects {
            ops.extend(retract_ops(&s, &rid, sub));
        }
        ops.push(Op::AnswerDiagnostic {
            id: rid.clone(),
            answer: DiagnosticAnswer {
                choice,
                text: text.clone(),
                status: "applied".to_string(),
                error: None,
            },
        });
        // All or nothing: a refused retract records no answer, so the proposal stays
        // answerable. Mirrors docs/compiler/goals/ratify.md#retract.
        let commit = Commit {
            all_or_nothing: true,
            ..Commit::store("ratify")
        };
        let report = s.apply(ops, &commit);
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
                error: None,
            },
        }],
        &Commit::store("answer"),
    );
    if !report.skipped.is_empty() {
        return Err(report.skipped.join("; "));
    }
    Ok(json!({"status": "handling", "id": rid, "reply": text}))
}

// What a rewrite does to a requirement's transition. Mirrors
// docs/compiler/model/diagnostic.md#answers.
#[derive(Debug, PartialEq)]
pub(crate) enum Follow {
    // No replaced run touches a state or the trigger the old quote names.
    Stays,
    // Every touched part maps one to one: the transition as the new quote states it.
    Moves(Transition),
    // A touched part does not map one to one; the review decides.
    Ambiguous,
}

// One word of a quote: its lowercased text and its byte range in the quote.
struct Word {
    text: String,
    at: Range<usize>,
}

// A word is a run of letters, digits, apostrophes, and hyphens, trimmed of leading and
// trailing apostrophes and hyphens.
fn words(s: &str) -> Vec<Word> {
    let is_word = |c: char| c.is_alphanumeric() || c == '\'' || c == '\u{2019}' || c == '-';
    let is_edge = |c: char| c == '\'' || c == '\u{2019}' || c == '-';
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let push = |b: usize, e: usize, out: &mut Vec<Word>| {
        let run = &s[b..e];
        let lead = run.len() - run.trim_start_matches(is_edge).len();
        let trail = run.len() - run.trim_end_matches(is_edge).len();
        if lead + trail < run.len() {
            let (b, e) = (b + lead, e - trail);
            out.push(Word {
                text: s[b..e].to_lowercase(),
                at: b..e,
            });
        }
    };
    for (i, c) in s.char_indices() {
        match (is_word(c), start) {
            (true, None) => start = Some(i),
            (false, Some(b)) => {
                push(b, i, &mut out);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(b) = start {
        push(b, s.len(), &mut out);
    }
    out
}

// The replaced runs between two word lists: an LCS alignment, each maximal stretch of
// unmatched words one run (old range, new range). Ties delete before they insert, so
// the runs are deterministic.
fn replaced_runs(a: &[Word], b: &[Word]) -> Vec<(Range<usize>, Range<usize>)> {
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i].text == b[j].text {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut runs = Vec::new();
    let (mut i, mut j) = (0, 0);
    let mut open: Option<(usize, usize)> = None;
    while i < n || j < m {
        if i < n && j < m && a[i].text == b[j].text {
            if let Some((oi, oj)) = open.take() {
                runs.push((oi..i, oj..j));
            }
            i += 1;
            j += 1;
            continue;
        }
        open.get_or_insert((i, j));
        if j == m || (i < n && lcs[i + 1][j] >= lcs[i][j + 1]) {
            i += 1;
        } else {
            j += 1;
        }
    }
    if let Some((oi, oj)) = open {
        runs.push((oi..n, oj..m));
    }
    runs
}

// Where a phrase's words occur in a word list.
fn occurrences(hay: &[Word], needle: &[Word]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return Vec::new();
    }
    (0..=hay.len() - needle.len())
        .filter(|&p| {
            hay[p..p + needle.len()]
                .iter()
                .zip(needle)
                .all(|(x, y)| x.text == y.text)
        })
        .collect()
}

// Whether a replaced run touches the words at `span`: its removed words overlap them,
// or it inserts strictly inside them.
fn touches(run: &(Range<usize>, Range<usize>), span: &Range<usize>) -> bool {
    let old = &run.0;
    if old.is_empty() {
        old.start > span.start && old.start < span.end
    } else {
        old.start < span.end && span.start < old.end
    }
}

// Follow a quote rewrite onto a requirement's transition: a state one replaced run
// removes exactly (and replaces with words) takes the inserted words; a trigger whose
// touching runs all replace words inside it takes them spliced in; anything else a run
// touches is ambiguous. Mirrors docs/compiler/model/diagnostic.md#answers.
pub(crate) fn follow_transition(old: &str, new: &str, t: &Transition) -> Follow {
    let (a, b) = (words(old), words(new));
    let runs = replaced_runs(&a, &b);
    let new_text = |r: &Range<usize>| new[b[r.start].at.start..b[r.end - 1].at.end].to_string();
    let spans = |w: &[Word]| -> Vec<Range<usize>> {
        occurrences(&a, w)
            .into_iter()
            .map(|p| p..p + w.len())
            .collect()
    };
    let mut next = t.clone();
    let mut moved = false;
    for to in [false, true] {
        let value = if to { &t.to } else { &t.from };
        let spans = spans(&words(value));
        let hit: Vec<&(Range<usize>, Range<usize>)> = runs
            .iter()
            .filter(|r| spans.iter().any(|s| touches(r, s)))
            .collect();
        if hit.is_empty() {
            continue;
        }
        if spans.len() != 1 || hit.len() != 1 || hit[0].0 != spans[0] || hit[0].1.is_empty() {
            return Follow::Ambiguous;
        }
        let state = new_text(&hit[0].1);
        if to {
            next.to = state;
        } else {
            next.from = state;
        }
        moved = true;
    }
    if let Some(trigger) = &t.trigger {
        let tw = words(trigger);
        let spans = spans(&tw);
        let hit: Vec<&(Range<usize>, Range<usize>)> = runs
            .iter()
            .filter(|r| spans.iter().any(|s| touches(r, s)))
            .collect();
        if !hit.is_empty() {
            if spans.len() != 1 {
                return Follow::Ambiguous;
            }
            let span = &spans[0];
            let inside = hit.iter().all(|(o, n)| {
                !o.is_empty() && !n.is_empty() && o.start >= span.start && o.end <= span.end
            });
            if !inside {
                return Follow::Ambiguous;
            }
            let mut text = trigger.clone();
            for (o, n) in hit.iter().rev() {
                let (first, last) = (&tw[o.start - span.start], &tw[o.end - 1 - span.start]);
                text.replace_range(first.at.start..last.at.end, &new_text(n));
            }
            next.trigger = Some(text);
            moved = true;
        }
    }
    if moved {
        Follow::Moves(next)
    } else {
        Follow::Stays
    }
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
// provenance flips to the quote. An attribute proposal (its entity quoted through mentions) flips the
// attribute's own provenance and resolves the proposal itself, since the store's
// ratification touches node-level provenance only.
fn ratify_ops(s: &Store, did: &str, subject: &str, source: &SourceRef) -> Vec<Op> {
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
                    remove_aliases: Vec::new(),
                    clear_parent: false,
                    not_about_add: Vec::new(),
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
                        remove_aliases: Vec::new(),
                        clear_parent: false,
                        not_about_add: Vec::new(),
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
    let line = |name: &str, vars: &[(&str, &str)]| HANDLING.fill(name, vars) + "\n";
    let mut s = line(
        "heading",
        &[
            ("id", &rid),
            ("rule", &d.rule),
            ("severity", &d.severity),
            ("finding", &d.message),
        ],
    );
    if let Some(p) = &d.prompt {
        s.push_str(&line("question", &[("question", &p.question)]));
    }
    if let Some(a) = &d.answer {
        s.push_str(&line("answer", &[("answer", &a.text)]));
    }
    s.push('\n');
    s.push_str(&line("subjects", &[]));
    for sub in &d.subjects {
        let sid = store.resolve_id(sub).to_string();
        if let Some(r) = store.graph.requirements.get(&sid) {
            match r.source.as_ref() {
                Some(src) => s.push_str(&line(
                    "subject-quoted",
                    &[
                        ("id", &sid),
                        ("statement", &r.statement),
                        ("doc", &src.doc),
                        ("section", &src.section),
                        ("quote", &src.quote),
                    ],
                )),
                None => s.push_str(&line(
                    "subject-derived",
                    &[
                        ("id", &sid),
                        ("statement", &r.statement),
                        ("provenance", &crate::session::provenance_line(r)),
                    ],
                )),
            }
        } else if let Some(c) = store.graph.criteria.get(&sid) {
            s.push_str(&line(
                "subject-criterion",
                &[
                    ("id", &sid),
                    ("criterion", &serde_json::to_string(c).unwrap()),
                ],
            ));
        } else if let Some(e) = store.graph.entities.get(&sid) {
            s.push_str(&line(
                "subject-entity",
                &[
                    ("id", &sid),
                    ("name", &e.name),
                    ("definition", e.definition.as_deref().unwrap_or("")),
                ],
            ));
        } else {
            s.push_str(&line("subject", &[("id", sub)]));
        }
    }
    s.push('\n');
    s.push_str(&HANDLING.fill("act", &[("id", &rid)]));
    Ok(s)
}

// The handling contract for an answer the calling agent applies.
// Mirrors docs/frontends/acp.md#questions-in-chat.
pub(crate) const HANDLING: crate::payload::Fragments = crate::payload::Fragments(include_str!(
    "../../docs/compiler/goals/prompts/answer-handling.md"
));

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

// The answers recorded as `handling`: what an answer session still owes. A tab runs
// nothing in the background, so its next build runs one session per id here
// (docs/frontends/wasm.md#the-inbox).
pub fn handling_ids(out: &Path) -> Vec<String> {
    Store::load(out)
        .graph
        .diagnostics
        .iter()
        .filter(|(_, d)| {
            d.lifecycle == "open" && d.answer.as_ref().is_some_and(|a| a.status == "handling")
        })
        .map(|(id, _)| id.clone())
        .collect()
}

// Record an answer session's outcome on the answer: `handled`, or `failed` with the
// error. Every runner lands the same record, so every frontend shows the same
// progress from the store.
pub fn record_outcome(out: &Path, id: &str, outcome: &Result<(), String>) {
    let (status, error) = match outcome {
        Ok(()) => ("handled", None),
        Err(e) => ("failed", Some(e.clone())),
    };
    let mut s = Store::load(out);
    let rid = s.resolve_id(id).to_string();
    if let Some(a) = s.graph.diagnostics.get(&rid).and_then(|d| d.answer.clone()) {
        let mut a = a;
        a.status = status.to_string();
        a.error = error;
        let _ = s.apply(
            vec![Op::AnswerDiagnostic {
                id: rid.clone(),
                answer: a,
            }],
            &Commit::store("answer"),
        );
    }
}

// Frontends with no live session (LSP, GUI) hand a `handling` answer to one focused
// background session; `answer.status` moves to handled or failed when it lands, so
// every frontend shows the same progress from the store.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_handler(project: Project, out: PathBuf, id: String) {
    if crate::vfs::is_scoped() {
        return;
    }
    std::thread::spawn(move || {
        let _ = run_handler(&project, &out, &id);
    });
}

// Run the answer session for a `handling` answer and record its outcome on the
// answer (`handled`, or `failed` with the error). The terminal runs it in the
// foreground (docs/frontends/cli.md#jazyk-answer); the other frontends spawn it.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_handler(project: &Project, out: &Path, id: &str) -> Result<(), String> {
    // A newer store runs no session and records nothing (docs/compiler/graph.md#store-version).
    if let Some(why) = crate::store::newer_store(out) {
        return Err(why);
    }
    let outcome = handle(project, out, id);
    if let Err(e) = &outcome {
        eprintln!("jazyk: answer session for {} failed: {}", id, e);
    }
    record_outcome(out, id, &outcome);
    outcome
}

#[cfg(not(target_arch = "wasm32"))]
fn handle(project: &Project, out: &Path, id: &str) -> Result<(), String> {
    let llm = crate::cli::resolve_llm(
        &crate::cli::Options::default(),
        &project.llm,
        &crate::project::load_global_llm(),
        |n| crate::sys::env_var(n).ok(),
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
        let dir = std::env::temp_dir().join(format!(
            "jazyk-answer-{}-{}",
            label,
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
        crate::vfs::write(
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

    fn seed_combined(label: &str) -> (Project, PathBuf, String, Vec<String>) {
        let (project, out, original) = seed_decree(label);
        answer(&project, &out, &original, Reply::Choice(0), None).unwrap();
        let mut s = Store::load(&out);
        let (parsed, _) = crate::reconcile::parse_all(&project);
        s.sync_docs(&parsed);
        let report = s.apply(
            vec![
                "An Order paid on day 21 is accepted.",
                "An Order unpaid on day 22 is rejected.",
            ]
            .into_iter()
            .enumerate()
            .map(|(i, text)| Op::CreateCriterion {
                id: format!("crit:proposal-{}", i),
                criterion: crate::model::Criterion {
                    requirement: "req:pay-1".into(),
                    statement: text.into(),
                    kind: "positive".into(),
                    provenance: Some(Provenance::Derived {
                        from: vec!["req:pay-1".into()],
                        reasoning: "covers the deadline".into(),
                    }),
                    ..Default::default()
                },
            })
            .collect(),
            &session(),
        );
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        let (id, d) = s
            .graph
            .diagnostics
            .iter()
            .find(|(_, d)| d.lifecycle == "open" && d.subjects.len() == 2)
            .unwrap();
        (project, out, id.clone(), d.subjects.clone())
    }

    #[test]
    fn combined_ratification_accepts_once_with_individual_quotes() {
        let (project, out, did, ids) = seed_combined("combined-accept");
        let s = Store::load(&out);
        let text = s.graph.diagnostics[&did].prompt.as_ref().unwrap().options[0]
            .edit
            .as_ref()
            .unwrap()
            .new_text
            .clone();
        assert!(s.graph.diagnostics[&did].prompt.as_ref().unwrap().freeform);
        answer(&project, &out, &did, Reply::Choice(0), None).unwrap();
        let s = Store::load(&out);
        assert_eq!(s.graph.diagnostics[&did].lifecycle, "resolved");
        let doc = crate::vfs::read_to_string(project.root.join("docs/pay.md")).unwrap();
        assert_eq!(doc.matches(&text).count(), 1);
        assert_eq!(s.graph.diagnostics[&did].subjects, ids);
        for id in ids {
            let c = &s.graph.criteria[&id];
            assert_eq!(c.source.as_ref().unwrap().quote, c.statement);
        }
    }

    #[test]
    fn combined_ratification_retracts_all_and_partial_ratify_keeps_remainder() {
        let (project, out, did, ids) = seed_combined("combined-partial");
        let mut s = Store::load(&out);
        let c = &s.graph.criteria[&ids[0]];
        let source = SourceRef {
            doc: "docs/pay.md".into(),
            section: "/pay".into(),
            quote: c.statement.clone(),
        };
        // The direct ratification path has the same provenance gates as the store.
        crate::vfs::write(
            project.root.join("docs/pay.md"),
            format!(
                "# Pay\nAn Order is paid within 30 days.\n\n{}\n",
                source.quote
            ),
        )
        .unwrap();
        let (parsed, _) = crate::reconcile::parse_all(&project);
        s.sync_docs(&parsed);
        let r = s.apply(
            vec![Op::RatifyProvenance {
                id: ids[0].clone(),
                source,
            }],
            &session(),
        );
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
        assert_eq!(s.graph.diagnostics[&did].lifecycle, "open");
        assert_eq!(s.graph.diagnostics[&did].subjects, vec![ids[1].clone()]);
        answer(&project, &out, &did, Reply::Choice(1), None).unwrap();
        let s = Store::load(&out);
        assert!(s.graph.criteria.contains_key(&ids[0]));
        assert!(!s.graph.criteria.contains_key(&ids[1]));
    }

    #[test]
    fn combined_ratification_retracts_every_member() {
        let (project, out, did, ids) = seed_combined("combined-retract");
        answer(&project, &out, &did, Reply::Choice(1), None).unwrap();
        let s = Store::load(&out);
        for id in ids {
            assert!(!s.graph.criteria.contains_key(&id));
        }
        assert_eq!(s.graph.diagnostics[&did].lifecycle, "resolved");
    }

    #[test]
    fn combined_ratification_absorbs_later_proposals_and_refreshes_edits() {
        let (_, out, did, ids) = seed_combined("combined-later");
        let mut s = Store::load(&out);
        let report = s.apply(
            vec![Op::CreateCriterion {
                id: "crit:later".into(),
                criterion: crate::model::Criterion {
                    requirement: "req:pay-1".into(),
                    statement: "An Order paid on day 20 is accepted.".into(),
                    kind: "positive".into(),
                    provenance: Some(Provenance::Derived {
                        from: vec!["req:pay-1".into()],
                        reasoning: "another boundary".into(),
                    }),
                    ..Default::default()
                },
            }],
            &session(),
        );
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        assert_eq!(s.graph.diagnostics[&did].subjects.len(), 3);
        let before = serde_json::to_value(&s.graph.diagnostics[&did]).unwrap();
        s.apply(Vec::new(), &session());
        assert_eq!(
            serde_json::to_value(&s.graph.diagnostics[&did]).unwrap(),
            before
        );
        let report = s.apply(
            vec![Op::UpdateCriterion {
                id: ids[0].clone(),
                statement: Some("An Order paid on day 19 is accepted.".into()),
                kind: None,
                measure: None,
                given: None,
                when: None,
                then: None,
                entities: None,
            }],
            &session(),
        );
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        assert!(
            s.graph.diagnostics[&did].prompt.as_ref().unwrap().options[0]
                .edit
                .as_ref()
                .unwrap()
                .new_text
                .contains("day 19")
        );
        assert_eq!(
            s.graph
                .diagnostics
                .values()
                .filter(|d| d.lifecycle == "open" && d.rule == RATIFICATION)
                .count(),
            1
        );
    }

    fn journal_kind(out: &Path, generation: u64) -> crate::model::JournalEntry {
        let path = crate::layout::journal_dir(out).join(format!("g{}.yaml", generation));
        serde_norway::from_str(&crate::vfs::read_to_string(&path).unwrap()).unwrap()
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
        let text = crate::vfs::read_to_string(project.root.join("docs/pay.md")).unwrap();
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
        crate::vfs::remove_dir_all(&project.root).ok();
    }

    // A custom reply instructs the follow-up agent and never becomes prose.
    #[test]
    fn ratification_freeform_reply_queues_instructions_without_editing() {
        let (project, out, did) = seed_decree("ratify-instructions");
        let path = project.root.join("docs/pay.md");
        let before = crate::vfs::read_to_string(&path).unwrap();
        let instruction =
            "This is already stated, instead just link the existing text to this criterion";
        let v = answer(&project, &out, &did, Reply::Text(instruction.into()), None).unwrap();
        assert_eq!(v["status"], "handling");
        assert_eq!(crate::vfs::read_to_string(&path).unwrap(), before);
        let s = Store::load(&out);
        assert!(s.graph.requirements["req:pay-1"].source.is_none());
        let d = &s.graph.diagnostics[&did];
        assert_eq!(d.lifecycle, "open");
        assert_eq!(d.answer.as_ref().unwrap().status, "handling");
        assert_eq!(d.answer.as_ref().unwrap().text, instruction);
        assert!(handling_prompt(&out, &did).unwrap().contains(instruction));
        crate::vfs::remove_dir_all(&project.root).ok();
    }

    #[test]
    fn combined_ratification_freeform_queues_instructions_for_all_criteria() {
        let (project, out, did, ids) = seed_combined("combined-instructions");
        let path = project.root.join("docs/pay.md");
        let before = crate::vfs::read_to_string(&path).unwrap();
        let v = answer(
            &project,
            &out,
            &did,
            Reply::Text("Link existing prose for each criterion".into()),
            None,
        )
        .unwrap();
        assert_eq!(v["status"], "handling");
        assert_eq!(crate::vfs::read_to_string(&path).unwrap(), before);
        let prompt = handling_prompt(&out, &did).unwrap();
        let s = Store::load(&out);
        for id in ids {
            let c = &s.graph.criteria[&id];
            assert!(prompt.contains(&c.statement));
            assert!(prompt.contains(&c.requirement));
            assert!(c.source.is_none());
        }
        crate::vfs::remove_dir_all(&project.root).ok();
    }

    // Retracting a decree over a formerly quoted requirement restores the prior
    // value and source from the decree's journal entry and resolves the proposal.
    // Mirrors docs/compiler/goals/ratify.md#retract.
    #[test]
    fn ratification_retract_restores_the_prior_value() {
        let (project, out, did) = seed_decree("retract");
        let before = Store::load(&out).status.generation;
        let v = answer(&project, &out, &did, Reply::Choice(1), None).unwrap();
        assert_eq!(v["status"], "applied");
        let s = Store::load(&out);
        assert_eq!(s.status.generation, before + 1);
        let r = &s.graph.requirements["req:pay-1"];
        assert_eq!(r.statement, "An Order is paid within 30 days.");
        assert_eq!(
            r.source.as_ref().unwrap().quote,
            "An Order is paid within 30 days."
        );
        assert!(r.provenance.is_none());
        let d = &s.graph.diagnostics[&did];
        assert_eq!(d.lifecycle, "resolved");
        assert_eq!(d.answer.as_ref().unwrap().status, "applied");
        assert!(!s
            .status
            .has_change(crate::store::CHANGE_PROVENANCE_PENDING, "req:pay-1"));
        assert_eq!(journal_kind(&out, before + 1).kind, "ratify");
        // The document was never touched; the restored quote still locates.
        let text = crate::vfs::read_to_string(project.root.join("docs/pay.md")).unwrap();
        assert!(text.contains("within 30 days"), "{}", text);
        crate::vfs::remove_dir_all(&project.root).ok();
    }

    // A fact created by decree has no prior quote to restore: retracting deletes it.
    // Mirrors docs/compiler/goals/ratify.md#retract.
    #[test]
    fn ratification_retract_deletes_a_decree_created_fact() {
        let dir = std::env::temp_dir().join(format!(
            "jazyk-answer-retract-created-{}",
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
        crate::vfs::write(
            dir.join("docs/pay.md"),
            "# Pay\n\nAn Order is paid within 30 days.\n",
        )
        .unwrap();
        let project = Project::load(&dir);
        let out = project.out.clone();
        let (parsed, _) = crate::reconcile::parse_all(&project);
        let mut s = Store::load(&out);
        s.sync_docs(&parsed);
        let decree = Provenance::Decree {
            author: "owner".into(),
            at: "now".into(),
            note: None,
        };
        let proposal = s.ratification_proposal(
            "req:x-1",
            "An Order emits a receipt.",
            &decree,
            None,
            &["ent:order".into()],
            None,
        );
        let report = s.apply(
            vec![
                Op::CreateEntity {
                    id: "ent:order".into(),
                    entity: crate::model::Entity {
                        name: "Order".into(),
                        ..Default::default()
                    },
                },
                Op::CreateRequirement {
                    id: "req:x-1".into(),
                    requirement: crate::model::Requirement {
                        statement: "An Order emits a receipt.".into(),
                        entities: vec!["ent:order".into()],
                        provenance: Some(decree),
                        ..Default::default()
                    },
                },
                Op::ReportDiagnostic {
                    id: String::new(),
                    diagnostic: proposal,
                },
            ],
            &Commit::store("decree"),
        );
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        let s = Store::load(&out);
        let rid = s
            .graph
            .requirements
            .keys()
            .find(|k| k.starts_with("req:x"))
            .cloned()
            .unwrap();
        let did = s
            .graph
            .diagnostics
            .iter()
            .find(|(_, d)| d.rule == "ratification-pending")
            .map(|(id, _)| id.clone())
            .unwrap();
        let retract = s.graph.diagnostics[&did]
            .prompt
            .as_ref()
            .unwrap()
            .options
            .iter()
            .position(|o| o.answer.as_deref() == Some("retract"))
            .unwrap();
        let v = answer(&project, &out, &did, Reply::Choice(retract), None).unwrap();
        assert_eq!(v["status"], "applied");
        let s = Store::load(&out);
        assert!(!s.graph.requirements.contains_key(&rid));
        assert_eq!(s.graph.diagnostics[&did].lifecycle, "resolved");
        crate::vfs::remove_dir_all(&project.root).ok();
    }

    // The edit-answer path end to end: the file changes on disk, the hashes are
    // absorbed (no recompile owed for the edited doc), the answer is recorded as
    // applied, and the diagnostic resolves in the same changeset.
    #[test]
    fn edit_answer_is_a_dual_write_that_resolves() {
        let dir =
            std::env::temp_dir().join(format!("jazyk-answer-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
        crate::vfs::write(
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
                    goal: None,
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
                                facts: Vec::new(),
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
        let text = crate::vfs::read_to_string(dir.join("docs/pay.md")).unwrap();
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
        crate::vfs::remove_dir_all(&dir).ok();
    }

    // The same phrase in an earlier section must not catch the edit: the search is
    // scoped to the option's own section.
    #[test]
    fn edit_answer_lands_in_its_own_section() {
        let dir = std::env::temp_dir().join(format!(
            "jazyk-answer-scope-test-{}",
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
        crate::vfs::write(
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
                    goal: None,
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
                                facts: Vec::new(),
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
        let text = crate::vfs::read_to_string(dir.join("docs/pay.md")).unwrap();
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
        crate::vfs::remove_dir_all(&dir).ok();
    }

    // An edit option's answer is the judgment of the pair its diagnostic concerned:
    // the rewrite opens no rejudge-pair for that pair, while the revised requirement
    // still pairs with its other neighbor.
    // Mirrors docs/compiler/model/diagnostic.md#answers.
    #[test]
    fn edit_answer_judges_the_pair_it_concerned() {
        let dir = std::env::temp_dir().join(format!(
            "jazyk-answer-pair-test-{}",
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write(
            { let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path },
            "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n",
        )
        .unwrap();
        let quotes = [
            "An Order is paid by card within 30 days of delivery.",
            "An Order is paid by card within 21 days of delivery.",
            "An Order paid by card is refunded within 60 days of delivery.",
        ];
        crate::vfs::write(
            dir.join("docs/pay.md"),
            format!("# Pay\n\n{}\n\n{}\n\n{}\n", quotes[0], quotes[1], quotes[2]),
        )
        .unwrap();
        let project = Project::load(&dir);
        let out = project.out.clone();
        let (parsed, _) = crate::reconcile::parse_all(&project);
        let mut s = Store::load(&out);
        s.sync_docs(&parsed);
        let sec = s.docs["docs/pay.md"]
            .sections
            .keys()
            .next()
            .unwrap()
            .clone();
        let mut ops = vec![Op::CreateEntity {
            id: "ent:order".into(),
            entity: crate::model::Entity {
                name: "Order".into(),
                ..Default::default()
            },
        }];
        for (i, q) in quotes.iter().enumerate() {
            ops.push(Op::CreateRequirement {
                id: format!("req:pay-{}", i + 1),
                requirement: crate::model::Requirement {
                    statement: q.to_string(),
                    entities: vec!["ent:order".into()],
                    source: Some(SourceRef {
                        doc: "docs/pay.md".into(),
                        section: sec.clone(),
                        quote: q.to_string(),
                    }),
                    ..Default::default()
                },
            });
        }
        let report = s.apply(ops, &session());
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        // Earlier builds judged every pair: no created record is left.
        s.status
            .changes
            .retain(|c| c.kind != crate::store::CHANGE_REQ_CREATED);
        s.apply(
            vec![Op::ReportDiagnostic {
                id: String::new(),
                diagnostic: Diagnostic {
                    goal: None,
                    rule: "contradiction".into(),
                    severity: "error".into(),
                    subjects: vec!["req:pay-1".into(), "req:pay-2".into()],
                    message: "an Order cannot be due in both 30 and 21 days".into(),
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
                                facts: Vec::new(),
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
        let id = s
            .graph
            .diagnostics
            .iter()
            .find(|(_, d)| d.rule == "contradiction")
            .map(|(id, _)| id.clone())
            .unwrap();
        drop(s);

        answer(&project, &out, &id, Reply::Choice(0), None).unwrap();
        let s = Store::load(&out);
        assert!(s.graph.requirements["req:pay-1"]
            .statement
            .contains("within 21 days"));
        // Both others are computed neighbors of the revised requirement.
        let nbrs: Vec<String> = s
            .requirement_neighbors_scored("req:pay-1")
            .into_iter()
            .map(|(_, id)| id)
            .collect();
        assert!(nbrs.contains(&"req:pay-2".to_string()), "{:?}", nbrs);
        assert!(nbrs.contains(&"req:pay-3".to_string()), "{:?}", nbrs);
        let revised = s
            .status
            .changes
            .iter()
            .find(|c| c.kind == crate::store::CHANGE_REQ_REVISED && c.subject == "req:pay-1")
            .expect("the rewrite revised req:pay-1");
        assert_eq!(revised.judged(), vec!["req:pay-2".to_string()]);
        let board = crate::board::Board::derive(&s, &project, &crate::board::tests::control_auto());
        let pair = |a: &str, b: &str| crate::goals::pair_target(a, b);
        let ids: Vec<&str> = board.goals.iter().map(|g| g.id.as_str()).collect();
        let judged = format!("g:rejudge-pair:{}", pair("req:pay-1", "req:pay-2"));
        let other = format!("g:rejudge-pair:{}", pair("req:pay-1", "req:pay-3"));
        assert!(board.goal(&judged).is_none(), "{:?}", ids);
        assert!(board
            .waiting_pair(&pair("req:pay-1", "req:pay-2"))
            .is_none());
        assert!(board.goal(&other).is_some(), "{:?}", ids);
        crate::vfs::remove_dir_all(&dir).ok();
    }

    fn arc(from: &str, to: &str, trigger: &str) -> Transition {
        Transition {
            subject: "ent:theo".into(),
            from: from.into(),
            to: to.into(),
            trigger: Some(trigger.into()),
            guard: None,
        }
    }

    // The mechanical follow: a replaced run that is exactly one state moves it, a run
    // inside the trigger splices into it, an untouched transition stays, and a run
    // wider than a state is ambiguous. Mirrors docs/compiler/model/diagnostic.md#answers.
    #[test]
    fn a_rewrite_follows_onto_a_transition_only_when_one_to_one() {
        let t = arc(
            "acquaintance",
            "ally",
            "Ines confronts Theo about the survey",
        );
        let old =
            "When\nInes confronts Theo about the survey, Theo moves from acquaintance to ally.";
        let new =
            "When\nInes confronts Theo about the survey, Theo moves from acquaintance to rival.";
        assert_eq!(
            follow_transition(old, new, &t),
            Follow::Moves(arc(
                "acquaintance",
                "rival",
                "Ines confronts Theo about the survey"
            ))
        );
        // A table row: the pipes are not words.
        let row = arc(
            "acquaintance",
            "rival",
            "Ines confronts Theo about the survey",
        );
        assert_eq!(
            follow_transition(
                "| acquaintance | rival | Ines confronts Theo about the survey |",
                "| acquaintance | ally | Ines confronts Theo about the survey |",
                &row
            ),
            Follow::Moves(arc(
                "acquaintance",
                "ally",
                "Ines confronts Theo about the survey"
            ))
        );
        // Both states replaced one to one.
        assert_eq!(
            follow_transition(
                "Theo moves from acquaintance to ally.",
                "Theo moves from stranger to rival.",
                &t
            ),
            Follow::Moves(arc(
                "stranger",
                "rival",
                "Ines confronts Theo about the survey"
            ))
        );
        // A run inside the trigger splices into it.
        assert_eq!(
            follow_transition(old, &old.replace("the survey", "the ledger"), &t),
            Follow::Moves(arc(
                "acquaintance",
                "ally",
                "Ines confronts Theo about the ledger"
            ))
        );
        // A rewrite that touches neither state nor trigger leaves it.
        assert_eq!(
            follow_transition(old, &old.replace("moves", "shifts"), &t),
            Follow::Stays
        );
        // One run removes both states with no one-to-one replacement.
        assert_eq!(
            follow_transition(
                "Theo moves from acquaintance to ally.",
                "Theo turns against her.",
                &t
            ),
            Follow::Ambiguous
        );
        // A state removed with nothing in its place.
        assert_eq!(
            follow_transition(
                "Theo moves from acquaintance to ally.",
                "Theo moves from acquaintance.",
                &t
            ),
            Follow::Ambiguous
        );
    }

    // A project with one quoted transition requirement and a contradiction whose one
    // edit option rewrites its sentence to `new_text`, carrying `facts`.
    fn seed_arc(
        label: &str,
        new_text: &str,
        facts: Vec<crate::model::FactChange>,
    ) -> (Project, PathBuf, String) {
        let dir = std::env::temp_dir().join(format!(
            "jazyk-answer-arc-{}-{}",
            label,
            crate::sys::process_id()
        ));
        crate::vfs::remove_dir_all(&dir).ok();
        crate::vfs::create_dir_all(dir.join("docs")).unwrap();
        crate::vfs::write({ let path = dir.join(".jazyk/jazyk.toml"); crate::vfs::create_dir_all(path.parent().unwrap()).unwrap(); path }, "version = 1\n\n[[sets]]\nid = \"ds_main\"\nname = \"Main\"\ndocs = [\"docs/**/*.md\"]\nroots = []\nsource = []\n").unwrap();
        let quote =
            "When Ines confronts Theo about the survey, Theo moves from acquaintance to ally.";
        crate::vfs::write(
            dir.join("docs/arcs.md"),
            format!("# Arcs\n\n## The midpoint\n\n{}\n", quote),
        )
        .unwrap();
        let project = Project::load(&dir);
        let out = project.out.clone();
        let (parsed, _) = crate::reconcile::parse_all(&project);
        let mut s = Store::load(&out);
        s.sync_docs(&parsed);
        let sec = s.docs["docs/arcs.md"]
            .sections
            .keys()
            .find(|k| k.contains("midpoint"))
            .unwrap()
            .clone();
        let entity = |name: &str| crate::model::Entity {
            name: name.into(),
            ..Default::default()
        };
        let report = s.apply(
            vec![
                Op::CreateEntity {
                    id: "ent:ines".into(),
                    entity: entity("Ines"),
                },
                Op::CreateEntity {
                    id: "ent:theo".into(),
                    entity: entity("Theo"),
                },
                Op::CreateRequirement {
                    id: "req:arcs-29".into(),
                    requirement: crate::model::Requirement {
                        statement: quote.into(),
                        entities: vec!["ent:ines".into(), "ent:theo".into()],
                        transition: Some(arc(
                            "acquaintance",
                            "ally",
                            "Ines confronts Theo about the survey",
                        )),
                        source: Some(SourceRef {
                            doc: "docs/arcs.md".into(),
                            section: sec.clone(),
                            quote: quote.into(),
                        }),
                        ..Default::default()
                    },
                },
                Op::ReportDiagnostic {
                    id: String::new(),
                    diagnostic: Diagnostic {
                        goal: None,
                        rule: "contradiction".into(),
                        severity: "error".into(),
                        subjects: vec!["req:arcs-29".into()],
                        message: "ally or rival".into(),
                        reasoning: None,
                        lifecycle: "open".into(),
                        triage: None,
                        prompt: Some(DiagnosticPrompt {
                            question: "Ally or rival?".into(),
                            options: vec![PromptOption {
                                label: "rival".into(),
                                edit: Some(SuggestedEdit {
                                    doc: "docs/arcs.md".into(),
                                    section: sec.clone(),
                                    old_text: "Theo moves from acquaintance to ally.".into(),
                                    new_text: new_text.into(),
                                    facts,
                                }),
                                answer: None,
                            }],
                            freeform: true,
                        }),
                        answer: None,
                        created: None,
                        updated: None,
                    },
                },
            ],
            &session(),
        );
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        let did = s.graph.diagnostics.keys().next().unwrap().clone();
        (project, out, did)
    }

    // An option with no facts: the transition follows the rewrite mechanically, in the
    // same changeset as the prose. Mirrors docs/compiler/model/diagnostic.md#answers.
    #[test]
    fn an_answer_moves_the_transition_its_rewrite_changes() {
        let (project, out, did) = seed_arc(
            "follow",
            "Theo moves from acquaintance to rival.",
            Vec::new(),
        );
        let before = Store::load(&out).status.generation;
        let v = answer(&project, &out, &did, Reply::Choice(0), None).unwrap();
        assert_eq!(v["followed"][0]["id"], "req:arcs-29", "{}", v);
        let s = Store::load(&out);
        assert_eq!(s.status.generation, before + 1, "one changeset");
        let r = &s.graph.requirements["req:arcs-29"];
        assert!(r.statement.ends_with("to rival."), "{}", r.statement);
        assert_eq!(
            r.transition,
            Some(arc(
                "acquaintance",
                "rival",
                "Ines confronts Theo about the survey"
            ))
        );
        assert_eq!(s.graph.diagnostics[&did].lifecycle, "resolved");
        crate::vfs::remove_dir_all(&project.root).ok();
    }

    // Listed facts apply through the update_requirement gates in the same changeset,
    // and a requirement whose transition a fact sets is not followed mechanically.
    #[test]
    fn an_answer_applies_the_options_listed_facts() {
        let facts = vec![
            crate::model::FactChange {
                id: "req:arcs-29".into(),
                field: "transition".into(),
                value: json!({"subject": "ent:theo", "from": "acquaintance", "to": "rival",
                              "trigger": "Ines confronts Theo about the stolen survey"}),
            },
            crate::model::FactChange {
                id: "req:arcs-29".into(),
                field: "facets".into(),
                value: json!([{"facet": "behavior", "reasoning": "a stance change on a plot event"}]),
            },
        ];
        let (project, out, did) =
            seed_arc("facts", "Theo moves from acquaintance to rival.", facts);
        let before = Store::load(&out).status.generation;
        let v = answer(&project, &out, &did, Reply::Choice(0), None).unwrap();
        assert_eq!(v["followed"], json!([]), "{}", v);
        let s = Store::load(&out);
        assert_eq!(s.status.generation, before + 1, "one changeset");
        let r = &s.graph.requirements["req:arcs-29"];
        assert_eq!(
            r.transition,
            Some(arc(
                "acquaintance",
                "rival",
                "Ines confronts Theo about the stolen survey"
            ))
        );
        assert_eq!(r.facets.len(), 1);
        assert_eq!(r.facets[0].facet, "behavior");
        assert!(r.source.as_ref().unwrap().quote.ends_with("to rival."));
        let entry = journal_kind(&out, before + 1);
        assert_eq!(entry.kind, "answer");
        crate::vfs::remove_dir_all(&project.root).ok();
    }

    // A fact the gates refuse fails the whole answer: the document, the graph, and the
    // diagnostic stay as they were, and the diagnostic stays answerable.
    #[test]
    fn a_refused_fact_fails_the_whole_answer() {
        let facts = vec![crate::model::FactChange {
            id: "req:arcs-29".into(),
            field: "transition".into(),
            value: json!({"subject": "ent:survey", "from": "hidden", "to": "found"}),
        }];
        let (project, out, did) =
            seed_arc("refused", "Theo moves from acquaintance to rival.", facts);
        let before = Store::load(&out).status.generation;
        let err = answer(&project, &out, &did, Reply::Choice(0), None).unwrap_err();
        assert!(
            err.contains("refused") && err.contains("ent:survey"),
            "{}",
            err
        );
        let text = crate::vfs::read_to_string(project.root.join("docs/arcs.md")).unwrap();
        assert!(text.contains("to ally."), "{}", text);
        let s = Store::load(&out);
        assert_eq!(s.status.generation, before, "nothing committed");
        let d = &s.graph.diagnostics[&did];
        assert_eq!(d.lifecycle, "open");
        assert!(d.answer.is_none());
        assert_eq!(
            s.graph.requirements["req:arcs-29"]
                .transition
                .as_ref()
                .unwrap()
                .to,
            "ally"
        );
        crate::vfs::remove_dir_all(&project.root).ok();
    }

    // An ambiguous rewrite leaves the transition as it was, names the requirement in
    // the reply, and the revision opens the review that re-reads it.
    #[test]
    fn an_ambiguous_rewrite_leaves_the_transition_to_review() {
        let (project, out, did) = seed_arc("ambiguous", "Theo turns against her.", Vec::new());
        let v = answer(&project, &out, &did, Reply::Choice(0), None).unwrap();
        assert_eq!(v["review"], json!(["req:arcs-29"]), "{}", v);
        assert_eq!(v["followed"], json!([]), "{}", v);
        let s = Store::load(&out);
        let r = &s.graph.requirements["req:arcs-29"];
        assert!(
            r.statement.ends_with("Theo turns against her."),
            "{}",
            r.statement
        );
        assert_eq!(
            r.transition,
            Some(arc(
                "acquaintance",
                "ally",
                "Ines confronts Theo about the survey"
            ))
        );
        assert!(s
            .status
            .has_change(crate::store::CHANGE_REQ_REVISED, "req:arcs-29"));
        crate::vfs::remove_dir_all(&project.root).ok();
    }
}
