// Text normalization, search, pair scoring, and requirement neighbors.
// Mirrors docs/compiler/reconciler.md#pairs.
use super::*;

pub(crate) fn normalize(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

// The function words pair selection ignores: articles, prepositions, pronouns,
// auxiliaries, quantifiers. Shared by the store's scoring and the hints' word lists.
// Mirrors docs/compiler/reconciler.md#pairs.
pub(crate) const CONTENT_STOP: [&str; 113] = [
    "the", "a", "an", "shall", "to", "of", "in", "on", "for", "is", "are", "be", "or", "and", "if",
    "with", "by", "it", "its", "when", "then", "that", "which", "this", "system", "not", "no",
    "only", "all", "each", "at", "as", "from", "into", "than", "has", "have", "had", "having",
    "their", "they", "them", "there", "these", "those", "once", "need", "needs", "must", "may",
    "can", "will", "would", "should", "could", "do", "does", "did", "done", "any", "some", "one",
    "two", "per", "before", "after", "within", "without", "every", "more", "less", "so", "up",
    "down", "out", "over", "under", "about", "also", "both", "either", "other", "such", "own",
    "same", "here", "where", "how", "what", "while", "until", "since", "because", "but", "however",
    "yet", "who", "whose", "whom", "was", "were", "been", "being", "get", "gets", "got", "make",
    "makes", "made", "use", "uses", "used", "via",
];

// A content token: not a stop word, and three letters or more unless it is a number.
// A token is an entity's boilerplate from this many statements naming the entity
// (and at least half of them). Mirrors docs/compiler/reconciler.md#pairs.
const BOILERPLATE_MIN: usize = 4;

// The one stemmer, in two stages. The suffix stage drops one of "ing", "ed", "s"
// from a word long enough to keep three letters: the vocabulary of the goal hints and
// the lookalike name tokens, whose stems ride in change records and remembered
// lookalike scores. Mirrors docs/compiler/reconciler.md#pairs.
pub(crate) fn suffix_stem(t: &str) -> String {
    for suffix in ["ing", "ed", "s"] {
        if t.len() > suffix.len() + 2 && t.ends_with(suffix) {
            return t[..t.len() - suffix.len()].to_string();
        }
    }
    t.to_string()
}

// The pair scorer's stem: the suffix stage, then a trailing "e", so "files" and
// "filed" both land on "fil" and "reverses" meets "reversed": the one stem per word
// the pairing rules count on.
pub(crate) fn content_stem(t: &str) -> String {
    let mut s = suffix_stem(t);
    if s.len() > 3 && s.ends_with('e') {
        s.pop();
    }
    s
}

pub(crate) fn is_content_token(t: &str) -> bool {
    !t.is_empty()
        && !CONTENT_STOP.contains(&t)
        && (t.len() >= 3 || t.chars().all(|c| c.is_ascii_digit()))
}

// Punctuation-insensitive statement normalization: the requirement natural key. A comma
// or spacing edit to a sentence keeps matching its existing requirement.
// The punctuation-insensitive form of a statement (graph.md#mutations): case,
// spacing, and punctuation fall away, but a comparison operator, a sign, and a
// decimal point stay, since `x > 30` and `x < 30` are different facts.
pub(crate) fn normalize_statement(s: &str) -> String {
    let chars: Vec<char> = s.to_lowercase().chars().collect();
    let mut tokens: Vec<String> = Vec::new();
    let mut tok = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let prev = i.checked_sub(1).map(|j| chars[j]);
        let next = chars.get(i + 1).copied();
        let digit = |c: Option<char>| c.is_some_and(|c| c.is_ascii_digit());
        if c.is_alphanumeric() || (c == '.' && digit(prev) && digit(next)) {
            tok.push(c);
            continue;
        }
        // A sign is a minus right before a digit that does not follow a word (`-1`,
        // not `2024-09` or `read-only`).
        let sign = c == '-' && digit(next) && !prev.is_some_and(|p| p.is_alphanumeric());
        if !tok.is_empty() {
            tokens.push(std::mem::take(&mut tok));
        }
        if sign {
            tok.push('-');
        } else if "<>=!≤≥≠+*/%".contains(c) {
            tokens.push(c.to_string());
        }
    }
    if !tok.is_empty() {
        tokens.push(tok);
    }
    tokens.join(" ")
}

