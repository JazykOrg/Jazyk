// The journal's cost report: what every build since the store began spent, by goal
// kind, by document, and per requirement and criterion at the graph's current size.
// A reading over the journal and the graph; it writes nothing and calls no model.
// Mirrors docs/frontends/cli.md#jazyk-status.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::model::{Graph, JournalEntry, Provenance, SourceRef};

// The bucket for a goal whose target no document holds.
pub const NO_DOCUMENT: &str = "(no document)";

#[derive(Default)]
struct Line {
    goals: u64,
    tokens: f64,
    // The cached input among the tokens (docs/compiler/sessions.md#budgets).
    cached: f64,
    unmeasured: u64,
}

impl Line {
    fn measured(&self) -> u64 {
        self.goals - self.unmeasured
    }

    fn to_json(&self) -> Value {
        // A line with no measured goal has an unknown spend, never a zero one.
        let (tokens, cached) = if self.measured() > 0 {
            (
                json!(self.tokens.round() as u64),
                json!(self.cached.round() as u64),
            )
        } else {
            (Value::Null, Value::Null)
        };
        json!({"goals": self.goals, "tokens": tokens, "cached": cached, "unmeasured": self.unmeasured})
    }
}

// The report over the out directory: the journal as written, the graph as loaded.
pub fn report_at(out: &Path) -> Value {
    let store = crate::store::Store::load(out);
    report(&crate::goals::read_journal(out), &store.graph)
}

pub fn report(entries: &[JournalEntry], graph: &Graph) -> Value {
    let mut sessions = 0u64;
    // Sessions carrying a figure: measured by the harness, or reported by the agent
    // (`tokensSource: agent`); `reported` is the part of `measured` the agent priced.
    let mut measured = 0u64;
    let mut reported = 0u64;
    let mut tokens = 0u64;
    // Cached input over the measured sessions; an entry without the field adds 0.
    let mut cached = 0u64;
    let mut by_kind: BTreeMap<String, Line> = BTreeMap::new();
    let mut by_doc: BTreeMap<String, Line> = BTreeMap::new();
    for e in entries.iter().filter(|e| e.kind == "session") {
        sessions += 1;
        if let Some(t) = e.tokens {
            measured += 1;
            if e.tokens_source.as_deref() == Some(crate::model::TOKENS_SOURCE_AGENT) {
                reported += 1;
            }
            tokens += t;
            cached += e.cached_tokens.unwrap_or(0);
        }
        let mut goals: Vec<&str> = e.resolved_goals.iter().map(|r| r.goal.as_str()).collect();
        if goals.is_empty() {
            goals = e.batch.iter().map(String::as_str).collect();
        }
        // A session that names no goal still spent: it lands under its own kind.
        let goals = if goals.is_empty() { vec![""] } else { goals };
        let n_goals = goals.len() as f64;
        let share = e.tokens.map(|t| {
            (
                t as f64 / n_goals,
                e.cached_tokens.unwrap_or(0) as f64 / n_goals,
            )
        });
        for goal in goals {
            let (kind, target) = crate::goals::parse_goal_id(goal).unwrap_or(("(no goal)", ""));
            charge(by_kind.entry(kind.to_string()).or_default(), share, 1.0);
            let docs = documents_of(target, graph);
            let n = docs.len() as f64;
            let mut seen: BTreeMap<&str, f64> = BTreeMap::new();
            for d in &docs {
                *seen.entry(d.as_str()).or_default() += 1.0 / n;
            }
            for (doc, frac) in seen {
                charge(by_doc.entry(doc.to_string()).or_default(), share, frac);
            }
        }
    }
    let per = |count: usize| {
        if measured > 0 && count > 0 {
            json!((tokens as f64 / count as f64).round() as u64)
        } else {
            Value::Null
        }
    };
    let lines = |m: &BTreeMap<String, Line>| {
        let mut v: Vec<(&String, &Line)> = m.iter().collect();
        v.sort_by(|a, b| {
            b.1.tokens
                .partial_cmp(&a.1.tokens)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.1.unmeasured.cmp(&a.1.unmeasured))
                .then(a.0.cmp(b.0))
        });
        v.into_iter()
            .map(|(k, l)| {
                let mut o = l.to_json();
                o["name"] = json!(k);
                o
            })
            .collect::<Vec<_>>()
    };
    let gens: Vec<u64> = entries.iter().map(|e| e.generation).collect();
    json!({
        "first": gens.iter().min(),
        "last": gens.iter().max(),
        "sessions": sessions,
        "measured": measured - reported,
        "reported": reported,
        "unmeasured": sessions - measured,
        "tokens": if measured > 0 { json!(tokens) } else { Value::Null },
        "cached": if measured > 0 { json!(cached) } else { Value::Null },
        "by_kind": lines(&by_kind),
        "by_document": lines(&by_doc),
        "requirements": graph.requirements.len(),
        "criteria": graph.criteria.len(),
        "per_requirement": per(graph.requirements.len()),
        "per_criterion": per(graph.criteria.len()),
    })
}

