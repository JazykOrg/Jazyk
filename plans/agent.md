# Plan: the agent and the goal system

Status: proposal for iteration. Read with [ir-stages](./ir-stages.md) (doctrine,
compile and GC), [ir-graph](./ir-graph.md) (the graph and every diagram),
[ripple](./ripple.md) (propagation and observing), [orchestration](./orchestration.md)
(implementation notes).

## The agent

One agent. Task variety lives in goal kinds, which are data: a contract
paragraph, a resolution gate, hints, skills. The agent's session prompt is one
fixed contract (resolve the listed goals with the tools, over the loaded graph,
finish with `done`); everything task-specific arrives as data. The executor is
pluggable per ACP profile (the embedded agent, Claude Code, OpenCode), with
overrides per goal kind or per goal class, so extraction can run cheap while
GC judgment runs on the strongest model available.

The model never creates, routes, or prioritizes goals. It resolves them, fails
them, or parks them. Derivation, grouping, readiness, gates, budgets, and
causality are harness code.

## Goals

Compilation is a goal board. The reconciler derives goals from disk state the
way it derives the dirty set; a build is converged when both goal classes
([compile and GC](./ir-stages.md#compile-and-garbage-collection)) derive an
empty board of mandatory goals and the checks pass.

```yaml
g:retrace:view:usecase/holds:
  kind: retrace                    # from the catalog below
  mandatory: true                  # mandatory blocks convergence; optional advises
  target: view:usecase/holds
  change: {deleted: req:orders-6, in: g409}   # the disk evidence; also the identity
  cause: {generation: 409, mutation: 2, via: view-member}
  state: open        # open | resolved {generation, justification}
                     # | failed {reason} | parked | blocked {on}
  hints:
    - load view:usecase/holds
    - skill flow-views
```

- Correctness debts (the graph no longer agrees with itself or the docs) are
  mandatory goals: compile work.
- Restructuring pressure is GC work (decoupling, splitting, combining). A GC
  goal becomes ready only when no compile goal is open in its target's cone,
  so restructuring always sees settled content, and the build interleaves the
  two classes in bursts rather than phases. Soft thresholds open optional goals, hard thresholds
  escalate them to mandatory (the two thresholds on every
  [limit](./ir-graph.md#size-limits)); mandatory blocks convergence in either
  class.
- Dismissal is not goal state. Dismissing a size goal writes to the graph: the
  node's own limit is raised, recorded with decree provenance, and the goal
  stops deriving until the raised threshold is crossed in turn.
- `change` is the attached evidence (the section diff, the deleted node, the
  crossed threshold) and the goal's identity: re-deriving the board matches a
  goal to its predecessor by the change.
- `cause` names the committed change that spawned the goal: the generation, the
  mutation within it, and the edge or computation that carried the dirtiness.
- `hints` are computed and honest: what to load, which skill explains the
  shape, which tool typically resolves the kind. Suggestions; the gates are
  the truth.
- `blocked` goals wait on a human (an unanswered decision prompt, a
  ratification proposal, a gated release) and render on every status surface.

Goals are derived, not stored. The board recomputes from disk whenever
consulted, so any process computes the same board, an interrupted build
resumes anywhere, and a no-op rebuild derives zero goals. The inputs are the
documents, the graph, the ledger, and the durable change records: every commit
writes the typed dirtiness it caused (which sections changed, which
requirements were created, revised, or deleted, which thresholds crossed) into
`status.yaml` beside the parked work, and `derive_goals` reads a goal's
`change` from exactly that record. Current graph state alone cannot say
"revised since last judged"; the change records can, and resolving a goal
clears its record. What else is recorded is progress: resolutions with
justifications and failures with reasons in the journal, parked and failed
goals in `status.yaml`, dismissals as the limit bumps above. The graph never
stores goals, so it cannot grow with them.

## A compile, end to end

`jazyk compile` on a project with edits pending:

- Parse and diff the documents, derive the board. The terminal prints the
  summary first: `compile: 27 goals (12 reconcile-section, 9 rejudge-pair,
  6 retrace), 3 blocked`.
- The agent never sees the whole board. Sessions run one at a time, and each
  gets one batch: the scheduler takes the highest ready tier, groups open goals
  by locality (one document, one entity neighborhood, one view), and fills the
  batch until the context budget says stop. A batch is one to a handful of
  goals; the count is a consequence of budget and locality, never a fixed
  number.
- The session prompt lists exactly the batch's goals, the loaded graph for
  their locality, and one summary line for the rest ("21 goals elsewhere, not
  this session's").
- Each commit re-derives the board and recomputes derived data (relationships,
  state machines, default views). Goals a mutation opened either join the
  running session (same locality, fits the budget) or wait for a later one.
  The live trace and the GUI board show counts ticking down, each resolution
  landing with its justification.
- As each locality's compile goals settle, its GC goals become ready and run
  in a burst, often in the session that just finished the locality (the graph
  is already loaded, the thinking is warm): `gc burst: abstract-entity
  ent:order (54 > 50)`. A GC session sees the locality's final counts, so an
  entity that doubled its requirements this build is abstracted once,
  holistically. GC commits can reopen compile goals (a split entity
  re-enqueues its reviews); the loop runs compile for that cone and returns,
  bounded by flip detection and budgets.
- The verdict carries what remains (`converged, 2 blocked on answers,
  1 optional advised`), and [`jazyk ripple`](./ripple.md#observing-a-run)
  replays how any goal came to exist.

Compilation is sequential: one build at a time (the build lease enforces it),
one session at a time within it. Parallel sessions are a later optimization;
nothing in the design depends on them.

## Sessions

One session per goal batch, fresh context, retries clean. The prompt is
assembled, never authored per task: the fixed contract, the goal list with each
kind's contract paragraph and hints, the initially loaded graph for the batch's
locality, and the active skills. The toolset is the union of what the batch's
goal kinds need, computed by the harness, so a batch of extraction goals still
sees a small toolset.

Session mechanics carry over from the current turn design: staged mutations
validated as staged, reads showing the session's snapshot with a note when
staged work shadows them, the repeated-call guard, budgets on rounds and
mutations, retry once then park, and the finish contract (`done` runs the batch
gates; a session that ends with valid staged work commits it).

## Loading the graph

The context is an explicit working set, not an accident of what was prompted.
The serving maintains the loaded set and renders its status into every round,
so the agent always knows what is loaded, what could be loaded next, and what
it costs.

```
## Loaded (14.2k/24k chars)
- view:class/commerce   12 entities, 18 edges shown; 9 members unloaded  [h:view:class/commerce:members]
- ent:order             full: 7 requirements, parent ent:order-service   [3 more edges: h:ent:order:related]
- ent:customer          stub (definition only)                           [5 edges loadable: h:ent:customer]
- docs/orders.md#/orders/holds   section body
Consider unloading: ent:customer (not referenced in 6 rounds, no open goal touches it)
```

Tools:

- `load({target, depth?})`: load a node and its immediate neighborhood. Targets
  are any node id, section reference, or view id.
- `expand({handle})`: load the frontier behind a handle; every truncation emits
  a handle with a size estimate.
- `unload({target})`: drop an item from the loaded set.
- `graph_status({})`: re-render the status block on demand (a condensed form
  rides on every mutating reply).
- `search`, `read_section`, `get_entity`, `diagnostics`: reads; a read's
  subject joins the loaded set as a stub.

The policy is deterministic and budget-driven. Loading A brings A in full, its
edges, and each neighbor as a stub (name, one definition line, stereotype, its
own edge count); neighbors' neighbors are counts only. The walk stops at the
budget and emits handles, so overload is impossible by construction: the agent
sees "9 members unloaded" and chooses. Unloading frees budget for the rest of
the session: unloaded items leave the status, their handles close, and later
replies stop rendering them. The serving suggests unload candidates (least
recently referenced, not named by any open goal) and, past a high-water mark,
refuses further `load` calls until something is unloaded. Loading an
already-loaded target is a repeat, answered by the repeated-call guard.

## Skills

A skill is a prompt payload with the working knowledge for one shape: how flow
views order their members, what a good abstraction split looks like. Skills are payload files embedded at compile time, like
the goal contracts.

- Auto-load: loading a node kind brings its skill once per session (load a
  `view:usecase/...` and the flow-views skill appears).
- Manual: `load_skill({name})`, with a skill index line in the status.
- Skills render once per session, count against the context budget, and are
  capped. Unloading the last node of a kind marks the skill inactive: the text
  already in context stands, the status just stops advertising it.
- Skill text is medium-neutral; the model adapts its wording to the medium it
  is reading, as it does everywhere else.

## The goal catalog

`M` mandatory; `O` optional; `O→M` optional, escalating to mandatory past its
hard threshold; `B` blocked-on-human. A kind derives only when its input
exists in the graph
([what the content activates](./ir-stages.md#what-the-content-activates)).

Compile goals:

| kind | m | created when | resolved when (the gate) |
|---|---|---|---|
| `place-anchors` | M | alignment proposals pending for a document | every proposal decided |
| `reconcile-section` | M | a section is dirty or unprocessed | coverage mark staged or recorded; stale anchors addressed; extractions honest (statements, edges, transition facets, attributes) |
| `rejudge-pair` | M | a requirement was created or revised; sticky pairs | a verdict per neighbor in `evidence` (duplicate, contradiction, consistent) |
| `review-entity` | M | an entity's fact set changed | definition current; lookalikes judged; diagnostics filed or resolved |
| `retrace` | M | a node's upstream died or changed (a view member deleted, an instance's type attribute gone) | repaired, re-derived, or deleted; nothing dangling |
| `conform-instance` | M | an instance or the model under it changed | values and links conform, or the finding is filed |
| `bind` | M | a requirement owes a binding, or its binding went stale (requirement-changed, artifact-gone) | ledger row recorded |
| `generate` | M | an entity's facts differ from the ledger | `record_generation` landed |
| `verify` | M | a row's derived status says action | verdict recorded |
| `ratify` | B | a derived or decree fact awaits its prose | human accepts the docsgen proposal (dual write) or retracts the fact |
| `answer` | B | a diagnostic carries an unanswered prompt | the human answers; applying the answer is a new goal with the answer as cause |

GC goals:

| kind | m | created when | resolved when (the gate) |
|---|---|---|---|
| `declare-edges` | O | a multi-entity requirement has no `edges` | edges declared, or justification says the statement is not structural |
| `dedupe-candidates` | O | cross-document lookalikes score high | merged, or kept with reasoning |
| `curate-view` | O | new nodes match a view's query; a flow view's coverage check flags an unplaced behavior requirement | membership decided (added, or excluded with note on the view) |
| `split-view` | O→M | a view crosses its member or edge limit | sub-views created and linked, or members collapsed under parents |
| `abstract-entity` | O→M | an entity crosses its requirement or child limit | sub-entities introduced with `parent`, detail moved, docs proposals staged |

Notes on the load-bearing rows:

- `retrace` is one kind. Delete a requirement and the entity that carried it,
  the flow view that stepped through it, and the instance that conformed to it
  each surface as a `retrace` goal with the same cause, each hinting what to
  load to see the damage. The gate is uniform: nothing may keep pointing at
  the dead node. Derived data needs no retrace: relationships, state machines,
  and default views recompute at commit.
- `abstract-entity` and `split-view` are where containment is exercised:
  introduce a parent, distribute children, let lifting keep coarse views true.
  Their skill carries the judgment guidance: split by cohesion of
  requirements, respect scopes, never invent concepts the docs cannot support,
  propose docs sentences for the new structure. Proposing component structure
  where none exists is the same move at the top of the tree, with a
  `decision` prompt for the human.
- Judgment gates verify completeness, not correctness: a `rejudge-pair` gate
  checks that a verdict with reasoning exists per pair; it cannot know a
  "consistent" verdict is true. Verdict quality is a benchmarking concern,
  taken up after the first implementation.
- `ratify` and `answer` are the human seams. They keep the report honest: a
  build with open blocked goals is "converged, awaiting 2 answers", never
  silently done.

Each kind ships a contract paragraph (a payload file, embedded like all
prompts), a gate implementation, and a hint computer.

## Resolving, failing, bubbling

- `mark_goal_done({goal, justification, evidence?})`: the justification is
  mandatory and concise, one or two sentences of why the goal is complete; the
  prompt demands brevity, the journal records it, and `jazyk ripple` shows it
  beside each step. The serving validates the claim against the kind's gate and
  rejects a false one with the gate named.
- `mark_goal_failed({goal, reason})`: always available. A goal that cannot be
  accomplished (documents too deeply contradictory, a target that no longer
  makes sense) must be failable, or the board fills with dishonestly resolved
  goals. A failed goal keeps its target, so the failure surfaces on the thing
  itself everywhere it renders. A failed mandatory goal blocks convergence; a
  failed optional goal is recorded and stands. Parked is different: "ran out of
  budget", resumed next build.
- Bubbling: staged mutations are validated when staged, and the same
  computation previews the goals a mutation will open; the tool reply says so
  ("this delete will open: retrace view:usecase/holds (member gone), retrace
  ent:order (statement gone)"). At commit the previews become real goals with
  causes. They join the running session when they fit its locality and budget;
  otherwise they wait. Downstream work is never silent and never
  model-invented.

## What the model sees

Every session prompt is assembled deterministically, so it can be shown before
it is spent. `jazyk preview` renders the next session's prompt exactly as the
model would receive it (`jazyk preview <goal|target>` for the batch that goal
would join), and the GUI shows the same pane before a release in manual mode.
The transcript records the same rendering per round, so post-hoc review sees
what the model saw, verbatim.

```
[agent contract, fixed]

[skill: extraction (active)]

## Project
- build 12, generation 412, manual mode
- diagnostics: 1 error (contradiction diag:contradiction-3), 4 warnings
- board: 2 goals in this session; 21 elsewhere; 3 blocked on human answers

## Goals
- [g:reconcile-section:docs/orders.md#/orders/holds] mandatory
  The section changed (diff in the loaded body). Bring the graph in line:
  extract, update, cover. Gate: coverage marks staged, stale anchors addressed.
- [g:retrace:view:usecase/holds] mandatory
  Member req:orders-6 was deleted in g409 (reason: duplicate). Repair the flow,
  or drop the member. Gate: nothing dangling. Hint: load view:usecase/holds.

## Loaded (9.8k/24k chars)
- docs/orders.md#/orders/holds   section body, with the diff marked
- ent:order    full: 7 requirements, parent ent:order-service   [3 more edges: h:ent:order:related]
- view:usecase/holds   stub   [loadable: h:view:usecase/holds]
```

Contract paragraphs are short and imperative: what the goal means, what
evidence the gate wants, what not to do (the review asymmetry: a wrong delete
destroys information, a missed duplicate only leaves a finding; when in doubt
keep both and file), and that justifications and failure reasons are one or two
sentences, never essays. The feedback contract rides once, high: confusing
instructions and tools go to `report_feedback`, and the session continues on
best judgment.

## Ordering and convergence

- Within compile, readiness tiers order the work: alignment before ingest,
  ingest before judgment, judgment before ledger goals; document link levels
  order ingest batches, roots first. A GC goal is ready when its target's
  cone is quiet; the two classes interleave in bursts.
- Convergence: both classes derive empty of open or failed mandatory goals,
  checks clean. The verdict carries the counts: `converged`, or
  `incomplete: 3 open, 1 failed, 2 blocked, 5 optional advised`.
- Budgets: per session (rounds, mutations, context) and per build (goal
  resolutions), compile outranking GC when tight. Parked goals resume first
  next build.
- Oscillation between the classes (GC splits, compile review merges back)
  is caught by flip detection on the target's natural key; the pair parks as
  one `unstable-derivation` diagnostic with both justifications side by side,
  blocked on a human.
