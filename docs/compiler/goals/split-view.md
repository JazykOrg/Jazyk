# The split-view goal

Goal: resolve one view over a limit. A class view of thirty entities, a sequence view of
fourteen participants, or an object view of twenty instances is unreadable, and
readability is computed, not taste ([limits](../graph.md#limits)). The model brings the
view within its limit by collapse, by exclusion, or by splitting it into sub-views along
the structure the documents state, and links the sub-views so the coarse view stays true
and the detail stays one click away. Nothing is dropped: every member and every arrow of
the original survives in the parent view, in a sub-view, or lifted into a collapsed
node.

- Kind: `split-view`. Class: GC. Optional past the soft threshold, mandatory past the
  hard one ([escalation](../reconciler.md#escalation)).
- Unit: one view. Id: `g:split-view:<view id>`.
- Ready when no compile goal is open or parked in the view's
  [cone](../reconciler.md#cones) ([readiness](../reconciler.md#readiness)). The view is
  split once, over its final membership for the build, never over a stream of partial
  states.

## Created when

One [change record](../graph.md#change-records) kind derives the goal:
`threshold-crossed` on a view (`via: limits`), written by the limit counts at commit
([derived data](../graph.md#derived-data)) when a count is over the view's threshold.
Five limits of [the registry](../graph.md#the-registry) apply to views:

| limit | soft | hard | counts |
|---|---|---|---|
| `members-per-structural-view` | 20 | 30 | shown members of a class, object, package, component, composite, or deployment view, after `collapse` |
| `edges-per-view` | 40 | 60 | arrows the view draws after lifting and collapse, one per direction-and-type group |
| `members-per-flow-view` | 12 | 20 | members of a use-case, activity, sequence, communication, timing, or overview view |
| `participants-per-sequence-view` | 8 | 12 | the union of the members' initiators and receivers of a sequence or communication view |
| `instances-per-object-view` | 15 | 25 | members of an object view |

A view's own bump (`limits: {<limit>: n}`) is the soft threshold for that view
([per-node bumps](../graph.md#per-node-bumps)). `detail` carries the limit, the count,
the thresholds in force, and for `edges-per-view` the shown pairs with the most arrows.
E.g.:

```yaml
- id: c421-0
  generation: 421
  mutation: 0
  kind: threshold-crossed
  subject: view:class/commerce
  via: limits
  detail: {limit: members-per-structural-view, count: 23, soft: 20, hard: 30}
```

- The record is level-triggered: it stands while the count is over the threshold and
  clears on its own when the count falls back under it by any path (a merge, an
  exclusion, a collapse, a delete). Resolving the goal clears it, and the next commit
  writes it again when the count is still over.
- Default views cross limits like any view: a scope of forty entities puts its default
  class view over the limit, and the split makes the view curated
  ([default views](../model/view.md#default-views)).
- The view renders meanwhile: a structural view with its largest subtrees auto-collapsed,
  a flow or object view with every member, both with a visible note in the title
  ([over-limit views](../diagrams.md#over-limit-views)). A violation never truncates a
  rendering silently.

### Escalation and dismissal

- Soft: the goal derives optional. It advises, rides in the verdict as `optional`, and
  the build converges around it.
- Hard: the same goal is mandatory. `mandatory` is recomputed at every derivation from
  the current count, so a count that drops back under the hard value de-escalates the
  goal. The build cannot report `converged` while a mandatory `split-view` goal is open
  or failed ([convergence](../compilation.md#convergence)).
- Dismissal is a graph write, never goal state. A human raises the view's own limit with
  `bump_limit`, recorded with decree provenance in the journal (`kind: decree`,
  [journal](../graph.md#journal)). The goal derives only when the count crosses `n`, and
  escalates when it crosses `n` plus the registry's distance between soft and hard. No
  session can bump: the tool is a human path (the [GUI board](../../frontends/gui.md#board),
  chat), and a session that finds the count honest fails the goal recommending the bump,
  so the recommendation surfaces on the view.
- Nothing tunes limits in `jazyk.toml`. The registry is built into the binary, and bumps
  are per node.

The trace prints the burst as it starts:
`gc burst: split-view view:class/commerce (23 > 20)`
([compile and garbage collection](../compilation.md#compile-and-garbage-collection)).

### Batching

The twin views of one flow cross together: a use-case view and the sequence view derived
from the same cluster share members and a title
([default views](../model/view.md#default-views)), so their goals batch together and
split along the same phases. A structural view's goal batches with the goals on its
members ([batching](../reconciler.md#batching)); an
[`abstract-entity`](./abstract-entity.md) goal on a member often resolves both.

## Gate

Under the limit, nothing dropped, sub-views linked. At `mark_goal_done` the harness
recomputes the view's counts over the staged state and checks:

- The crossed count is within the limit's soft value, or the view's bump. A split that
  only de-escalates (from over hard to over soft) commits its work at `done` but does
  not resolve the goal: the goal stays open, optional, and the session continues or
  fails it with the reason.
- No member lost: every node the view listed before the session is still in its
  `members`, or hidden under a member the view collapses (structural kinds), or a member
  of a sub-view staged in this session, or listed in the view's `excluded` with a note
  naming the view that holds it.
- No arrow dropped: every relationship contribution among the original members is drawn
  in the view after lifting (direct, or inside a lifted arrow on a collapsed member), or
  drawn in a staged sub-view whose members hold both ends
  ([lifting and collapse](../diagrams.md#lifting-and-collapse)).
- Every staged sub-view is linked to the view ([linking](#linking-sub-views)), and every
  sub-view is itself within its limits.

### Linking sub-views

Views nest. A sub-view is linked to its parent view in one of three ways, each one the
harness can check:

- Through `collapse`, the structural way: the parent collapses an entity, and the
  sub-view is the view of the same kind whose `query.parent` is that entity or whose
  members are all its descendants. The rendered collapsed node carries a link to the
  sub-view's picture ([lifting and collapse](../diagrams.md#lifting-and-collapse)).
- Through an `overview`, the flow way: the flow is split by phase into sub-views (a
  sequence view per phase, and a use-case view per phase for the use-case twin), and an
  `overview` view with the parent's title lists one member per phase in phase order. The
  overview emitter references the sequence view containing each member
  ([view kinds](../model/view.md#kinds)). The parent view keeps one member per phase, the
  phase's first step, so it reads as the coarse flow and stays within the limit.
- Through `excluded`, for kinds with neither containment nor flow (object, deployment):
  the members moved to a sub-view are excluded from the parent with a note naming the
  sub-view id.

A sub-view is `upsert_view` with the parent's kind (or `overview`), a title naming what
it details (`Commerce: Order Service`, `Checkout: Payment`), and a `query` or a member
list. It carries derived provenance from the parent view and its members
([view fields](../model/view.md#fields)); views are not ratified, since a view has no
sentence to gain.

### What the gate does not check

Which structure the split follows. The skill carries the rule: split along what the
documents state (a child container, a scope, a stereotype, a heading, a "then", a
handoff to another initiator), never along a structure they do not state, and never a
flow the documents present as one. When the members have no containment or stated break
to split along, the pressure is the container's, not the view's: the goal fails naming
the entity, and [`abstract-entity`](./abstract-entity.md) on it is the resolution. When
the count is honest and every split would invent structure, the goal fails recommending
the bump. A failed mandatory goal blocks convergence and surfaces on the view
([parked and failed](../reconciler.md#parked-and-failed)).

At `done`, the per-mutation gates hold ([validation gates](../graph.md#validation-gates)):
view members exist and match the kind, `query.parent` resolves, `delete_view` needs a
reason and is rejected on a default view. A clean batch commits
([commit](../sessions.md#commit)). The commit recomputes the counts of every touched
view, renders them ([diagrams](../diagrams.md)), and previews the goals the split opens:
a sub-view over its own limit is a `split-view` goal of its own
([bubbling](../reconciler.md#bubbling)).

## Hints

The hint computer emits, per goal:

- `load <view>`: the view in full.
- `<count> > <limit> (<limit name>, soft <s>, hard <h>)`: the change.
- `collapse <ent> (<n> descendants hidden, <m> arrows lifted)`: for a structural view,
  the members with the largest subtrees, best first, with the effect on both counts.
- `break after <req> (<sentence>)`: for a flow view, the members at which the documents
  break the flow (a section boundary, a "then", a change of initiator), in order.
- `participants: <n> (<names>)`: for a sequence or communication view.
- `linked <view>`: the sub-views already linked to this view.
- `skill structural-views` or `skill flow-views`, per kind.
- `update_view`, `upsert_view`: the tools that resolve the kind.

## What the model sees

The goal block in the [session prompt](../sessions.md#the-prompt) carries the contract
paragraph from [`./prompts/split-view.md`](./prompts/split-view.md), the change in one
line, the gate in one line, and the hints. E.g.:

```text
- [g:split-view:view:class/commerce] optional
  This view is over a limit. Load it with its members and their relationships. Resolve
  in order, with reasoning: collapse the entities whose internals the view does not
  need (each collapsed node links to the sub-view detailing it; create it when none
  exists), exclude members that belong to another view with notes, then split along
  the structure the documents state, the parent collapsing what each sub-view details.
  Every member and every arrow survives somewhere. Never split along a structure the
  documents do not state; when there is none, fail naming the entity.
  Change: 23 members > 20 (members-per-structural-view, soft 20, hard 30) (g421).
  Gate: under the limit through linked sub-views or collapsed members, no edge dropped.
  Hints: load view:class/commerce; collapse ent:order-service (9 descendants hidden,
  14 arrows lifted); skill structural-views.
```

The skill follows the view's kind and is active from the first round
([skills](../sessions.md#skills)): [structural-views](../skills/structural-views.md)
(membership per kind, collapse and lifting, the resolution order) or
[flow-views](../skills/flow-views.md) (phases, the overview, participants).

The initially [loaded set](../context.md#the-loaded-set) holds, per goal:

- The view in full ([view fields](../model/view.md#fields)): `kind`, `title`, `query`,
  `members` in order, `excluded`, `collapse`, `limits`.
- For a structural view: each member as a stub with its children count, and the
  relationships among the members one line each with the arrow count after lifting
  ([relationship](../model/relationship.md#rendering)). Over-budget lists become handles
  ([policy](../context.md#policy)).
- For a flow view: each member's statement on one line with its initiator and receiver,
  the section boundaries marked between members, and the participant set.
- The sub-views already linked to the view, as stubs.

### Resolving in order

- Collapse first. In a structural view, `update_view` `collapse` on the entities whose
  internals the view does not need: a service in a system view, a chapter in a book
  view, a department in a company view. The hidden subtree's relationships lift to the
  collapsed node, and the node links to the sub-view detailing it, created with
  `upsert_view` when none exists.
- Then exclude members that belong to another view, with notes naming it.
- Then split along the structure the documents state. Structural: one sub-view per child
  container, per scope, or per stereotype (the actors and interfaces in one component
  view, the internals in another), the parent collapsing the entity each sub-view
  details. Flow: one sequence view per phase along the documents' own breaks and an
  overview referencing them; a sequence view over its participant limit splits by
  phase, never by dropping the members whose participants are inconvenient. Object: one
  sub-view per worked example or per type, the parent excluding what moved.
- Never hide an entity to hide an arrow. An arrow that looks wrong is a statement that
  looks wrong: read the contributing requirements (the arrow's justification) and leave
  the finding to the statement's judgment goals.

## Tools

The `split-view` [toolset](../tools.md#toolsets): the
[read tools](../tools.md#read-tools), the [goal tools](../tools.md#goal-tools), the
[view tools](../tools.md#view-tools) (`upsert_view`, `update_view`, `delete_view`), and
[`report_feedback`](../tools.md#feedback-tool). No entity or requirement tools: a split
never edits a fact, and a pressure that is the entity's fails toward `abstract-entity`.
`delete_view` serves one case: a curated sub-view this session created and left empty.
See [write tools](../tools.md#write-tools).
