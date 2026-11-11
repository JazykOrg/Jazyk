use super::*;

// ---- the fan-out variant ----
// A level is a node's direct children; the top level of a scope is its parentless
// entities, addressed as `scope:<scope>`. Mirrors docs/compiler/concepts/levels.md.

// Ids per candidate before the rest folds into a count.
pub const FAN_OUT_CANDIDATE_IDS: usize = 12;

// The scope behind a `scope:<scope>` target. Mirrors
// docs/compiler/concepts/levels.md#the-scope-root.
pub fn scope_target(target: &str) -> Option<&str> {
    crate::board::scope_target(target)
}

// The level a node sits in: its parent, or its scope's root form when parentless.
pub fn level_of(store: &Store, id: &str) -> Option<String> {
    let e = store.graph.entities.get(store.resolve_id(id))?;
    Some(level_key(e.parent.as_deref(), &e.scope))
}

fn level_key(parent: Option<&str>, scope: &str) -> String {
    match parent.filter(|p| !p.is_empty()) {
        Some(p) => p.to_string(),
        None => store::scope_root_target(scope),
    }
}

// The members of a level, by id: the direct children of a node, or the parentless
// entities of a scope for the root form.
pub fn level_members(store: &Store, target: &str) -> Vec<String> {
    crate::board::level_members(store, target)
}

// The standing fan-out crossing on a level: the count and the thresholds in force
// (the node's bump folded in by the record writer).
#[derive(Clone, Debug, PartialEq)]
pub struct FanOut {
    pub count: u64,
    pub soft: u64,
    pub hard: u64,
}

impl FanOut {
    // Over hard the goal is mandatory. Mirrors docs/compiler/reconciler.md#escalation.
    pub fn mandatory(&self) -> bool {
        self.count > self.hard
    }

    // The count the level must reach: hard for a mandatory goal, soft otherwise.
    // Mirrors docs/compiler/goals/abstract-entity.md#the-fan-out-gate.
    pub fn limit_in_force(&self) -> u64 {
        if self.mandatory() {
            self.hard
        } else {
            self.soft
        }
    }
}

pub fn fan_out_crossing(store: &Store, target: &str) -> Option<FanOut> {
    store
        .status
        .changes
        .iter()
        .find(|c| {
            c.kind == store::CHANGE_THRESHOLD_CROSSED
                && c.subject == target
                && c.detail["limit"] == limits::CHILDREN_PER_ENTITY
        })
        .map(|c| FanOut {
            count: c.detail["count"].as_u64().unwrap_or(0),
            soft: c.detail["soft"].as_u64().unwrap_or(0),
            hard: c.detail["hard"].as_u64().unwrap_or(0),
        })
}

// One coupling partition: an ordered id list (at most FAN_OUT_CANDIDATE_IDS, the rest
// counted) with its internal weight.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub ids: Vec<String>,
    pub rest: usize,
    pub weight: u64,
}

// Every descendant of the level's members lifted to its member, the members to
// themselves.
fn lift_map(store: &Store, members: &[String]) -> BTreeMap<String, String> {
    let children = children_index(store);
    let mut lift = BTreeMap::new();
    for m in members {
        lift.insert(m.clone(), m.clone());
        for d in descendants(&children, m) {
            lift.insert(d, m.clone());
        }
    }
    lift
}

// weight(a, b) over the level's members: the requirements referencing both plus the
// derived relationships between them, descendants lifted to the member they sit under.
// Keys are ordered pairs (a < b). Mirrors docs/compiler/reconciler.md#fan-out.
pub fn coupling_weights(store: &Store, members: &[String]) -> BTreeMap<(String, String), u64> {
    let lift = lift_map(store, members);
    let mut weights: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut bump = |a: &str, b: &str| {
        if a != b {
            let key = if a < b {
                (a.to_string(), b.to_string())
            } else {
                (b.to_string(), a.to_string())
            };
            *weights.entry(key).or_insert(0) += 1;
        }
    };
    for r in store.graph.requirements.values() {
        let lifted: BTreeSet<&str> = r
            .entities
            .iter()
            .filter_map(|e| lift.get(store.resolve_id(e)))
            .map(String::as_str)
            .collect();
        let lifted: Vec<&str> = lifted.into_iter().collect();
        for (i, a) in lifted.iter().enumerate() {
            for b in &lifted[i + 1..] {
                bump(a, b);
            }
        }
    }
    for rel in store.graph.relationships.values() {
        if let [a, b] = rel.members.as_slice() {
            if let (Some(la), Some(lb)) =
                (lift.get(store.resolve_id(a)), lift.get(store.resolve_id(b)))
            {
                bump(la, lb);
            }
        }
    }
    weights
}

