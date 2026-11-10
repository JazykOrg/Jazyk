use super::*;

// ---- GC: dedupe-candidates ----

pub struct DedupeCandidates;

// The weight of one name token: its inverse document frequency over the graph's entity
// token sets, 1 for a token held by `lookalike-distinctive-holders` entities or fewer,
// falling from there. Mirrors docs/compiler/goals/dedupe-candidates.md#the-lookalike-score.
pub fn token_weight(entities: usize, holders: usize) -> f64 {
    let e = entities as f64;
    let k = LOOKALIKE_DISTINCTIVE_HOLDERS as f64;
    let w = (1.0 + e / holders.max(1) as f64).ln() / (1.0 + e / k).ln();
    w.min(1.0)
}

// The weight of every token any entity's name or aliases hold, over the given token
// sets (one per entity in the graph).
fn token_weights(tokens: &[BTreeSet<String>]) -> BTreeMap<String, f64> {
    let mut holders: BTreeMap<&str, usize> = BTreeMap::new();
    for ts in tokens {
        for t in ts {
            *holders.entry(t.as_str()).or_default() += 1;
        }
    }
    holders
        .into_iter()
        .map(|(t, n)| (t.to_string(), token_weight(tokens.len(), n)))
        .collect()
}

// The name token sets of every entity, in id order.
fn all_name_tokens(store: &Store) -> Vec<BTreeSet<String>> {
    store.graph.entities.values().map(name_tokens).collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lookalike {
    pub a: String,
    pub b: String,
    pub score: f64,
    pub tokens: f64,
    pub documents: f64,
    pub shared: Vec<String>,
    pub docs_a: BTreeSet<String>,
    pub docs_b: BTreeSet<String>,
}

// The pairs the structural contributions tie: a composition, aggregation,
// generalization, or instantiation between two entities, both ways round.
fn tied_pairs(store: &Store) -> BTreeSet<(String, String)> {
    let mut tied: BTreeSet<(String, String)> = BTreeSet::new();
    for rel in store.graph.relationships.values() {
        for c in &rel.contributions {
            if matches!(
                c.r#type.as_str(),
                "composition" | "aggregation" | "generalization" | "instantiation"
            ) {
                tied.insert((c.a.clone(), c.b.clone()));
                tied.insert((c.b.clone(), c.a.clone()));
            }
        }
    }
    tied
}

// One candidate pair's score, None when the pair fails a candidate rule or the
// threshold. The pair rules and the formula of
// docs/compiler/goals/dedupe-candidates.md#the-lookalike-score.
#[allow(clippy::too_many_arguments)]
fn lookalike_of(
    store: &Store,
    tied: &BTreeSet<(String, String)>,
    weights: &BTreeMap<String, f64>,
    a: &str,
    ta: &BTreeSet<String>,
    docs_a: &BTreeSet<String>,
    b: &str,
    tb: &BTreeSet<String>,
    docs_b: &BTreeSet<String>,
) -> Option<Lookalike> {
    let (ea, eb) = (&store.graph.entities[a], &store.graph.entities[b]);
    if ea.scope != eb.scope
        || store.is_ancestor(a, b)
        || store.is_ancestor(b, a)
        || tied.contains(&(a.to_string(), b.to_string()))
    {
        return None;
    }
    if docs_a.union(docs_b).count() < 2 {
        return None;
    }
    let (score, tokens, documents, shared) = score_pair(weights, ta, docs_a, tb, docs_b)?;
    if score + 1e-9 < LOOKALIKE_THRESHOLD {
        return None;
    }
    Some(Lookalike {
        a: a.to_string(),
        b: b.to_string(),
        score,
        tokens,
        documents,
        shared,
        docs_a: docs_a.clone(),
        docs_b: docs_b.clone(),
    })
}

// The formula over two token sets and two mention-document sets, the tokens weighted
// by `weights` (a token outside the map weighs 1): (score, tokens, documents, shared
// tokens), each figure rounded to two decimals; None when either name has no token or
// the names share none. The threshold is the caller's.
fn score_pair(
    weights: &BTreeMap<String, f64>,
    ta: &BTreeSet<String>,
    docs_a: &BTreeSet<String>,
    tb: &BTreeSet<String>,
    docs_b: &BTreeSet<String>,
) -> Option<(f64, f64, f64, Vec<String>)> {
    if ta.is_empty() || tb.is_empty() {
        return None;
    }
    let shared: Vec<String> = ta.intersection(tb).cloned().collect();
    if shared.is_empty() {
        return None;
    }
    let weigh = |ts: &[String]| -> f64 {
        ts.iter()
            .map(|t| weights.get(t).copied().unwrap_or(1.0))
            .sum()
    };
    let sum = |ts: &BTreeSet<String>| -> f64 {
        ts.iter()
            .map(|t| weights.get(t).copied().unwrap_or(1.0))
            .sum()
    };
    let tokens = 2.0 * weigh(&shared) / (sum(ta) + sum(tb));
    let union = docs_a.union(docs_b).count().max(1);
    let documents = docs_a.intersection(docs_b).count() as f64 / union as f64;
    let score = LOOKALIKE_TOKEN_WEIGHT * tokens + LOOKALIKE_DOCUMENT_WEIGHT * documents;
    let round = |x: f64| (x * 100.0).round() / 100.0;
    Some((round(score), round(tokens), round(documents), shared))
}

// The lookalike score over every candidate pair: two entities of one scope, neither
// an ancestor of the other, no structural contribution between them, spanning at
// least two mention documents. The name tokens and mention documents of every entity
// are computed once, and an inverted token index names the pairs that share a token:
// a pair sharing none scores nothing, so only those pairs are scored, in id order.
// Mirrors docs/compiler/goals/dedupe-candidates.md and
// docs/compiler/graph.md#derived-data.
pub fn lookalike_scores(store: &Store) -> Vec<Lookalike> {
    let tied = tied_pairs(store);
    let ids: Vec<&String> = store.graph.entities.keys().collect();
    let tokens: Vec<BTreeSet<String>> = ids
        .iter()
        .map(|id| name_tokens(&store.graph.entities[*id]))
        .collect();
    let docs: Vec<BTreeSet<String>> = ids
        .iter()
        .map(|id| {
            store.graph.entities[*id]
                .mentions
                .iter()
                .map(|m| m.doc.clone())
                .collect()
        })
        .collect();
    let weights = token_weights(&tokens);
    let mut by_token: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, ts) in tokens.iter().enumerate() {
        for t in ts {
            by_token.entry(t.as_str()).or_default().push(i);
        }
    }
    let mut pairs: BTreeSet<(usize, usize)> = BTreeSet::new();
    for holders in by_token.values() {
        for (k, i) in holders.iter().enumerate() {
            for j in &holders[k + 1..] {
                pairs.insert((*i, *j));
            }
        }
    }
    pairs
        .into_iter()
        .filter_map(|(i, j)| {
            lookalike_of(
                store, &tied, &weights, ids[i], &tokens[i], &docs[i], ids[j], &tokens[j], &docs[j],
            )
        })
        .collect()
}

