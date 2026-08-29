# The curate-view goal

Goal: decide membership for one view against the nodes the harness matched to it. Two
kinds of match arrive: an entity that a curated structural view's `query` picked up at a
recompute, and a `behavior` or `failure-mode` requirement the flow placement check found
in no flow view and matched to this one. The model adds each matched node in its place or
excludes it with a note on the view. Either way the view records the decision, and the
next build reads it instead of asking again. Membership is the whole of the judgment:
arrows come from relationships, order defaults to the documents, and the rendering is
recomputed from the result ([view](../model/view.md)).

- Kind: `curate-view`. Class: GC. Optional: it never blocks convergence and rides in the
  verdict as `optional`.
- Unit: one view. Id: `g:curate-view:<view id>`.
- Ready when no compile goal is open or parked in the view's
  [cone](../reconciler.md#cones): its members, the nodes matched to it, their entities,
  and the sections anchoring them ([readiness](../reconciler.md#readiness)). Curation
  sees final statements, so a step is placed once, over its final wording.

## Created when

Two [change record](../graph.md#change-records) kinds derive the goal:

- `query-match` (`via: query`): the recompute at commit found a node that newly matches
  a curated view's `query` ([membership](../model/view.md#membership)). The node joins
  `members` at that commit, so the diagram is current meanwhile; the record asks whether
  it belongs. `detail` names the matched ids and the query clause each matched. A
  default view never writes the record: the harness owns its membership and a match
  joins silently. Any mutation on a default view makes it curated
  ([default views](../model/view.md#default-views)).
- `flow-unplaced` (`via: flow-placement`): the flow placement
  [check](../compilation.md#checks) at the end of a build found a `behavior` requirement
  in no flow view and excluded from none (`unplaced-behavior`), or a `failure-mode`
  requirement represented in no branch (`unrepresented-failure-mode`). The check files
  the diagnostic and writes the record on the flow view the requirement fits best.
  `detail` names the requirement, its facet, the diagnostic, and the alternatives.

The check matches a requirement to a view deterministically:

- the flow view of the requirement's own cluster, keyed by its actor and its document
  ([flow clusters](../model/view.md#default-views)), when one exists;
- else the flow view sharing the most entities with the requirement, ties broken by the
  same document first, then by document order of the views' first members;
- the rest of the ranking rides in `detail` as alternatives.

A cluster of one member derives no default view, which is the common way a requirement
ends up unplaced: its actor's other statements live in another document. The check runs
only where flow views exist; a project with no flow view writes no record. E.g.:

```yaml
- id: c419-0
  generation: 419
  mutation: 0
  kind: flow-unplaced
  subject: view:usecase/customer-shop
  via: flow-placement
  detail: {requirement: req:returns-2, facet: behavior, diagnostic: diag:unplaced-behavior-1,
           shared: [ent:customer], alternatives: [view:usecase/customer-returns]}
```

Records on one view fold into one goal per build
([goal derivation](../reconciler.md#goal-derivation)). Resolving the goal clears them. A
`flow-unplaced` record clears on its own when the requirement becomes a member of any
flow view or is excluded from one with a note, by any path; a `query-match` record
clears when the node leaves the view or the query.

### Batching

View locality ([batching](../reconciler.md#batching)): goals on views that share members
batch together, and the twin views of one flow (the use-case view and the sequence view
of one cluster share members and a title) batch together, so a step is placed in both in
one session. A `curate-view` goal batches with the goals on the matched nodes' entities
when they are in the same burst.

## Gate

Membership decided. At `mark_goal_done`, `evidence` carries one verdict per matched
node, `added` or `excluded`, and the harness checks over the staged state:

- `added`: the node is in the view's `members`. A query match is there already; the
  verdict confirms it and the justification says why it belongs. A flow requirement is
  added with `update_view` `add_members` at its position, or with `members` (the whole
  ordered list) when the placement reorders the flow. A `failure-mode` member follows
  the step whose failure it handles, so the activity emitter draws the branch there.
- `excluded`: the view's `excluded` lists the node with a non-empty note (`update_view`
  `exclude`). A placeholder note (`none`, `n/a`) counts as absent. The note names the
  sentence or the rule: "constraint, not flow", "example, not flow", "belongs to
  view:usecase/customer-returns". A node that belongs to another flow is added there in
  the same session, and the note says which.
- A matched node in neither list is named in the rejection.

What the gate does not check is placement quality: whether the step sits where the
documents put it. The justification names the sentence that orders it (a "then", a
numbered step, a trigger chain, a state the next step requires), and document order
stands unless the documents state otherwise
([flow order](../model/view.md#default-views)).

Bounds:

- A decision that pushes the view over a limit is correct here and derives
  [`split-view`](./split-view.md) at commit. Never satisfy a limit by dropping a member.
- `delete_view` on a default view is rejected; exclusion is the move. A query that
  matches wrongly is narrowed (`scope`, `parent`, `stereotype`, `depth`) rather than
  replaced by a list, so later matches keep arriving as `query-match`.
- A missing arrow in a structural view is a missing edge
  ([`declare-edges`](./declare-edges.md) work) or a missing statement, never something a
  view adds.
- An added member is of the view's kind: entities for structural kinds, requirements for
  flow kinds ([validation gates](../graph.md#validation-gates)).

At `done`, the per-mutation gates hold and a clean batch commits
([commit](../sessions.md#commit)). Any mutation on a default view clears its `default`;
the view is curated from then on, its `query` (when it has one) keeps matching, and its
`excluded` and `collapse` lists stand. The flow placement check at the end of the build
resolves the diagnostics whose requirement is placed or excluded.

The goal fails (`mark_goal_failed`) when the matched requirement fits no flow the
documents state and excluding it would only hide it: the failure surfaces on the view,
the diagnostic stands for the author, and the author decides whether the documents owe
a flow ([parked and failed](../reconciler.md#parked-and-failed)).

## Hints

The hint computer emits, per goal:

- `load <view>`: the view in full.
- `load <req> (unplaced behavior)`, `load <req> (unrepresented failure mode)`, or
  `load <ent> (query match: <clause>)`: each matched node, with why it matched.
- `alternative <view> (shares <n> entities)`: the other flow views the requirement could
  belong to, best first.
- `after <req> (<trigger>)`: for a failure-mode requirement, the member whose trigger
  its condition names, when one does.
- `skill flow-views` for a flow view, `skill structural-views` for a structural view
  ([view kinds](../model/view.md#kinds)).
- `update_view`: the tool that resolves the kind.

## What the model sees

The goal block in the [session prompt](../sessions.md#the-prompt) carries the contract
paragraph from [`./prompts/curate-view.md`](./prompts/curate-view.md), the change in one
line, the gate in one line, and the hints. E.g.:

```text
- [g:curate-view:view:usecase/customer-shop] optional
  The harness matched nodes to this view. Load the view and each matched node. Decide
  every matched node exactly one way: add it with update_view add_members at its
  position in the flow, or exclude it with update_view exclude and a note naming the
  sentence or the rule that keeps it out. Document order stands unless the documents
  state the order; a failure-mode member sits right after the step whose failure it
  handles. Never invent a member, never delete a default view.
  Change: req:returns-2 (behavior) in no flow view (g419); shares ent:customer.
  Gate: every matched node is a member, or excluded with a note.
  Hints: load view:usecase/customer-shop; load req:returns-2 (unplaced behavior);
  alternative view:usecase/customer-returns (shares 1 entity); skill flow-views.
```

The skill follows the view's kind and is active from the first round
([skills](../sessions.md#skills)): [flow-views](../skills/flow-views.md) for use-case,
activity, sequence, communication, timing, and overview views (ordering, branches,
participants); [structural-views](../skills/structural-views.md) for class, object,
package, component, composite, and deployment views (membership per kind, query versus
list, collapse).

The initially [loaded set](../context.md#the-loaded-set) holds, per goal:

- The view in full ([view fields](../model/view.md#fields)): `kind`, `title`, `query`,
  `members` in order (one line per member: a requirement's statement with its initiator
  and receiver, or an entity stub), `excluded` with notes, `collapse`.
- Each matched node in full: a requirement with its statement, quote, entities, facets,
  and transition ([requirement](../model/requirement.md#fields)); an entity with its
  definition, stereotype, parent, and edges ([entity](../model/entity.md#fields)).
- For a flow match: the requirement's entities as stubs, and the alternative views as
  stubs with member counts and actors.
- For a query match: the relationships between the node and the members, one line each,
  so the arrows the addition brings are visible before it is confirmed.
- The open diagnostic the check filed, when the change is `flow-unplaced`.

### Placement

- Members of a flow are `behavior` and `failure-mode` statements. A constraint, a
  quality bound, or a structural sentence is not a step: exclude it with a note. A worked
  example is an instance, not a step.
- A flow carries one intent: what one initiator sets out to do and what the subject does
  in response. A statement that belongs to another intent is that flow's member. When no
  flow carries the intent, `upsert_view` creates one (a `use-case` view with the
  requirement and the members that share its intent), and the note on this view names
  it. An `activity` twin of a use-case view is created the same way when the branches
  deserve drawing.
- A failure-mode member sits after the step whose failure it handles. One with no step
  to branch from is another flow's member, or a statement the documents left unattached:
  exclude it with a note saying which, and the diagnostic stands for the author.
- A query match belongs when the view exists to show it: an entity of the scope in a
  class view, a child of the container in a component view, an instance of the type in
  an object view. One that belongs to another view is excluded with a note naming it.
  Instances belong in object views, not class views.
- Retitle with `update_view` when the derived title reads as a fragment; the title names
  the intent in the documents' terms, and the twin views of a flow follow it.

## Tools

The `curate-view` [toolset](../tools.md#toolsets): the
[read tools](../tools.md#read-tools), the [goal tools](../tools.md#goal-tools), the
[view tools](../tools.md#view-tools) (`upsert_view`, `update_view`, `delete_view`), and
[`report_feedback`](../tools.md#feedback-tool). No entity or requirement tools: a
facet the session disagrees with is excluded with a note ("constraint, not flow") and
re-judged by the compile goals that own the statement, and a missing edge is
`declare-edges` work. `delete_view` serves one case: a curated view this session
created and left empty. See [write tools](../tools.md#write-tools).