// Greedy agglomeration over the level's members: singletons merge by the highest
// total weight between two clusters, ties to the pair whose ids sort first, until the
// cluster count is at or under `soft` and every cluster has two or more members or is
// a singleton nothing touches. A zero-weight merge is a coupling artifact, so the
// agglomeration stops there. A singleton is no grouping and is not a candidate (the
// member lines show it); the rest go largest first, capped at `soft` candidates.
// Deterministic over the same graph. Mirrors docs/compiler/reconciler.md#fan-out.
pub fn coupling_candidates(store: &Store, target: &str, soft: u64) -> Vec<Candidate> {
    let members = level_members(store, target);
    let weights = coupling_weights(store, &members);
    let weight = |a: &str, b: &str| -> u64 {
        let key = if a < b {
            (a.to_string(), b.to_string())
        } else {
            (b.to_string(), a.to_string())
        };
        weights.get(&key).copied().unwrap_or(0)
    };
    let between = |x: &[String], y: &[String]| -> u64 {
        x.iter()
            .map(|a| y.iter().map(|b| weight(a, b)).sum::<u64>())
            .sum()
    };
    let internal = |c: &[String]| -> u64 {
        c.iter()
            .enumerate()
            .map(|(i, a)| c[i + 1..].iter().map(|b| weight(a, b)).sum::<u64>())
            .sum()
    };
    let mut clusters: Vec<Vec<String>> = members.iter().map(|m| vec![m.clone()]).collect();
    loop {
        let touched = |i: usize| {
            clusters
                .iter()
                .enumerate()
                .any(|(j, o)| j != i && between(&clusters[i], o) > 0)
        };
        let settled = clusters.len() as u64 <= soft
            && clusters
                .iter()
                .enumerate()
                .all(|(i, c)| c.len() >= 2 || !touched(i));
        if settled {
            break;
        }
        // A candidate is a cohesive subset the model can name: a merge never grows
        // a cluster past half the soft threshold, so a fully connected level yields
        // several candidates, never one tautological whole.
        let cap = (soft / 2).max(2) as usize;
        let mut best: Option<(u64, usize, usize)> = None;
        for i in 0..clusters.len() {
            for j in i + 1..clusters.len() {
                if clusters[i].len() + clusters[j].len() > cap {
                    continue;
                }
                let w = between(&clusters[i], &clusters[j]);
                if best.is_none_or(|(bw, _, _)| w > bw) {
                    best = Some((w, i, j));
                }
            }
        }
        match best {
            Some((w, i, j)) if w > 0 => {
                let other = clusters.remove(j);
                clusters[i].extend(other);
                clusters[i].sort();
                clusters.sort_by(|a, b| a[0].cmp(&b[0]));
            }
            _ => break,
        }
    }
    let mut scored: Vec<(Vec<String>, u64)> = clusters
        .into_iter()
        .filter(|c| c.len() >= 2)
        .map(|c| {
            let w = internal(&c);
            (c, w)
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.len()
            .cmp(&a.0.len())
            .then(b.1.cmp(&a.1))
            .then(a.0[0].cmp(&b.0[0]))
    });
    scored.truncate(soft as usize);
    scored
        .into_iter()
        .map(|(mut ids, weight)| {
            let rest = ids.len().saturating_sub(FAN_OUT_CANDIDATE_IDS);
            ids.truncate(FAN_OUT_CANDIDATE_IDS);
            Candidate { ids, rest, weight }
        })
        .collect()
}

