// Batching: the context estimate, withholding, batch formation, and the locality
// keys goals join on. Mirrors docs/compiler/reconciler.md#batching.
use super::*;

// The characters a goal's initially loaded set costs, so a batch fills under the
// context budget. Mirrors docs/compiler/reconciler.md#batching.
pub fn estimate(store: &Store, g: &Goal) -> usize {
    match g.kind.as_str() {
        "reconcile-section" => {
            let body = split_section_ref(&g.target)
                .and_then(|(d, s)| {
                    store
                        .docs
                        .get(&d)
                        .and_then(|r| r.sections.get(&s).map(|x| x.raw.len()))
                })
                .unwrap_or(0);
            body + 1_200
        }
        "place-anchors" => {
            let proposals = store
                .status
                .alignment
                .iter()
                .find(|b| b.doc == g.target)
                .map(|b| b.proposals.len())
                .unwrap_or(0);
            800 + 700 * proposals
        }
        "rejudge-pair" => {
            // Both sides load full plus their neighbor stubs: the cost follows
            // the statements, never a flat guess (a 13-pair batch of long
            // statements does not fit where 13 flat guesses said it would).
            let side = |id: &str| {
                store
                    .graph
                    .requirements
                    .get(store.resolve_id(id))
                    .map(|r| 260 + r.statement.len() + 90 * r.entities.len())
                    .unwrap_or(400)
            };
            match g.target.split_once('~') {
                Some((a, b)) => 700 + side(a) + side(b),
                None => 1_400,
            }
        }
        "review-entity" => 1_500 + 140 * store.requirements_referencing(&g.target).len(),
        // The listed requirements load full and each yields two or three criteria.
        "derive-criteria" => {
            900 + 700
                * (g.change["none"].as_array().map(|a| a.len()).unwrap_or(0)
                    + g.change["revised"].as_array().map(|a| a.len()).unwrap_or(0))
        }
        // The caps variant loads the node's requirements; the fan-out variant loads
        // the level as stubs (docs/compiler/goals/abstract-entity.md#fan-out-hints).
        "abstract-entity" => {
            1_500
                + 160 * store.requirements_referencing(&g.target).len()
                + 120 * level_members(store, &g.target).len()
        }
        "curate-view" | "split-view" | "retrace" => {
            let members = store
                .graph
                .views
                .get(&g.target)
                .map(|v| v.members.len())
                .unwrap_or(0);
            1_200 + 90 * members
        }
        "dedupe-candidates" => 2_400,
        _ => 1_500,
    }
}

impl Board {
    // ---- batching ----

    // Hold back goals a build parked mid-run: they stay open on the board (the build
    // writes `parked` at its close) but take no batch, and the batches re-form
    // without them so their locality neighbors still run.
    // Mirrors docs/compiler/reconciler.md#parked-and-failed.
    pub fn withhold(&mut self, ids: &BTreeSet<String>, store: &Store, proj: &Project) {
        let held: Vec<String> = ids
            .iter()
            .filter(|id| self.batches.iter().any(|b| b.goals.contains(id)))
            .cloned()
            .collect();
        if held.is_empty() {
            return;
        }
        for id in held {
            self.readiness.insert(
                id,
                Ready::Blocked("parked in this build; the next build resumes it first".into()),
            );
        }
        self.batches = self.form_batches(store, proj);
    }

