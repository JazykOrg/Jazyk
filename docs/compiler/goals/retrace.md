# The retrace goal

`retrace` repairs one node whose upstream died: a curated view's member was deleted, an
instance's type or a type attribute is gone, an attribute of an entity rests on a
sentence that vanished, a node a derived fact's `from` cited was removed. It is one
kind whatever the node: delete a requirement, and the flow view that stepped through
it, the instance that conformed to it, and the derived entity that cited it each
surface as a `retrace` goal with the same cause, each hinting what to load to see the
damage. The gate is uniform: nothing on the target may keep pointing at a dead node.
Derived data needs no retrace: [relationships](../model/relationship.md#recompute),
[state machines](../model/state-machine.md#derivation), and
[default views](../model/view.md#default-views) recompute at commit, and query-based
membership recomputes with them.

- Class: compile. Mandatory. Readiness tier 2.
- Unit: one node: a curated view, an entity (an instance included), or a requirement
  with derived provenance. Goal id `g:retrace:<node id>`, e.g.
  `g:retrace:view:usecase/holds`.
- Skill: by the target's kind. A flow view brings [`flow-views`](../skills/flow-views.md);
  a structural view brings [`structural-views`](../skills/structural-views.md); an
  instance brings [`conformance`](../skills/conformance.md); a derived entity or a
  requirement with derived provenance brings [`abstraction`](../skills/abstraction.md)
  (the `from` re-derivation); an entity whose only loss is an attribute's anchor brings
  none, the contract paragraph says what to keep.

## Created when

The goal derives from two [change record](../graph.md#change-records) kinds, both
written on the deletion path ([garbage collection](../graph.md#garbage-collection)):

- `view-member-gone`: a delete removes a node listed in a curated view's `members`,
  `excluded`, or `collapse`. The subject is the view; `via` is the list
  (`members`, `excluded`, `collapse`); `detail` names the dead node, the reason, the
  generation, and the position in the list. Only a listed member derives the goal: a
  default view is recomputed and a query re-matched at the same commit
  ([membership](../model/view.md#membership)).
- `node-deleted`: the sweep writes one per live node that still points at the dead
  node, `via` naming the reference: `from` on derived provenance (an entity, view, or
  requirement synthesized from the dead node), `edges` on the requirement that stated
  an instance's `instantiation` (the instance lost its type), `attributes` (an
  attribute whose provenance quote died with its section).

Deleting an entity still referenced by requirements or named as a `parent` is refused
([mutations](../graph.md#mutations)), so `entities`, `transition`, and `parent` never
dangle. A requirement deleted while entities reference it is the ordinary case: those
entities get `entity-changed` records and a [`review-entity`](./review-entity.md) goal,
not a retrace; `retrace` derives only where a stored reference or a provenance on the
live node names the dead one. A merge rewrites every reference mechanically and derives
no retrace. A section removed from a document kills its quotes, the sweep deletes the
requirements they anchored, and the records above follow from those deletions.

E.g.:

```yaml
- id: c409-2
  generation: 409
  mutation: 2
  kind: view-member-gone
  subject: view:usecase/holds
  via: members
  detail: {deleted: req:orders-6, reason: duplicate, in: g409, position: 3}
```

derives:

```yaml
g:retrace:view:usecase/holds:
  kind: retrace
  class: compile
  mandatory: true
  target: view:usecase/holds
  unit: view
  change: {deleted: req:orders-6, in: g409}
  cause: {generation: 409, mutation: 2, via: members}
  state: open
  hints:
    - load view:usecase/holds
    - skill flow-views
```

Several dead references on one target fold into one goal
([change records](../reconciler.md#change-records)). The record clears when the goal
resolves or when the target itself is deleted.

## Readiness

- Tier 2: ready when no tier 0 or 1 goal is open or parked
  ([readiness](../reconciler.md#readiness)). Ingest runs first because the section that
  killed a statement often states it again under a new id; by the time the retrace runs,
  the replacement exists to point at.
- Locality is the view or node neighborhood ([batching](../reconciler.md#batching)): a
  view's retrace batches with the goals on its members, and the several retraces one
  deletion opened share a session, since they share the cause and the neighborhood.
- The skill follows the target's kind at assembly, and loading the target brings it
  again for a batch of mixed kinds ([skills](../sessions.md#skills)).

## Gate

Nothing on the target points at a dead or missing node, over the store plus what the
session has staged:

- A view: every id in `members`, `excluded`, and `collapse` exists.
- An entity: `parent` exists; every attribute's provenance names live nodes or a quote
  that locates; a `from` list names live nodes.
- A requirement: `entities`, every edge end, `transition.subject`, and `from` name live
  nodes.
- A target deleted in this session passes: nothing is left to point.

`mark_goal_done({goal, justification, evidence?})` is validated against this and
rejected naming the reference still dangling. `evidence` may list one disposition per
dangling reference (`repaired`, `re-derived`, `dropped`, `deleted`) for the journal;
the gate reads the graph, not the list. `done` runs the same gate over every goal in
the batch and the per-mutation gates ([validation gates](../graph.md#validation-gates)):
`update_view` members exist, `delete_view` carries a reason, a re-anchored attribute's
quote locates.

What the gate cannot see, the contract forbids: re-creating the dead node to make a
reference valid, and inventing a replacement the documents do not state. A dead
requirement whose section is gone cannot be re-created anyway (its quote would not
locate); one deleted as a duplicate from a live section could be, and the session must
not. `mark_goal_failed({goal, reason})` is for a target the loss leaves meaningless but
that a human should see before it is deleted: a curated flow whose surviving members
are branches without a step, an instance whose type the documents stopped stating. A
failed goal keeps its record and surfaces on the target; it blocks convergence.

## Hints

Computed by the harness and rendered under the goal block:

- The dead node: id, kind, its last `statement` or title from the journal, deleted in
  `g<N>` with the reason and the mutation, and the `jazyk ripple <target> --back`
  pointer ([`jazyk ripple`](../../frontends/cli.md#jazyk-ripple)).
- Whether the dead id resolves through `redirects.yaml` to a survivor (a merge target)
  or is a tombstone.
- The dangling references on the target, computed: `members[3]`, `collapse`,
  `attributes.tier.provenance`, `from[0]`.
- Candidate replacements: requirements sourced from the same section as the dead one
  whose statements share its content tokens (the pair scorer,
  [pairs](../reconciler.md#pairs)), so a statement re-extracted under a new id is found
  without a search.
- For a view: the member order with the dead member's position marked, and the
  `failure-mode` member right after it, whose branch depended on the dead step.
- For an instance: the type it instantiated, and whether another requirement still
  states the instantiation.
- `load <target>`, `skill <name>` by the target's kind, and the tool per disposition:
  `update_view` `remove_members` or `add_members`, `update_entity`,
  `update_requirement`, `delete_view`.

## What the model sees

The session prompt is [assembled](../sessions.md#the-prompt) like every session's: the
agent contract, the active skills, the project block, the goals block, the loaded set.
The goal block carries the contract paragraph from
[`prompts/retrace.md`](./prompts/retrace.md): load the target in full and find every
place it still points at the dead node; decide per reference between repair (point at
the surviving node that states the same thing, or at a merge's redirect target),
re-derive (a view member replaced by the requirement that carries the step under a new
id, an instance value re-anchored to the sentence that still states it), and delete (a
member dropped, an attribute or edge removed, a view with no flow left deleted); read
the surviving members of a flow as a flow before deciding; keep on an entity only what
a surviving statement supports; never re-create the dead node or invent a replacement;
file a diagnostic when the loss leaves a real gap; delete a target with nothing left to
justify it, with the reason in the justification. Then the change in one line, the gate
in one line, and the hints.

The active skill follows the target: `flow-views` for a use-case, activity, sequence,
communication, timing, or overview view (read the survivors as a flow; a removed step
can leave a branch without its condition; a view with no flow left is deleted);
`structural-views` for a class, object, package, component, composite, deployment, or
state view (drop the member or point it at its redirect target; a collapsed entity that
died leaves `collapse`); `conformance` for an instance (re-link it through the
requirement that states the example, or file `nonconformant-instance`); `abstraction`
for a derived entity or a derived requirement (keep it when the surviving `from` set
still supports it, otherwise move its detail back and delete it); none for an entity
whose only loss is an attribute's anchor.

The initially loaded set for the batch holds:

- The target in full ([policy](../context.md#policy)): a view with its members in
  order, each member's statement and entities, the dead one marked
  `(deleted in g409: duplicate)`; an entity with its attributes and their provenance,
  the dead provenance marked; a derived fact with its `from` list, the dead entry
  marked, and the survivors as stubs.
- The dead node's tombstone line: id, kind, last statement or title, generation,
  reason.
- For an instance, its type as a stub and the requirement that stated the example,
  when it survives.
- The candidate replacements as stubs, each with its statement and section.

E.g.:

```
## Goals
- [g:retrace:view:usecase/holds] mandatory
  [contract paragraph]
  Change: member req:orders-6 deleted in g409 (reason: duplicate), position 3 of 5.
  Gate: nothing on the view points at a dead node.
  Hints: candidate req:orders-11 "Held orders expire after 30 days" (same section,
  tokens hold, expire); req:orders-7 (failure-mode) followed the dead step;
  load view:usecase/holds; skill flow-views

## Loaded (5.6k/24k chars)
- view:usecase/holds   full: 5 members, 1 deleted; participants ent:customer, ent:order-service
- req:orders-11   stub   docs/orders.md#/orders/holds
- req:orders-6    tombstone: deleted in g409 (duplicate) "Held orders expire after 21 days."
skills: flow-views (active); extraction, judgment, structural-views, abstraction, conformance (load_skill)
```

`jazyk preview <goal>` renders the prompt before it is spent
([preview](../sessions.md#preview)).

## Tools

The `retrace` toolset ([toolsets](../tools.md#toolsets)):

- The [read tools](../tools.md#read-tools): `load`, `expand`, `unload`, `graph_status`,
  `search`, `read_section`, `get_entity`, `get_view`, `diagnostics`.
- The [goal tools](../tools.md#goal-tools): `mark_goal_done`, `mark_goal_failed`,
  `load_skill`, `done`.
- The [view tools](../tools.md#view-tools): `upsert_view`, `update_view` (`members`,
  `add_members`, `remove_members`, `exclude`, `collapse`), `delete_view({id, reason})`.
- Entity tools: `upsert_entity`, `update_entity` (an attribute removed or re-anchored,
  a `parent` corrected), `delete_entity({id, reason})`, `merge_entities`.
- Requirement tools: `upsert_requirement` (a value re-anchored to the sentence that
  still states it), `update_requirement` (an edge or an entity reference on a derived
  requirement), `delete_requirement({id, reason})`.
- `report_diagnostic({rule, severity, subjects, message, reasoning, prompt?})`:
  `missing-link` when the documents rely on a step or a part they stopped stating;
  `decision` with a prompt when the repair is a human's choice (drop the flow, or
  restate the step).
- [`report_feedback`](../tools.md#feedback-tool).

No `set_coverage`, no `resolve_diagnostic`, and no anchor tools: the diagnostics that
named the dead node are settled by the store at commit
([journal](../graph.md#journal)), and a section's coverage is the
`reconcile-section` goal's. A default view is never a retrace target and never
deleted; it recomputes. A delete staged here previews the retraces it opens in the
tool reply ([bubbling](../reconciler.md#bubbling)), so the session sees the next hop
of the cascade before it commits it; the `retrace` on a derived fact whose only
justification died ends the cascade by deleting the fact, and
[justification closure](../compilation.md#checks) confirms it after the build.