// Same fact reworded: one statement's content tokens contained in the other's. A
// resumed build often re-extracts "X" as "X using Y" from the same sentence; that is
// one fact, not two. The loose form serves the stale-anchor path, where the dead quote
// is the signal that a statement is being re-recorded.
pub(crate) fn statement_subsumes(a: &str, b: &str) -> bool {
    let toks = |s: &str| -> std::collections::BTreeSet<String> {
        normalize_statement(s)
            .split(' ')
            .map(String::from)
            .collect()
    };
    let (ta, tb) = (toks(a), toks(b));
    !ta.is_empty() && !tb.is_empty() && (ta.is_subset(&tb) || tb.is_subset(&ta))
}

// The tight form for two statements quoting the same live sentence: two facts from one
// sentence share most of their words too ("An Order is paid within 21 days of
// placement" inside "When an Order is not paid within 21 days of placement, Orderly
// cancels it"), so a rewording may add at most two tokens and no negation.
// Mirrors docs/compiler/tools.md#write-tools.
// The obligation modals are set aside and an `-s` or `-ies` ending reduced, so "empties"
// restates "shall empty"; `can`, `should`, and `may` change what is required and count.
pub(crate) fn statement_rewords(a: &str, b: &str) -> bool {
    const NEGATIONS: [&str; 12] = [
        "not",
        "never",
        "no",
        "unless",
        "except",
        "otherwise",
        "cannot",
        // A permission or a recommendation is not the obligation.
        "can",
        "may",
        "should",
        "might",
        "could",
    ];
    let toks = |s: &str| -> std::collections::BTreeSet<String> {
        normalize_statement(s)
            .split(' ')
            .filter(|w| !["shall", "must", "will"].contains(w))
            .map(|w| {
                if w.len() > 4 && w.ends_with("ies") {
                    format!("{}y", &w[..w.len() - 3])
                } else if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") {
                    w[..w.len() - 1].to_string()
                } else {
                    w.to_string()
                }
            })
            .collect()
    };
    let (ta, tb) = (toks(a), toks(b));
    if ta.is_empty() || tb.is_empty() {
        return false;
    }
    let (small, large) = if ta.len() <= tb.len() {
        (&ta, &tb)
    } else {
        (&tb, &ta)
    };
    small.is_subset(large)
        && large.len() - small.len() <= 2
        && !large
            .difference(small)
            .any(|t| NEGATIONS.contains(&t.as_str()))
}

// Whitespace-insensitive containment: a quote wrapped across source lines still locates.
pub fn text_contains(hay: &str, needle: &str) -> bool {
    let h = hay.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = needle.split_whitespace().collect::<Vec<_>>().join(" ");
    !n.is_empty() && h.contains(&n)
}

impl Store {
    // Deterministic search over names and aliases: exact, then substring, then token overlap.
    pub fn search(&self, query: &str) -> Vec<(String, String, String)> {
        let q = normalize(query);
        let q_tokens: BTreeSet<&str> = q.split(' ').collect();
        let mut scored: Vec<(u32, String, String, String)> = Vec::new();
        for (id, e) in &self.graph.entities {
            let mut names = vec![normalize(&e.name)];
            names.extend(e.aliases.iter().map(|a| normalize(a)));
            let mut best: Option<u32> = None;
            for n in &names {
                let tier = if *n == q {
                    Some(0)
                } else if n.contains(&q) || q.contains(n.as_str()) {
                    Some(1)
                } else {
                    let n_tokens: BTreeSet<&str> = n.split(' ').collect();
                    let overlap = q_tokens.intersection(&n_tokens).count();
                    if overlap > 0 {
                        Some(2)
                    } else {
                        None
                    }
                };
                if let Some(t) = tier {
                    best = Some(best.map_or(t, |b: u32| b.min(t)));
                }
            }
            if let Some(t) = best {
                scored.push((
                    t,
                    id.clone(),
                    e.name.clone(),
                    e.definition.clone().unwrap_or_default(),
                ));
            }
        }
        scored.sort();
        scored
            .into_iter()
            .take(8)
            .map(|(_, id, n, d)| (id, n, d))
            .collect()
    }

    // Whether a quote locates inside the named section (whitespace-insensitive).
    pub fn quote_locates(&self, doc: &str, section: &str, quote: &str) -> bool {
        self.docs
            .get(doc)
            .and_then(|d| d.sections.get(section))
            .map(|s| s.kind != "blob" && text_contains(&s.raw, quote))
            .unwrap_or(false)
    }

