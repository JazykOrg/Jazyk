# Plan: the generic agent and the goal system

Status: draft for iteration. Detailed design under [ir-stages](./ir-stages.md).
Companions: [ir-graph](./ir-graph.md) (the shapes the agent edits),
[ripple](./ripple.md) (how goals chain), and
[orchestration](./orchestration.md) (the registry goal kinds live in).

This file supersedes the earlier typed-agent roster. There is one agent. What used
to be agent types are now goal kinds: data, not code paths. The agent is given an
area of focus and a list of goals; it works the graph with tools, and the harness,
never the model, decides when a goal is truly done.

## One agent, many goals

- One generic agent contract: a single session prompt that says what an agent is
  here (resolve the listed goals using the tools, over the focus provided, finish
  with `done`), identical for every session. Everything task-specific arrives as
  data: the goal list, each goal's contract paragraph, the initial focus, and
  [skills](#skills).
- Compilation is a goal board. The reconciler derives goals from disk state
  exactly as it derives the dirty set today; a build is converged when no
  mandatory goal is open and the checks pass. There is no separate task queue:
  the goal board is the queue.
- The executor stays pluggable per
  [ACP profile](./orchestration.md#per-stage-executors): the embedded agent,
  Claude Code, OpenCode. Profiles can differ per goal family (extraction goals on
  a local model, composition goals on a strong one).
- The model never creates, routes, or prioritizes goals. It resolves them. Goal
  derivation, grouping, readiness, and verification are harness code. This is the
  existing division of labor, restated for goals.

## The goal

```yaml
goal:g-1042:
  kind: retrace                    # from the catalog below
  mandatory: true                  # mandatory blocks convergence; optional advises
  target: uc:order-expires
  detail: step 2 refines req:orders-6, which was deleted in g88
  cause: {generation: 88, mutation: 2, via: traces/refines}   # see ripple.md
  state: open                      # open | resolved {generation} | parked | blocked {on}
  hints:
    - focus uc:order-expires
    - focus view:usecase/customer (the use case appears there)
    - skill usecase-editing
```

- `mandatory` goals are correctness debts: something changed and the graph no
  longer agrees with itself or the docs. They block the `converged` verdict.
- Optional goals are quality pressure: a limit exceeded, an abstraction advised, a
  duplicate suspected. They never block convergence; unresolved ones surface as
  warnings, exactly the standing diagnostics do today. An optional goal ignored
  long enough does not escalate; it stays visible.
- `cause` ties every goal to the committed change that spawned it, which is what
  makes the [ripple](./ripple.md) renderable.
- `hints` are computed, cheap, and honest: which focus loads the needed context,
  which skill explains the shape, which tool typically resolves the kind. Hints
  are suggestions; the gates are the truth.
- `blocked` goals wait on a human (an unanswered ADR, a ratification proposal, a
  gated release). They render in every status surface so a converged-but-waiting
  build says what it waits for.

Goals are durable files derived from disk, like the queue today: any process
recomputes the same board, an interrupted build resumes anywhere, a no-op rebuild
derives zero goals and makes zero LLM calls.

## Goal lifecycle

- Derive: the reconciler computes goals from section diffs, trace-edge dirtiness,
  limit checks, ledger state, and pending human answers. Deterministic,
  idempotent.
- Group: open goals batch into sessions by locality: goals sharing a document, an
  entity neighborhood, or a view are one session, bounded by the context budget.
  What used to be "one turn per document" is now the natural batch of that
  document's goals. Readiness tiers order batches
  ([ordering](#ordering-and-convergence)).
- Resolve: the agent works the batch and calls
  `mark_goal_done({goal, evidence?})` per goal. The serving validates against the
  goal kind's gate: a `reconcile-section` goal needs its coverage marks staged, a
  `rejudge-pair` goal needs a verdict per neighbor in `evidence`, a `retrace`
  goal needs the broken links repaired or the node deleted. A false claim is
  rejected with the gate named, like any invalid call.
- Bubble: staged mutations are validated the moment they are staged; the same
  computation previews the goals a mutation will spawn, and the tool reply says
  so: "this delete will open: retrace uc:order-expires (step 2), retrace
  view:class/orders (member gone)". At commit the previewed goals become real,
  with `cause` filled. The agent may finish its batch and let the next session
  take them, or, when they fit the budget, the serving appends them to the open
  session's board with their hints. Downstream work is never silent and never
  model-invented.
- Park: a goal the batch could not finish parks with the reason, resumes first
  next build. Budgets bound sessions and builds as today.

## Sessions

One session per goal batch, fresh context, retries clean (the existing turn
discipline). The session prompt is assembled, not authored:

- the generic agent contract (fixed, one payload file),
- the goal list with each kind's contract paragraph and hints,
- the initial focus: the pack for the batch's locality, rendered by the focus
  system below,
- the active skills for what is in focus.

The toolset is the union of what the batch's goal kinds need, computed by the
harness. Weak-model discipline survives: a batch of extraction goals still sees a
small toolset; the union only grows when the batch genuinely mixes families.

## The focus system

The context is an explicit, visible working set, not an accident of what was
prompted. The serving maintains the focus set and renders its status into every
round, so the agent always knows three things: what is loaded, what could be
loaded next, and what it costs.

```
## Focus (14.2k/24k chars)
- view:class/commerce   12 entities, 18 edges shown; 9 members unloaded  [h:view:commerce:members]
- ent:order             full: 7 requirements, parent ent:commerce        [3 more edges: h:ent:order:traces]
- ent:customer          stub (definition only)                           [5 edges loadable: h:ent:customer]
- docs/orders.md#/orders/holds   section body
Consider unfocus: ent:customer (not referenced in 6 rounds, no open goal touches it)

## Goals
- [g-1041 mandatory] reconcile-section docs/orders.md#/orders/holds      open
- [g-1042 mandatory] retrace uc:order-expires (step 2, deleted req)      open   hint: focus uc:order-expires
- [g-1043 optional]  abstract-entity ent:order (54 requirements > 50)    open   skill: abstraction
```

Tools:

- `focus({target, depth?})`: load a node and its immediate neighborhood under the
  loading policy below. Targets: any node id, a section reference, a view id.
- `expand({handle})`: the existing frontier mechanism, unchanged: every truncation
  is a handle with a size estimate.
- `unfocus({target})`: drop an item from the focus set.
- `graph_status({})`: re-render the status block on demand (it also rides
  condensed on every mutating reply).
- `search`, `read_section`, `get_entity`, `diagnostics`: the existing reads,
  unchanged; a read's subject joins the focus as a stub.

Loading policy, deterministic and budget-driven:

- Focusing A loads A in full, its edges, and each neighbor as a stub: name, one
  definition line, stereotype, and its own edge count ("ent:invoice, 7 edges
  loadable"). Neighbors' neighbors are counts only.
- The pack never exceeds the budget: when the next stub would cross it, the walk
  stops and emits handles instead, exactly the existing context engine rule. The
  "already 300 edges loaded" case is therefore impossible by construction; the
  agent sees "9 members unloaded" and chooses.
- Unfocus frees budget for the rest of the session: dropped items leave the
  status, their handles close, and subsequent packs and replies stop rendering
  them. The serving suggests unfocus candidates (least recently referenced, not
  named by any open goal in the batch); past a high-water mark it warns that
  further `focus` calls will be refused until something is unloaded.
- A `focus` on something already loaded is a repeat, answered by the
  repeated-call guard as today.

## Skills

A skill is a prompt payload with the working knowledge for one shape: the use case
format and its invariants, how to read and edit a state machine node, what a good
abstraction split looks like, the profile's vocabulary. Skills live as payload
files beside the goal contracts (embedded at compile time, docs and binary sharing
bytes, as prompts do today).

- Auto-load: focusing a node kind loads its skill once per session (focus a
  `view:usecase/...` and the usecase-editing skill appears; focus an `sm:` and the
  statechart skill does). Goal hints name skills the same way.
- Manual: `load_skill({name})` for the agent, and a skill index line in the status
  so it knows what exists.
- Skills render once and are listed as active in the status; unfocusing the last
  node of a kind marks its skill inactive (the text has been seen; the status
  stops advertising it).
- Profiles contribute skills: the narrative profile's usecase skill speaks plot
  threads and scenes, the organization profile's speaks processes.

## The goal catalog

The complete set. `M` mandatory, `O` optional, `B` blocked-on-human. Stage numbers
are the [ladder](./ir-stages.md#the-stage-ladder); a goal kind exists only when its
stage is active.

| kind | m | stage | created when | resolved when (the gate) |
|---|---|---|---|---|
| `place-anchors` | M | 1 | alignment proposals pending for a document | every proposal decided |
| `reconcile-section` | M | 1 | a section is dirty or unprocessed | coverage mark staged or recorded; stale anchors addressed; extractions honest |
| `rejudge-pair` | M | 1 | a requirement was created or revised; sticky pairs | a verdict per neighbor in `evidence` (duplicate, contradiction, consistent) |
| `review-entity` | M | 1 | an entity's fact set changed | definition current; lookalikes judged; diagnostics filed or resolved |
| `declare-edges` | O | 1 | a multi-entity requirement has no `edges` | edges declared, or `evidence` says the statement is not structural |
| `dedupe-candidates` | O | 1 | cross-document lookalikes score high | merged, or kept with reasoning |
| `derive-usecases` | M | 2 | a cluster's membership changed | every cluster requirement refined by a step or marked |
| `retrace` | M | 2-6 | any node's upstream trace died or changed (a step's requirement deleted, a message's operation gone, a view member gone, an instance's attribute gone) | broken links repaired, re-derived, or the node deleted; nothing dangling |
| `extend-usecase` | O | 2 | an `If ... then` requirement is unrefined by any extension | extension added or `missing-error-requirement` diagnostic filed |
| `model-domain` | M | 3 | structural facts changed in a scope cluster | attributes, roles, cardinalities current; contradictions filed |
| `conform-instance` | M | 4 | an instance or the model under it changed | values and links conform, or the conformance diagnostic is filed |
| `partition` | M,B | 5 | composition on, no accepted partition ADR | partition ADR staged; blocked until answered |
| `design-component` | M | 5 | allocation candidates, proposed operations, or answered ADRs pending | every candidate accepted or marked; operations satisfy or carry reasoning |
| `derive-statemachine` | M | 6 | a stateful entity's triggering requirements changed | transitions refine requirements; machine current |
| `derive-interaction` | M | 6 | a use case's steps, allocation, or interfaces changed | messages ride steps and name operations (or refine requirements, composition off) |
| `curate-view` | O | any | new nodes match a view's scope; a view has no members for its query | membership decided (added, or excluded with note) |
| `split-view` | O | any | a view exceeds its size limits | sub-views created and linked, or members collapsed under parents |
| `abstract-entity` | O | any | an entity exceeds requirement or child limits | sub-entities introduced with `parent`, detail moved, docs proposals staged |
| `ratify` | B | any | a derived or decree fact awaits its prose | human accepts the docsgen proposal (dual write) or retracts the fact |
| `bind` | M | 7 | a requirement owes a binding | ledger row recorded (existing gate) |
| `generate` | M | 7 | an entity or component's facts differ from the ledger | `record_generation` landed (existing gate) |
| `verify` | M | 7 | a row's derived status says action | verdict recorded (existing gate) |
| `answer` | B | any | a diagnostic or ADR carries an unanswered prompt | the human answers; the answer's application is a new goal with the answer as cause |

Notes on the load-bearing rows:

- `retrace` is one kind, not five. The user-visible behavior the plan promises
  (delete a requirement, and the entity, the use case, and the class view that
  referenced it each surface as work) is three `retrace` goals with the same
  cause, each hinting the focus that shows the damage. The gate is uniform:
  nothing may keep pointing at the dead node.
- `abstract-entity` and `split-view` are where
  [containment](./ir-graph.md#containment-and-lifting) is exercised: introduce a
  parent, distribute children, let lifting keep coarse views true. Their skill
  carries the judgment guidance (split by cohesion of requirements, respect
  scopes, never invent concepts the docs cannot support, propose docs sentences
  for the new structure).
- `partition`, `ratify`, and `answer` are the human seams. They keep the
  convergence report honest: a build with open blocked goals is "converged,
  awaiting 2 answers", never silently done.

Each kind has: a contract paragraph (the prompt payload, one file per kind, same
embed discipline as today's prompts), a gate implementation, a hint computer, and
a benchmark case that grades an executor profile on it before it is trusted.

## Prompt assembly, per session

```
[generic agent contract]                 # fixed
[skill: <active skills for initial focus>]
## Goals
  <goal list: id, kind, contract paragraph, detail, hints>
## Focus
  <initial pack + status block>
```

The contract paragraphs are short and imperative, in the style of the existing
task instructions: what the goal means, what evidence the gate wants, what not to
do (the review asymmetry, the honesty rules). The feedback contract rides once,
high, as today.

## Ordering and convergence

- Readiness tiers replace waves: a goal is ready when the goals it depends on are
  closed in its cone. The ladder gives the tiers (alignment before ingest before
  judgment before use cases before domain before instances before composition
  before dynamics before ledger goals), the existing document levels order
  stage-1 batches, and disjoint localities run in parallel under the concurrency
  bound. The scheduler is still deterministic code; "what runs next" is a query.
- Convergence: no open mandatory goals, checks clean. Blocked goals and optional
  goals ride the verdict as counts (`converged, 2 blocked, 5 advised`).
- Budgets: per session (rounds, mutations, context), per build (goal resolutions),
  per stage family when tight, earlier tiers first. Parked goals resume first.
- Oscillation: two goals resolving each other back and forth (a proposed
  operation bouncing between dynamics and composition) is caught by flip
  detection on the goal target's natural key; the pair parks as one
  `unstable-derivation` diagnostic with both reasonings, blocked on a human.

## What stays true

- The model owns judgment inside a goal; the harness owns everything around it:
  derivation, grouping, readiness, gates, budgets, causes.
- Success is read from the store and the board, never from the agent's word.
- One agent contract, goal kinds as data, skills as payloads: adding a stage is
  goal kinds plus gates plus skills plus benchmark cases in the registry, no new
  agent.