// The document a member is mentioned in most, its requirements' sources when it has
// no mention; ties to the name that sorts first. A naming hint for the model.
// Mirrors docs/compiler/concepts/levels.md#naming.
pub fn dominant_document(store: &Store, id: &str) -> Option<String> {
    let mut by_doc: BTreeMap<String, usize> = BTreeMap::new();
    if let Some(e) = store.graph.entities.get(id) {
        for m in &e.mentions {
            *by_doc.entry(m.doc.clone()).or_insert(0) += 1;
        }
    }
    if by_doc.is_empty() {
        for rid in store.requirements_referencing(id) {
            if let Some(s) = &store.graph.requirements[&rid].source {
                *by_doc.entry(s.doc.clone()).or_insert(0) += 1;
            }
        }
    }
    by_doc
        .into_iter()
        .max_by_key(|(d, n)| (*n, std::cmp::Reverse(d.clone())))
        .map(|(d, _)| d)
}

// The fan-out hints for a level: the change, the candidates, each member with its
// stereotype and document, the existing groupings under the node.
// Mirrors docs/compiler/goals/abstract-entity.md#fan-out-hints.
pub(super) fn fan_out_hints(
    store: &Store,
    target: &str,
    f: &FanOut,
    cands: &[Candidate],
) -> Vec<String> {
    let mut hints = vec![format!(
        "fan-out {} > {} ({}, soft {}, hard {})",
        f.count,
        f.soft,
        limits::CHILDREN_PER_ENTITY,
        f.soft,
        f.hard
    )];
    for c in cands {
        let more = if c.rest > 0 {
            format!(" +{} more", c.rest)
        } else {
            String::new()
        };
        hints.push(format!(
            "candidate [{}]{} (weight {})",
            c.ids.join(", "),
            more,
            c.weight
        ));
        if crate::board::scope_target(target).is_some() {
            // The default scope's top level, not a scope anyone set
            // (docs/compiler/goals/abstract-entity.md#the-fan-out-variant).
            hints.push(format!(
                    "{} is the top level of the scope: its parentless entities, not a scope you set; a grouping under a documented name gives them a parent",
                    target
                ));
        }
    }
    let members = level_members(store, target);
    // The node's own document: the section that lists a member there is the heading
    // it sits under, the partition the model groups by first.
    // Mirrors docs/compiler/concepts/levels.md#naming.
    let node_doc = if store.graph.entities.contains_key(target) {
        dominant_document(store, target)
    } else {
        None
    };
    for m in &members {
        let e = &store.graph.entities[m];
        let mut line = format!("member {}", m);
        if let Some(s) = &e.stereotype {
            line.push_str(&format!(" «{}»", s));
        }
        let listing = node_doc.as_ref().and_then(|doc| {
            e.mentions
                .iter()
                .find(|mn| &mn.doc == doc)
                .map(|mn| format!("{}#{}", mn.doc, mn.section))
        });
        let dominant = dominant_document(store, m);
        match (listing, dominant) {
            (Some(l), Some(d)) if !l.starts_with(&format!("{}#", d)) => {
                line.push_str(&format!(" ({}; {})", l, d));
            }
            (Some(l), _) => line.push_str(&format!(" ({})", l)),
            (None, Some(d)) => line.push_str(&format!(" ({})", d)),
            (None, None) => {}
        }
        hints.push(line);
    }
    for m in &members {
        let n = level_members(store, m).len();
        if n > 0 {
            hints.push(format!("grouping {} ({} children)", m, n));
        }
    }
    // The namesake: a member whose name is the heading of the document that names
    // it and other members of the level. That entity is the level's node for the
    // document's entities. Mirrors docs/compiler/goals/abstract-entity.md#fan-out-hints.
    for m in &members {
        let Some(doc) = dominant_document(store, m) else {
            continue;
        };
        let Some(title) = store.docs.get(&doc).and_then(|d| {
            d.sections
                .values()
                .find(|s| s.kind == "root")
                .map(|s| s.title.clone())
        }) else {
            continue;
        };
        let name = &store.graph.entities[m].name;
        if !title.trim().eq_ignore_ascii_case(name.trim()) {
            continue;
        }
        let others = members
            .iter()
            .filter(|o| *o != m && dominant_document(store, o).as_deref() == Some(doc.as_str()))
            .count();
        if others > 0 {
            hints.push(format!(
                "namesake {} ({} is headed {}; {} other member(s) of the level from it)",
                m, doc, title, others
            ));
        }
    }
    hints
}