    pub fn requirements_referencing(&self, entity_id: &str) -> Vec<String> {
        if let Some(ix) = ScopeIndex::of(self) {
            return ix
                .requirements_of(self)
                .get(entity_id)
                .cloned()
                .unwrap_or_default();
        }
        self.graph
            .requirements
            .iter()
            .filter(|(_, r)| r.entities.iter().any(|e| self.resolve_id(e) == entity_id))
            .map(|(id, _)| id.clone())
            .collect()
    }

    // Content tokens of a statement: normalized tokens minus stop words and the given
    // entities' own name tokens, reduced to crude stems so "reverses" meets "reverse"
    // and "sorting" meets "sort". Feeds requirement_neighbors.
    fn content_tokens(&self, statement: &str, entities: &[String]) -> BTreeSet<String> {
        self.content_token_words(statement, entities, content_stem)
            .into_iter()
            .map(|(stemmed, _)| stemmed)
            .collect()
    }

    // The one content tokenizer: the statement's normalized words that are content
    // tokens, each stemmed by `stem` and paired with the word as written, minus the
    // given entities' own name tokens under the same stem. The scorer stems with
    // `content_stem`, the hints with `suffix_stem`.
    pub(crate) fn content_token_words(
        &self,
        statement: &str,
        entities: &[String],
        stem: fn(&str) -> String,
    ) -> Vec<(String, String)> {
        let mut name_toks: BTreeSet<String> = BTreeSet::new();
        for e in entities {
            if let Some(ent) = self.graph.entities.get(self.resolve_id(e)) {
                for n in std::iter::once(&ent.name).chain(ent.aliases.iter()) {
                    for t in normalize_statement(n).split(' ') {
                        name_toks.insert(stem(t));
                    }
                }
            }
        }
        normalize_statement(statement)
            .split(' ')
            .filter(|t| is_content_token(t))
            .map(|t| (stem(t), t.to_string()))
            .filter(|(stemmed, _)| is_content_token(stemmed) && !name_toks.contains(stemmed))
            .collect()
    }

    // The scorer's inputs over the whole graph, one walk of the requirements: the
    // hub; per entity (resolved id) how many requirements name it and how many of
    // those carry each content token (the boilerplate source) and which they are;
    // and each requirement's content tokens. Computed once per pass and shared by
    // every requirement the pass scores. Mirrors docs/compiler/reconciler.md#pairs.
    pub fn pair_scoring(&self) -> PairScoring {
        #[cfg(test)]
        PAIR_SCORINGS.with(|n| n.set(n.get() + 1));
        let mut counts: BTreeMap<String, (usize, BTreeMap<String, usize>)> = BTreeMap::new();
        let mut by_entity: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut tokens: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (rid, r) in &self.graph.requirements {
            let toks = self.content_tokens(&r.statement, &r.entities);
            let ids: BTreeSet<&str> = r.entities.iter().map(|e| self.resolve_id(e)).collect();
            for id in ids {
                let e = counts.entry(id.to_string()).or_default();
                e.0 += 1;
                for t in &toks {
                    *e.1.entry(t.clone()).or_default() += 1;
                }
                by_entity
                    .entry(id.to_string())
                    .or_default()
                    .push(rid.clone());
            }
            tokens.insert(rid.clone(), toks);
        }
        PairScoring {
            hub: self.hub_entity(),
            counts,
            by_entity,
            tokens,
        }
    }

    // An entity's boilerplate: the content tokens carried by at least four of the
    // statements naming it and by at least half of them, the wording every statement
    // of that entity shares. Mirrors docs/compiler/reconciler.md#pairs.
    fn boilerplate_of(
        counts: &BTreeMap<String, (usize, BTreeMap<String, usize>)>,
        entities: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for e in entities {
            if let Some((n, toks)) = counts.get(e.as_ref()) {
                for (t, c) in toks {
                    if *c >= BOILERPLATE_MIN && *c * 2 >= *n {
                        out.insert(t.clone());
                    }
                }
            }
        }
        out
    }

    // The boilerplate of the entities two requirements share, as content stems
    // (`content_stem`), for a hint that drops the same words the scorer drops.
    pub fn pair_boilerplate(&self, a: &Requirement, b: &Requirement) -> BTreeSet<String> {
        self.pair_boilerplate_with(a, b, &self.pair_scoring())
    }