    pub(super) fn form_batches(&self, store: &Store, proj: &Project) -> Vec<Batch> {
        let ready: Vec<&Goal> = self.ready_goals();
        // Generation runs leaf first over the relationships and the containment tree:
        // every ready generate target's position in that order.
        // Mirrors docs/compiler/goals/generate.md#readiness.
        let generate_targets: Vec<String> = ready
            .iter()
            .filter(|g| g.kind == "generate")
            .map(|g| g.target.clone())
            .collect();
        let topo: BTreeMap<String, usize> = crate::gen::generation_order(store, &generate_targets)
            .into_iter()
            .enumerate()
            .map(|(i, t)| (t, i))
            .collect();
        let generate_rank = |g: &Goal| {
            (g.kind == "generate")
                .then(|| topo.get(&g.target).copied())
                .flatten()
        };
        let executor = |g: &Goal| proj.executors.resolve(&g.kind, &g.class).map(String::from);
        // Group key: class, tier, executor. GC goals sit after the compile tiers,
        // mandatory before optional.
        let mut groups: BTreeMap<(u8, u8, Option<String>), Vec<&Goal>> = BTreeMap::new();
        for g in ready {
            let key = match Class::parse(&g.class) {
                Class::Compile => (0, goals::tier(&g.kind).unwrap_or(9), executor(g)),
                Class::Gc => (1, if g.mandatory { 0 } else { 1 }, executor(g)),
            };
            groups.entry(key).or_default().push(g);
        }
        let mut batches: Vec<Batch> = Vec::new();
        // A locality is where a batch starts, not where it must end: the document
        // and ledger localities are walls, the node, view, and level localities pack
        // on into the same batch until the budget is spent.
        // Mirrors docs/compiler/reconciler.md#batching.
        let walled = |id: &str| {
            self.goal(id).is_some_and(|g| {
                matches!(
                    g.kind.as_str(),
                    "place-anchors" | "reconcile-section" | "bind" | "generate" | "verify"
                )
            })
        };
        // The skills of the batch's goal kinds render into the same context budget
        // as the loaded set (sessions.md#budgets): the estimates fill what the skill
        // payloads of the goals actually in the batch leave, never the union over
        // the whole tier (a pair batch pays for judgment, not for the conformance
        // skill of an instance goal it does not hold).
        let skills_size = |names: &[&str]| -> usize {
            names
                .iter()
                .filter_map(|n| {
                    crate::session::SKILLS
                        .iter()
                        .find(|(k, _)| k == n)
                        .map(|(_, p)| p.len())
                })
                .sum()
        };
        let budget_with = |names: &[&str]| -> usize {
            limits::CONTEXT_BUDGET
                .saturating_sub(skills_size(names))
                .max(6_000)
        };
        for ((class, tier, exec), members) in groups {
            let localities = self.localities(store, &members);
            let mut current: Vec<String> = Vec::new();
            let mut skills: Vec<&str> = Vec::new();
            let mut label = String::new();
            let mut size = 0usize;
            let mut sections = 0usize;
            let mut criteria = 0usize;
            // The requirements the batch already loads: a pair pays only for the sides
            // the batch does not hold yet (docs/compiler/reconciler.md#batching).
            let mut charged: BTreeSet<String> = BTreeSet::new();
            let flush = |current: &mut Vec<String>,
                         skills: &mut Vec<&str>,
                         label: &mut String,
                         batches: &mut Vec<Batch>| {
                skills.clear();
                if !current.is_empty() {
                    batches.push(Batch {
                        id: String::new(),
                        class: if class == 0 {
                            Class::Compile
                        } else {
                            Class::Gc
                        },
                        tier: if class == 0 { Some(tier) } else { None },
                        goals: std::mem::take(current),
                        executor: exec.clone(),
                        locality: std::mem::take(label),
                    });
                }
            };
            for (locality, mut ids) in localities {
                let wall = ids.iter().any(|id| walled(id));
                if wall {
                    flush(&mut current, &mut skills, &mut label, &mut batches);
                    size = 0;
                    sections = 0;
                    criteria = 0;
                    charged.clear();
                }
                // Parked goals first, pairs before entity reviews, sections in
                // document order, deepest abstraction first. Development starts
                // before standalone binding within a locality.
                ids.sort_by_cached_key(|id| {
                    let g = self.goal(id).unwrap();
                    let parked = !matches!(g.state, GoalState::Parked);
                    let (kind, order) = match g.kind.as_str() {
                        "rejudge-pair" => (0, 0),
                        "generate" => (0, generate_rank(g).unwrap_or(usize::MAX)),
                        "bind" => (2, 0),
                        "abstract-entity" => (1, usize::MAX - entity_depth(store, &g.target)),
                        "reconcile-section" => (1, section_order(store, &g.target)),
                        _ => (1, 0),
                    };
                    (parked, kind, order, id.clone())
                });
                for id in ids {
                    let g = self.goal(&id).unwrap();
                    let sides: Vec<String> = match (g.kind.as_str(), g.target.split_once('~')) {
                        ("rejudge-pair", Some((a, b))) => vec![a.to_string(), b.to_string()],
                        _ => Vec::new(),
                    };
                    let mut cost = if sides.is_empty() {
                        estimate(store, g)
                    } else {
                        let side = |id: &str| {
                            store
                                .graph
                                .requirements
                                .get(store.resolve_id(id))
                                .map(|r| 260 + r.statement.len() + 90 * r.entities.len())
                                .unwrap_or(400)
                        };
                        700 + sides
                            .iter()
                            .filter(|x| !charged.contains(*x))
                            .map(|x| side(x))
                            .sum::<usize>()
                    };
                    let is_section = g.kind == "reconcile-section";
                    let is_criteria = g.kind == "derive-criteria";
                    let max_sections =
                        (limits::SESSION_ROUNDS / limits::ROUNDS_PER_SECTION) as usize;
                    let mut with_goal = skills.clone();
                    for s in goals::skills_for(&g.kind, store, &g.target) {
                        if !with_goal.contains(&s) {
                            with_goal.push(s);
                        }
                    }
                    let over = !current.is_empty()
                        && (size + cost > budget_with(&with_goal)
                            || (is_section && sections >= max_sections)
                            || (is_criteria && criteria >= limits::CRITERIA_GOALS_PER_BATCH));
                    if over {
                        flush(&mut current, &mut skills, &mut label, &mut batches);
                        size = 0;
                        sections = 0;
                        criteria = 0;
                        charged.clear();
                        // A fresh batch holds none of the pair's sides yet.
                        cost = estimate(store, g);
                        with_goal = goals::skills_for(&g.kind, store, &g.target);
                    }
                    if current.is_empty() {
                        label = locality.clone();
                    }
                    skills = with_goal;
                    current.push(id);
                    size += cost;
                    if is_section {
                        sections += 1;
                    }
                    if is_criteria {
                        criteria += 1;
                    }
                    charged.extend(sides);
                }
                if wall {
                    flush(&mut current, &mut skills, &mut label, &mut batches);
                    size = 0;
                    sections = 0;
                    criteria = 0;
                    charged.clear();
                }
            }
            flush(&mut current, &mut skills, &mut label, &mut batches);
        }
        // Compile tiers first, then GC with mandatory batches before optional ones;
        // within a tier, batches holding parked goals first, then document level and
        // path. Mirrors docs/compiler/reconciler.md#gc-gating.
        let rank = |b: &Batch| {
            let parked = b
                .goals
                .iter()
                .any(|id| matches!(self.goal(id).map(|g| &g.state), Some(GoalState::Parked)));
            let optional = b
                .goals
                .iter()
                .all(|id| self.goal(id).is_some_and(|g| !g.mandatory));
            let level = b
                .goals
                .first()
                .and_then(|id| self.goal(id))
                .and_then(|g| target_doc(&g.target))
                .and_then(|d| self.doc_levels.get(&d).copied())
                .unwrap_or(0);
            // Generation batches follow the leaf-first order of their earliest goal.
            let generation = b
                .goals
                .iter()
                .filter_map(|id| self.goal(id))
                .filter_map(&generate_rank)
                .min();
            (
                matches!(b.class, Class::Gc),
                b.tier.unwrap_or(0),
                optional,
                !parked,
                level,
                generation,
                b.locality.clone(),
            )
        };
        // A batch is labeled by the key most of its goals share, never by the
        // union-find root of its locality group: forty-one pair batches over two
        // characters once all read as the notebook scene's.
        // Mirrors docs/compiler/reconciler.md#batching.
        for b in &mut batches {
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for id in &b.goals {
                if let Some(g) = self.goal(id) {
                    for k in locality_keys(store, g) {
                        *counts.entry(k).or_insert(0) += 1;
                    }
                }
            }
            if let Some((k, _)) = counts.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))) {
                b.locality = k.clone();
            }
        }
        batches.sort_by_cached_key(rank);
        // Numbering skips the ids live leases hold, so a batch formed while another
        // is being worked never takes the working batch's id (a session once read the
        // board's fresh `b24-1` as its own and reported the two as inconsistent).
        let mut n = 0;
        for b in batches.iter_mut() {
            loop {
                n += 1;
                let id = format!("b{}-{}", self.generation, n);
                if !self.held_batches.contains(&id) {
                    b.id = id;
                    break;
                }
            }
        }
        batches
    }

    // Locality groups within one class and tier: document, node neighborhood, view,
    // or ledger entity. Mirrors docs/compiler/reconciler.md#batching.
    pub(super) fn localities(
        &self,
        store: &Store,
        members: &[&Goal],
    ) -> Vec<(String, Vec<String>)> {
        // Union-find over the keys each goal touches.
        let mut parent: BTreeMap<String, String> = BTreeMap::new();
        fn find(parent: &mut BTreeMap<String, String>, k: &str) -> String {
            let p = parent.get(k).cloned().unwrap_or_else(|| k.to_string());
            if p == k {
                return p;
            }
            let root = find(parent, &p);
            parent.insert(k.to_string(), root.clone());
            root
        }
        fn union(parent: &mut BTreeMap<String, String>, a: &str, b: &str) {
            let (ra, rb) = (find(parent, a), find(parent, b));
            if ra != rb {
                parent.insert(ra, rb);
            }
        }
        let mut keys_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for g in members {
            let keys = locality_keys(store, g);
            for k in &keys {
                parent.entry(k.clone()).or_insert_with(|| k.clone());
            }
            for w in keys.windows(2) {
                union(&mut parent, &w[0], &w[1]);
            }
            keys_of.insert(g.id.clone(), keys);
        }
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for g in members {
            let keys = &keys_of[&g.id];
            let root = keys
                .first()
                .map(|k| find(&mut parent, k))
                .unwrap_or_else(|| g.id.clone());
            groups.entry(root).or_default().push(g.id.clone());
        }
        let mut out: Vec<(String, Vec<String>)> = groups
            .into_iter()
            .map(|(root, ids)| {
                let label = keys_of[&ids[0]].first().cloned().unwrap_or(root);
                (label, ids)
            })
            .collect();
        out.sort();
        out
    }
}

