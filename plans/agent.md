# Plan: the agent and the goal system

Status: draft for iteration. Detailed design under [ir-stages](./ir-stages.md).
Companions: [ir-graph](./ir-graph.md) (the shapes the agent edits),
[ripple](./ripple.md) (how goals chain), and
[orchestration](./orchestration.md) (the registry goal kinds live in).

There is one agent. What used to be agent types are goal kinds: data, not code
paths. The agent is given a list of goals and a slice of the graph loaded into its
context; it works the graph with tools, and the harness, never the model, decides
when a goal is truly done.

## One agent, many goals

- One generic agent contract: a single session prompt that says what an agent is
  here (resolve the listed goals using the tools, over the loaded graph, finish
  with `done`), identical for every session. Everything task-specific arrives as
  data: the goal list, each goal's contract paragraph, the loaded graph, and
  [skills](#skills).
- Compilation is a goal board. The reconciler derives goals from disk state
  exactly as it derives the dirty set today; a build is converged when no
  mandatory goal is open or failed and the checks pass. There is no separate task
  queue: the goal board is the queue.
- The executor stays pluggable per
  [ACP profile](./orchestration.md#per-stage-executors). This design assumes a
  capable model: navigating the graph, managing what is loaded, and working a
  goal board are agentic behaviors small local models fumble. That is accepted
  for now, and the harness holds regardless (gates bounce bad calls, junk never
  lands). Benchmarking executor profiles per goal kind is deferred until after
  the first implementation, once real use shows what needs grading.
- The model never creates, routes, or prioritizes goals. It resolves them, fails
  them, or parks them. Goal derivation, grouping, readiness, and verification
  are harness code. This is the existing division of labor, restated for goals.

## What a compile looks like

`jazyk compile` on a project with edits pending:

- Parse and diff the documents, derive the board. The terminal prints the
  summary first: `board: 47 goals (12 reconcile-section, 9 rejudge-pair,
  6 retrace, ...), 3 blocked, 5 optional`.
- The agent never sees the whole board. Sessions run one at a time (parallel
  compilation is shelved for now), and each session gets one batch: the
  scheduler takes the highest ready tier, groups open goals by locality (one
  document, one entity neighborhood, one view), and fills the batch until the
  context budget says stop. A batch is typically one to a handful of goals; the
  count is a consequence of the budget and the locality, never a fixed number.
- The session prompt lists exactly the batch's goals, the loaded graph for
  their locality, and one summary line for the rest ("41 goals elsewhere, not
  this session's"), so the model knows the world is bigger but bounded
  elsewhere.
- Each commit re-derives the board. Goals a mutation opened either join the
  running session (same locality, fits the budget) or wait for a later one. The
  live trace and the GUI board show the counts ticking down, each resolution
  landing with its justification.
- The loop ends when the board derives empty of mandatory goals and the checks
  pass. The verdict carries what remains (`converged, 2 blocked on answers,
  5 optional advised`), and [`jazyk ripple`](./ripple.md#observing-a-run)
  replays how any goal came to exist and what resolving it caused.

## The goal

```yaml
g:retrace:uc:order-expires:
  kind: retrace                    # from the catalog below
  mandatory: true                  # mandatory blocks convergence; optional advises
  target: uc:order-expires
  change: {deleted: req:orders-6, in: g88}   # the disk evidence; also the identity
  cause: {generation: 88, mutation: 2, via: traces/refines}   # see ripple.md
  state: open        # open | resolved {generation, justification}
                     # | failed {reason} | parked | blocked {on}
  hints:
    - load uc:order-expires
    - load view:usecase/customer (the use case appears there)
    - skill usecase-editing
```

- `mandatory` goals are correctness debts: something changed and the graph no
  longer agrees with itself or the docs. They block the `converged` verdict.
- Optional goals are pressure, not debt. The rule is graded: getting big opens an
  optional goal; too big escalates it to mandatory. Every
  [limit](./ir-graph.md#size-limits) carries the two thresholds. Restructuring
  therefore happens as things grow, inside the same convergence loop, never as a
  separate cleanup pass: a small edit that tips an entity over its hard limit
  makes the split part of that build.
- Dismissal is not goal state. Dismissing a size goal writes to the graph: the
  node's own limit is raised and recorded with decree provenance
  ([size limits](./ir-graph.md#size-limits)). The goal then simply stops
  deriving until the raised threshold is crossed in turn; nothing needs to
  remember the dismissal but the limit itself.
- `change` is the attached evidence: the section diff, the deleted node, the
  crossed threshold. It is what the agent works from, and it is the goal's
  identity: re-deriving the board matches a goal to its predecessor by the
  change, confidently.
- `cause` ties every goal to the committed change that spawned it, which is what
  makes the [ripple](./ripple.md) renderable.
- `hints` are computed, cheap, and honest: what to load, which skill explains the
  shape, which tool typically resolves the kind. Hints are suggestions; the gates
  are the truth.
- `blocked` goals wait on a human (an unanswered ADR, a ratification proposal, a
  gated release). They render in every status surface so a converged-but-waiting
  build says what it waits for.

Goals are derived, not stored. The board is recomputed from disk (documents,
graph, ledger, status) whenever it is consulted, so any process computes the
same board, an interrupted build resumes anywhere, and a no-op rebuild derives
zero goals and makes zero LLM calls. What is recorded is progress, never the
board: resolutions with their justifications and failures with their reasons go
to the journal, parked and failed goals persist in `status.yaml` so they survive
a restart, and dismissals are the graph writes above. The graph itself never
stores goals, so it cannot grow with them.

## Goals and diagnostics

One advisory system, not two. Goals are the actionable surface: anything jazyk
wants done, sized, split, re-judged, or repaired is a goal, and limit advisories
live only here. Diagnostics remain what they already are: recorded judgments
about content (a contradiction, an ambiguity, a conformance finding), sticky,
with prompts, answers, and human triage. The two reference each other: resolving
a `rejudge-pair` goal may file a `contradiction` diagnostic; a human answering
that diagnostic's prompt opens a goal to apply the answer. Nothing is recorded
twice.

## Goal lifecycle

- Derive: the reconciler computes goals on demand from section diffs, trace-edge
  dirtiness, limit thresholds, ledger state, and pending human answers.
  Deterministic, idempotent.
- Group: open goals batch into sessions by locality: goals sharing a document, an
  entity neighborhood, or a view are one session, bounded by the context budget.
  Readiness tiers order batches ([ordering](#ordering-and-convergence)).
- Resolve: the agent works the batch and calls
  `mark_goal_done({goal, justification, evidence?})` per goal. The
  `justification` is mandatory and concise, one or two sentences of why the goal
  is complete or what was done; the prompt demands brevity, the journal records
  it, and [`jazyk ripple`](./ripple.md#observing-a-run) shows it beside each
  step. The serving validates the claim against the goal kind's gate: a
  `reconcile-section` goal needs its coverage marks staged, a `rejudge-pair`
  goal needs a verdict per neighbor in `evidence`, a `retrace` goal needs the
  broken links repaired or the node deleted. A false claim is rejected with the
  gate named, like any invalid call.
- Fail: `mark_goal_failed({goal, reason})` is always available. A goal that
  cannot be accomplished (the documents contradict themselves too deeply, the
  target no longer makes sense, an instruction is wrong) must be failable, or
  the board fills with dishonestly resolved goals. A failed goal keeps its
  target, so the failure surfaces on the thing itself (the section, the entity,
  the view) everywhere that thing renders. A failed mandatory goal blocks
  convergence and demands a human look; a failed optional goal is recorded and
  stands.
- Bubble: staged mutations are validated the moment they are staged; the same
  computation previews the goals a mutation will spawn, and the tool reply says
  so: "this delete will open: retrace uc:order-expires (step 2), retrace
  view:class/orders (member gone)". At commit the previewed goals become real,
  with `cause` filled. The agent may finish its batch and let the next session
  take them, or, when they fit the budget, the serving appends them to the open
  session's board with their hints. Downstream work is never silent and never
  model-invented.
- Park: a goal the batch could not finish in budget parks with the reason,
  resumes first next build. Parked is "ran out of road"; failed is "this cannot
  be done as stated". The distinction is load-bearing for the report.

## Sessions

One session per goal batch, fresh context, retries clean (the existing turn
discipline). The session prompt is assembled, not authored:

- the generic agent contract (fixed, one payload file),
- the goal list with each kind's contract paragraph and hints,
- the initially loaded graph: the pack for the batch's locality, rendered by the
  loading machinery below,
- the active skills for what is loaded.

The toolset is the union of what the batch's goal kinds need, computed by the
harness. A batch of extraction goals still sees a small toolset; the union only
grows when the batch genuinely mixes families.

## Loading the graph into context

The context is an explicit, visible working set, not an accident of what was
prompted. The serving maintains the loaded set and renders its status into every
round, so the agent always knows three things: what is loaded, what could be
loaded next, and what it costs.

```
## Loaded (14.2k/24k chars)
- view:class/commerce   12 entities, 18 edges shown; 9 members unloaded  [h:view:commerce:members]
- ent:order             full: 7 requirements, parent ent:commerce        [3 more edges: h:ent:order:traces]
- ent:customer          stub (definition only)                           [5 edges loadable: h:ent:customer]
- docs/orders.md#/orders/holds   section body
Consider unloading: ent:customer (not referenced in 6 rounds, no open goal touches it)
```

Tools:

- `load({target, depth?})`: load a node and its immediate neighborhood under the
  policy below. Targets: any node id, a section reference, a view id.
- `expand({handle})`: the existing frontier mechanism, unchanged: every
  truncation is a handle with a size estimate.
- `unload({target})`: drop an item from the loaded set.
- `graph_status({})`: re-render the status block on demand (it also rides
  condensed on every mutating reply).
- `search`, `read_section`, `get_entity`, `diagnostics`: the existing reads,
  unchanged; a read's subject joins the loaded set as a stub.

Loading policy, deterministic and budget-driven:

- Loading A brings A in full, its edges, and each neighbor as a stub: name, one
  definition line, stereotype, and its own edge count ("ent:invoice, 7 edges
  loadable"). Neighbors' neighbors are counts only.
- The loaded set never exceeds the budget: when the next stub would cross it,
  the walk stops and emits handles instead, exactly the existing context engine
  rule. The "already 300 edges loaded" case is impossible by construction; the
  agent sees "9 members unloaded" and chooses.
- Unloading frees budget for the rest of the session: dropped items leave the
  status, their handles close, and subsequent replies stop rendering them. The
  serving suggests unload candidates (least recently referenced, not named by
  any open goal in the batch); past a high-water mark it warns that further
  `load` calls will be refused until something is unloaded.
- A `load` of something already loaded is a repeat, answered by the
  repeated-call guard as today.

## Skills

A skill is a prompt payload with the working knowledge for one shape: the use
case format and its invariants, how to read and edit a state machine node, what a
good abstraction split looks like, the profile's vocabulary. Skills live as
payload files beside the goal contracts (embedded at compile time, docs and
binary sharing bytes, as prompts do today).

- Auto-load: loading a node kind brings its skill once per session (load a
  `view:usecase/...` and the usecase-editing skill appears; load an `sm:` and
  the statechart skill does). Goal hints name skills the same way.
- Manual: `load_skill({name})` for the agent, and a skill index line in the
  status so it knows what exists.
- Skills render once and are listed as active in the status; unloading the last
  node of a kind marks its skill inactive (the text has been seen; the status
  stops advertising it). Active skills count against the context budget and are
  capped per session.
- Profiles contribute skills: the narrative profile's usecase skill speaks plot
  threads and scenes, the organization profile's speaks processes.

## The goal catalog

The complete set. `M` mandatory, `O` optional (escalates to `M` past its hard
threshold), `B` blocked-on-human. Stage numbers are the
[ladder](./ir-stages.md#the-stage-ladder); a goal kind exists only when its stage
is active.

| kind | m | stage | created when | resolved when (the gate) |
|---|---|---|---|---|
| `place-anchors` | M | 1 | alignment proposals pending for a document | every proposal decided |
| `reconcile-section` | M | 1 | a section is dirty or unprocessed | coverage mark staged or recorded; stale anchors addressed; extractions honest |
| `rejudge-pair` | M | 1 | a requirement was created or revised; sticky pairs | a verdict per neighbor in `evidence` (duplicate, contradiction, consistent) |
| `review-entity` | M | 1 | an entity's fact set changed | definition current; lookalikes judged; diagnostics filed or resolved |
| `declare-edges` | O | 1 | a multi-entity requirement has no `edges` | edges declared, or justification says the statement is not structural |
| `dedupe-candidates` | O | 1 | cross-document lookalikes score high | merged, or kept with reasoning |
| `derive-usecases` | M | 2 | a cluster's membership changed | every cluster requirement refined by a step or marked |
| `retrace` | M | 2-6 | any node's upstream trace died or changed (a step's requirement deleted, a message's operation gone, a view member gone, an instance's attribute gone) | broken links repaired, re-derived, or the node deleted; nothing dangling |
| `extend-usecase` | O | 2 | a failure-mode requirement is unrefined by any extension | extension added or `missing-error-requirement` diagnostic filed |
| `model-domain` | M | 3 | structural facts changed in a scope cluster | attributes, roles, cardinalities current; contradictions filed |
| `conform-instance` | M | 4 | an instance or the model under it changed | values and links conform, or the conformance diagnostic is filed |
| `partition` | M | 5 | composition on, no accepted partition ADR | partition proposed and recorded; ADR answerable afterward |
| `design-component` | M | 5 | allocation candidates, proposed operations, or answered ADRs pending | every candidate accepted or marked; operations satisfy or carry reasoning |
| `derive-statemachine` | M | 6 | a stateful entity's triggering requirements changed | transitions refine requirements; machine current |
| `derive-interaction` | M | 6 | a use case's steps, allocation, or interfaces changed | messages ride steps and name operations (or refine requirements, composition off) |
| `curate-view` | O | any | new nodes match a view's scope; a view has no members for its query | membership decided (added, or excluded with note) |
| `split-view` | O→M | any | a view crosses its soft limit (hard limit escalates) | sub-views created and linked, or members collapsed under parents |
| `abstract-entity` | O→M | any | an entity crosses its requirement or child soft limit (hard limit escalates) | sub-entities introduced with `parent`, detail moved, docs proposals staged |
| `ratify` | B | any | a derived or decree fact awaits its prose | human accepts the docsgen proposal (dual write) or retracts the fact |
| `bind` | M | 7 | a requirement owes a binding | ledger row recorded (existing gate) |
| `generate` | M | 7 | an entity or component's facts differ from the ledger | `record_generation` landed (existing gate) |
| `verify` | M | 7 | a row's derived status says action | verdict recorded (existing gate) |
| `answer` | B | any | a diagnostic or ADR carries an unanswered prompt | the human answers; the answer's application is a new goal with the answer as cause |

Notes on the load-bearing rows:

- `retrace` is one kind, not five. Delete a requirement and the entity, the use
  case, and the class view that referenced it each surface as a `retrace` goal
  with the same cause, each hinting what to load to see the damage. The gate is
  uniform: nothing may keep pointing at the dead node.
- `abstract-entity` and `split-view` are where
  [containment](./ir-graph.md#containment-and-lifting) is exercised: introduce a
  parent, distribute children, let lifting keep coarse views true. Their skill
  carries the judgment guidance (split by cohesion of requirements, respect
  scopes, never invent concepts the docs cannot support, propose docs sentences
  for the new structure). Restructuring is not gated: it runs as it becomes due,
  in whatever build tipped the threshold.
- Judgment gates are honest about what they check: a `rejudge-pair` gate
  verifies completeness (a verdict per pair, with reasoning), not correctness.
  The harness cannot know a "consistent" verdict is true; verdict quality is a
  benchmarking concern, deferred until after the first implementation.
- `ratify` and `answer` are the human seams. They keep the convergence report
  honest: a build with open blocked goals is "converged, awaiting 2 answers",
  never silently done.

Each kind has: a contract paragraph (the prompt payload, one file per kind, same
embed discipline as today's prompts), a gate implementation, and a hint
computer. Benchmark cases per kind come after the first implementation, once
real use shows what needs grading.

## What the model sees

Every session prompt is assembled deterministically, so it can be shown before
it is spent. `jazyk preview` renders the next session's prompt exactly as the
model would receive it (`jazyk preview <goal|target>` for the batch that goal
would join), and the GUI shows the same pane before a release in manual mode.
Reviewing what the model will see is a first-class surface, not a debug flag,
and the transcript records the same rendering per round, so post-hoc review
sees what the model saw, verbatim.

```
[agent contract, fixed]

[skill: requirement-extraction (active)]

## Project
- build 88, generation 412, manual mode
- diagnostics: 1 error (contradiction diag:contradiction-3), 4 warnings
- board: 2 goals in this session; 41 elsewhere; 3 blocked on human answers

## Goals
- [g:reconcile-section:docs/orders.md#/orders/holds] mandatory
  The section changed (diff in the loaded body). Bring the graph in line:
  extract, update, cover. Gate: coverage marks staged, stale anchors addressed.
- [g:retrace:uc:order-expires] mandatory
  Step 2 refines req:orders-6, deleted in g88 (reason: duplicate). Repair,
  re-derive, or delete. Gate: nothing dangling. Hint: load uc:order-expires.

## Loaded (9.8k/24k chars)
- docs/orders.md#/orders/holds   section body, with the diff marked
- ent:order    full: 7 requirements, parent ent:commerce   [3 more edges: h:ent:order:traces]
- uc:order-expires   stub   [loadable: h:uc:order-expires]
```

The contract paragraphs are short and imperative, in the style of the existing
task instructions: what the goal means, what evidence the gate wants, what not
to do (the review asymmetry, the honesty rules), and that both `mark_goal_done`
justifications and `mark_goal_failed` reasons are one or two sentences, never
essays. The feedback contract rides once, high, as today.

## Ordering and convergence

- Sessions are sequential: one compilation at a time, one session at a time
  within it (the build lease already enforces the former). Parallel sessions
  are shelved as a later optimization; nothing in the goal design depends on
  them.
- Readiness tiers replace waves: a goal is ready when the goals it depends on
  are closed in its cone. The ladder gives the tiers (alignment before ingest
  before judgment before use cases before domain before instances before
  composition before dynamics before ledger goals), and the existing document
  levels order stage-1 batches.
- Convergence: no open or failed mandatory goals, checks clean. The verdict
  carries the counts: `converged`, or
  `incomplete: 3 open, 1 failed, 2 blocked, 5 optional advised`.
- Budgets: per session (rounds, mutations, context), per build (goal
  resolutions), earlier tiers first when tight. Parked goals resume first.
- Oscillation: two goals resolving each other back and forth (a proposed
  operation bouncing between dynamics and composition) is caught by flip
  detection on the goal target's natural key; the pair parks as one
  `unstable-derivation` diagnostic with both justifications side by side,
  blocked on a human.

## What stays true

- The model owns judgment inside a goal; the harness owns everything around it:
  derivation, grouping, readiness, gates, budgets, causes.
- Success is read from the store and the board, never from the agent's word, and
  every resolution carries its recorded why.
- One agent contract, goal kinds as data, skills as payloads: adding a stage is
  goal kinds plus gates plus skills in the registry, no new agent.