    // `pair_boilerplate` over scorer inputs computed once for a pass.
    pub fn pair_boilerplate_with(
        &self,
        a: &Requirement,
        b: &Requirement,
        scoring: &PairScoring,
    ) -> BTreeSet<String> {
        let bs: BTreeSet<&str> = b.entities.iter().map(|e| self.resolve_id(e)).collect();
        let shared: Vec<&str> = a
            .entities
            .iter()
            .map(|e| self.resolve_id(e))
            .filter(|e| bs.contains(e))
            .collect();
        if shared.is_empty() {
            return BTreeSet::new();
        }
        Self::boilerplate_of(&scoring.counts, shared)
    }

    // The graph's hub: the entity co-referenced with the most other entities across
    // requirements. A tie names none, and so does a graph whose entities never meet.
    // Mirrors docs/compiler/reconciler.md#pairs.
    pub(super) fn hub_entity(&self) -> Option<String> {
        let mut peers: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for r in self.graph.requirements.values() {
            let ids: BTreeSet<&str> = r.entities.iter().map(|e| self.resolve_id(e)).collect();
            for a in &ids {
                for b in &ids {
                    if a != b {
                        peers.entry(a).or_default().insert(b);
                    }
                }
            }
        }
        let best = peers.values().map(|p| p.len()).max().unwrap_or(0);
        if best == 0 {
            return None;
        }
        let mut hubs = peers.iter().filter(|(_, p)| p.len() == best);
        let hub = hubs.next().map(|(id, _)| id.to_string());
        if hubs.next().is_some() {
            return None;
        }
        hub
    }

    // The section reference a document opens with: its first section in order.
    pub(super) fn opening_section(&self, doc: &str) -> Option<&str> {
        self.docs
            .get(doc)?
            .sections
            .iter()
            .min_by_key(|(_, s)| s.order)
            .map(|(r, _)| r.as_str())
    }

    // Deterministic neighbor set for one requirement: other requirements sharing an
    // entity, scored by overlapping content tokens, at least two shared, best six.
    // The rejudge-pair locality. Mirrors docs/compiler/reconciler.md#pairs.
    pub fn requirement_neighbors(&self, rid: &str) -> Vec<String> {
        self.requirement_neighbors_scored(rid)
            .into_iter()
            .map(|(_, id)| id)
            .collect()
    }

    // The neighbor set with each neighbor's score, best first. The score is
    // symmetric (shared tokens, shared entities, the transition and intro boosts
    // read the same from either side), so a pair scores the same whichever side
    // derived it. Mirrors docs/compiler/reconciler.md#pairs.
    pub fn requirement_neighbors_scored(&self, rid: &str) -> Vec<(usize, String)> {
        self.requirement_neighbors_scored_with(rid, &self.pair_scoring())
    }