// `share` is one goal's (tokens, cached) slice of its session's spend, or None for
// an unmeasured session.
fn charge(line: &mut Line, share: Option<(f64, f64)>, frac: f64) {
    line.goals += 1;
    match share {
        Some((t, c)) => {
            line.tokens += t * frac;
            line.cached += c * frac;
        }
        None => line.unmeasured += 1,
    }
}

// The documents a goal target sits in: a section reference or a document path names
// its own; a node's is the document its quote sits in; a pair's, both sides'. A
// target no document holds lands under NO_DOCUMENT.
fn documents_of(target: &str, graph: &Graph) -> Vec<String> {
    let sides: Vec<&str> = if is_path(target) {
        vec![target]
    } else {
        target.split('~').collect()
    };
    sides
        .into_iter()
        .map(|side| {
            if is_path(side) {
                side.split('#').next().unwrap_or(side).to_string()
            } else {
                node_document(side, graph).unwrap_or_else(|| NO_DOCUMENT.to_string())
            }
        })
        .collect()
}

fn is_path(target: &str) -> bool {
    target.contains('#')
        || (target.contains('/') && !target.contains(':'))
        || crate::md::is_document_target(target)
}

fn quoted(p: &Option<Provenance>, source: &Option<SourceRef>) -> Option<String> {
    match p {
        Some(Provenance::Quote(s)) => Some(s.doc.clone()),
        _ => source.as_ref().map(|s| s.doc.clone()),
    }
}

fn node_document(id: &str, graph: &Graph) -> Option<String> {
    node_document_at(id, graph, 0)
}

// One hop at most: a criterion names its requirement, and a requirement names a
// document. A record that loops never recurses past that.
fn node_document_at(id: &str, graph: &Graph, depth: usize) -> Option<String> {
    if depth > 1 {
        return None;
    }
    let mut cur = id;
    for _ in 0..graph.redirects.len() {
        match graph.redirects.get(cur) {
            Some(next) if !next.is_empty() => cur = next,
            _ => break,
        }
    }
    if let Some(e) = graph.entities.get(cur) {
        if let Some(Provenance::Quote(s)) = &e.provenance {
            return Some(s.doc.clone());
        }
        return e.mentions.first().map(|m| m.doc.clone());
    }
    if let Some(r) = graph.requirements.get(cur) {
        return quoted(&r.provenance, &r.source);
    }
    if let Some(c) = graph.criteria.get(cur) {
        return quoted(&c.provenance, &c.source)
            .or_else(|| node_document_at(&c.requirement, graph, depth + 1));
    }
    None
}