// The lookalike scores by the plain pairwise scan over every entity pair: the
// reference the token index is checked against.
#[cfg(test)]
pub(crate) fn lookalike_scores_scan(store: &Store) -> Vec<Lookalike> {
    let tied = tied_pairs(store);
    let weights = token_weights(&all_name_tokens(store));
    let ids: Vec<&String> = store.graph.entities.keys().collect();
    let mut out = Vec::new();
    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            let (a, b) = (ids[i], ids[j]);
            let (ea, eb) = (&store.graph.entities[a], &store.graph.entities[b]);
            let docs_a: BTreeSet<String> = ea.mentions.iter().map(|m| m.doc.clone()).collect();
            let docs_b: BTreeSet<String> = eb.mentions.iter().map(|m| m.doc.clone()).collect();
            let (ta, tb) = (name_tokens(ea), name_tokens(eb));
            out.extend(lookalike_of(
                store, &tied, &weights, a, &ta, &docs_a, b, &tb, &docs_b,
            ));
        }
    }
    out
}

// The formula's value for one pair without the candidate rules or the threshold: what a
// remembered verdict records and is compared against. 0 when the names share no token.
// Mirrors docs/compiler/goals/dedupe-candidates.md#verdicts-are-remembered.
pub fn lookalike_pair_score(store: &Store, a: &str, b: &str) -> f64 {
    let (Some(ea), Some(eb)) = (store.graph.entities.get(a), store.graph.entities.get(b)) else {
        return 0.0;
    };
    let (ta, tb) = (name_tokens(ea), name_tokens(eb));
    let docs_a: BTreeSet<String> = ea.mentions.iter().map(|m| m.doc.clone()).collect();
    let docs_b: BTreeSet<String> = eb.mentions.iter().map(|m| m.doc.clone()).collect();
    let weights = token_weights(&all_name_tokens(store));
    score_pair(&weights, &ta, &docs_a, &tb, &docs_b)
        .map(|(score, _, _, _)| score)
        .unwrap_or(0.0)
}