    // `requirement_neighbors_scored` over scorer inputs computed once for a pass:
    // the sweep and the goal derivation score every changed requirement against
    // the same graph, so the hub and the token counts are computed once, not per
    // record. Mirrors docs/compiler/reconciler.md#pairs.
    pub fn requirement_neighbors_scored_with(
        &self,
        rid: &str,
        scoring: &PairScoring,
    ) -> Vec<(usize, String)> {
        let Some(req) = self.graph.requirements.get(rid) else {
            return Vec::new();
        };
        let subject_entities: BTreeSet<&str> =
            req.entities.iter().map(|e| self.resolve_id(e)).collect();
        let PairScoring {
            hub,
            counts,
            by_entity,
            tokens,
        } = scoring;
        let toks: BTreeSet<String> = tokens
            .get(rid)
            .cloned()
            .unwrap_or_else(|| self.content_tokens(&req.statement, &req.entities));
        let norm = crate::derive::normalize_state;
        // The transition the subject carries, its subject resolved and its states
        // normalized, so a restated transition meets its original.
        let transition = req
            .transition
            .as_ref()
            .map(|t| (self.resolve_id(&t.subject), norm(&t.from), norm(&t.to)));
        // Whether the subject sits in its document's opening section, or in a later one.
        let placement = req.source.as_ref().and_then(|s| {
            let opening = self.opening_section(&s.doc)?;
            let order = self.docs.get(&s.doc)?.sections.get(&s.section)?.order;
            Some((s.doc.as_str(), s.section == opening, order))
        });
        // The candidates: every requirement naming one of the subject's entities. The
        // final sort is total (score, then id), so the walk's order does not matter.
        let mut candidates: BTreeSet<&String> = BTreeSet::new();
        for e in &subject_entities {
            if let Some(list) = by_entity.get(*e) {
                candidates.extend(list.iter());
            }
        }
        let mut scored: Vec<(usize, &String)> = Vec::new();
        for oid in candidates {
            if oid == rid {
                continue;
            }
            let Some(other) = self.graph.requirements.get(oid) else {
                continue;
            };
            let other_entities: BTreeSet<&str> =
                other.entities.iter().map(|e| self.resolve_id(e)).collect();
            let shared_entities = other_entities.intersection(&subject_entities).count();
            if shared_entities == 0 {
                continue;
            }
            // The shared entities' boilerplate leaves the pool: "supports" and
            // "operation" on every operation bullet of one page say nothing about
            // any two of them.
            let boilerplate =
                Self::boilerplate_of(counts, other_entities.intersection(&subject_entities));
            let computed;
            let other_toks: &BTreeSet<String> = match tokens.get(oid) {
                Some(t) => t,
                None => {
                    computed = self.content_tokens(&other.statement, &other.entities);
                    &computed
                }
            };
            let shared = other_toks
                .intersection(&toks)
                .filter(|t| !boilerplate.contains(*t))
                .count();
            // The hub never counts as a shared entity: sharing it says little (a
            // hub-sharing flood once derived forty-nine pair goals with one real pair
            // among them, and the hub plus one entity paired every statement of a shop
            // with no word in common). A pair sharing only the hub needs three tokens.
            let hub_shared = hub
                .as_deref()
                .map(|h| subject_entities.contains(h) && other_entities.contains(h))
                .unwrap_or(false);
            let shared_entities = shared_entities - usize::from(hub_shared);
            let hub_only = hub_shared && shared_entities == 0;
            let (entity_score, token_bar) = if hub_only {
                (0, 3)
            } else {
                (shared_entities, 2)
            };
            // Both carry a transition with the same subject and the same from and to:
            // a restated transition is the likeliest duplicate. Qualifies on its own,
            // scores two more.
            let same_transition = match (&transition, &other.transition) {
                (Some((subj, from, to)), Some(t)) => {
                    self.resolve_id(&t.subject) == *subj
                        && norm(&t.from) == *from
                        && norm(&t.to) == *to
                }
                _ => false,
            };
            // Two transitions of one subject with different ends are siblings of one
            // machine, distinct by construction: the state machine checks judge their
            // conflicts (nondeterministic-transition), never a pair. Six transition
            // bullets once opened twenty-eight pairs, every one consistent.
            let sibling_transitions = match (&transition, &other.transition) {
                (Some((subj, _, _)), Some(t)) => {
                    self.resolve_id(&t.subject) == *subj && !same_transition
                }
                _ => false,
            };
            if sibling_transitions {
                continue;
            }
            // An intro-versus-steps restatement: one sourced in the document's opening
            // section, the other in a later section of the same document. Qualifies
            // with one shared entity and one shared token (typically the verb stem),
            // scores one more.
            let intro_restatement = match (&placement, &other.source) {
                (Some((doc, intro, order)), Some(s)) if s.doc == *doc => {
                    let other_order = self
                        .docs
                        .get(&s.doc)
                        .and_then(|d| d.sections.get(&s.section))
                        .map(|sec| sec.order);
                    let other_intro = self.opening_section(&s.doc) == Some(s.section.as_str());
                    match other_order {
                        Some(o) => (*intro && o > *order) || (other_intro && *order > o),
                        None => false,
                    }
                }
                _ => false,
            };
            // Two shared content tokens qualify, and so do two shared entities with
            // one token: a restatement built from the same entities shares few other
            // words, since the shared names leave the token pool, but two entities
            // alone are what every statement of one page shares, and a same-entity
            // restatement with no word in common is review-entity's catch.
            // Mirrors docs/compiler/reconciler.md#pairs.
            let qualifies = shared >= token_bar
                || (shared_entities >= 2 && shared >= 1)
                || same_transition
                || (intro_restatement && shared >= 1);
            if qualifies {
                let boost = usize::from(same_transition) * 2 + usize::from(intro_restatement);
                scored.push((shared + entity_score + boost, oid));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        scored
            .into_iter()
            .take(6)
            .map(|(score, id)| (score, id.clone()))
            .collect()
    }

    // A created or revised record's candidates: the requirement's neighbors and
    // sticky partners minus the partners `detail.judged` lists, each with its score
    // (a sticky partner outside the neighbor set scores zero; it is pinned anyway).
    // Mirrors docs/compiler/reconciler.md#pairs.
    pub fn pair_review_candidates(&self, rec: &ChangeRecord) -> Vec<(String, usize)> {
        self.pair_review_candidates_with(rec, &self.pair_scoring())
    }

    // `pair_review_candidates` over scorer inputs computed once for a pass.
    pub fn pair_review_candidates_with(
        &self,
        rec: &ChangeRecord,
        scoring: &PairScoring,
    ) -> Vec<(String, usize)> {
        let judged = rec.judged();
        let mut out: Vec<(String, usize)> = self
            .requirement_neighbors_scored_with(&rec.subject, scoring)
            .into_iter()
            .map(|(score, id)| (id, score))
            .collect();
        for partner in self.sticky_partners(&rec.subject) {
            if !out.iter().any(|(id, _)| *id == partner) {
                out.push((partner, 0));
            }
        }
        out.retain(|(id, _)| !judged.contains(id));
        out
    }

    // The pair set: computed neighbors plus sticky partners. Open contradiction and
    // duplicate-requirement diagnostics tie pairs; editing one side of a known pair
    // always re-judges the other.
    pub fn pair_review_neighbors(&self, rid: &str) -> Vec<String> {
        let mut nbrs = self.requirement_neighbors(rid);
        for partner in self.sticky_partners(rid) {
            if !nbrs.contains(&partner) {
                nbrs.push(partner);
            }
        }
        nbrs
    }

    // The sticky partners of a requirement: the other living subjects of every open
    // contradiction or duplicate-requirement diagnostic naming it, in diagnostic order.
    fn sticky_partners(&self, rid: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for d in self.graph.diagnostics.values() {
            if d.lifecycle != "open"
                || !(d.rule == "contradiction" || d.rule == "duplicate-requirement")
            {
                continue;
            }
            if !d.subjects.iter().any(|s| s == rid) {
                continue;
            }
            for sub in &d.subjects {
                if sub != rid && self.graph.requirements.contains_key(sub) && !out.contains(sub) {
                    out.push(sub.clone());
                }
            }
        }
        out
    }

    // Open diagnostic counts by severity, suppressed excluded: the health line that
    // rides beside every verdict. Mirrors docs/compiler/compilation.md#convergence.
    pub fn open_diag_counts(&self) -> BTreeMap<String, u64> {
        let mut m: BTreeMap<String, u64> = BTreeMap::new();
        for d in self.graph.diagnostics.values() {
            // Proposals are questions, counted on their own line
            // (docs/compiler/model/diagnostic.md#ratification-proposals).
            if d.lifecycle == "open"
                && d.triage.as_deref() != Some("suppressed")
                && d.rule != "ratification-pending"
            {
                *m.entry(d.severity.clone()).or_insert(0) += 1;
            }
        }
        m
    }

    // Open ratification proposals: one per derived or decreed fact that stands.
    pub fn pending_proposals(&self) -> u64 {
        self.graph
            .diagnostics
            .values()
            .filter(|d| d.lifecycle == "open" && d.rule == "ratification-pending")
            .count() as u64
    }

    // An open judged diagnostic naming this node: reason enough for a pair judgment
    // even when the neighbor computation finds nothing.
    pub fn has_open_judged_diag(&self, id: &str) -> bool {
        self.graph.diagnostics.values().any(|d| {
            d.lifecycle == "open"
                && JUDGED_RULES.contains(&d.rule.as_str())
                && d.subjects.iter().any(|s| s == id)
        })
    }

    // Whether a pair judgment on a requirement still has work: the requirement exists
    // and either computed neighbors or an open judged diagnostic tie it to a judgment.
    pub fn pair_review_due(&self, rid: &str) -> bool {
        self.graph.requirements.contains_key(rid)
            && (!self.pair_review_neighbors(rid).is_empty() || self.has_open_judged_diag(rid))
    }
}

// How many pair scorings this thread computed: a test's measure of passes.
#[cfg(test)]
thread_local! {
    pub(super) static PAIR_SCORINGS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
