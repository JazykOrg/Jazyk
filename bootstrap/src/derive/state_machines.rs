use super::*;

// ---- state machines ----

// State names compare after trimming, lowercasing, and collapsing whitespace.
pub fn normalize_state(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

// One machine per entity any transition names as subject: states in document order
// (first spelling kept), a restated transition merged into one arrow listing its
// contributing requirements, arrows ordered by lowest requirement id, `initial` the one
// state no transition enters. Mirrors docs/compiler/model/state-machine.md#derivation.
pub fn recompute_state_machines(store: &mut Store) {
    let ix = Indices::build(store);
    recompute_state_machines_with(store, &ix);
}

pub(super) fn recompute_state_machines_with(store: &mut Store, ix: &Indices) {
    let mut machines: BTreeMap<String, StateMachine> = BTreeMap::new();
    for rid in &ix.document_order {
        let r = &store.graph.requirements[rid];
        let Some(t) = r.transition.as_ref() else {
            continue;
        };
        let subject = store.resolve_id(&t.subject).to_string();
        if !store.graph.entities.contains_key(&subject) {
            continue;
        }
        let key = format!("sm:{}", entity_slug(&subject));
        let m = machines.entry(key).or_insert_with(|| StateMachine {
            subject: subject.clone(),
            ..Default::default()
        });
        let mut spelled = |name: &str| -> String {
            let norm = normalize_state(name);
            match m.states.iter().find(|s| normalize_state(s) == norm) {
                Some(s) => s.clone(),
                None => {
                    let s = name.split_whitespace().collect::<Vec<_>>().join(" ");
                    m.states.push(s.clone());
                    s
                }
            }
        };
        let from = spelled(&t.from);
        let to = spelled(&t.to);
        // Two contributions merge when from and to are the same states and their
        // triggers and guards agree: equal after normalization, or named by only one
        // side. Distinct triggers or distinct guards stay distinct arrows, the way a
        // relationship keeps one group per direction and type.
        let agrees = |a: &Option<String>, b: &Option<String>| match (a, b) {
            (Some(a), Some(b)) => normalize_state(a) == normalize_state(b),
            _ => true,
        };
        match m.transitions.iter_mut().find(|a| {
            a.from == from
                && a.to == to
                && agrees(&a.trigger, &t.trigger)
                && agrees(&a.guard, &t.guard)
        }) {
            Some(a) => {
                if !a.requirements.contains(rid) {
                    a.requirements.push(rid.clone());
                }
                // The first spelling in document order stays; a side that named no
                // trigger or guard takes the one the restatement names.
                if a.trigger.is_none() {
                    a.trigger = t.trigger.clone();
                }
                if a.guard.is_none() {
                    a.guard = t.guard.clone();
                }
            }
            None => m.transitions.push(StateTransition {
                from,
                to,
                trigger: t.trigger.clone(),
                guard: t.guard.clone(),
                requirements: vec![rid.clone()],
            }),
        }
    }
    for m in machines.values_mut() {
        for a in m.transitions.iter_mut() {
            a.requirements.sort();
        }
        m.transitions
            .sort_by(|a, b| a.requirements[0].cmp(&b.requirements[0]));
        let entered: BTreeSet<String> = m
            .transitions
            .iter()
            .map(|t| normalize_state(&t.to))
            .collect();
        let candidates: Vec<&String> = m
            .states
            .iter()
            .filter(|s| !entered.contains(&normalize_state(s)))
            .collect();
        m.initial = if candidates.len() == 1 {
            Some(candidates[0].clone())
        } else {
            None
        };
    }
    store.graph.state_machines = machines;
}
