use super::*;

// The graph after the staged work, as far as references go: nodes created, updated,
// deleted, and merged. The retrace gate reads what the target points at here.
fn references_after(store: &Store, staged: &[Op]) -> Store {
    let mut after = store.clone();
    let g = &mut after.graph;
    for op in staged {
        match op {
            Op::CreateEntity { id, entity } => {
                g.entities.insert(id.clone(), entity.clone());
            }
            Op::UpdateEntity {
                id,
                parent,
                clear_parent,
                set_attributes,
                add_attributes,
                provenance,
                ..
            } => {
                if let Some(e) = g.entities.get_mut(id) {
                    if let Some(p) = parent {
                        e.parent = Some(p.clone());
                    }
                    if *clear_parent {
                        e.parent = None;
                    }
                    if let Some(a) = set_attributes {
                        e.attributes = a.clone();
                    }
                    for a in add_attributes {
                        e.attributes
                            .retain(|x| !x.name.eq_ignore_ascii_case(&a.name));
                        e.attributes.push(a.clone());
                    }
                    if let Some(p) = provenance {
                        e.provenance = Some(p.clone());
                    }
                }
            }
            Op::DeleteEntity { id, .. } | Op::DissolveEntity { id, .. } => {
                g.entities.remove(id);
            }
            Op::MergeEntities { keep, absorb, .. } => {
                g.entities.remove(absorb);
                g.redirects.insert(absorb.clone(), keep.clone());
            }
            Op::CreateRequirement { id, requirement } => {
                g.requirements.insert(id.clone(), requirement.clone());
            }
            Op::UpdateRequirement {
                id,
                entities,
                edges,
                transition,
                provenance,
                ..
            } => {
                if let Some(r) = g.requirements.get_mut(id) {
                    if let Some(x) = entities {
                        r.entities = x.clone();
                    }
                    if let Some(x) = edges {
                        r.edges = x.clone();
                    }
                    if let Some(x) = transition {
                        r.transition = Some(x.clone());
                    }
                    if let Some(x) = provenance {
                        r.provenance = Some(x.clone());
                    }
                }
            }
            Op::DeleteRequirement { id, .. } => {
                g.requirements.remove(id);
            }
            Op::RetractDecree { id, .. } => {
                g.entities.remove(id);
                g.requirements.remove(id);
                g.views.remove(id);
            }
            Op::CreateView { id, view } => {
                g.views.insert(id.clone(), view.clone());
            }
            Op::UpdateView {
                id,
                members,
                add_members,
                remove_members,
                collapse,
                exclude,
                ..
            } => {
                if let Some(v) = g.views.get_mut(id) {
                    if let Some(ms) = members {
                        v.members = ms.clone();
                    }
                    for x in add_members {
                        if !v.members.contains(x) {
                            v.members.push(x.clone());
                        }
                    }
                    v.members.retain(|x| !remove_members.contains(x));
                    v.collapse.retain(|x| !remove_members.contains(x));
                    v.excluded.retain(|x| !remove_members.contains(&x.id));
                    if let Some(c) = collapse {
                        v.collapse = c.clone();
                    }
                    for x in exclude {
                        v.members.retain(|y| y != &x.id);
                        if !v.excluded.iter().any(|y| y.id == x.id) {
                            v.excluded.push(x.clone());
                        }
                    }
                }
            }
            Op::DeleteView { id, .. } => {
                g.views.remove(id);
            }
            _ => {}
        }
    }
    after
}

// ---- retrace ----

pub struct Retrace;