// The text form. Mirrors the example in docs/frontends/cli.md#jazyk-status.
pub fn render(r: &Value) -> String {
    let sessions = r["sessions"].as_u64().unwrap_or(0);
    if sessions == 0 {
        return "cost: no session in the journal\n".to_string();
    }
    let mut out = String::new();
    let range = format!("g{}..g{}", r["first"], r["last"]);
    // The agent-reported count prints only when there is one.
    let reported = match r["reported"].as_u64() {
        Some(n) if n > 0 => format!(", {} agent-reported", n),
        _ => String::new(),
    };
    let head = format!(
        "cost: {}, {} sessions ({} measured{}, {} unmeasured)",
        range, sessions, r["measured"], reported, r["unmeasured"]
    );
    // The cached figure and its column print only when some entry carried one: a
    // journal from endpoints that report no cache read looks as it always did.
    let cached_total = r["cached"].as_u64().unwrap_or(0);
    let show_cached = cached_total > 0;
    match r["tokens"].as_u64() {
        Some(t) if show_cached => out.push_str(&format!(
            "{}, {} tokens, {} cached ({}%)\n",
            head,
            t,
            cached_total,
            (cached_total as f64 * 100.0 / t.max(1) as f64).round() as u64
        )),
        Some(t) => out.push_str(&format!("{}, {} tokens\n", head, t)),
        None => out.push_str(&format!("{}, spend unknown\n", head)),
    }
    let num = |v: &Value| {
        v.as_u64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "-".into())
    };
    let cached_col = |l: &Value| {
        if show_cached {
            format!("{:>9}", num(&l["cached"]))
        } else {
            String::new()
        }
    };
    let cached_head = if show_cached {
        format!("{:>9}", "cached")
    } else {
        String::new()
    };
    let width = |key: &str| {
        r[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|l| l["name"].as_str().unwrap_or("").len())
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0)
            .max(20)
    };
    let w = width("by_kind");
    out.push_str(&format!(
        "by kind:{:pad$}{:>7}{:>9}{}{:>10}{:>12}\n",
        "",
        "goals",
        "tokens",
        cached_head,
        "per goal",
        "unmeasured",
        pad = w + 2 - "by kind:".len()
    ));
    for l in r["by_kind"].as_array().into_iter().flatten() {
        let goals = l["goals"].as_u64().unwrap_or(0);
        let unmeasured = l["unmeasured"].as_u64().unwrap_or(0);
        let per_goal = match l["tokens"].as_u64() {
            Some(t) if goals > unmeasured => {
                (t as f64 / (goals - unmeasured) as f64).round().to_string()
            }
            _ => "-".into(),
        };
        out.push_str(&format!(
            "  {:w$}{:>7}{:>9}{}{:>10}{:>12}\n",
            l["name"].as_str().unwrap_or(""),
            goals,
            num(&l["tokens"]),
            cached_col(l),
            per_goal,
            unmeasured,
            w = w
        ));
    }
    let w = width("by_document");
    out.push_str(&format!(
        "by document:{:pad$}{:>7}{:>9}{}{:>12}\n",
        "",
        "goals",
        "tokens",
        cached_head,
        "unmeasured",
        pad = w + 2 - "by document:".len()
    ));
    for l in r["by_document"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:w$}{:>7}{:>9}{}{:>12}\n",
            l["name"].as_str().unwrap_or(""),
            num(&l["goals"]),
            num(&l["tokens"]),
            cached_col(l),
            num(&l["unmeasured"]),
            w = w
        ));
    }
    if r["tokens"].is_null() {
        out.push_str(&format!(
            "per requirement, per criterion: unknown, no session was measured ({} requirements, {} criteria)\n",
            r["requirements"], r["criteria"]
        ));
    } else {
        out.push_str(&format!(
            "per requirement: {} tokens ({} requirements)\n",
            num(&r["per_requirement"]),
            r["requirements"]
        ));
        out.push_str(&format!(
            "per criterion: {} tokens ({} criteria)\n",
            num(&r["per_criterion"]),
            r["criteria"]
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Criterion, Entity, Requirement, Resolved};
    use crate::store::Store;

    fn src(doc: &str) -> SourceRef {
        SourceRef {
            doc: doc.into(),
            section: String::new(),
            quote: "q".into(),
        }
    }

    fn session(g: u64, goals: &[&str], tokens: Option<u64>) -> JournalEntry {
        JournalEntry {
            build: format!("g{}", g),
            generation: g,
            kind: "session".into(),
            resolved_goals: goals
                .iter()
                .map(|id| Resolved {
                    goal: id.to_string(),
                    justification: "j".into(),
                    evidence: Value::Null,
                })
                .collect(),
            tokens,
            ..Default::default()
        }
    }

    #[test]
    fn the_report_splits_spend_by_kind_and_document_and_keeps_unmeasured_apart() {
        let out =
            std::env::temp_dir().join(format!("jazyk-costs-test-{}", crate::sys::process_id()));
        crate::vfs::remove_dir_all(&out).ok();
        let mut store = Store {
            out: out.clone(),
            ..Default::default()
        };
        let g = &mut store.graph;
        g.entities.insert(
            "ent:order".into(),
            Entity {
                name: "Order".into(),
                mentions: vec![src("docs/orders.md")],
                ..Default::default()
            },
        );
        for (id, doc) in [
            ("req:orders-1", "docs/orders.md"),
            ("req:pay-1", "docs/pay.md"),
        ] {
            g.requirements.insert(
                id.into(),
                Requirement {
                    statement: "s".into(),
                    provenance: Some(Provenance::Quote(src(doc))),
                    ..Default::default()
                },
            );
        }
        // A criterion without a quote of its own takes its requirement's document.
        g.criteria.insert(
            "crit:pay-1".into(),
            Criterion {
                requirement: "req:pay-1".into(),
                statement: "c".into(),
                ..Default::default()
            },
        );
        // A merged requirement resolves to its survivor's document.
        g.redirects
            .insert("req:orders-9".into(), "req:orders-1".into());
        store.save_for_test();
        let mut entries = vec![
            JournalEntry {
                build: "g1".into(),
                generation: 1,
                kind: "edit".into(),
                ..Default::default()
            },
            // 1000 over two goals: 500 to each section's document.
            session(
                2,
                &[
                    "g:reconcile-section:docs/orders.md#/orders",
                    "g:reconcile-section:docs/pay.md#/pay",
                ],
                Some(1000),
            ),
            // 600 over a pair across two documents and an entity review.
            session(
                3,
                &[
                    "g:rejudge-pair:req:orders-9~req:pay-1",
                    "g:review-entity:ent:order",
                ],
                Some(600),
            ),
            // An external agent's session: counted, never priced.
            session(
                4,
                &["g:derive-criteria:crit:pay-1", "g:curate-view:view:x"],
                None,
            ),
            session(5, &["g:curate-view:view:x"], Some(100)),
        ];
        // Two entries carry a cached input figure; the rest predate the field or
        // came from an endpoint that reports none, and add 0 cached.
        entries[1].cached_tokens = Some(400);
        entries[4].cached_tokens = Some(50);
        crate::vfs::create_dir_all(crate::layout::journal_dir(&out)).unwrap();
        for e in &entries {
            crate::vfs::write(
                crate::layout::journal_dir(&out).join(format!("g{}.yaml", e.generation)),
                serde_norway::to_string(e).unwrap(),
            )
            .unwrap();
        }
        let r = report_at(&out);
        assert_eq!(r["first"], 1);
        assert_eq!(r["last"], 5);
        assert_eq!(r["sessions"], 4);
        assert_eq!(r["measured"], 3);
        assert_eq!(r["unmeasured"], 1);
        assert_eq!(r["tokens"], 1700);
        assert_eq!(r["cached"], 450);
        let line = |key: &str, name: &str| -> Value {
            r[key]
                .as_array()
                .unwrap()
                .iter()
                .find(|l| l["name"] == name)
                .cloned()
                .unwrap_or_else(|| panic!("no {} line {}", key, name))
        };
        let k = line("by_kind", "reconcile-section");
        assert_eq!(
            (
                k["goals"].clone(),
                k["tokens"].clone(),
                k["unmeasured"].clone()
            ),
            (json!(2), json!(1000), json!(0))
        );
        assert_eq!(line("by_kind", "rejudge-pair")["tokens"], 300);
        assert_eq!(line("by_kind", "review-entity")["tokens"], 300);
        // Cached input follows the same split: 400 over two reconcile goals, none on
        // the pair whose entry lacks the field, 50 on the one measured curate goal.
        assert_eq!(k["cached"], 400);
        assert_eq!(line("by_kind", "rejudge-pair")["cached"], 0);
        assert_eq!(line("by_kind", "curate-view")["cached"], 50);
        assert_eq!(line("by_kind", "derive-criteria")["cached"], Value::Null);
        let dc = line("by_kind", "derive-criteria");
        assert_eq!(
            (
                dc["goals"].clone(),
                dc["tokens"].clone(),
                dc["unmeasured"].clone()
            ),
            (json!(1), Value::Null, json!(1))
        );
        let cv = line("by_kind", "curate-view");
        assert_eq!(
            (
                cv["goals"].clone(),
                cv["tokens"].clone(),
                cv["unmeasured"].clone()
            ),
            (json!(2), json!(100), json!(1))
        );
        // The heaviest kind leads.
        assert_eq!(r["by_kind"][0]["name"], "reconcile-section");
        // orders.md: 500 + 150 (its side of the pair) + 300 (the entity review).
        let o = line("by_document", "docs/orders.md");
        assert_eq!(
            (
                o["goals"].clone(),
                o["tokens"].clone(),
                o["unmeasured"].clone()
            ),
            (json!(3), json!(950), json!(0))
        );
        // pay.md: 500 + 150, and the unmeasured criterion goal through its requirement.
        let p = line("by_document", "docs/pay.md");
        assert_eq!(
            (
                p["goals"].clone(),
                p["tokens"].clone(),
                p["unmeasured"].clone()
            ),
            (json!(3), json!(650), json!(1))
        );
        let n = line("by_document", NO_DOCUMENT);
        assert_eq!(
            (
                n["goals"].clone(),
                n["tokens"].clone(),
                n["unmeasured"].clone()
            ),
            (json!(2), json!(100), json!(1))
        );
        assert_eq!(r["requirements"], 2);
        assert_eq!(r["criteria"], 1);
        assert_eq!(r["per_requirement"], 850);
        assert_eq!(r["per_criterion"], 1700);
        let text = render(&r);
        assert!(
            text.starts_with("cost: g1..g5, 4 sessions (3 measured, 1 unmeasured), 1700 tokens, 450 cached (26%)\n"),
            "{}",
            text
        );
        let kline = text
            .lines()
            .find(|l| l.trim_start().starts_with("reconcile-section"))
            .unwrap();
        assert_eq!(
            kline.split_whitespace().collect::<Vec<_>>(),
            ["reconcile-section", "2", "1000", "400", "500", "0"]
        );
        assert!(
            text.contains("per requirement: 850 tokens (2 requirements)"),
            "{}",
            text
        );
        let oline = text
            .lines()
            .find(|l| l.trim_start().starts_with("docs/orders.md"))
            .unwrap();
        assert_eq!(
            oline.split_whitespace().collect::<Vec<_>>(),
            ["docs/orders.md", "3", "950", "200", "0"]
        );
        // A kind no session measured prints no number, not a zero.
        let dline = text
            .lines()
            .find(|l| l.trim_start().starts_with("derive-criteria"))
            .unwrap();
        assert!(dline.contains(" - "), "{}", dline);
        crate::vfs::remove_dir_all(&out).ok();
    }

    #[test]
    fn a_journal_with_no_measured_session_says_its_spend_is_unknown() {
        let r = report(
            &[session(2, &["g:reconcile-section:docs/a.md#/a"], None)],
            &Graph::default(),
        );
        assert_eq!(r["tokens"], Value::Null);
        assert_eq!(r["per_requirement"], Value::Null);
        let text = render(&r);
        assert!(text.contains("spend unknown"), "{}", text);
        assert!(
            text.contains("unknown, no session was measured"),
            "{}",
            text
        );
        assert_eq!(r["cached"], Value::Null);
    }

    // A journal whose entries carry no cachedTokens (older entries, endpoints that
    // report no cache read) prints as before: no cached figure, no cached column, and
    // the JSON says 0 cached over the measured sessions, not unknown.
    #[test]
    fn a_journal_without_cached_tokens_prints_no_cached_column() {
        let r = report(
            &[
                session(2, &["g:reconcile-section:docs/a.md#/a"], Some(1000)),
                session(3, &["g:review-entity:ent:a"], None),
            ],
            &Graph::default(),
        );
        assert_eq!(r["cached"], 0);
        assert_eq!(r["by_kind"][0]["cached"], 0);
        let text = render(&r);
        assert!(
            text.starts_with("cost: g2..g3, 2 sessions (1 measured, 1 unmeasured), 1000 tokens\n"),
            "{}",
            text
        );
        assert!(!text.contains("cached"), "{}", text);
        let kline = text
            .lines()
            .find(|l| l.trim_start().starts_with("reconcile-section"))
            .unwrap();
        assert_eq!(
            kline.split_whitespace().collect::<Vec<_>>(),
            ["reconcile-section", "1", "1000", "1000", "0"]
        );
    }

    // A session the agent priced itself (tokensSource: agent) counts in the tokens like
    // a measured one and apart from it in the session counts.
    // Mirrors docs/frontends/cli.md#jazyk-status.
    #[test]
    fn an_agent_reported_session_counts_in_the_tokens_and_apart_in_the_sessions() {
        let mut reported = session(3, &["g:review-entity:ent:a"], Some(2000));
        reported.tokens_source = Some("agent".into());
        reported.cached_tokens = Some(500);
        let r = report(
            &[
                session(2, &["g:reconcile-section:docs/a.md#/a"], Some(1000)),
                reported,
                session(4, &["g:curate-view:view:x"], None),
            ],
            &Graph::default(),
        );
        assert_eq!(r["sessions"], 3);
        assert_eq!(r["measured"], 1);
        assert_eq!(r["reported"], 1);
        assert_eq!(r["unmeasured"], 1);
        assert_eq!(r["tokens"], 3000);
        assert_eq!(r["cached"], 500);
        let text = render(&r);
        assert!(
            text.starts_with("cost: g2..g4, 3 sessions (1 measured, 1 agent-reported, 1 unmeasured), 3000 tokens, 500 cached (17%)\n"),
            "{}",
            text
        );
    }
}
