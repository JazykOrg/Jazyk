# The abstract-entity goal

Goal: abstract one entity over a limit. A service that accrued sixty requirements, a
system with twenty-five direct children, an order whose lifecycle names fifteen states:
each is one node carrying what the documents describe as several. The model splits it
into sub-entities under it, moves the detail down, and stages the sentences the
documents should gain. Containment is the structural answer to scale
([containment](../model/entity.md#containment)): after the split, lifting keeps every
coarse view true, and the ratification loop pushes the invented structure into prose or
out of the graph.

The kind has two changes. The caps variant (this page up to [the fan-out
variant](#the-fan-out-variant)) splits one node downward when its requirements or states
are over a limit. The fan-out variant groups a level upward when a node's direct children
are over the `children-per-entity` limit ([levels](../concepts/levels.md#levels)).

- Kind: `abstract-entity`. Class: GC. Optional past the soft threshold, mandatory past
  the hard one ([escalation](../reconciler.md#escalation)).
- Unit: one entity. Id: `g:abstract-entity:<ent>`; under the fan-out variant the target
  may be a scope root, `g:abstract-entity:scope:<scope>`
  ([the scope root](../concepts/levels.md#the-scope-root)).
- Ready when no compile goal is open or parked in the entity's
  [cone](../reconciler.md#cones): everything under it in the containment tree, its
  requirements, the views that show it, and the sections anchoring them
  ([readiness](../reconciler.md#readiness)). The cone of a top-level «system» is most of
  the graph, which is correct: abstracting the top of the tree waits for everything
  under it. The session therefore sees final counts, and an entity that doubled its
  requirements this build is abstracted once, holistically.

## Created when

One [change record](../graph.md#change-records) kind derives the goal:
`threshold-crossed` on an entity (`via: limits`), written by the limit counts at commit
([derived data](../graph.md#derived-data)) when a count is over the entity's threshold.
Two limits of [the registry](../graph.md#the-registry) derive the caps variant; the
third, `children-per-entity`, derives the [fan-out variant](#the-fan-out-variant):

| limit | soft | hard | counts |
|---|---|---|---|
| `requirements-per-entity` | 50 | 80 | requirements listing the entity in `entities` |
| `states-per-state-machine` | 12 | 20 | states of the entity's derived [state machine](../model/state-machine.md#derivation) `sm:<slug>`; the record's subject is the machine's subject, since the machine derives from the subject's requirements |
| `children-per-entity` | 9 | 15 | entities whose `parent` is the entity, or the parentless entities of a scope for the root form; the fan-out variant |

The entity's own bump (`limits: {<limit>: n}`) is the soft threshold for that entity
([per-node bumps](../graph.md#per-node-bumps)). `detail` carries the limit, the count,
and the thresholds in force. E.g.:

```yaml
- id: c420-0
  generation: 420
  mutation: 0
  kind: threshold-crossed
  subject: ent:order
  via: limits
  detail: {limit: requirements-per-entity, count: 54, soft: 50, hard: 80}
```

- The record is level-triggered: it stands while the count is over the threshold and
  clears on its own when the count falls back under it by any path (a delete, a merge
  elsewhere, a re-pointed statement). Resolving the goal clears it, and the next commit
  writes it again when the count is still over.
- Records for two limits on one entity fold into one goal whose `change` names both.

### Escalation and dismissal

- Soft: the goal derives optional, rides in the verdict as `optional`, and the build
  converges around it.
- Hard: the same goal is mandatory. `mandatory` is recomputed at every derivation from
  the current count, so a count that drops back under the hard value de-escalates the
  goal. The build cannot report `converged` while a mandatory `abstract-entity` goal is
  open or failed ([convergence](../compilation.md#convergence)).
- Dismissal is a graph write, never goal state. A human raises the entity's own limit
  with `bump_limit`, recorded with decree provenance in the journal (`kind: decree`,
  [journal](../graph.md#journal)). The goal derives only when the count crosses `n`, and
  escalates when it crosses `n` plus the registry's distance between soft and hard. No
  session can bump: the tool is a human path (the
  [GUI board](../../frontends/gui.md#board), chat), and a session that finds the count
  honest fails the goal recommending the bump, so the recommendation surfaces on the
  entity.
- Nothing tunes limits in `jazyk.toml`. The registry is built into the binary, and bumps
  are per node.

The trace prints the burst as it starts: `gc burst: abstract-entity ent:order (54 > 50)`
([compile and garbage collection](../compilation.md#compile-and-garbage-collection)).

### Batching

When several `abstract-entity` goals are ready in one cone (an entity and its ancestor
both over a limit), the batch takes the deepest target first; the ancestor's goal
re-derives from the counts the commit leaves, since a child's split changes the
ancestor's child count and the arrows lifted to it. An entity's goal batches with the
[`split-view`](./split-view.md) goals on the views that show it
([batching](../reconciler.md#batching)): a view over its member limit because its
entities were never grouped is resolved by the same split.

## Gate

Sub-entities with `parent`, detail moved, docs proposals staged, count within the limit.
At `mark_goal_done` the harness checks over the staged state:

- The crossed count is within the limit's soft value, or the entity's bump. A split that
  only de-escalates commits its work at `done` but does not resolve the goal: the goal
  stays open, optional, and the session continues or fails it with the reason.
- Every entity staged with `parent` under the target (directly, or under an intermediate
  parent staged in the same session) carries derived provenance: `from` naming the
  target and the requirements or children moved under it, `reasoning` the cohesion it
  follows, and a non-empty `definition` written as the sentence the documents should
  gain ([provenance](../model.md#provenance)). Its `scope` is the target's: a split
  never crosses a scope ([scope and containment](../concepts/scopes.md#scope-and-containment)).
- Detail moved: each staged sub-entity has at least one requirement re-pointed to it
  (`update_requirement` passing only `id` and `entities`) or at least one child
  re-parented to it (`update_entity` `parent`). A sub-entity with nothing under it is
  invented structure and is named in the rejection.
- For the state limit: each staged sub-entity is the `transition.subject` of at least
  one re-pointed transition statement, and no state is merged away: the union of the
  states across the target's machine and the sub-entities' machines after the split is
  the union before it ([derivation](../model/state-machine.md#derivation)).
- The target survives: no `delete_entity` or `merge_entities` on it, and no requirement
  loses its last entity.
- Docs proposals staged, mechanically: the commit that lands a derived fact files its
  `ratification-pending` diagnostic and writes the `provenance-pending` record, and one
  [`ratify`](./ratify.md) goal derives per sub-entity, blocked on a human. The gate
  checks the material the proposal needs: the definition sentence, and the `reasoning`
  naming the section it targets.
- An existing entity re-parented under the target (a noun the documents already have as
  an entity elsewhere) has no new fact of its own to ratify, so the session stages the
  derived requirement that states the whole-part ([docs proposals](#docs-proposals)) and
  the gate checks it is there.
- `parent` stays acyclic and agrees with stated composition
  ([validation gates](../graph.md#validation-gates)).

What the gate does not check: cohesion, naming, and whether the documents support the
concept. The [abstraction skill](../skills/abstraction.md) carries that judgment: split by
cohesion of the requirements, name each sub-entity in the documents' own wording, never
invent a concept the documents cannot support, search before creating.

At `done`, the per-mutation gates hold and a clean batch commits
([commit](../sessions.md#commit)). The goal fails (`mark_goal_failed`) when the
requirements cohere into no groups the documents name, or when the pressure is one
oversized section's: `section-too-large` is the author's to split, and a graph split is
the answer only when the requirements cohere into sub-concepts the documents name. The
failure surfaces on the entity with the recommendation (a section to split, or a bump
when the count is honest) ([parked and failed](../reconciler.md#parked-and-failed)). A
failed mandatory goal blocks convergence.

Stability: a split re-derived on a later build lands on the same natural keys (the same
names under the same parent), so the upserts are no-ops
([the natural key under containment](../concepts/identity.md#the-natural-key-under-containment)).
A split that a compile review merges back and a later GC session re-derives is a flip
between the classes: two flips of one natural key park the pair as
`unstable-derivation`, blocked on a human
([flip detection](../reconciler.md#flip-detection)).

### Docs proposals

- The sentence is the sub-entity's `definition`, written in the documents' vocabulary as
  prose the author would keep: "The order service contains a pricing module that
  computes totals and applies discounts." Never graph jargon.
- The harness picks the target section ([the proposal](./ratify.md#the-proposal)): the
  section that defines the parent, or a new sub-document beside it when that section is
  over its size threshold. The session's `reasoning` names the section it has in mind,
  for the reviewer.
- One proposal per fact. A parent that gains three parts is three definitions, or one
  derived requirement naming the three. The session may stage that requirement:
  `upsert_requirement` with `provenance: {derived: {from, reasoning}}` in place of
  `section` and `quote`, `from` the target and the parts, `reasoning` the cohesion the
  split follows, `statement` the whole-part sentence ("The order service consists of the
  cart module, the pricing module, and the fulfillment module."), `entities` the target
  and the parts, one `composition` edge from the target to each part, `facets`
  constraint ([write tools](../tools.md#write-tools)). Accepted, it becomes the quoted
  sentence that states the containment, and `parent` agrees with a stated composition
  from then on ([containment](../model/entity.md#containment)).
- The proposals stand as `blocked` counts. Until ratified, the sub-entities are
  `derived`: invented until the documents say so. Retracting one moves its requirements
  and mentions back to its parent ([retract](./ratify.md#retract)); the owner may also
  write the sentence by hand, and the next `reconcile-section` lands on the pending fact
  under its natural key ([write the sentence by hand](./ratify.md#write-the-sentence-by-hand)).

### The decision at the top of the tree

When the target is a containment root with no children, and no requirement states a
whole-part with it as the whole, the split proposes component structure where the
documents state none. That is the same move at the top of the tree, with one more step:
the session files a `decision` diagnostic on the target
([rules catalog](../model/diagnostic.md#rules-catalog)) with a
[prompt](../model/diagnostic.md#prompts): one question naming the proposed parts, one
`answer` option per candidate structure (the split as staged, the entity kept whole, and
any alternative the requirements support), `freeform: true`. The commit writes
`prompt-unanswered`, and the blocked [`answer`](./answer.md) goal derives.

- The split lands with the diagnostic. The decision is the owner's chance to overrule at
  the structural level, not a gate on the session: the ratification proposals per part
  ask about each sentence, the decision asks about the shape.
- Generation groups by containment where it exists
  ([generation](../../consumers/gen.md)), so the ruling is owed before the deliverable
  is regrouped. The `answer` goal rides in the verdict as `blocked`
  ([convergence](../compilation.md#convergence)) and on the board summary line, and in
  `manual` mode the `generate` goals on the target and its new children wait on the
  generate release ([readiness](./generate.md#readiness)), which is where the owner
  holds the deliverable until the decision is answered.
- The answer session applies the ruling: keep the structure (the diagnostic resolves,
  the ratification proposals stand); keep the entity whole (the sub-entities are
  retracted, their requirements return to the target, and the target's limit is bumped
  by decree so the goal stops deriving); an alternative structure (the sub-entities are
  re-derived along the ruling).

### After the split

- Compile goals reopen in the cone and the loop runs them before returning: the commit
  writes `entity-changed` on the target and each sub-entity
  ([`review-entity`](./review-entity.md)), `requirement-revised` on each moved statement
  ([`rejudge-pair`](./rejudge-pair.md)), and the ledger comparison flags the target's
  changed fact set for [`generate`](./generate.md)
  ([compile and garbage collection](../compilation.md#compile-and-garbage-collection)).
- Derived data recomputes at commit: relationships regroup around the sub-entities, the
  target's machine shrinks to the transitions still on it, a level view derives for the
  target once it has two children ([level views](../diagrams.md#level-views)), and the
  scope root's level view keeps showing the target with the sub-entities lifted into it
  ([default views](../model/view.md#default-views)).
- Lifting keeps the coarse views true: a relationship touching a sub-entity lifts to the
  target wherever a view shows the target collapsed
  ([lifting and collapse](../diagrams.md#lifting-and-collapse)). The session collapses
  the target in the coarse views that do not need the internals with `update_view`
  `collapse`, and gives the internals a sub-view when they earn one.
- The docsgen page of the target carries the proposals and the new pictures
  ([ratification proposals](../../consumers/docsgen.md#ratification-proposals)).

## Hints

The hint computer emits, per goal:

- `load <ent>`: the entity in full and every requirement on it, across all documents;
  over-budget lists become handles.
- `<count> > <limit> (<limit name>, soft <s>, hard <h>)`: the change, one line per
  crossed limit.
- `load sm:<slug>`: for the state limit, the machine with its states and transitions.
- `group <noun> (<n> requirements, <section>)`: candidate cohesion groups, best first:
  content tokens (minus the entity's own name tokens) recurring across at least three
  of the entity's statements, with the section title where most of them are quoted.
  Suggestions from token counts, never judgments; the gate is the truth.
- `child <ent> (<n> requirements)`: existing children as stubs, so a noun that already
  is an entity is re-parented, never minted twice.
- `load <section>`: the section that defines the entity (the proposal's target), with
  `section-too-large` noted when the check filed it.
- `root: no parent, no composition stated`: when the target is at the top of the tree,
  so the session knows the `decision` prompt is owed.
- `skill abstraction`.
- `upsert_entity`, `update_requirement`, `update_entity`: the tools that resolve the
  kind.

## What the model sees

The goal block in the [session prompt](../sessions.md#the-prompt) carries the contract
paragraph from [`./prompts/abstract-entity.md`](./prompts/abstract-entity.md), the
change in one line, the gate in one line, and the hints. E.g.:

```text
- [g:abstract-entity:ent:order] optional
  This entity is over a limit. Load it in full with every requirement on it before
  deciding anything; the counts are final for this build, so abstract once. Group the
  requirements by cohesion; a group is a sub-entity only when the documents already
  have a noun for it. Search before creating. Introduce each sub-entity with
  upsert_entity (parent this entity, the definition written as the sentence the docs
  should gain), move the grouped statements with update_requirement passing only id
  and entities, keep the statements about the whole on this entity. At the top of the
  tree, file a decision prompt. Never invent a concept the documents cannot support.
  Change: 54 requirements > 50 (requirements-per-entity, soft 50, hard 80) (g420).
  Gate: sub-entities with parent and derived provenance, detail moved, proposals
  staged, count within the limit.
  Hints: load ent:order; group pricing (9 requirements, /orders/pricing); group
  fulfillment (7 requirements, /orders/fulfillment); skill abstraction.
```

The [abstraction skill](../skills/abstraction.md) is active from the first round
([skills](../sessions.md#skills)): splitting by cohesion, respecting scopes and stated
containment, building the tree, moving the detail, writing the proposals, the docs split
versus graph split line, stability.

The initially [loaded set](../context.md#the-loaded-set) holds, per goal:

- The entity in full ([entity](../model/entity.md#fields)): `definition`, `aliases`,
  `scope`, `stereotype`, `parent` with its chain, `attributes`, mentions with one parent
  chain each, and every requirement referencing it with statement, quote, facets,
  transition, and edges, grouped by source section. Over-budget groups become handles
  ([policy](../context.md#policy)), and the hints say which to expand first.
- The existing children as stubs, with their requirement counts.
- For the state limit: the derived machine, states and transitions each with its
  requirement ([state machine](../model/state-machine.md)).
- The section that defines the entity, as its body, and the size diagnostics on it.
- The views that show the entity, as stubs with member counts, so the session knows
  where to collapse.

### Splitting

- Group by what the statements are about: a sub-concept, a phase, a role, an
  interface, a lifecycle stage, a recurring noun. A group is a sub-entity when the
  documents already have a noun for it (a section title, a repeated phrase, a list
  item) and its statements are about that noun directly. A group with no noun in the
  documents is not a sub-entity.
- Re-point each grouped requirement with `update_requirement`, passing only `id` and
  `entities`: the sub-entity in place of the target when the statement is about the
  part, the target kept when the statement is about the whole. `transition.subject` and
  every edge end stay in `entities`. Attributes move with their concept through
  `update_entity` on the sub-entity.
- Over the children limit, the goal derives with the fan-out change instead, and the
  level is grouped rather than the node split ([the fan-out variant](#the-fan-out-variant)).
- Over the state limit, the subject conflates phases or concepts. Split it into a
  sub-entity per phase whose states are that phase's, and re-point each transition
  statement's `entities` and `transition.subject` to the phase whose states it uses.
  Never merge states to lower the count.
- The target keeps the statements about the whole and its definition; refresh the
  definition to name the parts when the documents' wording does. Never delete the
  target, never move a statement about the whole, never re-split what a review merged
  back.

## Tools

The `abstract-entity` [toolset](../tools.md#toolsets): the
[read tools](../tools.md#read-tools) (`load`, `expand`, `unload`, `graph_status`, `search`, `read_section`, `get_entity`, `get_view`, `diagnostics`), the [goal tools](../tools.md#goal-tools) (`mark_goal_done`, `mark_goal_failed`, `load_skill`, `done`),
`upsert_entity`, `update_entity`, `upsert_requirement`, `update_requirement`, the
[view tools](../tools.md#view-tools) (`upsert_view`, `update_view`), `report_diagnostic`
(rule `decision`), and [`report_feedback`](../tools.md#feedback-tool). Under this goal
`upsert_entity` takes `provenance: {derived: {from, reasoning}}` in place of `mention`,
and `upsert_requirement` the same in place of `section` and `quote`: the session passes
the provenance in the call, `from` naming the target and the requirements and children
it moves under the new node, `reasoning` the cohesion the split follows. The store
records what the call carries and never synthesizes `from`; the gate reads `from` off
the staged call ([gate](#gate)). No deletes and no merges: an
abstraction adds structure and moves detail, and what a merged or dead node needs is
[`review-entity`](./review-entity.md)'s or [`retrace`](./retrace.md)'s work. See
[write tools](../tools.md#write-tools). The toolset also carries `group_entities` and
`dissolve_entity`, the tools of the [fan-out variant](#fan-out-tools).

## The fan-out variant

A level is a node's direct children ([levels](../concepts/levels.md#levels)); the top
level of a scope is its parentless entities
([the scope root](../concepts/levels.md#the-scope-root)). When a level outgrows the
`children-per-entity` limit, the goal derives on the node that holds it with the
`fan-out` change, and the session groups the children into groupings under the node
([groupings](../concepts/levels.md#groupings)). The caps variant splits one node
downward; the fan-out variant builds the level above its members. Both add structure with
derived provenance and leave the ratification loop to push it into prose. A grouping is
an entity with derived provenance from its members, a definition, the members' former
parent as its parent, and no mentions; it persists across rebuilds, and only a crossed
limit reopens it.

### When it derives

- The limit: `children-per-entity`, soft 9, hard 15, counting the entities whose
  `parent` is the node; the scope root counts its parentless entities under the same row
  ([the registry](../graph.md#the-registry)).
- Crossing soft writes the record at commit and derives the goal optional; hard
  escalates it to mandatory; dropping back under soft clears the record without a
  session, the level-triggered rule of [created when](#created-when). The reconciler's
  [fan-out](../reconciler.md#fan-out) derivation names the record and the target form.
- Target: the node whose children are over the limit, or `scope:<scope>` for the top
  level (`g:abstract-entity:scope:public` for the default scope). The cone is the node's
  subtree, the whole scope for the root form; the goal is ready when no compile goal is
  open in it, the existing GC rule ([GC gating](../reconciler.md#gc-gating)). Leaves
  reconcile first, groupings form cone by cone, and each new level's views derive after.
- Minimum membership is not a limit row: a derived grouping with fewer than two children
  dissolves in [the sweep](../graph.md#the-sweep) at commit, so nothing derives for it.

### The change

`change: {fan_out: n, limit: {soft, hard}, candidates: [[id, ...], ...]}`. `fan_out` is
the count of direct children, `limit` the thresholds in force (the node's own bump is its
soft value, [per-node bumps](../graph.md#per-node-bumps)), `candidates` the coupling
partitions the hint computer proposes: a deterministic greedy agglomeration over the
node's children, weighted by the requirements and derived relationships shared between
them, descendants lifted to the child ([coupling hints](../reconciler.md#fan-out)). The
harness computes coupling and never names a grouping; the model names and judges and
never counts. E.g.:

```yaml
change:
  fan_out: 12
  limit: {soft: 9, hard: 15}
  candidates:
    - [ent:cart, ent:pricing, ent:checkout, ent:discount]
    - [ent:shipment, ent:carrier, ent:tracking]
    - [ent:invoice, ent:payment]
```

### The fan-out gate

At `mark_goal_done` the harness checks over the staged state:

- The node's direct children count is at or under the hard threshold when the goal is
  mandatory, or at or under the soft threshold (or the node's bump) when it is optional.
- Every grouping the session created has at least two children, a non-empty
  `definition`, derived provenance whose `from` names exactly its members, and the node
  as its `parent` ([provenance](../model.md#provenance)). A grouping never crosses
  levels: its members share one current parent, and it takes that parent.
- No member changed scope
  ([scope and containment](../concepts/scopes.md#scope-and-containment)).
- A child moved under an existing sibling that already contains it conceptually
  (`update_entity` `parent`) lowers fan-out without a new grouping; the justification
  carries the reason.
- `parent` stays acyclic and agrees with stated composition
  ([validation gates](../graph.md#validation-gates)).
- The docs proposals follow mechanically: the commit files `ratification-pending` per
  grouping and one [`ratify`](./ratify.md) goal derives per grouping, blocked on a
  human. The proposal phrases the grouping as prose for the document that owns the node,
  or the front door for a top-level grouping ([naming](../concepts/levels.md#naming)).

What the gate does not check: whether the domain would recognize the grouping, its name,
or its boundary. The [abstraction skill](../skills/abstraction.md) carries that judgment.
The justification is one or two sentences naming the groupings and why the domain would
recognize them.

Stability: a grouping re-derived on a later build lands on the same natural key (the
same name under the same parent), so the upsert is a no-op. A child that moves between
the same two parents across generations is a reparent flip: the second move parks like a
cross-class flip ([flip detection](../reconciler.md#flip-detection)).

### Declining

A level can be genuinely flat: nine peers with no cohesion among them, each a concept of
its own. Then the goal fails (`mark_goal_failed`) with a reason naming why the level is
flat, never a grouping invented to satisfy the count. The failure surfaces on the target
([parked and failed](../reconciler.md#parked-and-failed)); a failed mandatory goal blocks
convergence ([convergence](../compilation.md#convergence)), and a human answers it by
decree (the node's own bump) or with prose that states the structure. Declining is honest
work, graded like a grouping.

### Fan-out hints

The hint computer emits, per fan-out goal:

- `load <target>`: the node with its direct children as stubs, or `scope:<scope>`, which
  loads the top level as stubs.
- `fan-out <n> > <soft> (children-per-entity, soft 9, hard 15)`: the change.
- `candidate [<id>, ...] (weight <w>)`: one line per coupling partition, largest first,
  at most the soft threshold of candidates and 12 ids per candidate (the rest summarized
  as a count), each with its internal weight. Suggestions from counts, never judgments.
- `member <ent> «<stereotype>» (<document>)`: each child with its stereotype and the
  document it is mentioned in most. Documents and headings are strong naming hints.
- `grouping <ent> (<n> children)`: every existing grouping under the node, so a child
  that belongs in one is moved there, never regrouped beside it.
- `namesake <ent> (<document> is headed <title>; <n> other member(s) of the level from
  it)`: a member whose name is the heading of the document that names it and other
  members of the level. A stated entity that carries the area's name is the level's
  node for that document's entities: they move under it with `update_entity`
  `parent`, and no twin grouping is minted (a name is judged like an entity name, and
  a node is both a thing and the frame of the level below it, the way a service is a
  component and the parent of its modules).
- `skill abstraction`.
- `group_entities`, `update_entity`, `dissolve_entity`: the tools that resolve the
  variant.

### The fan-out prompt

The goal block carries the fan-out paragraph of
[`./prompts/abstract-entity.md`](./prompts/abstract-entity.md), the change in one line,
the gate in one line, and the hints. E.g.:

```text
- [g:abstract-entity:scope:public] optional
  This level is over its fan-out limit. Load the target and its children; the cone is
  quiet, so the level is final for this build. A grouping is a concept a reader of the
  documents would recognize and name, never a coupling artifact: the candidates are
  boundaries to accept, adjust with reasons, or decline. Prefer a name the documents
  use for the area; search before creating. Stage each grouping with group_entities
  (name, a one-sentence definition of its responsibility, members, a stereotype from
  the existing vocabulary or none, reasoning). A flat level fails with that reason.
  Change: fan-out 12 > 9 (children-per-entity, soft 9, hard 15) (g431).
  Gate: children at or under the limit; every grouping with two or more members, a
  definition, derived provenance naming its members, and this node as parent.
  Hints: load scope:public; candidate [ent:cart, ent:pricing, ent:checkout,
  ent:discount] (weight 14); candidate [ent:shipment, ent:carrier, ent:tracking]
  (weight 9); member ent:cart «component» (orders.md); skill abstraction.
```

The initially [loaded set](../context.md#the-loaded-set) holds, per goal:

- The node in full, or the scope's top level as stubs for the root form.
- Each child as a stub with its stereotype, requirement count, child count, and the
  document it is mentioned in most.
- The derived relationships among the children
  ([relationship](../model/relationship.md)), the material of the candidates.
- The level's views as stubs ([level views](../diagrams.md#level-views)).
- The existing groupings under the node with their children.

Requirements are not loaded up front: the level is judged by its members and their
relationships, and `expand` reaches a member's statements when a boundary needs them.

### Grouping

- A grouping is a concept a reader of the documents would recognize and name, never a
  coupling artifact. The candidates are boundaries: accept one, adjust it with a reason
  (a candidate mixing two responsibilities splits, two candidates the documents treat as
  one area merge), or decline it.
- Name it in the documents' wording: a document's name or heading is the strongest hint
  (`payment.md` suggests a Payments grouping), and a lookalike of an existing area
  reuses that entity ([naming](../concepts/levels.md#naming)). The stereotype comes from
  the existing vocabulary (`system`, `component`, `module`, and so on) or is absent; no
  stereotype marks a grouping.
- A split the documents do not suggest (a model, view, controller split) is a choice.
  State it as one in the reasoning and let the ratification proposal carry it to the
  owner.
- Stage each grouping with `group_entities`; move a child that belongs under an existing
  sibling with `update_entity` `parent`; dissolve a grouping that lost its reason with
  `dissolve_entity`. Higher levels carry no requirements of their own: lifting covers
  edges and flows ([lifting and collapse](../diagrams.md#lifting-and-collapse)).
- After the commit, a level view derives for each grouping with two or more children and
  the diagrams link down into it ([drill-down](../concepts/levels.md#drill-down),
  [drill-down rendering](../diagrams.md#drill-down)); the ancestor's goal re-derives
  from the counts the commit leaves ([batching](#batching)).

### Fan-out tools

The `abstract-entity` [toolset](../tools.md#toolsets) carries two tools for this
variant, both gated at staging ([write tools](../tools.md#write-tools)):

- `group_entities({name, definition, members, stereotype?, reasoning})`: stages one
  derived entity (provenance `derived` from `members`, the members' shared current
  parent as its parent, the members' scope) and reparents every member under it, as one
  changeset. Gates: at least two members; every member resolves; all members share one
  current parent; `near-duplicate` against existing names as `upsert_entity`;
  `definition` and `reasoning` non-empty. Answers with the new id and the members moved.
- `dissolve_entity({id, reason})`: the inverse, for a grouping with derived provenance
  and no mentions: children reparent to its parent, and the entity tombstones with a
  redirect to its parent. Refused on an entity a document states (`stated-entity`:
  revise the documents instead).

`update_entity` `parent` stays the single-move path. The read tools, the goal tools, the
view tools, and `report_feedback` apply as in [tools](#tools). No `delete_entity` and no
`merge_entities`.