// The references on a node that name a dead or missing node.
pub fn dangling_references(store: &Store, id: &str) -> Vec<String> {
    let alive = |x: &str| {
        let r = store.resolve_id(x);
        store.graph.entities.contains_key(r) || store.graph.requirements.contains_key(r)
    };
    let mut out = Vec::new();
    if let Some(v) = store.graph.views.get(id) {
        for (i, m) in v.members.iter().enumerate() {
            if !alive(m) {
                out.push(format!("members[{}] {}", i, m));
            }
        }
        for m in &v.collapse {
            if !alive(m) {
                out.push(format!("collapse {}", m));
            }
        }
        for x in &v.excluded {
            if !alive(&x.id) {
                out.push(format!("excluded {}", x.id));
            }
        }
        if let Some(Provenance::Derived { from, .. }) = &v.provenance {
            for (i, f) in from.iter().enumerate() {
                if !alive(f) && !store.graph.views.contains_key(f) {
                    out.push(format!("from[{}] {}", i, f));
                }
            }
        }
    }
    if let Some(e) = store.graph.entities.get(id) {
        if let Some(p) = &e.parent {
            if !alive(p) {
                out.push(format!("parent {}", p));
            }
        }
        for a in &e.attributes {
            match &a.provenance {
                Provenance::Quote(s) => {
                    if !store.quote_locates(&s.doc, &s.section, &s.quote) {
                        out.push(format!(
                            "attributes.{}.provenance {}#{}",
                            a.name, s.doc, s.section
                        ));
                    }
                }
                Provenance::Derived { from, .. } => {
                    for f in from {
                        if !alive(f) {
                            out.push(format!("attributes.{}.from {}", a.name, f));
                        }
                    }
                }
                Provenance::Decree { .. } => {}
            }
        }
        if let Some(Provenance::Derived { from, .. }) = &e.provenance {
            for (i, f) in from.iter().enumerate() {
                if !alive(f) {
                    out.push(format!("from[{}] {}", i, f));
                }
            }
        }
    }
    if let Some(r) = store.graph.requirements.get(id) {
        for e in &r.entities {
            if !alive(e) {
                out.push(format!("entities {}", e));
            }
        }
        for edge in &r.edges {
            for end in [&edge.a, &edge.b] {
                if !alive(end) {
                    out.push(format!("edges {}", end));
                }
            }
        }
        if let Some(t) = &r.transition {
            if !alive(&t.subject) {
                out.push(format!("transition.subject {}", t.subject));
            }
        }
        if let Some(Provenance::Derived { from, .. }) = &r.provenance {
            for (i, f) in from.iter().enumerate() {
                if !alive(f) {
                    out.push(format!("from[{}] {}", i, f));
                }
            }
        }
    }
    out
}