// The scored pairs the board asks about: every candidate pair over the threshold whose
// verdict is not remembered (no `lookalikes` entry at the pair's score) and that no
// creation note told apart. The name index writes records for exactly these.
// Mirrors docs/compiler/goals/dedupe-candidates.md#created-when.
pub fn pending_lookalikes(store: &Store) -> Vec<Lookalike> {
    let told_apart = |x: &str, y: &str| {
        store
            .graph
            .entities
            .get(x)
            .is_some_and(|e| e.distinct_from.contains_key(y))
    };
    lookalike_scores(store)
        .into_iter()
        .filter(|l| {
            let remembered = store
                .status
                .lookalike(&pair_target(&l.a, &l.b))
                .is_some_and(|v| (v.score - l.score).abs() < 1e-6);
            !remembered && !told_apart(&l.a, &l.b) && !told_apart(&l.b, &l.a)
        })
        .collect()
}

// Every journal entry, by generation.
pub fn read_journal(out: &std::path::Path) -> Vec<JournalEntry> {
    let dir = out.join("journal");
    let mut entries: Vec<(u64, JournalEntry)> = Vec::new();
    let Ok(rd) = crate::vfs::read_dir(&dir) else {
        return Vec::new();
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(n) = name
            .strip_prefix('g')
            .and_then(|s| s.strip_suffix(".yaml"))
            .and_then(|s| s.parse::<u64>().ok())
        else {
            continue;
        };
        if let Some(entry) = read_journal_entry(&e.path(), n) {
            entries.push((n, entry));
        }
    }
    entries.sort_by_key(|(n, _)| *n);
    entries.into_iter().map(|(_, e)| e).collect()
}

// The journal entries of the generations in `(from, to]`, oldest first: the files
// named by those generations alone, so a build reading what one session wrote parses
// nothing older. Mirrors docs/compiler/graph.md#journal.
pub fn read_journal_range(out: &std::path::Path, from: u64, to: u64) -> Vec<JournalEntry> {
    let dir = out.join("journal");
    (from + 1..=to)
        .filter_map(|g| read_journal_entry(&dir.join(format!("g{}.yaml", g)), g))
        .collect()
}

// One journal entry by path; `generation` is the file's, stamped on an entry that
// carries none. A missing file is nothing; a file that does not parse is skipped with
// one warning per process naming it, so a torn entry is visible and never silent.
// Mirrors docs/compiler/graph.md#journal.
pub fn read_journal_entry(path: &std::path::Path, generation: u64) -> Option<JournalEntry> {
    let text = match crate::vfs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn_journal_once(path, &e.to_string());
            }
            return None;
        }
    };
    let mut entry = match serde_norway::from_str::<JournalEntry>(&text) {
        Ok(entry) => entry,
        Err(e) => {
            warn_journal_once(path, &e.to_string());
            return None;
        }
    };
    if entry.generation == 0 {
        entry.generation = generation;
    }
    Some(entry)
}

fn warn_journal_once(path: &std::path::Path, why: &str) {
    static WARNED: std::sync::Mutex<BTreeSet<std::path::PathBuf>> =
        std::sync::Mutex::new(BTreeSet::new());
    let fresh = WARNED
        .lock()
        .map(|mut set| set.insert(path.to_path_buf()))
        .unwrap_or(true);
    if fresh {
        eprintln!(
            "[jazyk] warning: journal entry {} skipped: {}",
            path.display(),
            why
        );
    }
}

