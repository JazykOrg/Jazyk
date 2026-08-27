# Plan: ripple, convergence, and observing it

Status: proposal for iteration. Read with [ir-stages](./ir-stages.md) (doctrine,
the two build stages), [ir-graph](./ir-graph.md) (the graph and every diagram),
[agent](./agent.md) (goals and sessions), [orchestration](./orchestration.md)
(implementation notes).

The target is a stable system: docs, graph, diagrams, deliverable, and tests as
one fixed point. A human edits any surface; compile detects the change, the
agent converges the rest, and the whole run is explainable afterward.

## Edit paths

Every human edit enters as one of four paths, and all four end the same way:
goals derived, sessions scheduled, fixed point restored.

- Edit prose. Parse, align, dirty sections, reconcile, cascade.
- Edit a quote-provenanced fact through a diagram or the GUI inspector. The
  fact's provenance names the sentence, so the edit is a dual write: the model
  proposes the sentence rewrite, the human accepts it, and the prose
  replacement commits with the graph mutation in one changeset (changing a
  class-diagram arrow's cardinality rewrites the sentence behind the
  requirement that declared it). The commit absorbs the new section hashes, so
  the edit does not dirty the document it just changed; downstream goals
  derive from the graph change. When no proposed rewrite is accepted, the edit
  falls through to a decree plus a ratification proposal: the compiler never
  rewrites a source document without an accepted proposal.
- Edit a derived fact, or add a fact with no prose behind it: a decree. The
  changeset lands graph-only with `decree` provenance. Downstream goals derive
  normally; upward, the decree queues a ratification proposal (docsgen renders
  the sentence the docs should gain, as a diagnostic prompt with a suggested
  edit). Accepting writes the prose and flips the provenance to `quote`;
  rejecting retracts the decree. The compiler never overwrites a decree; it
  files diagnostics against it.
- Edit the deliverable or tests. Ledger statuses flip (`code-changed`,
  `test-changed`), verification reruns, and the unclaimed report feeds
  decompile drafts.

Deletion runs the same paths in reverse: dead prose kills quotes, which kills
quote-provenanced facts (garbage collection with tombstone redirects), which
opens `retrace` goals on the views and instances that referenced them; derived
data (relationships, state machines, default views) simply recomputes.

## Ambiguity

Anything the deliverable needs that the documents do not state is an ambiguity.
Generation does not stall on one: it chooses with best judgment, records the
choice, and raises it, graded by the scope of what had to be invented. "Build
me a Facebook" is an error (the invention is the whole product); an unspecified
out-of-memory behavior is a warning; an unspecified background color is info a
human may suppress. Ratification is how the debt is repaid: every derived and
decreed fact carries a proposal for the sentence the docs should gain, and the
graph converges toward fully quoted.

The docs absorb the detail by dividing, not bloating: a document states the
high level, sub-documents carry the detail, every one readable on its own.
`doc-too-large` and `section-too-large` tell the human where to split, incoming
links keep the parts bound to the whole, and ratification proposals can target
a new sub-document rather than cramming a parent.

Measuring the grade has a promising instrument: the deliverable itself.
Generated mass attached to no requirement is exactly the invented detail, and
the ledger with the unclaimed report already computes attachment. "App like
Facebook", three words, shows up as an enormous unattached remainder; docs
written near pseudo-code leave almost none. Measured at generation time, the
unattached remainder grades the ambiguity, and a later pass can bubble those
emerged details up into the IR and the docs.

## Causality

Every [goal](./agent.md#goals) carries its cause, and that record is the whole
ripple story. Every committed changeset (a session, a dual write, a decree,
garbage collection) appends a journal entry with a generation number; the entry
records `resolved_goals` (each with its one-line justification) and
`opened_goals`. Human edits are generation-stamped the same way: a prose save
that dirties sections journals an `edit` entry, so the root of every ripple is
itself a generation.

The ripple DAG is derivable, never stored: start at a generation, follow opened
goals to the generations that resolved them, repeat; or walk backward from any
node through its `updated` markers to the human edit that started everything.
The journal is the ground truth; the DAG is a rendering over it.

## Observing a run

Realtime:

- The live trace: session lifecycle events, tool rows with condensed arguments,
  model text, per-session token counts, and `goal` events (opened or resolved,
  with cause). `--verbose` shows the cascade as it happens.
- The GUI board: compile and cleanup as columns, goals as cards (open, blocked
  with reason, parked, failed), arrows lighting up as causes fire. A card click
  opens the live session (the follow-session machinery).
- `jazyk watch` prints one line per goal: what opened, why, what session took
  it.

Post compile:

- `jazyk ripple <target|generation|doc>`: the ripple DAG rooted at a change.
  For a target, the last cascade that touched it; for a generation, the full
  tree forward; `--back` shows causes instead of consequences.
- The build report: the causality DAG for the whole build, cost beside it,
  parked and failed goals with reasons.
- Journal diffs between builds remain the release-diff surface for project
  management.

The trace a one-sentence edit leaves (`orders.md`: "held orders expire after
21 days" becomes "30 days"):

```
edit g87 docs/orders.md /orders/holds (human)
└─ reconcile-section docs/orders.md g88: req:orders-6 revised (quote, statement, transition guard)
   │  recomputed at commit: sm:order (held→expired guard), view:sequence/holds
   ├─ rejudge-pair (req:orders-6 ~ req:payment-9) g89: consistent
   └─ bind req:orders-6: row stale (requirement-changed)
      └─ verify req:orders-6: fail (test asserts 21) → generate ent:order g90
         └─ verify req:orders-6: pass
cleanup: no goals derived
converged: 3 sessions, 2 recomputes, 29k tokens
```

Every line is a journal entry; every indent is a goal with its cause and
justification on record. The recompute line is the payoff of derived data: the
state machine and the sequence view followed the requirement without a session.

## Termination

Ripple must not mean runaway:

- The cone: goals open only along stored references and computed derivations,
  so a change reaches exactly the nodes with a justification path through it.
- Idempotence: a session that re-derives an unchanged conclusion stages a no-op
  upsert; no mutation, no new goal, and that branch of the cascade dies. This
  is what makes convergence a fixed point rather than a loop.
- Budgets: per session and per build, compile outranking cleanup when tight.
  Exhaustion parks with an `incomplete-build` diagnostic, resumed next build.
  Unfinished work is never silent.
- Flip detection catches oscillation, including between cleanup and compile,
  and parks it for a human.