impl GoalKind for Retrace {
    fn kind(&self) -> &'static str {
        "retrace"
    }
    fn class(&self) -> Class {
        Class::Compile
    }
    fn unit(&self) -> &'static str {
        "node"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        let mut by_subject: BTreeMap<String, Vec<&ChangeRecord>> = BTreeMap::new();
        for rec in records_of(
            store,
            &[store::CHANGE_VIEW_MEMBER_GONE, store::CHANGE_NODE_DELETED],
        ) {
            let exists = store.graph.views.contains_key(&rec.subject)
                || store.graph.entities.contains_key(&rec.subject)
                || store.graph.requirements.contains_key(&rec.subject);
            if !exists {
                continue;
            }
            if rec.kind == store::CHANGE_NODE_DELETED
                && !matches!(rec.via.as_str(), "from" | "edges" | "attributes")
            {
                continue;
            }
            by_subject.entry(rec.subject.clone()).or_default().push(rec);
        }
        let mut out = Vec::new();
        for (id, recs) in by_subject {
            let mut dead: Vec<String> = Vec::new();
            let mut vias: BTreeSet<String> = BTreeSet::new();
            for r in &recs {
                dead.extend(str_list(&r.detail["gone"]));
                dead.extend(str_list(&r.detail["deleted"]));
                vias.insert(r.via.clone());
            }
            dead.sort();
            dead.dedup();
            let cause = earliest_cause(&recs);
            let change = json!({"deleted": dead, "via": vias, "in": format!("g{}", cause.as_ref().map(|c| c.generation).unwrap_or(0))});
            let mut hints = Vec::new();
            for d in &dead {
                let fate = match store.graph.redirects.get(d) {
                    Some(t) if !t.is_empty() => format!("redirects to {}", t),
                    Some(_) => "tombstone".to_string(),
                    None => "gone".to_string(),
                };
                hints.push(format!(
                    "{} deleted in g{} ({}); jazyk ripple {} --back",
                    d,
                    cause.as_ref().map(|c| c.generation).unwrap_or(0),
                    fate,
                    d
                ));
            }
            let dangling = dangling_references(store, &id);
            if !dangling.is_empty() {
                hints.push(format!("dangling: {}", dangling.join(", ")));
            }
            if let Some(v) = store.graph.views.get(&id) {
                hints.push(format!(
                    "{} view, {} member(s) in order: {}",
                    v.kind,
                    v.members.len(),
                    v.members
                        .iter()
                        .map(|m| if dead.contains(m) {
                            format!("{} (deleted)", m)
                        } else {
                            m.clone()
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if let Some(ty) = crate::derive::instance_types(store).get(&id) {
                hints.push(format!("instance of {}", ty));
            }
            hints.push(format!("load {}", id));
            for s in skills_for("retrace", store, &id) {
                hints.push(format!("skill {}", s));
            }
            hints.push("update_view remove_members or add_members, update_entity, update_requirement, delete_view".into());
            out.push(build_goal(self, &id, true, change, cause, hints));
        }
        out
    }
    fn ready(&self, _goal: &Goal, board: &Board) -> Ready {
        tier2_ready(board)
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        let mut lines = Vec::new();
        for g in batch {
            lines.push(format!("- {} full", g.target));
            for d in str_list(&g.change["deleted"]) {
                lines.push(format!("- {} tombstone", d));
            }
        }
        lines.join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "upsert_view",
            "update_view",
            "delete_view",
            "upsert_entity",
            "update_entity",
            "delete_entity",
            "merge_entities",
            "upsert_requirement",
            "update_requirement",
            "delete_requirement",
            "report_diagnostic",
        ]
    }
    fn gates(&self, store: &Store, staged: &[Op]) -> Vec<Violation> {
        let mut out = Vec::new();
        for op in staged {
            if let Op::CreateEntity { id, entity } = op {
                if store.graph.redirects.get(id).is_some_and(|t| t.is_empty())
                    || store.graph.redirects.iter().any(|(dead, t)| {
                        t.is_empty() && dead == &format!("ent:{}", crate::md::slug(&entity.name))
                    })
                {
                    out.push(Violation::new(
                        "recreated-node",
                        format!(
                            "{} re-creates a deleted node; point at a survivor instead",
                            id
                        ),
                    ));
                }
            }
            if let Op::DeleteView { id, reason } = op {
                if reason.trim().is_empty() {
                    out.push(Violation::new(
                        "reason-required",
                        format!("delete_view {} carries no reason", id),
                    ));
                }
            }
        }
        out
    }
    // Nothing on the target points at a dead or missing node over the store plus the
    // staged work; a target deleted in this session passes.
    // Mirrors docs/compiler/goals/retrace.md#gate.
    fn goal_gate(&self, cx: &GateContext) -> Option<Violation> {
        let after = references_after(cx.store, cx.staged);
        let target = after.resolve_id(&cx.goal.target).to_string();
        let dangling = dangling_references(&after, &target);
        (!dangling.is_empty()).then(|| {
            Violation::new(
                "dangling-reference",
                format!(
                    "{}: {} still points at a dead node: {}; repair, re-derive, or drop each reference (never re-create the dead node), or delete the target",
                    cx.goal.goal,
                    target,
                    dangling.join(", ")
                ),
            )
        })
    }
    fn prompt(&self) -> &'static str {
        include_str!("../../../docs/compiler/goals/prompts/retrace.md")
    }
}