// A grouping staged in the changeset: a derived entity with no mentions, its members
// the entities whose parent becomes it in the same changeset.
struct StagedGrouping<'a> {
    id: &'a str,
    entity: &'a Entity,
    from: &'a [String],
    members: Vec<String>,
}

// The containment tree after the staged changeset: every surviving id with the parent
// and scope it holds at commit (a created grouping's members, a moved child, a
// dissolved grouping's children landing on its parent).
struct Projection<'a> {
    store: &'a Store,
    created: BTreeMap<&'a str, &'a Entity>,
    dissolved: BTreeMap<&'a str, Option<String>>,
    moved: BTreeMap<&'a str, &'a str>,
    all_ids: Vec<String>,
}

impl<'a> Projection<'a> {
    fn new(store: &'a Store, staged: &'a [Op]) -> Projection<'a> {
        let created: BTreeMap<&str, &Entity> = staged
            .iter()
            .filter_map(|o| match o {
                Op::CreateEntity { id, entity } => Some((id.as_str(), entity)),
                _ => None,
            })
            .collect();
        let deleted: BTreeSet<&str> = staged
            .iter()
            .filter_map(|o| match o {
                Op::DeleteEntity { id, .. }
                | Op::MergeEntities { absorb: id, .. }
                | Op::DissolveEntity { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let parent_before = |id: &str| -> Option<String> {
            store
                .graph
                .entities
                .get(store.resolve_id(id))
                .and_then(|e| e.parent.clone())
                .filter(|p| !p.is_empty())
        };
        // A dissolved grouping's children land on its parent.
        let dissolved: BTreeMap<&str, Option<String>> = staged
            .iter()
            .filter_map(|o| match o {
                Op::DissolveEntity { id, parent, .. } => Some((
                    id.as_str(),
                    parent
                        .clone()
                        .filter(|p| !p.is_empty())
                        .or_else(|| parent_before(id)),
                )),
                _ => None,
            })
            .collect();
        let moved: BTreeMap<&str, &str> = staged
            .iter()
            .filter_map(|o| match o {
                Op::UpdateEntity {
                    id,
                    parent: Some(p),
                    ..
                } => Some((id.as_str(), p.as_str())),
                _ => None,
            })
            .collect();
        let all_ids: Vec<String> = store
            .graph
            .entities
            .keys()
            .cloned()
            .chain(created.keys().map(|k| k.to_string()))
            .filter(|id| !deleted.contains(id.as_str()))
            .collect();
        Projection {
            store,
            created,
            dissolved,
            moved,
            all_ids,
        }
    }

    fn stored(&self, id: &str) -> Option<&'a Entity> {
        self.store.graph.entities.get(self.store.resolve_id(id))
    }

    fn parent_before(&self, id: &str) -> Option<String> {
        self.stored(id)
            .and_then(|e| e.parent.clone())
            .filter(|p| !p.is_empty())
    }

    fn scope_of(&self, id: &str) -> Option<String> {
        self.created
            .get(id)
            .map(|e| e.scope.clone())
            .or_else(|| self.stored(id).map(|e| e.scope.clone()))
    }

    fn parent_after(&self, id: &str) -> Option<String> {
        let p = match self.moved.get(id) {
            Some(p) => Some(p.to_string()).filter(|p| !p.is_empty()),
            None => self
                .created
                .get(id)
                .and_then(|e| e.parent.clone())
                .or_else(|| self.parent_before(id)),
        };
        match p.as_deref().and_then(|p| self.dissolved.get(p)) {
            Some(grand) => grand.clone(),
            None => p,
        }
    }

    // The level's member count after the changeset.
    fn level_count(&self, level: &str) -> u64 {
        self.all_ids
            .iter()
            .filter(|id| {
                let scope = self.scope_of(id).unwrap_or_default();
                level_key(self.parent_after(id).as_deref(), &scope) == level
            })
            .count() as u64
    }
}

// One level against its standing fan-out record: the count after the changeset is at
// or under the limit in force (hard when the goal is mandatory, soft otherwise). None
// when no record stands on the level.
fn fan_out_level_violation(proj: &Projection, level: &str) -> Option<Violation> {
    let f = fan_out_crossing(proj.store, level)?;
    let count = proj.level_count(level);
    if count <= f.limit_in_force() {
        return None;
    }
    Some(Violation::new(
        "fan-out-over-limit",
        format!(
            "{}: {} children after the changeset, over {} ({}, soft {}, hard {}); group more members, or fail the goal naming why the level is flat",
            level,
            count,
            f.limit_in_force(),
            limits::CHILDREN_PER_ENTITY,
            f.soft,
            f.hard
        ),
    ))
}

// The fan-out gate for one goal at mark_goal_done: the staged gates, then the goal's
// own level whether or not the changeset touched it, so a goal marked done without
// work still faces its count. Mirrors
// docs/compiler/goals/abstract-entity.md#the-fan-out-gate.
// The split-view claim: the target view, with this session's staged view updates
// applied, is within the soft value of every limit it crossed (or its bump). A split
// that leaves the view over the soft value commits at done but resolves nothing.
// Mirrors docs/compiler/goals/split-view.md#gate.
pub fn split_view_goal_gate(store: &Store, staged: &[Op], target: &str) -> Option<Violation> {
    let id = store.resolve_id(target).to_string();
    let mut v = store.graph.views.get(&id)?.clone();
    for op in staged {
        match op {
            Op::DeleteView { id: d, .. } if store.resolve_id(d) == id => return None,
            Op::UpdateView {
                id: u,
                members,
                add_members,
                remove_members,
                query,
                collapse,
                exclude,
                ..
            } if store.resolve_id(u) == id => {
                if let Some(ms) = members {
                    v.members = ms.clone();
                }
                for x in add_members {
                    if !v.members.contains(x) {
                        v.members.push(x.clone());
                    }
                }
                v.members.retain(|x| !remove_members.contains(x));
                if let Some(q) = query {
                    v.query = Some(q.clone());
                }
                if let Some(c) = collapse {
                    v.collapse = c.clone();
                }
                for x in exclude {
                    v.members.retain(|y| y != &x.id);
                }
            }
            _ => {}
        }
    }
    let mut after = store.clone();
    after.graph.views.insert(id.clone(), v);
    let c = crate::derive::threshold_crossings(&after)
        .into_iter()
        .find(|c| c.subject == id)?;
    Some(Violation::new(
        "over-limit",
        format!(
            "{} still has {} {} after the staged work, over the soft value {}; bring the view itself under it (for a flow view, keep one member per phase, the phase's first step, beside the overview), or fail the goal with the reason",
            id, c.count, c.limit, c.soft
        ),
    ))
}

pub fn fan_out_goal_gate(store: &Store, staged: &[Op], target: &str) -> Option<Violation> {
    if let Some(v) = fan_out_gates(store, staged).into_iter().next() {
        return Some(v);
    }
    fan_out_level_violation(&Projection::new(store, staged), target)
}

// The fan-out gate over the staged changeset. Every grouping created has two or more
// children, a definition, derived provenance naming exactly its members, and the
// members' shared current parent as its own (a grouping never crosses levels); no
// member changed scope; and every level the changeset touched that stands over its
// fan-out limit is brought to the limit in force (hard when the goal is mandatory,
// soft otherwise). A level the changeset leaves alone is a declined or failed goal's,
// never this gate's. Mirrors docs/compiler/goals/abstract-entity.md#the-fan-out-gate.
pub fn fan_out_gates(store: &Store, staged: &[Op]) -> Vec<Violation> {
    let mut out = Vec::new();
    let proj = Projection::new(store, staged);
    let created = &proj.created;

    // A grouping holds a level and carries no requirements of its own; a derived
    // entity a staged requirement points at is the caps variant's sub-entity.
    let carries_requirements = |id: &str| {
        staged.iter().any(|o| match o {
            Op::UpdateRequirement {
                entities: Some(e), ..
            } => e.iter().any(|x| x == id),
            Op::CreateRequirement { requirement, .. } => {
                requirement.entities.iter().any(|x| x == id)
            }
            _ => false,
        })
    };
    let groupings: Vec<StagedGrouping> = created
        .iter()
        .filter_map(|(id, entity)| match &entity.provenance {
            Some(Provenance::Derived { from, .. })
                if entity.mentions.is_empty() && !carries_requirements(id) =>
            {
                let members: Vec<String> = proj
                    .all_ids
                    .iter()
                    .filter(|m| m.as_str() != *id && proj.parent_after(m).as_deref() == Some(*id))
                    .cloned()
                    .collect();
                if members.is_empty() {
                    return None;
                }
                Some(StagedGrouping {
                    id,
                    entity,
                    from,
                    members,
                })
            }
            _ => None,
        })
        .collect();
    let mut touched: BTreeSet<String> = BTreeSet::new();
    for g in &groupings {
        touched.insert(level_key(g.entity.parent.as_deref(), &g.entity.scope));
        if g.members.len() < 2 {
            out.push(Violation::new(
                "grouping-too-small",
                format!(
                    "{}: a grouping holds two or more children ({} staged); below two there is nothing to judge",
                    g.id,
                    g.members.len()
                ),
            ));
        }
        if g.entity
            .definition
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            out.push(Violation::new(
                "definition-required",
                format!(
                    "{}: a grouping carries a one-sentence definition of its responsibility",
                    g.id
                ),
            ));
        }
        let named: BTreeSet<String> = g
            .from
            .iter()
            .map(|f| store.resolve_id(f).to_string())
            .collect();
        let members: BTreeSet<String> = g.members.iter().cloned().collect();
        if named != members {
            out.push(Violation::new(
                "provenance-mismatch",
                format!(
                    "{}: derived provenance names [{}] while its members are [{}]; from names exactly the members",
                    g.id,
                    named.iter().cloned().collect::<Vec<_>>().join(", "),
                    members.iter().cloned().collect::<Vec<_>>().join(", ")
                ),
            ));
        }
        let own_parent = g.entity.parent.clone().filter(|p| !p.is_empty());
        for m in &g.members {
            if created.contains_key(m.as_str()) {
                continue;
            }
            let was = proj.parent_before(m);
            if was != own_parent {
                out.push(Violation::new(
                    "cross-level",
                    format!(
                        "{}: a grouping never crosses levels; {} sits under {} while the grouping takes {}",
                        g.id,
                        m,
                        was.as_deref().unwrap_or("the scope root"),
                        own_parent.as_deref().unwrap_or("the scope root")
                    ),
                ));
            }
            if proj.scope_of(m).as_deref() != Some(g.entity.scope.as_str()) {
                out.push(Violation::new(
                    "scope-mismatch",
                    format!(
                        "{}: a grouping takes its members' scope; {} is in {}",
                        g.id,
                        m,
                        proj.scope_of(m).unwrap_or_default()
                    ),
                ));
            }
        }
    }
    for (id, p) in &proj.moved {
        if let Some(before) = level_of(store, id) {
            touched.insert(before);
        }
        touched.insert(level_key(Some(p), &proj.scope_of(id).unwrap_or_default()));
    }
    for (id, grand) in &proj.dissolved {
        touched.insert(level_key(
            grand.as_deref(),
            &proj.scope_of(id).unwrap_or_default(),
        ));
    }
    for level in touched {
        out.extend(fan_out_level_violation(&proj, &level));
    }
    out
}
