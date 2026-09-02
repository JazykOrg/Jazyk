# Levels

Requirements stay on the entities the documents state. Those entities summarize into
higher-level entities, and those into higher still. Each level gets its own diagrams
(structure, use cases, sequences) describing the relationships among that level's
entities. Digging into an entity shows the level below it. A level that grows too large
draws garbage collection to regroup or split it; a grouping that shrinks too small
dissolves. The build converges on a navigable architecture of the project.

E.g. the top diagram shows three boxes, User, Frontend, and Backend, with the use cases
and sequences among them. Digging into Backend shows the server, the database, the
queue, and the cache. Digging into the server shows its modules. Digging into a module
shows the classes behind it. Nothing here is a fixed depth: the tree is as deep as the
documents and the model's judgment make it.

This is the top half of the containment tree, not a second structure. The tree is the
`parent` field on entities ([containment](../model/entity.md#containment)). Rendering
already lifts a relationship that touches a hidden descendant to the nearest shown
ancestor ([lifting and collapse](../diagrams.md#lifting-and-collapse)). Levels add the
upward pressure that builds the tree and the views that show each level.

## Levels

- A level is a node's set of direct children. It is never a global horizontal slice.
  The tree is the structure, and uneven depths are fine: one branch may stop at a
  service while another goes down to classes.
- The top level of a scope is its parentless entities. See
  [the scope root](#the-scope-root).
- A level is bounded by the `children-per-entity` limit
  ([limits](../graph.md#limits)). Crossing the soft threshold derives an optional
  [`abstract-entity`](../goals/abstract-entity.md) goal on the node with the `fan-out`
  change; crossing the hard threshold makes it mandatory
  ([escalation](../reconciler.md#escalation)). Dropping back under the soft threshold
  clears the record without a session. See [fan-out](../reconciler.md#fan-out).
- The harness computes coupling; the model names and judges. Coupling hints are
  deterministic partitions over the derived relationship graph. The model may accept a
  partition, adjust it with reasons, or decline with a reason. The harness never names a
  grouping; the model never counts.
- Higher levels carry no requirements of their own. Lifting covers edges and flows. A
  grouping gets a definition and a ratification proposal, nothing more. Derived summary
  statements stay out until a diagram needs one.
- Levels converge bottom-up without a new scheduler. A GC goal is ready only when no
  compile goal is open in its target's cone ([cones](../reconciler.md#cones)), so
  leaves reconcile first, groupings form cone by cone, and each new level's views derive
  after.
- A `level-shape` check closes every build: every node with at least two children has
  its structural level view, no node is over the hard fan-out threshold, and no derived
  grouping has fewer than two children. See [checks](../compilation.md#checks).

## Groupings

A grouping is an entity that exists to hold a level. It is authored, with a minted id,
never derived data recomputed at commit. It persists across rebuilds; only a crossed
limit reopens it. Its fields:

- `provenance`: `derived`, with `from` naming exactly its members and `reasoning` saying
  why the domain would recognize them as one thing. See
  [provenance](../model.md#provenance).
- `definition`: one sentence stating the grouping's responsibility.
- `parent`: the members' former parent. A grouping never crosses levels: its members
  share one current parent, and the grouping takes that parent.
- `stereotype`: chosen from the existing vocabulary (`system`, `component`, `module`,
  and so on) or none. There is no grouping stereotype.
- No `mentions`. No document states a grouping; that is what makes it derived.

An entity the documents state (quote provenance) can hold children too. It is a grouping
in role, not in provenance. The dissolve rule never touches it, and `dissolve_entity`
refuses it with `stated-entity`: revise the documents instead.

The dissolve rule: a derived grouping with fewer than two children dissolves in the
deterministic sweep at commit. Its children reparent to the grandparent and a tombstone
redirect stays, so anything holding the old id still resolves. Below two members there
is nothing to judge. See [the sweep](../graph.md#the-sweep) and
[identity](./identity.md#operations-preserve-identity).

A grouping is created and removed through two write tools in the graph group,
`group_entities` and `dissolve_entity`, both gated at staging
([write tools](../tools.md#write-tools)). `update_entity` with `parent` stays the path
for moving one child. A session may also lower fan-out by moving a child under an
existing sibling that already contains it conceptually, with a reason.

A grouping is invented until ratified. Its ratification proposal phrases it as prose for
the document that owns its parent, or for the front door when the grouping is
top-level: the compiler proposes the architecture chapter the documents never wrote.
Accepting the proposal flips the grouping to quote provenance, and it becomes a stated
entity. See [ratification proposals](../model/diagnostic.md#ratification-proposals).

Reparents are watched like any other alternation: a child that moves between the same
two parents across generations parks the second move. See
[flip detection](../reconciler.md#flip-detection).

## The scope root

- The top level of a [scope](./scopes.md) is its parentless entities. It is a level
  with no node above it.
- Where a goal, a view, or a loaded set needs a target for it, the address is
  `scope:<scope>`. E.g. `scope:public` for the default scope.
- The scope root counts its parentless entities under `children-per-entity`, the same
  row as any node. Crossing it derives the `fan-out` goal on `scope:<scope>`, and the
  goal's cone is the whole scope.
- A grouping minted at the root has no parent, so it becomes a new parentless entity,
  and its ratification proposal targets the front door.
- The scope root's level view is the per-scope view: `view:component/scope-<scope>` or
  `view:class/scope-<scope>` by the kind rule in [level views](../diagrams.md#level-views).
- Loading `scope:<scope>` renders the top level as stubs. See
  [the loaded set](../context.md#the-loaded-set).

## Naming

The harness never names a grouping. The model names it under one doctrine, carried by
the [abstraction skill](../skills/abstraction.md):

- A grouping is a concept a reader of the documents would recognize and name, never a
  coupling artifact. A cohesive cluster with no name the domain would use is not a
  grouping yet.
- Documents and headings are the strongest naming hints. A `payment.md` suggests a
  Payments grouping; a `## Fulfillment` heading suggests Fulfillment. The model prefers
  a name the documents already use for the area. The fan-out hints name the document
  each member is mentioned in most for this reason.
- Boundaries follow the cohesion hints, and the model may split or merge a candidate
  with a reason: a candidate that mixes two responsibilities splits; two candidates the
  documents treat as one area merge.
- A split the documents do not suggest is a choice. A model, view, controller split is
  an example: the documents may never mention it. The model states it as a choice in
  its reasoning, and the ratification proposal carries the choice to the owner, who
  writes it into the documents or declines it.
- A grouping's name is judged like an entity name: search before create, and a
  lookalike of an existing area reuses it. See [judgment](./judgment.md).
- Declining is honest work. A level that is genuinely flat (nine peers with no
  cohesion) fails the goal with that reason rather than inventing tiers.

## Drill-down

Every node with at least two children, the scope root included, gets its own diagrams:
one structural level view over its children, and use case and sequence views over the
flows that lift into the level. A level view includes the outside entities whose lifted
edges touch the level, so the top diagram shows User beside Frontend and Backend. See
[level views](../diagrams.md#level-views) and
[default views](../model/view.md#default-views).

Drill-down is navigation over those views. Digging into an entity shows the level below
it:

- Every rendered member whose entity has a level view links to it: hyperlinks in the
  `.puml`, anchors in the `.svg`, and a `children` list on the view. See
  [drill-down](../diagrams.md#drill-down).
- [Docsgen](../../consumers/docsgen.md) nests a page per level: the node's definition,
  its level views, its members with links down, and a breadcrumb up.
- The [GUI](../../frontends/gui.md) shows the containment tree and a breadcrumb over the
  diagram panel. The [viewer](../../frontends/viewer.md) prints the tree with each
  node's view ids.

Diagrams stay projections. A level view derives from the tree and the requirements on
its leaves; deleting a rendering loses nothing, and a reparent redraws every level it
touches on the next commit.