impl GoalKind for DedupeCandidates {
    fn kind(&self) -> &'static str {
        "dedupe-candidates"
    }
    fn class(&self) -> Class {
        Class::Gc
    }
    fn unit(&self) -> &'static str {
        "entity pair"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut out = Vec::new();
        // A remembered verdict at the pair's score, or a note at creation that told
        // the two apart, is the standing verdict: the pair is not asked about again.
        // Mirrors docs/compiler/goals/dedupe-candidates.md#verdicts-are-remembered.
        for l in pending_lookalikes(store) {
            let target = pair_target(&l.a, &l.b);
            let filed = store.graph.diagnostics.values().any(|d| {
                d.lifecycle == "open"
                    && d.rule == "duplicate-entity"
                    && d.subjects.iter().any(|s| s == &l.a)
                    && d.subjects.iter().any(|s| s == &l.b)
            });
            if filed {
                continue;
            }
            let record = store
                .status
                .changes
                .iter()
                .find(|c| c.kind == CHANGE_LOOKALIKE && c.subject == target);
            let cause = record.map(|c| c.cause()).unwrap_or(Cause {
                generation: store.status.generation,
                mutation: 0,
                via: "lookalike".into(),
            });
            let only_a: Vec<&String> = l.docs_a.difference(&l.docs_b).collect();
            let only_b: Vec<&String> = l.docs_b.difference(&l.docs_a).collect();
            let change = json!({
                "score": l.score, "tokens": l.tokens, "documents": l.documents,
                "shared": l.shared,
                "mentions": {&l.a: l.docs_a, &l.b: l.docs_b},
            });
            let mut hints = vec![
                format!("load {}; load {}", l.a, l.b),
                format!(
                    "score {} (tokens {}, documents {}; shared: {})",
                    l.score,
                    l.tokens,
                    l.documents,
                    l.shared.join(", ")
                ),
            ];
            if !only_a.is_empty() {
                hints.push(format!(
                    "{} mentions only {}",
                    only_a
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    l.a
                ));
            }
            if !only_b.is_empty() {
                hints.push(format!(
                    "{} mentions only {}",
                    only_b
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    l.b
                ));
            }
            let both: Vec<&String> = store
                .graph
                .requirements
                .iter()
                .filter(|(_, r)| {
                    r.entities.iter().any(|e| store.resolve_id(e) == l.a)
                        && r.entities.iter().any(|e| store.resolve_id(e) == l.b)
                })
                .map(|(id, _)| id)
                .collect();
            for r in both.iter().take(4) {
                hints.push(format!("load {} (names both)", r));
            }
            let (pa, pb) = (
                store.graph.entities[&l.a].parent.clone(),
                store.graph.entities[&l.b].parent.clone(),
            );
            if pa != pb {
                for p in [pa, pb].into_iter().flatten() {
                    hints.push(format!(
                        "load {} (a parent; two parents mean two entities)",
                        p
                    ));
                }
            }
            hints.push(
                "skill judgment; merge_entities or report_diagnostic duplicate-entity".into(),
            );
            out.push(build_goal(self, &target, false, change, Some(cause), hints));
        }
        out
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        gc_ready(goal, board)
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        let mut seen = BTreeSet::new();
        for g in batch {
            if let Some((a, b)) = pair_members(&g.target) {
                for id in [a, b] {
                    if seen.insert(id.to_string()) {
                        lines.push(format!("- {} full", id));
                    }
                }
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &["merge_entities", "update_entity", "report_diagnostic"]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = Vec::new();
        for op in staged {
            if let Op::MergeEntities {
                keep,
                absorb,
                reason,
            } = op
            {
                if reason.trim().is_empty() {
                    out.push(Violation::new(
                        "reason-required",
                        format!("merge_entities {} into {} carries no reason", absorb, keep),
                    ));
                }
                if store.is_ancestor(absorb, keep) {
                    out.push(Violation::new(
                        "parent-cycle",
                        format!(
                            "merging {} into {} would make the survivor its own ancestor",
                            absorb, keep
                        ),
                    ));
                }
            }
        }
        out
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../../docs/compiler/goals/prompts/dedupe-candidates.md")
    }
}
