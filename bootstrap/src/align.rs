// Alignment: match the fresh section trees against the stored ones, classify every
// change, and carry each anchor (requirement source, entity mention) to the section that
// now holds its text. Exact moves apply mechanically; everything else is a proposal for
// the align-doc turn. Deterministic; no model. Mirrors docs/compiler/alignment.md.
use crate::model::*;
use crate::store::text_contains;
use std::collections::{BTreeMap, BTreeSet};

// The alignment thresholds, registry constants (docs/compiler/graph.md#budgets-and-thresholds).
#[derive(Clone, Debug)]
pub struct Thresholds {
    pub move_similarity: f64,
    pub split_coverage: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            move_similarity: crate::limits::ALIGN_MOVE_SIMILARITY,
            split_coverage: crate::limits::ALIGN_SPLIT_COVERAGE,
        }
    }
}

// The least a candidate may score and still be shown to the model.
const CANDIDATE_FLOOR: f64 = 0.3;
// The least a part may be contained in a whole to count toward a split or merge.
const PART_FLOOR: f64 = 0.3;
// The least a further part must add to the whole's coverage to count: the guard that
// keeps a moved section with a loosely similar neighbor from reading as a split.
const PART_MARGIN: f64 = 0.2;
const MAX_CANDIDATES: usize = 3;
const EXCERPT_CONTEXT: usize = 3;

// A full section reference: document path plus internal reference.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Full {
    pub doc: String,
    pub section: String,
}

impl Full {
    pub fn new(doc: &str, section: &str) -> Full {
        Full {
            doc: doc.to_string(),
            section: section.to_string(),
        }
    }
    pub fn render(&self) -> String {
        format!("{}#{}", self.doc, self.section)
    }
}

// What the deterministic pass concluded for one build.
#[derive(Clone, Debug, Default)]
pub struct AlignPlan {
    // Every computed change except `unchanged`, for the journal and the packs.
    pub ops: Vec<SectionOp>,
    // Byte-identical sections under a new reference: applied without a model.
    pub exact_moves: Vec<(Full, Full)>,
    // New sections whose reference and hash are unchanged.
    pub unchanged: BTreeSet<Full>,
    // Anchors the model must place.
    pub proposals: Vec<AnchorProposal>,
    // Anchors with no candidate at all: stale anchors on their old document.
    pub homeless: Vec<(String, Full)>,
}

// One section's derived features, computed on demand.
struct Print {
    full: Full,
    // Identity for exact matching: the title plus the whitespace-collapsed body. Unlike
    // the raw hash it survives a heading level change and trailing blank lines, which
    // move no statement.
    identity: String,
    parent: Option<String>,
    order: usize,
    slug: String,
    words: Vec<String>,
    tokens: Vec<String>,
    shingles: BTreeSet<String>,
    normalized: String,
    raw: String,
}

