// Explain and ripple: a goal's cause and readiness, the goals a change opens, the
// bubbling preview, and the derived data a commit recomputes.
// Mirrors docs/frontends/cli.md#jazyk-explain.
use super::*;

impl Board {
    // ---- explain ----

    // A goal: its change, cause, readiness, blockers, hints. A target: the goals a
    // change to it would open. Mirrors docs/frontends/cli.md#jazyk-explain.
    pub fn explain(&self, store: &Store, target: &str) -> Option<String> {
        if let Some(g) = self.goal(target) {
            let mut s = format!(
                "{}  {}, {}, {}, {}\n",
                g.id,
                g.kind,
                g.class,
                if g.mandatory { "mandatory" } else { "optional" },
                state_word(&g.state)
            );
            s.push_str(&format!("  change: {}\n", condense_change(&g.change)));
            if let Some(c) = &g.cause {
                s.push_str(&format!(
                    "  cause:  g{} mutation {} via {}\n",
                    c.generation, c.mutation, c.via
                ));
            }
            let tier = goals::tier(&g.kind)
                .map(|t| format!("tier {}", t))
                .unwrap_or_else(|| "gc (waits for its cone)".into());
            match self.readiness.get(&g.id) {
                Some(Ready::Ready) => s.push_str(&format!("  ready:  {}; ready now\n", tier)),
                Some(Ready::Blocked(r)) => {
                    s.push_str(&format!("  ready:  {}; waits: {}\n", tier, r))
                }
                None => {}
            }
            let blockers = self.cone_blockers(&g.id);
            if !blockers.is_empty() {
                s.push_str(&format!("  cone:   {}\n", blockers.join(", ")));
            }
            if let Some(r) = self.parked_reasons.get(&g.id) {
                s.push_str(&format!("  parked: {}\n", r));
            }
            for r in self.records_of(&g.id) {
                s.push_str(&format!("  record: {}\n", r));
            }
            if !g.hints.is_empty() {
                s.push_str(&format!("  hints:  {}\n", g.hints.join("; ")));
            }
            return Some(s);
        }
        if let Some(w) = self.waiting_pair(target) {
            return Some(self.explain_waiting(store, w));
        }
        if !target_exists(store, target) {
            return None;
        }
        let mut s = format!("{}: a change here opens\n", target);
        for (kind, t, via) in opened_by_change(store, target) {
            s.push_str(&format!("  {:<18} {:<36} via {}\n", kind, t, via));
        }
        let recomputed = recomputed_by_change(store, target);
        if !recomputed.is_empty() {
            s.push_str(&format!(
                "  recomputed at commit: {}\n",
                recomputed.join(", ")
            ));
        }
        Some(s)
    }
}

// The goals a change to a target would open, walking the stored references from it.
// Mirrors docs/frontends/cli.md#jazyk-explain.
pub fn opened_by_change(store: &Store, target: &str) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    let mut push = |kind: &str, t: &str, via: &str| {
        let row = (kind.to_string(), t.to_string(), via.to_string());
        if !out.contains(&row) {
            out.push(row);
        }
    };
    let views_listing = |id: &str| -> Vec<String> {
        store
            .graph
            .views
            .iter()
            .filter(|(_, v)| {
                !v.default
                    && v.members
                        .iter()
                        .chain(v.collapse.iter())
                        .any(|m| store.resolve_id(m) == id)
            })
            .map(|(vid, _)| vid.clone())
            .collect()
    };
    if let Some((doc, sec)) = split_section_ref(target) {
        push("reconcile-section", target, "section-dirty");
        for (rid, r) in &store.graph.requirements {
            if r.anchored_at(&doc, &sec) {
                for n in store.pair_review_neighbors(rid) {
                    push("rejudge-pair", &goals::pair_target(rid, &n), "entities");
                }
                for e in &r.entities {
                    push("review-entity", store.resolve_id(e), "entities");
                }
                for v in views_listing(rid) {
                    push("retrace", &v, "members");
                }
            }
        }
        return out;
    }
    if store.docs.contains_key(target) {
        if let Some(rec) = store.docs.get(target) {
            for r in rec.sections.keys() {
                push(
                    "reconcile-section",
                    &format!("{}#{}", target, r),
                    "section-dirty",
                );
            }
        }
        return out;
    }
    if let Some(r) = store.graph.requirements.get(target) {
        for n in store.pair_review_neighbors(target) {
            push("rejudge-pair", &goals::pair_target(target, &n), "entities");
        }
        for e in &r.entities {
            push("review-entity", store.resolve_id(e), "entities");
        }
        for v in views_listing(target) {
            push("retrace", &v, "members");
        }
        push("bind", target, "ledger");
        return out;
    }
    if store.graph.entities.contains_key(target) {
        push("review-entity", target, "entity-changed");
        for (inst, ty) in crate::derive::instance_types(store) {
            if ty == target {
                push("conform-instance", &inst, "instantiation");
            }
        }
        for v in views_listing(target) {
            push("retrace", &v, "members");
        }
        for (cid, c) in &store.graph.entities {
            if c.parent.as_deref() == Some(target) {
                push("review-entity", cid, "parent");
            }
        }
        for (rid, r) in &store.graph.requirements {
            if r.entities.iter().any(|e| store.resolve_id(e) == target) {
                push("bind", rid, "ledger");
            }
        }
        push("generate", target, "ledger");
    }
    if store.graph.views.contains_key(target) {
        push("curate-view", target, "query");
    }
    out
}

