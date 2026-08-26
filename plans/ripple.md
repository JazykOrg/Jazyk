# Plan: ripple, convergence, and observing it

Status: draft for iteration. Detailed design under [ir-stages](./ir-stages.md).
Companions: [ir-graph](./ir-graph.md) (the shapes), [ir-agents](./ir-agents.md)
(who runs), [orchestration](./orchestration.md) (the registry and effects).

The target is a stable system: docs, graph, diagrams, deliverable, and tests as one
fixed point. A human edits any surface; compile detects the change, agents converge
the rest, and the whole run is explainable afterward: this edit caused these turns
caused these changes. This file defines the edit paths, the causality record, and
the observation surfaces.

## The stable system

Surfaces, and their standing in the fixed point:

- Documents: the source of truth. Everything else converges toward agreeing with
  them, and the ratification pressure pushes invented facts into them.
- Graph and diagrams: the IR. Diagrams are projections of the graph
  ([one graph, many diagrams](./ir-graph.md#one-graph-many-diagrams)), so "edit a
  diagram" and "edit the graph" are one path.
- Deliverable and tests: bound to the graph through the ledger; verification
  verdicts are the agreement measure.

Stable means: empty task queue across active stages, checks clean, no unratified
decree aging silently, verdicts current. Any edit anywhere breaks stability
locally; convergence restores it by walking the cone of the change, never the
whole system.

## Edit paths

Every human edit enters the system as one of four paths. All four end in the same
place: goals derived by the reconciler, sessions scheduled, fixed point restored
(the goal system is [ir-agents](./ir-agents.md)).

- Edit prose. The existing path: parse, align, dirty sections, `reconcile-doc`,
  cascade. Nothing new.
- Edit a quote-provenanced fact through a diagram or the GUI inspector. The fact's
  provenance names the sentence, so the edit is a dual write, generalizing the
  existing [dual-write tools](../docs/frontends/acp.md#dual-write-tools): the GUI
  stages the prose replacement and the graph mutation in one changeset. E.g.
  changing a class-diagram edge's cardinality from `1` to `1..*` rewrites the
  sentence behind the requirement that declared it, through `revise_requirement`.
  The commit absorbs the new section hash, so the edit does not dirty the document
  it just changed; downstream effects fire from the graph change. A fact whose
  sentence cannot carry the edit cleanly (the model proposes no rewrite the human
  accepts) falls through to a decree plus a ratification proposal.
- Edit a derived fact, or add a fact with no prose behind it: a decree. The
  changeset lands graph-only with `decree` provenance (author, time). Downstream
  effects fire normally: decreeing a new transition dirties the machine's checks,
  the tests bound to its requirements, and anything tracing through it. Upward,
  the decree queues a ratification proposal: docsgen renders the sentence the
  docs should gain, as a diagnostic prompt with a suggested edit, the same
  machinery contradiction prompts use. Accepting it writes the prose (dual
  write), the next reconcile flips the provenance to `quote`, and the proposal
  resolves. Rejecting it retracts the decree. A decree the docs later contradict
  draws a `contradiction` diagnostic naming both. Turns never overwrite a decree;
  they file diagnostics against it.
- Edit the deliverable or tests. The existing path: ledger statuses flip
  (`code-changed`, `test-changed`), verification reruns, the unclaimed report
  feeds decompile drafts. Unchanged, already effect-shaped.

Deletion rules follow the same logic in reverse: deleting prose kills quotes,
which kills quote-provenanced facts (existing GC), which dirties everything
downstream of them through traces; derived facts whose upstream died are
re-derived or garbage-collected with the same tombstone discipline entities have.

## Causality: goals carry their cause

An effect (the [typed handoff](./orchestration.md#typed-handoffs)) materializes as
a [goal](./ir-agents.md#the-goal) on the board, and the goal's cause record is the
whole ripple story:

```yaml
goal:g-1042:
  kind: derive-usecases
  target: cluster:customer/checkout
  cause:
    generation: 87              # the changeset that opened it
    mutation: 3                 # which staged mutation in that changeset
    via: traces/refines         # the edge or computation that carried dirtiness
  state: resolved               # open | resolved {generation} | parked | blocked
```

- Every committed changeset (a session, a dual write, a decree, GC) already
  appends a journal entry with a generation number; the entry gains
  `resolved_goals` (what this work closed) and `opened_goals` (what it caused).
  Human edits are generation-stamped the same way: a prose save that dirties
  sections journals an `edit` entry, so the root of every ripple is itself a
  generation.
- The ripple DAG of any change is then derivable, not stored: start at a
  generation, follow opened goals to the generations that resolved them, repeat.
  Backward: start at any node, its `updated` marker names generations, their
  resolved goals name causes, up to the human edit that started it.
- Goals are durable files beside the board state, journaled when resolved, so
  the DAG survives process restarts and is identical for every consumer, like
  the queue itself.

This is deliberately the same design as the queue: derived, durable, inspectable,
no private in-memory state. The model never opens a goal; it commits state, the
reconciler derives the goals, and the cause field records the derivation.

## Observing a run

Realtime, during compile:

- The live trace as today (`turnStart`, tool rows, model text, per-turn tokens),
  plus one new event kind: `goal` (opened or resolved, with cause). The
  `--verbose` stream shows the cascade as it happens.
- The GUI pipeline view ([orchestration](./orchestration.md#visibility)): stages
  as columns, work items as cards, effect arrows lighting up as they fire. A card
  click opens the live turn (the follow-session machinery exists).
- `jazyk watch` prints one line per goal at default verbosity: what opened, why,
  what session took it.

Post compile:

- `jazyk ripple <target|generation|doc>`: render the ripple DAG rooted at a
  change. For a target, the last cascade that touched it; for a generation, the
  full tree forward; with `--back`, causes instead of consequences.
- The build report in the GUI: the causality DAG for the whole build, per-stage
  cost beside it, parked leftovers and their blocking reasons.
- The journal remains the ground truth; `ripple` and the report are renderings
  over it. Journal diffs between builds stay the release-diff surface
  ([pm](../docs/consumers/pm.md#release-diffs-from-the-journal)).

Worked example, the trace the owner should be able to see after editing one
sentence (`orders.md`: "held orders expire after 21 days" becomes "30 days"):

```
edit g87 docs/orders.md /orders/holds (human)
└─ reconcile-doc docs/orders.md g88: req:orders-6 revised (quote, ears updated)
   ├─ review-requirement (req:orders-6 ~ req:payment-9) g89: consistent
   ├─ derive-statemachine ent:order g90: transition held→expired guard updated
   │  └─ checks: event completeness ok
   ├─ derive-usecases cluster:customer/holds g91: uc:order-expires step 2 requote
   │  └─ review-usecase uc:order-expires g92: consistent
   └─ bind-requirement req:orders-6: row stale (requirement-changed)
      └─ verify req:orders-6: fail (test asserts 21) → gen ent:order-expiry g93
         └─ verify req:orders-6: pass
converged: 6 turns, 2 stages touched, 41k tokens
```

Every line is a journal entry; every indent is a goal with its cause on record.

## Termination

Ripple must not mean runaway. The bounds, all existing machinery generalized:

- The cone: goals open only along trace edges and computed derivations, so a
  change reaches exactly the nodes with a justification path through it.
- Idempotence: a session that re-derives an unchanged conclusion stages a no-op
  upsert; no mutation, no new goal, the branch of the cascade dies there. This
  is what makes convergence a fixed point rather than a loop.
- Budgets: per-turn, per-build, per-stage; exhaustion parks with an
  `incomplete-build` diagnostic, resumed next build.
- Flip detection per node kind catches oscillation across stages
  ([ir-agents](./ir-agents.md#ordering-and-convergence)) and parks the pair for a
  human answer.