fn normalize_word(w: &str) -> String {
    w.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

// The words of a text, as written, and the normalized token of each (empty tokens are
// dropped along with their word).
fn tokenize(text: &str) -> (Vec<String>, Vec<String>) {
    let mut words = Vec::new();
    let mut tokens = Vec::new();
    for w in text.split_whitespace() {
        let t = normalize_word(w);
        if !t.is_empty() {
            words.push(w.to_string());
            tokens.push(t);
        }
    }
    (words, tokens)
}

fn shingles(tokens: &[String]) -> BTreeSet<String> {
    if tokens.len() < 3 {
        let mut s = BTreeSet::new();
        if !tokens.is_empty() {
            s.insert(tokens.join(" "));
        }
        return s;
    }
    tokens.windows(3).map(|w| w.join(" ")).collect()
}

fn dice(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    2.0 * a.intersection(b).count() as f64 / (a.len() + b.len()) as f64
}

// The share of `part` held by `whole`.
fn containment(part: &BTreeSet<String>, whole: &BTreeSet<String>) -> f64 {
    if part.is_empty() {
        return 0.0;
    }
    part.intersection(whole).count() as f64 / part.len() as f64
}

fn print(full: Full, sec: &Section) -> Print {
    let body: String = match sec.kind.as_str() {
        "heading" | "root" => sec.raw.lines().skip(1).collect::<Vec<_>>().join("\n"),
        _ => sec.raw.clone(),
    };
    let (words, tokens) = tokenize(&body);
    let shingles = shingles(&tokens);
    let normalized = sec.raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let identity = format!(
        "{}\n{}",
        sec.title.trim(),
        body.split_whitespace().collect::<Vec<_>>().join(" ")
    );
    Print {
        slug: full
            .section
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string(),
        full,
        identity,
        parent: sec.parent.clone(),
        order: sec.order,
        words,
        tokens,
        shingles,
        normalized,
        raw: sec.raw.clone(),
    }
}

// Longest common subsequence of two token lists, returning its length and the window
// of `hay` it spans (first and last matched index).
fn lcs(needle: &[String], hay: &[String]) -> (usize, Option<(usize, usize)>) {
    let (n, m) = (needle.len(), hay.len());
    if n == 0 || m == 0 {
        return (0, None);
    }
    let mut dp = vec![vec![0u16; m + 1]; n + 1];
    for i in 1..=n {
        for j in 1..=m {
            dp[i][j] = if needle[i - 1] == hay[j - 1] {
                dp[i - 1][j - 1] + 1
            } else {
                dp[i - 1][j].max(dp[i][j - 1])
            };
        }
    }
    let len = dp[n][m] as usize;
    if len == 0 {
        return (0, None);
    }
    // Walk back to find the span of hay the subsequence occupies.
    let (mut i, mut j) = (n, m);
    let (mut first, mut last) = (usize::MAX, 0usize);
    while i > 0 && j > 0 {
        if needle[i - 1] == hay[j - 1] {
            first = j - 1;
            last = last.max(j - 1);
            i -= 1;
            j -= 1;
        } else if dp[i - 1][j] >= dp[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    (len, Some((first, last)))
}

// An excerpt of `raw` around the first line holding `probe` (normalized compare), with
// context lines on either side. Falls back to the head of the section.
fn excerpt_around(raw: &str, probe: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    let (_, probe_tokens) = tokenize(probe);
    let head: Vec<String> = probe_tokens.into_iter().take(3).collect();
    let hit = if head.is_empty() {
        None
    } else {
        lines.iter().position(|l| {
            let (_, t) = tokenize(l);
            t.windows(head.len()).any(|w| w == head.as_slice())
        })
    };
    let center = hit.unwrap_or(0);
    let start = center.saturating_sub(EXCERPT_CONTEXT);
    let end = (center + EXCERPT_CONTEXT + 1).min(lines.len());
    lines[start..end].join("\n")
}

struct Candidate {
    full: Full,
    similarity: f64,
    quote_locates: bool,
    nearest: Option<String>,
    excerpt: String,
}

fn score_candidate(p: &Print, quote: &str) -> Option<Candidate> {
    if text_contains(&p.normalized, quote) {
        return Some(Candidate {
            full: p.full.clone(),
            similarity: 1.0,
            quote_locates: true,
            nearest: None,
            excerpt: excerpt_around(&p.raw, quote),
        });
    }
    let (_, q) = tokenize(quote);
    if q.is_empty() {
        return None;
    }
    let (len, span) = lcs(&q, &p.tokens);
    let similarity = len as f64 / q.len() as f64;
    if similarity < CANDIDATE_FLOOR {
        return None;
    }
    let nearest = span.map(|(a, b)| p.words[a..=b].join(" "));
    let excerpt = nearest
        .as_deref()
        .map(|n| excerpt_around(&p.raw, n))
        .unwrap_or_default();
    Some(Candidate {
        full: p.full.clone(),
        similarity,
        quote_locates: false,
        nearest,
        excerpt,
    })
}

// Every anchor sourced from one old section: requirement ids by source, entity ids by
// mention, each with the quote it carries. A mention that coincides with a requirement's
// source was derived from it at commit and follows the requirement when it is placed;
// only a mention of its own (an upsert_entity quote) is an anchor here.
fn anchors_in(graph: &Graph, at: &Full) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut sources: BTreeSet<&str> = BTreeSet::new();
    for (id, r) in &graph.requirements {
        let Some(src) = r.source.as_ref() else {
            continue;
        };
        if src.doc == at.doc && src.section == at.section {
            out.push((id.clone(), src.quote.clone()));
            sources.insert(src.quote.as_str());
        }
    }
    for (id, e) in &graph.entities {
        for m in &e.mentions {
            if m.doc == at.doc && m.section == at.section && !sources.contains(m.quote.as_str()) {
                out.push((id.clone(), m.quote.clone()));
            }
        }
    }
    out
}

// Match the stored trees against the fresh parse. `old` is every stored record; `parsed`
// every document on disk. Only documents whose content hash changed, appeared, or
// vanished take part in matching; the rest serve as homes for a quote search.
pub fn align(
    old: &BTreeMap<String, DocRecord>,
    parsed: &BTreeMap<String, (String, BTreeMap<String, Section>)>,
    graph: &Graph,
    th: &Thresholds,
) -> AlignPlan {
    let mut plan = AlignPlan::default();
    let changed: BTreeSet<String> = old
        .iter()
        .filter(|(d, rec)| {
            parsed
                .get(*d)
                .map(|(h, _)| h != &rec.content_hash)
                .unwrap_or(true)
        })
        .map(|(d, _)| d.clone())
        .chain(parsed.keys().filter(|d| !old.contains_key(*d)).cloned())
        .collect();
    if changed.is_empty() {
        return plan;
    }

    // Pools of prints: old sections from changed documents, new sections from changed
    // documents, and every new section anywhere (for the global quote search).
    let mut old_pool: Vec<Print> = Vec::new();
    for d in &changed {
        if let Some(rec) = old.get(d) {
            for (r, s) in &rec.sections {
                old_pool.push(print(Full::new(d, r), s));
            }
        }
    }
    let mut new_pool: Vec<Print> = Vec::new();
    for d in &changed {
        if let Some((_, secs)) = parsed.get(d) {
            for (r, s) in secs {
                new_pool.push(print(Full::new(d, r), s));
            }
        }
    }
    let all_new: Vec<Print> = parsed
        .iter()
        .flat_map(|(d, (_, secs))| secs.iter().map(move |(r, s)| print(Full::new(d, r), s)))
        .collect();

    let mut old_free: Vec<bool> = vec![true; old_pool.len()];
    let mut new_free: Vec<bool> = vec![true; new_pool.len()];
    // old index -> (op, new indices)
    let mut matched_old: BTreeMap<usize, (String, Vec<usize>, f64)> = BTreeMap::new();

    // Phase 1: exact. Same reference and hash is unchanged; same hash elsewhere is a
    // move, paired one to one preferring the same document, parent, and position.
    for (i, o) in old_pool.iter().enumerate() {
        if let Some(j) = new_pool
            .iter()
            .position(|n| n.full == o.full && n.identity == o.identity)
        {
            old_free[i] = false;
            new_free[j] = false;
            plan.unchanged.insert(o.full.clone());
        }
    }
    let mut exact_pairs: Vec<(i64, usize, usize)> = Vec::new();
    for (i, o) in old_pool.iter().enumerate() {
        if !old_free[i] {
            continue;
        }
        for (j, n) in new_pool.iter().enumerate() {
            if !new_free[j] || n.identity != o.identity {
                continue;
            }
            let mut pref: i64 = 0;
            if n.full.doc == o.full.doc {
                pref += 1000;
            }
            if n.parent == o.parent {
                pref += 100;
            }
            pref -= (n.order as i64 - o.order as i64).abs().min(99);
            exact_pairs.push((pref, i, j));
        }
    }
    exact_pairs.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    for (_, i, j) in exact_pairs {
        if old_free[i] && new_free[j] {
            old_free[i] = false;
            new_free[j] = false;
            matched_old.insert(i, ("moved-exact".into(), vec![j], 1.0));
            plan.exact_moves
                .push((old_pool[i].full.clone(), new_pool[j].full.clone()));
        }
    }

    // Phase 2: identity. Same reference, different hash: edited in place. The new
    // section stays available as a split or merge target.
    let mut edited_new: BTreeSet<usize> = BTreeSet::new();
    for (i, o) in old_pool.iter().enumerate() {
        if !old_free[i] {
            continue;
        }
        if let Some(j) = new_pool.iter().position(|n| n.full == o.full) {
            old_free[i] = false;
            new_free[j] = false;
            edited_new.insert(j);
            let sim = dice(&o.shingles, &new_pool[j].shingles);
            matched_old.insert(i, ("edited".into(), vec![j], sim));
        }
    }

    // Phase 3: fuzzy. Splits and merges first: they are one-sided containments, and
    // a half would otherwise pair with its whole as a move. Then moves, taken greedily
    // by descending Dice (git's rename rule), one to one.
    let score = |o: &Print, n: &Print| -> f64 {
        let mut s = dice(&o.shingles, &n.shingles);
        if o.slug == n.slug {
            s += 0.15;
        }
        if o.full.doc == n.full.doc {
            s += 0.05;
        }
        if o.parent == n.parent {
            s += 0.05;
        }
        s.min(1.0)
    };
    // Parts of a whole: candidates sorted by containment, kept while each adds enough
    // of the whole that the earlier parts did not cover.
    let assemble = |whole: &BTreeSet<String>,
                    mut parts: Vec<(f64, usize, &BTreeSet<String>)>|
     -> (Vec<usize>, f64) {
        parts.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)));
        let mut covered: BTreeSet<String> = BTreeSet::new();
        let mut kept: Vec<usize> = Vec::new();
        for (_, idx, sh) in parts {
            let gain = whole
                .iter()
                .filter(|x| sh.contains(*x) && !covered.contains(*x))
                .count() as f64
                / whole.len().max(1) as f64;
            if kept.is_empty() || gain >= PART_MARGIN {
                covered.extend(whole.iter().filter(|x| sh.contains(*x)).cloned());
                kept.push(idx);
            }
        }
        let coverage = covered.len() as f64 / whole.len().max(1) as f64;
        (kept, coverage)
    };
    let take_moves =
        |old_free: &mut Vec<bool>,
         new_free: &mut Vec<bool>,
         matched_old: &mut BTreeMap<usize, (String, Vec<usize>, f64)>| {
            let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
            for (i, o) in old_pool.iter().enumerate() {
                if !old_free[i] {
                    continue;
                }
                for (j, n) in new_pool.iter().enumerate() {
                    if !new_free[j] {
                        continue;
                    }
                    let s = score(o, n);
                    if s >= th.move_similarity {
                        pairs.push((s, i, j));
                    }
                }
            }
            pairs.sort_by(|a, b| {
                b.0.partial_cmp(&a.0)
                    .unwrap()
                    .then(a.1.cmp(&b.1))
                    .then(a.2.cmp(&b.2))
            });
            for (s, i, j) in pairs {
                if old_free[i] && new_free[j] {
                    old_free[i] = false;
                    new_free[j] = false;
                    matched_old.insert(i, ("moved".into(), vec![j], s));
                }
            }
        };
    // Splits: one old section covered by two or more new sections. The old may be
    // free, or `edited` in place: the common split keeps the original heading for one
    // half and gives the rest a new one, and that half must count as a part.
    for i in 0..old_pool.len() {
        let own = match matched_old.get(&i) {
            None if old_free[i] => None,
            Some((op, js, _)) if op == "edited" => Some(js[0]),
            _ => continue,
        };
        let o = &old_pool[i];
        let parts: Vec<(f64, usize, &BTreeSet<String>)> = new_pool
            .iter()
            .enumerate()
            .filter(|(j, _)| new_free[*j] || Some(*j) == own)
            .map(|(j, n)| (containment(&n.shingles, &o.shingles), j, &n.shingles))
            .filter(|(c, _, _)| *c >= PART_FLOOR)
            .collect();
        let (js, coverage) = assemble(&o.shingles, parts);
        if js.len() >= 2
            && coverage >= th.split_coverage
            && own.map(|j| js.contains(&j)).unwrap_or(true)
        {
            old_free[i] = false;
            for j in &js {
                new_free[*j] = false;
            }
            matched_old.insert(i, ("split".into(), js, coverage));
        }
    }
    // Merges: one free new covering two or more free old sections.
    let mut merged_new: BTreeMap<usize, (Vec<usize>, f64)> = BTreeMap::new();
    for j in 0..new_pool.len() {
        if !new_free[j] {
            continue;
        }
        let n = &new_pool[j];
        let parts: Vec<(f64, usize, &BTreeSet<String>)> = old_pool
            .iter()
            .enumerate()
            .filter(|(i, _)| old_free[*i])
            .map(|(i, o)| (containment(&o.shingles, &n.shingles), i, &o.shingles))
            .filter(|(c, _, _)| *c >= PART_FLOOR)
            .collect();
        let (is, coverage) = assemble(&n.shingles, parts);
        if is.len() >= 2 && coverage >= th.split_coverage {
            new_free[j] = false;
            for i in &is {
                old_free[*i] = false;
                matched_old.insert(*i, ("merged".into(), vec![j], coverage));
            }
            merged_new.insert(j, (is, coverage));
        }
    }
    take_moves(&mut old_free, &mut new_free, &mut matched_old);

    // Record the ops.
    let pct = |s: f64| (s * 100.0).round() / 100.0;
    for (i, (op, js, sim)) in &matched_old {
        if op == "merged" {
            continue;
        }
        plan.ops.push(SectionOp {
            op: if op == "moved-exact" {
                "moved".into()
            } else {
                op.clone()
            },
            from: vec![old_pool[*i].full.render()],
            to: js.iter().map(|j| new_pool[*j].full.render()).collect(),
            similarity: Some(pct(*sim)),
        });
    }
    for (j, (is, cov)) in &merged_new {
        plan.ops.push(SectionOp {
            op: "merged".into(),
            from: is.iter().map(|i| old_pool[*i].full.render()).collect(),
            to: vec![new_pool[*j].full.render()],
            similarity: Some(pct(*cov)),
        });
    }
    for (i, o) in old_pool.iter().enumerate() {
        if old_free[i] {
            plan.ops.push(SectionOp {
                op: "deleted".into(),
                from: vec![o.full.render()],
                to: vec![],
                similarity: None,
            });
        }
    }
    for (j, n) in new_pool.iter().enumerate() {
        if new_free[j] {
            plan.ops.push(SectionOp {
                op: "added".into(),
                from: vec![],
                to: vec![n.full.render()],
                similarity: None,
            });
        }
    }

    // Anchor relocation. Section matching supplied the candidates; the quote decides.
    for (i, o) in old_pool.iter().enumerate() {
        if plan.unchanged.contains(&o.full) {
            continue;
        }
        let matched = matched_old.get(&i);
        if matches!(matched, Some((op, _, _)) if op == "moved-exact") {
            continue;
        }
        for (anchor, quote) in anchors_in(graph, &o.full) {
            // A quote that still locates under its own reference has not moved,
            // whatever happened around it (an edit, or a split that kept the heading).
            if let Some(n) = new_pool.iter().find(|n| n.full == o.full) {
                if text_contains(&n.normalized, &quote) {
                    continue;
                }
            }
            let mut cands: Vec<Candidate> = Vec::new();
            let mut seen: BTreeSet<Full> = BTreeSet::new();
            let mut pool: Vec<&Print> = Vec::new();
            if let Some((_, js, _)) = matched {
                pool.extend(js.iter().map(|j| &new_pool[*j]));
            }
            pool.extend(new_pool.iter().filter(|n| n.full.doc == o.full.doc));
            // Elsewhere, only a verbatim hit counts: a fuzzy window in an unrelated
            // document is noise, and scoring every section of every document is not.
            pool.extend(
                all_new
                    .iter()
                    .filter(|n| n.full.doc != o.full.doc && text_contains(&n.normalized, &quote)),
            );
            for p in pool {
                if seen.insert(p.full.clone()) {
                    if let Some(c) = score_candidate(p, &quote) {
                        cands.push(c);
                    }
                }
            }
            cands.sort_by(|a, b| {
                b.similarity
                    .partial_cmp(&a.similarity)
                    .unwrap()
                    .then(a.full.cmp(&b.full))
            });
            cands.truncate(MAX_CANDIDATES);
            if cands.is_empty() {
                plan.homeless.push((anchor, o.full.clone()));
                continue;
            }
            plan.proposals.push(AnchorProposal {
                anchor,
                from: o.full.render(),
                quote: quote.clone(),
                excerpt: excerpt_around(&o.raw, &quote),
                candidates: cands
                    .into_iter()
                    .map(|c| AnchorCandidate {
                        section: c.full.render(),
                        similarity: pct(c.similarity),
                        quote_locates: c.quote_locates,
                        nearest: c.nearest,
                        excerpt: c.excerpt,
                    })
                    .collect(),
            });
        }
    }
    plan
}

