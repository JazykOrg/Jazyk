// The limits registry: built into the binary, never a project setting. Crossing a soft
// threshold opens an optional goal; crossing the hard one makes it mandatory. Session
// and build budgets, document quality thresholds, and alignment thresholds live here
// too. Mirrors docs/compiler/graph.md#limits.

pub struct Limit {
    pub name: &'static str,
    pub soft: u64,
    pub hard: u64,
    // The goal kind that resolves a crossing.
    pub goal: &'static str,
}

// One node's direct children, its level; the scope root counts its parentless entities
// under the same row. Crossing derives the fan-out variant of `abstract-entity`.
// Mirrors docs/compiler/concepts/levels.md#levels.
pub const CHILDREN_PER_ENTITY: &str = "children-per-entity";
pub const CHILDREN_PER_ENTITY_SOFT: u64 = 9;
pub const CHILDREN_PER_ENTITY_HARD: u64 = 15;

pub const LIMITS: [Limit; 8] = [
    Limit {
        name: "requirements-per-entity",
        soft: 50,
        hard: 80,
        goal: "abstract-entity",
    },
    Limit {
        name: CHILDREN_PER_ENTITY,
        soft: CHILDREN_PER_ENTITY_SOFT,
        hard: CHILDREN_PER_ENTITY_HARD,
        goal: "abstract-entity",
    },
    Limit {
        name: "members-per-structural-view",
        soft: 20,
        hard: 30,
        goal: "split-view",
    },
    Limit {
        name: "edges-per-view",
        soft: 40,
        hard: 60,
        goal: "split-view",
    },
    Limit {
        name: "members-per-flow-view",
        soft: 12,
        hard: 20,
        goal: "split-view",
    },
    Limit {
        name: "participants-per-sequence-view",
        soft: 8,
        hard: 12,
        goal: "split-view",
    },
    Limit {
        name: "instances-per-object-view",
        soft: 15,
        hard: 25,
        goal: "split-view",
    },
    Limit {
        name: "states-per-state-machine",
        soft: 12,
        hard: 20,
        goal: "abstract-entity",
    },
];

// The registry row for a limit name; None for a name outside the registry.
pub fn limit(name: &str) -> Option<&'static Limit> {
    LIMITS.iter().find(|l| l.name == name)
}

// The (soft, hard) thresholds for a node: the per-node bump is the soft threshold and
// the hard threshold is the bump plus the registry's distance between soft and hard,
// so escalation keeps its shape. Mirrors docs/compiler/graph.md#per-node-bumps.
pub fn threshold(name: &str, node_bump: Option<u64>) -> Option<(u64, u64)> {
    let l = limit(name)?;
    Some(match node_bump {
        Some(n) => (n, n + (l.hard - l.soft)),
        None => (l.soft, l.hard),
    })
}

// The limits that apply to an entity, and the ones that apply to a view.
pub const ENTITY_LIMITS: [&str; 3] = [
    "requirements-per-entity",
    CHILDREN_PER_ENTITY,
    "states-per-state-machine",
];
pub const VIEW_LIMITS: [&str; 5] = [
    "members-per-structural-view",
    "edges-per-view",
    "members-per-flow-view",
    "participants-per-sequence-view",
    "instances-per-object-view",
];

// Session budgets. Mirrors docs/compiler/sessions.md#budgets.
pub const SESSION_ROUNDS: u32 = 24;
// The embedded agent's flat runaway stop, in model round-trips (one may carry
// several tool calls). `JAZYK_AGENT_MAX_ROUNDS` overrides.
pub const AGENT_MAX_ROUNDS: u32 = 48;
pub const SESSION_MUTATIONS: usize = 64;
pub const CONTEXT_BUDGET: usize = 24_000;
pub const ROUNDS_PER_SECTION: u32 = 8;
pub const SKILLS_PER_SESSION: usize = 4;
// The fraction of the context budget past which `load` refuses.
pub const LOADED_HIGH_WATER: f64 = 0.9;

// The build cap: BUILD_SESSION_FACTOR times the derived goals, plus the floor.
pub const BUILD_SESSION_FACTOR: usize = 3;
pub const BUILD_SESSION_FLOOR: usize = 8;
// Consecutive zero-token session failures that stop a build early: an endpoint
// answering only errors must not grind the session cap into futile attempts.
pub const ENDPOINT_BREAKER: usize = 5;

// Document quality thresholds.
pub const MAX_SECTION_CHARS: usize = 6_000;
pub const MAX_DOC_SECTIONS: usize = 40;

// Alignment thresholds. Mirrors docs/compiler/alignment.md.
pub const ALIGN_MOVE_SIMILARITY: f64 = 0.5;
pub const ALIGN_SPLIT_COVERAGE: f64 = 0.6;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_eight_rows_with_soft_below_hard() {
        assert_eq!(LIMITS.len(), 8);
        for l in &LIMITS {
            assert!(l.soft < l.hard, "{}", l.name);
            assert!(
                l.goal == "abstract-entity" || l.goal == "split-view",
                "{}",
                l.name
            );
            assert_eq!(limit(l.name).map(|x| x.hard), Some(l.hard));
        }
        assert!(limit("no-such-limit").is_none());
        assert_eq!(threshold("edges-per-view", None), Some((40, 60)));
        assert_eq!(threshold("edges-per-view", Some(70)), Some((70, 90)));
        assert_eq!(
            threshold("requirements-per-entity", Some(70)),
            Some((70, 100))
        );
        assert_eq!(
            threshold(CHILDREN_PER_ENTITY, None),
            Some((CHILDREN_PER_ENTITY_SOFT, CHILDREN_PER_ENTITY_HARD))
        );
        assert_eq!(threshold(CHILDREN_PER_ENTITY, None), Some((9, 15)));
        assert_eq!(threshold(CHILDREN_PER_ENTITY, Some(12)), Some((12, 18)));
        assert_eq!(ENTITY_LIMITS.len() + VIEW_LIMITS.len(), LIMITS.len());
        for l in &LIMITS {
            assert!(ENTITY_LIMITS.contains(&l.name) || VIEW_LIMITS.contains(&l.name));
        }
    }
}