fn entity_depth(store: &Store, id: &str) -> usize {
    let mut depth = 0;
    let mut cur = store.graph.entities.get(id).and_then(|e| e.parent.clone());
    while let Some(p) = cur {
        depth += 1;
        if depth > 32 {
            break;
        }
        cur = store.graph.entities.get(&p).and_then(|e| e.parent.clone());
    }
    depth
}

fn section_order(store: &Store, target: &str) -> usize {
    split_section_ref(target)
        .and_then(|(d, s)| {
            store
                .docs
                .get(&d)
                .and_then(|r| r.sections.get(&s).map(|x| x.lines[0]))
        })
        .unwrap_or(usize::MAX)
}

// The keys a goal's locality joins on: its document, its entities, its requirements,
// its view. The first key labels the locality. Ledger goals join through their
// entity's component group root, so the ready goals of one component subtree form
// one batch (docs/consumers/gen.md#grouping-by-component).
pub(super) fn locality_keys(store: &Store, g: &Goal) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    fn entity_keys_into(store: &Store, keys: &mut Vec<String>, ids: &[String]) {
        for e in ids {
            keys.push(format!("ent {}", store.resolve_id(e)));
        }
    }
    let mut entity_keys = |ids: &[String]| entity_keys_into(store, &mut keys, ids);
    let mut own: Vec<String> = Vec::new();
    match g.kind.as_str() {
        "place-anchors" => own.push(format!("doc {}", g.target)),
        "reconcile-section" => {
            if let Some(d) = target_doc(&g.target) {
                own.push(format!("doc {}", d));
            }
        }
        "rejudge-pair" => {
            if let Some((a, b)) = goals::pair_members(&g.target) {
                let mut ents: Vec<String> = Vec::new();
                for r in [a, b] {
                    if let Some(req) = store.graph.requirements.get(r) {
                        ents.extend(req.entities.iter().cloned());
                    }
                }
                entity_keys(&ents);
                own.push(format!("req {}", a));
            }
        }
        "review-entity" | "abstract-entity" | "conform-instance" => {
            // Level locality: an abstraction joins through the level's members,
            // the node's direct children or the scope's parentless entities
            // (docs/compiler/reconciler.md#batching). The scope root is no entity
            // and never a key of its own.
            let mut ents: Vec<String> = if scope_target(&g.target).is_some() {
                Vec::new()
            } else {
                vec![g.target.clone()]
            };
            if g.kind == "abstract-entity" {
                ents.extend(level_members(store, &g.target));
            }
            if g.kind == "conform-instance" {
                if let Some(t) = g.change["type"].as_str() {
                    ents.push(t.to_string());
                }
            }
            if g.kind == "review-entity" {
                for rid in store.requirements_referencing(&g.target) {
                    if let Some(r) = store.graph.requirements.get(&rid) {
                        ents.extend(r.entities.iter().cloned());
                    }
                }
            }
            entity_keys(&ents);
        }
        "retrace" | "curate-view" | "split-view" => {
            own.push(format!("view {}", g.target));
            if let Some(v) = store.graph.views.get(&g.target) {
                let mut ents: Vec<String> = Vec::new();
                for m in &v.members {
                    if store.graph.entities.contains_key(m) {
                        ents.push(m.clone());
                    } else if let Some(r) = store.graph.requirements.get(m) {
                        ents.extend(r.entities.iter().cloned());
                    }
                }
                entity_keys(&ents);
            } else if let Some(r) = store.graph.requirements.get(&g.target) {
                entity_keys(&r.entities);
            } else {
                entity_keys(std::slice::from_ref(&g.target));
            }
        }
        "declare-edges" => {
            if let Some(r) = store.graph.requirements.get(&g.target) {
                entity_keys(&r.entities);
            }
        }
        "dedupe-candidates" => {
            if let Some((a, b)) = goals::pair_members(&g.target) {
                entity_keys(&[a.to_string(), b.to_string()]);
            }
        }
        "bind" | "verify" => {
            if let Some(e) = g.change["entity"].as_str() {
                let id = store.resolve_id(e).to_string();
                own.push(format!("ent {}", crate::gen::group_root(store, &id)));
            }
        }
        "generate" => own.push(format!("ent {}", crate::gen::group_root(store, &g.target))),
        _ => {}
    }
    // The kind's own key labels the locality; the entity keys join it.
    own.extend(keys);
    if own.is_empty() {
        own.push(format!("goal {}", g.id));
    }
    own
}
