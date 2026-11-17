use super::*;

pub struct ConsolidateRequirements;

// Connected components of near-identical obligations, independent of document location.
pub fn requirement_clusters(store: &Store) -> Vec<Vec<String>> {
    let mut clusters: Vec<BTreeSet<String>> = Vec::new();
    let reqs: Vec<_> = store.graph.requirements.iter().collect();
    for (i, (a, ra)) in reqs.iter().enumerate() {
        for (b, rb) in reqs.iter().skip(i + 1) {
            if !ra.entities.iter().any(|e| rb.entities.contains(e)) {
                continue;
            }
            let ta: BTreeSet<String> = store::normalize_statement(&ra.statement)
                .split_whitespace()
                .map(String::from)
                .collect();
            let tb: BTreeSet<String> = store::normalize_statement(&rb.statement)
                .split_whitespace()
                .map(String::from)
                .collect();
            let key = pair_target(a, b);
            if store.decisions.pairs.get(&key).is_some_and(|p| {
                p.verdict == "distinct"
                    && p.statements.get(*a)
                        == Some(&hash_hex(&store::normalize_statement(&ra.statement)))
                    && p.statements.get(*b)
                        == Some(&hash_hex(&store::normalize_statement(&rb.statement)))
            }) {
                continue;
            }
            let union = ta.union(&tb).count();
            if union == 0 || ta.intersection(&tb).count() * 10 < union * 8 {
                continue;
            }
            let mut component: BTreeSet<String> =
                [(*a).clone(), (*b).clone()].into_iter().collect();
            let mut n = 0;
            while n < clusters.len() {
                if !clusters[n].is_disjoint(&component) {
                    component.extend(clusters.remove(n));
                } else {
                    n += 1;
                }
            }
            clusters.push(component);
        }
    }
    clusters
        .into_iter()
        .filter(|c| c.len() >= limits::REQUIREMENT_CLUSTER_SOFT)
        .map(|c| c.into_iter().collect())
        .collect()
}

impl GoalKind for ConsolidateRequirements {
    fn kind(&self) -> &'static str {
        "consolidate-requirements"
    }
    fn class(&self) -> Class {
        Class::Gc
    }
    fn unit(&self) -> &'static str {
        "requirement cluster"
    }
    fn derive_goals(&self, store: &Store, _gen: &GenSettings) -> Vec<Goal> {
        requirement_clusters(store)
            .into_iter()
            .map(|cluster| {
                let mandatory = cluster.len() >= limits::REQUIREMENT_CLUSTER_HARD;
                let target = cluster[0].clone();
                build_goal(
                    self,
                    &target,
                    mandatory,
                    json!({"requirements": cluster}),
                    None,
                    cluster.iter().map(|id| format!("load {}", id)).collect(),
                )
            })
            .collect()
    }
    fn ready(&self, goal: &Goal, board: &Board) -> Ready {
        gc_ready(goal, board)
    }
    fn pack(&self, _store: &Store, batch: &[Goal]) -> String {
        batch
            .iter()
            .flat_map(|g| g.change["requirements"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .map(|id| format!("- {} full", id))
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn toolset(&self) -> &'static [&'static str] {
        &[
            "merge_requirements",
            "update_requirement",
            "report_diagnostic",
        ]
    }
    fn gates(&self, _store: &Store, _staged: &[Op]) -> Vec<Violation> {
        Vec::new()
    }
    fn prompt(&self) -> &'static str {
        include_str!(concat!(
            env!("OUT_DIR"),
            "/payloads/goals/prompts/goals/consolidate-requirements.md"
        ))
    }
}