// The document a proposal belongs to: where its best candidate lives.
pub fn target_doc(p: &AnchorProposal) -> String {
    p.candidates
        .first()
        .and_then(|c| split_section_ref(&c.section))
        .map(|(d, _)| d)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::md::parse_sections;

    fn docs(files: &[(&str, &str)]) -> BTreeMap<String, (String, BTreeMap<String, Section>)> {
        files
            .iter()
            .map(|(d, t)| (d.to_string(), (hash_hex(t), parse_sections(t))))
            .collect()
    }

    fn stored(
        parsed: &BTreeMap<String, (String, BTreeMap<String, Section>)>,
    ) -> BTreeMap<String, DocRecord> {
        parsed
            .iter()
            .map(|(d, (h, s))| {
                (
                    d.clone(),
                    DocRecord {
                        content_hash: h.clone(),
                        sections: s.clone(),
                        coverage: BTreeMap::new(),
                    },
                )
            })
            .collect()
    }

    fn graph_with(reqs: &[(&str, &str, &str, &str)]) -> Graph {
        let mut g = Graph::default();
        g.entities.insert(
            "ent:x".into(),
            Entity {
                name: "X".into(),
                ..Default::default()
            },
        );
        for (id, doc, sec, quote) in reqs {
            g.requirements.insert(
                id.to_string(),
                Requirement {
                    statement: format!("The system shall {}.", quote),
                    entities: vec!["ent:x".into()],
                    source: Some(SourceRef {
                        doc: doc.to_string(),
                        section: sec.to_string(),
                        quote: quote.to_string(),
                    }),
                    ..Default::default()
                },
            );
        }
        g
    }

    const BODY_A: &str = "The cart holds items a customer intends to buy. Items stay until checkout. A cart may hold up to fifty items at once.";
    const BODY_B: &str = "Orders are placed from a cart. An order records the address and the total. Payment is taken at placement.";

    fn op<'a>(plan: &'a AlignPlan, kind: &str) -> Vec<&'a SectionOp> {
        plan.ops.iter().filter(|o| o.op == kind).collect()
    }

    #[test]
    fn no_change_is_empty() {
        let before = docs(&[("a.md", &format!("# T\n\n## Cart\n{}\n", BODY_A))]);
        let plan = align(
            &stored(&before),
            &before,
            &Graph::default(),
            &Thresholds::default(),
        );
        assert!(plan.ops.is_empty() && plan.proposals.is_empty() && plan.exact_moves.is_empty());
    }

    #[test]
    fn byte_identical_move_is_exact_and_mechanical() {
        let before = docs(&[(
            "a.md",
            &format!("# T\n\n## Cart\n{}\n\n## Orders\n{}\n", BODY_A, BODY_B),
        )]);
        let after = docs(&[(
            "a.md",
            &format!(
                "# T\n\n## Group\n\n### Cart\n{}\n\n## Orders\n{}\n",
                BODY_A, BODY_B
            ),
        )]);
        let g = graph_with(&[("req:1", "a.md", "/t/cart", "Items stay until checkout.")]);
        let plan = align(&stored(&before), &after, &g, &Thresholds::default());
        assert_eq!(
            plan.exact_moves,
            vec![(
                Full::new("a.md", "/t/cart"),
                Full::new("a.md", "/t/group/cart")
            )]
        );
        assert!(plan.proposals.is_empty(), "{:?}", plan.proposals);
        assert_eq!(op(&plan, "added").len(), 1);
    }

    #[test]
    fn moved_and_edited_is_a_fuzzy_move_with_a_proposal() {
        let before = docs(&[(
            "a.md",
            &format!("# T\n\n## Cart\n{}\n\n## Orders\n{}\n", BODY_A, BODY_B),
        )]);
        let edited = BODY_A.replace("fifty", "sixty");
        let after = docs(&[(
            "a.md",
            &format!("# T\n\n## Orders\n{}\n\n## Basket\n{}\n", BODY_B, edited),
        )]);
        let g = graph_with(&[
            ("req:1", "a.md", "/t/cart", "Items stay until checkout."),
            (
                "req:2",
                "a.md",
                "/t/cart",
                "A cart may hold up to fifty items at once.",
            ),
        ]);
        let plan = align(&stored(&before), &after, &g, &Thresholds::default());
        let moved = op(&plan, "moved");
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].to, vec!["a.md#/t/basket"]);
        assert!(plan.exact_moves.is_empty());
        assert_eq!(plan.proposals.len(), 2);
        let p1 = plan.proposals.iter().find(|p| p.anchor == "req:1").unwrap();
        assert!(p1.candidates[0].quote_locates);
        assert_eq!(p1.candidates[0].section, "a.md#/t/basket");
        let p2 = plan.proposals.iter().find(|p| p.anchor == "req:2").unwrap();
        assert!(!p2.candidates[0].quote_locates);
        assert!(p2.candidates[0]
            .nearest
            .as_deref()
            .unwrap()
            .contains("sixty"));
        assert!(p2.candidates[0].similarity > 0.8);
    }

    #[test]
    fn parent_rename_moves_children_exactly() {
        let before = docs(&[(
            "a.md",
            &format!("# T\n\n## Group\nintro\n\n### Cart\n{}\n", BODY_A),
        )]);
        let after = docs(&[(
            "a.md",
            &format!("# T\n\n## Bunch\nintro\n\n### Cart\n{}\n", BODY_A),
        )]);
        let g = graph_with(&[(
            "req:1",
            "a.md",
            "/t/group/cart",
            "Items stay until checkout.",
        )]);
        let plan = align(&stored(&before), &after, &g, &Thresholds::default());
        assert!(plan.exact_moves.contains(&(
            Full::new("a.md", "/t/group/cart"),
            Full::new("a.md", "/t/bunch/cart")
        )));
        assert!(plan.proposals.is_empty());
    }

    #[test]
    fn split_sends_each_anchor_to_its_half() {
        let before = docs(&[("a.md", &format!("# T\n\n## Both\n{}\n{}\n", BODY_A, BODY_B))]);
        let after = docs(&[(
            "a.md",
            &format!("# T\n\n## Cart\n{}\n\n## Orders\n{}\n", BODY_A, BODY_B),
        )]);
        let g = graph_with(&[
            ("req:1", "a.md", "/t/both", "Items stay until checkout."),
            ("req:2", "a.md", "/t/both", "Payment is taken at placement."),
        ]);
        let plan = align(&stored(&before), &after, &g, &Thresholds::default());
        let split = op(&plan, "split");
        assert_eq!(split.len(), 1, "{:?}", plan.ops);
        assert_eq!(split[0].to.len(), 2);
        let p1 = plan.proposals.iter().find(|p| p.anchor == "req:1").unwrap();
        assert_eq!(p1.candidates[0].section, "a.md#/t/cart");
        let p2 = plan.proposals.iter().find(|p| p.anchor == "req:2").unwrap();
        assert_eq!(p2.candidates[0].section, "a.md#/t/orders");
    }

    #[test]
    fn split_that_keeps_the_original_heading_is_still_a_split() {
        let before = docs(&[("a.md", &format!("# T\n\n## Cart\n{}\n{}\n", BODY_A, BODY_B))]);
        let after = docs(&[(
            "a.md",
            &format!("# T\n\n## Cart\n{}\n\n## Orders\n{}\n", BODY_A, BODY_B),
        )]);
        let g = graph_with(&[
            ("req:1", "a.md", "/t/cart", "Items stay until checkout."),
            ("req:2", "a.md", "/t/cart", "Payment is taken at placement."),
        ]);
        let plan = align(&stored(&before), &after, &g, &Thresholds::default());
        let split = op(&plan, "split");
        assert_eq!(split.len(), 1, "{:?}", plan.ops);
        assert!(
            split[0].to.contains(&"a.md#/t/cart".to_string())
                && split[0].to.contains(&"a.md#/t/orders".to_string())
        );
        assert!(op(&plan, "edited").is_empty() && op(&plan, "added").is_empty());
        // The anchor whose sentence stayed in place still locates there: no proposal.
        assert_eq!(plan.proposals.len(), 1);
        assert_eq!(plan.proposals[0].anchor, "req:2");
        assert_eq!(plan.proposals[0].candidates[0].section, "a.md#/t/orders");
    }

    #[test]
    fn merge_pulls_both_anchors_into_one() {
        let before = docs(&[(
            "a.md",
            &format!("# T\n\n## Cart\n{}\n\n## Orders\n{}\n", BODY_A, BODY_B),
        )]);
        let after = docs(&[("a.md", &format!("# T\n\n## Both\n{}\n{}\n", BODY_A, BODY_B))]);
        let g = graph_with(&[
            ("req:1", "a.md", "/t/cart", "Items stay until checkout."),
            (
                "req:2",
                "a.md",
                "/t/orders",
                "Payment is taken at placement.",
            ),
        ]);
        let plan = align(&stored(&before), &after, &g, &Thresholds::default());
        let merged = op(&plan, "merged");
        assert_eq!(merged.len(), 1, "{:?}", plan.ops);
        assert_eq!(merged[0].from.len(), 2);
        assert_eq!(plan.proposals.len(), 2);
        assert!(plan
            .proposals
            .iter()
            .all(|p| p.candidates[0].section == "a.md#/t/both" && p.candidates[0].quote_locates));
    }

    #[test]
    fn cross_document_move_from_a_deleted_file() {
        let before = docs(&[
            ("a.md", &format!("# A\n\n## Cart\n{}\n", BODY_A)),
            ("b.md", "# B\nhello\n"),
        ]);
        let after = docs(&[(
            "b.md",
            &format!(
                "# B\nhello\n\n## Cart\n{}\n",
                BODY_A.replace("fifty", "sixty")
            ),
        )]);
        let g = graph_with(&[("req:1", "a.md", "/a/cart", "Items stay until checkout.")]);
        let plan = align(&stored(&before), &after, &g, &Thresholds::default());
        let moved = op(&plan, "moved");
        assert_eq!(moved.len(), 1, "{:?}", plan.ops);
        assert_eq!(moved[0].from, vec!["a.md#/a/cart"]);
        assert_eq!(moved[0].to, vec!["b.md#/b/cart"]);
        assert_eq!(plan.proposals[0].candidates[0].section, "b.md#/b/cart");
        assert_eq!(target_doc(&plan.proposals[0]), "b.md");
    }

    #[test]
    fn identical_twins_pair_one_to_one() {
        let before = docs(&[(
            "a.md",
            &format!(
                "# T\n\n## P\n\n### Item\n{}\n\n## Q\n\n### Item\n{}\n",
                BODY_A, BODY_A
            ),
        )]);
        let after = docs(&[(
            "a.md",
            &format!(
                "# T\n\n## R\n\n### Item\n{}\n\n## S\n\n### Item\n{}\n",
                BODY_A, BODY_A
            ),
        )]);
        let plan = align(
            &stored(&before),
            &after,
            &Graph::default(),
            &Thresholds::default(),
        );
        assert_eq!(plan.exact_moves.len(), 2);
        let targets: BTreeSet<String> = plan
            .exact_moves
            .iter()
            .map(|(_, t)| t.section.clone())
            .collect();
        assert_eq!(targets.len(), 2);
    }

    #[test]
    fn reordered_siblings_are_unchanged() {
        let before = docs(&[(
            "a.md",
            &format!("# T\n\n## Cart\n{}\n\n## Orders\n{}\n", BODY_A, BODY_B),
        )]);
        let after = docs(&[(
            "a.md",
            &format!("# T\n\n## Orders\n{}\n\n## Cart\n{}\n", BODY_B, BODY_A),
        )]);
        let plan = align(
            &stored(&before),
            &after,
            &Graph::default(),
            &Thresholds::default(),
        );
        assert!(plan.unchanged.contains(&Full::new("a.md", "/t/cart")));
        assert!(plan.ops.is_empty());
    }

    #[test]
    fn edited_in_place_keeps_locating_quotes_and_proposes_the_rest() {
        let before = docs(&[("a.md", &format!("# T\n\n## Cart\n{}\n", BODY_A))]);
        let after = docs(&[(
            "a.md",
            &format!("# T\n\n## Cart\n{}\n", BODY_A.replace("fifty", "sixty")),
        )]);
        let g = graph_with(&[
            ("req:1", "a.md", "/t/cart", "Items stay until checkout."),
            (
                "req:2",
                "a.md",
                "/t/cart",
                "A cart may hold up to fifty items at once.",
            ),
        ]);
        let plan = align(&stored(&before), &after, &g, &Thresholds::default());
        assert_eq!(op(&plan, "edited").len(), 1);
        assert_eq!(plan.proposals.len(), 1);
        assert_eq!(plan.proposals[0].anchor, "req:2");
        assert!(!plan.proposals[0].candidates[0].quote_locates);
    }

    #[test]
    fn deleted_without_a_candidate_is_homeless() {
        let before = docs(&[(
            "a.md",
            &format!("# T\n\n## Cart\n{}\n\n## Orders\n{}\n", BODY_A, BODY_B),
        )]);
        let after = docs(&[("a.md", &format!("# T\n\n## Orders\n{}\n", BODY_B))]);
        let g = graph_with(&[("req:1", "a.md", "/t/cart", "Items stay until checkout.")]);
        let plan = align(&stored(&before), &after, &g, &Thresholds::default());
        assert_eq!(op(&plan, "deleted").len(), 1);
        assert!(plan.proposals.is_empty());
        assert_eq!(
            plan.homeless,
            vec![("req:1".to_string(), Full::new("a.md", "/t/cart"))]
        );
    }
}
