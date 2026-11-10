// Derived data, recomputed on every commit and never written by a tool: relationships
// (per-direction contributions), state machines, default views (with the flow
// clustering), query membership, and the limit counts that write threshold-crossed
// records. Mirrors docs/compiler/graph.md#derived-data.
use crate::ids::entity_slug;
use crate::limits;
use crate::md;
use crate::model::*;
use crate::store::{
    scope_root_target, RecordBatch, Store, CHANGE_QUERY_MATCH, CHANGE_THRESHOLD_CROSSED,
};
use std::collections::{BTreeMap, BTreeSet};

mod levels;
mod order;
mod state_machines;
#[cfg(test)]
pub(crate) mod tests;
mod thresholds;
mod views;

use levels::{
    entity_document_rank_with, flow_view_level_index, level_flow_views, level_id, level_kind,
    level_title, Levels,
};
pub use levels::{
    flow_view_level, flow_view_levels, level_view_id, level_view_id_with, level_view_members,
    level_view_members_with, lift_into, view_levels,
};
#[cfg(test)]
use levels::{level_flow_views_scan, level_holds, level_members_of_scan, levels, Level};
pub(crate) use order::doc_levels_in;
pub use order::{doc_levels_from, document_order, Indices};
use state_machines::recompute_state_machines_with;
pub use state_machines::{normalize_state, recompute_state_machines};
pub use thresholds::{
    flow_participants, flow_view_participant_count, record_threshold_crossings,
    threshold_crossings, view_edge_count, Crossing, FLOW_KINDS,
};
use thresholds::{live_members, record_threshold_crossings_with};
#[cfg(test)]
use views::default_view_rules;
pub use views::{children_of_view, drawn_entities_of, query_matches, recompute_default_views};
use views::{
    doc_stem, doc_title, drawn_entities, recompute_default_views_with, title_case, DefaultView,
};

// Everything derived, in dependency order: relationships feed the level and object
// rules, state machines feed the state rule, views feed the limit counts.
pub fn recompute(store: &mut Store, build: &str, batch: &mut RecordBatch) {
    recompute_relationships(store);
    // The indices every later step reads, built once: none of the steps changes the
    // facts they index. Mirrors docs/compiler/graph.md#derived-data.
    let ix = Indices::build(store);
    recompute_state_machines_with(store, &ix);
    recompute_default_views_with(store, &ix, build, batch);
    record_threshold_crossings_with(store, &ix, batch);
}

// ---- relationships ----

// Group requirement edges by unordered entity pair, then by direction and type into
// contributions, each carrying the requirements behind it. An untyped edge contributes
// dependency. Mirrors docs/compiler/model/relationship.md#recompute.
pub fn recompute_relationships(store: &mut Store) {
    let mut rels: BTreeMap<String, Relationship> = BTreeMap::new();
    for (rid, r) in &store.graph.requirements {
        for e in &r.edges {
            let a = store.resolve_id(&e.a).to_string();
            let b = store.resolve_id(&e.b).to_string();
            if a == b
                || !store.graph.entities.contains_key(&a)
                || !store.graph.entities.contains_key(&b)
            {
                continue;
            }
            let (x, y) = if a <= b { (&a, &b) } else { (&b, &a) };
            let key = format!("rel:{}~{}", entity_slug(x), entity_slug(y));
            let t = e
                .rel_type
                .clone()
                .unwrap_or_else(|| DEFAULT_REL_TYPE.to_string());
            let rel = rels.entry(key).or_insert_with(|| Relationship {
                members: vec![x.clone(), y.clone()],
                contributions: Vec::new(),
            });
            match rel
                .contributions
                .iter_mut()
                .find(|c| c.a == a && c.b == b && c.r#type == t)
            {
                Some(c) => {
                    if !c.requirements.contains(rid) {
                        c.requirements.push(rid.clone());
                    }
                    // A group carries a cardinality only when every edge that states
                    // one agrees.
                    if let Some(card) = e.cardinality.as_ref() {
                        if c.cardinality.as_ref().is_some_and(|x| x != card) {
                            c.cardinality = None;
                        } else if c.cardinality.is_none() && c.requirements.len() == 1 {
                            c.cardinality = Some(card.clone());
                        }
                    }
                }
                None => rel.contributions.push(Contribution {
                    a: a.clone(),
                    b: b.clone(),
                    r#type: t,
                    cardinality: e.cardinality.clone(),
                    requirements: vec![rid.clone()],
                }),
            }
        }
    }
    for rel in rels.values_mut() {
        rel.contributions
            .sort_by(|p, q| (&p.a, &p.b, &p.r#type).cmp(&(&q.a, &q.b, &q.r#type)));
        for c in rel.contributions.iter_mut() {
            c.requirements.sort();
        }
    }
    store.graph.relationships = rels;
}

// Instance -> type, from the instantiation groups.
pub fn instance_types(store: &Store) -> BTreeMap<String, String> {
    if let Some(ix) = crate::store::ScopeIndex::of(store) {
        return ix.instance_types(|| scan_instance_types(store)).clone();
    }
    scan_instance_types(store)
}

fn scan_instance_types(store: &Store) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for rel in store.graph.relationships.values() {
        for c in &rel.contributions {
            if c.r#type == INSTANTIATION {
                out.insert(c.a.clone(), c.b.clone());
            }
        }
    }
    out
}

// The ledger comparison, persisted as ledger-stale change records when the build
// derives its board: one record per row the ledger and the graph disagree on, the
// detail refreshed in place, the record cleared when the row agrees again. Bind,
// generate, and verify goals stand on these records.
// Mirrors docs/compiler/graph.md#change-records.
pub fn record_ledger_stale(store: &mut Store, gs: &crate::gen::GenSettings) {
    let fresh = crate::gen::ledger_stale_records(store, gs);
    let mut changed = false;
    let before = store.status.changes.len();
    store.status.changes.retain(|c| {
        c.kind != crate::goals::CHANGE_LEDGER_STALE
            || fresh
                .iter()
                .any(|f| f.subject == c.subject && f.detail["goal"] == c.detail["goal"])
    });
    changed |= store.status.changes.len() != before;
    for f in fresh {
        let pos = store.status.changes.iter().position(|c| {
            c.kind == crate::goals::CHANGE_LEDGER_STALE
                && c.subject == f.subject
                && c.detail["goal"] == f.detail["goal"]
        });
        match pos {
            Some(i) => {
                if store.status.changes[i].detail != f.detail {
                    store.status.changes[i].detail = f.detail;
                    changed = true;
                }
            }
            None => {
                store.status.changes.push(f);
                changed = true;
            }
        }
    }
    if changed {
        // A refusal is reported on stderr; the next derivation records them again.
        store.write_status("ledger-stale records").ok();
    }
}