// The bubbling preview: the goals a staged changeset will open at commit, derived
// over the snapshot plus the staged ops, one human line each. The tool serving
// appends these to a mutating reply. Mirrors docs/compiler/reconciler.md#bubbling.
pub fn staged_opens(store: &Store, staged: &[crate::store::Op]) -> Vec<String> {
    use crate::store::Op;
    let mut out: Vec<String> = Vec::new();
    let mut push = |kind: &str, target: &str, why: &str| {
        let line = format!("{} {} ({})", kind, target, why);
        if !out.contains(&line) && out.len() < 12 {
            out.push(line);
        }
    };
    let views_listing = |id: &str| -> Vec<String> {
        store
            .graph
            .views
            .iter()
            .filter(|(_, v)| {
                !v.default
                    && v.members
                        .iter()
                        .chain(v.collapse.iter())
                        .any(|m| store.resolve_id(m) == id)
            })
            .map(|(vid, _)| vid.clone())
            .collect()
    };
    let derived_from = |id: &str| -> Vec<String> {
        let names = |p: &Option<crate::model::Provenance>| match p {
            Some(crate::model::Provenance::Derived { from, .. }) => {
                from.iter().any(|f| store.resolve_id(f) == id)
            }
            _ => false,
        };
        let mut v: Vec<String> = store
            .graph
            .entities
            .iter()
            .filter(|(_, e)| names(&e.provenance))
            .map(|(eid, _)| eid.clone())
            .collect();
        v.extend(
            store
                .graph
                .requirements
                .iter()
                .filter(|(_, r)| r.source.is_none() && names(&r.provenance))
                .map(|(rid, _)| rid.clone()),
        );
        v
    };
    for op in staged {
        match op {
            Op::ClassifyInput { doc, .. } => {
                push("reconcile-section", doc, "input processing mode")
            }
            Op::DeleteRequirement { id, .. } => {
                for v in views_listing(id) {
                    push("retrace", &v, "member gone");
                }
                for n in derived_from(id) {
                    push("retrace", &n, "justification gone");
                }
                if let Some(r) = store.graph.requirements.get(id) {
                    for e in &r.entities {
                        push("review-entity", store.resolve_id(e), "statement gone");
                    }
                }
            }
            Op::DeleteEntity { id, .. } => {
                for v in views_listing(id) {
                    push("retrace", &v, "member gone");
                }
                for n in derived_from(id) {
                    push("retrace", &n, "justification gone");
                }
            }
            Op::UpdateRequirement { id, statement, .. } if statement.is_some() => {
                for n in store.pair_review_neighbors(id) {
                    push("rejudge-pair", &goals::pair_target(id, &n), "revised");
                }
                if let Some(r) = store.graph.requirements.get(id) {
                    for e in &r.entities {
                        push("review-entity", store.resolve_id(e), "requirements changed");
                    }
                }
            }
            Op::MergeEntities { keep, .. } => {
                push("review-entity", store.resolve_id(keep), "merged");
            }
            Op::CreateRequirement { id, requirement } => {
                if store.graph.requirements.contains_key(id) {
                    for n in store.pair_review_neighbors(id) {
                        push("rejudge-pair", &goals::pair_target(id, &n), "revised");
                    }
                }
                for e in &requirement.entities {
                    push("review-entity", store.resolve_id(e), "requirements changed");
                }
            }
            _ => {}
        }
    }
    out
}

// The derived data a commit touching the target recomputes.
pub fn recomputed_by_change(store: &Store, target: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let ids: Vec<String> = match store.graph.requirements.get(target) {
        Some(r) => r
            .entities
            .iter()
            .map(|e| store.resolve_id(e).to_string())
            .collect(),
        None => vec![target.to_string()],
    };
    for id in &ids {
        for (rid, rel) in &store.graph.relationships {
            if rel.members.contains(id) && !out.contains(rid) {
                out.push(rid.clone());
            }
        }
        let sm = format!("sm:{}", crate::ids::entity_slug(id));
        if store.graph.state_machines.contains_key(&sm) && !out.contains(&sm) {
            out.push(sm);
        }
        for (vid, v) in &store.graph.views {
            if v.default
                && v.members
                    .iter()
                    .any(|m| store.resolve_id(m) == id || m == target)
                && !out.contains(vid)
            {
                out.push(vid.clone());
            }
        }
    }
    out
}
