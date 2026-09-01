# View

A view is the stored half of a diagram: which nodes one diagram includes, never how it
looks. A view renders its members plus every relationship among them, direct or lifted.
Geometry, layout, and styling are never stored. The rendering is build output, recomputed
on every commit under `<out>/diagrams/`. See [diagrams](../diagrams.md).

Views are authored nodes with one twist: default views derive on every commit, so nothing
must be curated to get diagrams. Curated views come from the
[`curate-view`](../goals/curate-view.md) and [`split-view`](../goals/split-view.md) goals
or from humans.

## Fields

- `kind`: one of the [catalog](#kinds).
- `title`: the human title, half of the natural key.
- `members`: ordered node ids. Entities for structural kinds, requirements for flow kinds,
  mixed where a kind wants both. Order is the flow order.
- `excluded`: `[{id, note}]`, nodes a query or a default derivation would include, kept
  out with a reason.
- `query`: `{scope?, parent?, stereotype?, depth?}`, membership by rule instead of list.
  `scope` matches the entities of a scope, `parent` the descendants of an entity,
  `stereotype` the entities with that label, `depth` how far below `parent` (or below the
  scope's roots) the match descends. A match excludes instances (entities that are `a` of
  an `instantiation` group): instances live in `object` views. Query matches join
  `members` at every commit. A query names at least one of `scope`, `parent`, or
  `stereotype`: an all-empty query counts as absent. Only structural kinds take one:
  a flow view's members are requirements, so an entity-matching query on one is
  refused.
- `collapse`: entity ids shown as one node despite their children. The hidden subtree's
  relationships lift to the collapsed node.
- `provenance`: `derived` for default views and for views a session curates (`from` the
  members and the rule or the change, `reasoning` why), `decree` for views a human
  creates. Views are not ratified: a view has no sentence to gain, and its justification
  closes through its members. See [provenance](../model.md#provenance).
- `default`: `true` on a view the recompute owns, absent otherwise. Any mutation on the
  view clears it. See [default views](#default-views).
- `limits`: per-node bumps above the built-in limits. See
  [shared fields](../model.md#shared-fields).
- `created` and `updated`: generation markers.

E.g.:

```yaml
view:sequence/checkout:
  kind: sequence         # any renderable kind from the catalog
  title: Checkout
  members: [req:shop-1, req:shop-3, req:shop-7, req:shop-8]   # ordered; the flow
  excluded: [{id: req:shop-9, note: "example, not flow"}]
  provenance: {derived: {from: [...], reasoning: default per flow cluster}}

view:class/commerce:
  kind: class
  title: Commerce
  query: {scope: commerce, depth: 1}   # membership by rule instead of list
  collapse: [ent:order]                # show as one node despite children
```

A default view as the store writes it:

```yaml
view:usecase/order-shop:
  kind: use-case
  title: "Order: Shop"
  members: [req:shop-7, req:shop-8]
  provenance: {derived: {from: [ent:order, req:shop-7, req:shop-8],
                         reasoning: "default view: use-case per flow cluster"}}
  default: true
  created: g3
  updated: g12
```

## Kinds

| kind | members | what the emitter draws | limits |
| --- | --- | --- | --- |
| `class` | entities | one class per member with its typed attributes; one arrow per relationship group among the members, lifted | structural, edges |
| `object` | entities (instances) | one object per member, named `instance : Type` through its `instantiation` group, with its attribute values; links from `association` groups among the members | `instances-per-object-view`, edges |
| `package` | entities (the packages) | one package per member holding its children as classes; relationships between packages lifted to the packages | structural, edges |
| `component` | entities | members as components; entities the members realize as interfaces (the lollipop); `dependency` toward an interface as the socket; members labeled `actor` as actors | structural, edges |
| `composite` | one entity | the member as the boundary, its children as parts, connectors from relationships among the parts and crossing the boundary | structural, edges |
| `deployment` | entities | each member as an artifact; every attribute with a `value` on a member is a placement, the artifact inside a node labeled `<value> <name>` (`region = EU` → `node "EU region"`), members sharing a placement share the node; a member without valued attributes is a bare artifact; nothing synthesizes topology | structural, edges |
| `use-case` | requirements (ordered) | one use case for the view; the members' actors | flow |
| `activity` | requirements (ordered) | one action per member in order; `failure-mode` members as branches | flow |
| `state` | one entity | the subject's derived [state machine](./state-machine.md) | states (on the subject) |
| `sequence` | requirements (ordered) | one message per member from its initiator to its receiver; the participants are the union | flow, participants |
| `communication` | requirements (ordered) | the same messages, numbered in order, the participants as boxes | flow, participants |
| `timing` | one entity and requirements | the subject's state machine as a lane, the members' `quality` measures as the timeline | flow |
| `overview` | requirements (ordered) | an activity frame whose steps reference the sequence views containing the members | flow |

Terms the flow kinds share:

- A member's message follows its first `dependency` edge, or its first edge when it has
  no dependency. The initiator is `a`. The receiver is `b`, resolved through
  `realization` when `b` is an «interface»: the one entity realizing it is the receiver.
  Several realizers draw `provider-ambiguous`. A member with no edges is a self-message
  on its first listed entity.
- The actors of a flow are the members' entities labeled `actor`. Where no member's entity
  carries that label, the actors are the initiators that never receive.

The limits: `structural` is `members-per-structural-view` (20 soft, 30 hard), `edges` is
`edges-per-view` (40, 60), `flow` is `members-per-flow-view` (12, 20), `participants` is
`participants-per-sequence-view` (8, 12), `instances-per-object-view` is (15, 25), all
resolved by `split-view`. `states` is `states-per-state-machine` (12, 20), resolved by
`abstract-entity` on the subject. See [limits](../graph.md#limits). Notation and what each
emitter reads in full are in [the emitters](../diagrams.md#the-emitters).

## Default views

Default views derive on every commit from what the graph contains. No configuration
enables them: a project with no transition has no state view, a project with no instance
has no object view. Six kinds derive, each with a stable id:

| kind | id | one per | members |
| --- | --- | --- | --- |
| `class` | `view:class/<scope>` | scope | the scope's entities (`query: {scope}`, which excludes instances) |
| `component` | `view:component/<system-slug>` | system: a containment root with at least one child | the system's children, the entities its descendants realize, and the entities outside the system that depend on those |
| `use-case` | `view:usecase/<cluster-slug>` | flow cluster | the cluster's requirements in document order |
| `sequence` | `view:sequence/<cluster-slug>` | flow cluster | the cluster's requirements that carry an edge, in document order |
| `state` | `view:state/<entity-slug>` | state machine | the subject |
| `object` | `view:object/<type-slug>` | type: an entity that is `b` of an `instantiation` group | the type's instances |

The title of a default is deterministic: the scope's name title-cased for `class`, the
system's `name` for `component`, `<actor name>: <document title>` for `use-case` and
`sequence` (the document title is its root section's title), the subject's `name` for
`state`, the type's `name` for `object`. A `use-case` view and the `sequence` view derived
from the same cluster share a title, so `view:usecase/order-shop` and
`view:sequence/order-shop` describe one flow. The `provenance` of a default is
`{derived: {from: [the rule's subject and the members], reasoning: "default view: <rule>"}}`.

`package`, `composite`, `deployment`, `activity`, `communication`, `timing`, and
`overview` views have no default: `curate-view` sessions create them (a `flow-unplaced`
change may call for a new flow, or for an `activity` twin of a `use-case` view),
`split-view` sessions create sub-views, and humans create them by decree.

Flow clusters are deterministic:

- Every requirement with a `behavior` facet joins a cluster. A `failure-mode` requirement
  joins too, so the branches it gives are in the flow. See
  [facets](./requirement.md#facets).
- The cluster key is the requirement's actor and its document. The actor is the entity
  labeled `actor` among the requirement's `entities` (the first such, in listed order), or
  the first listed entity when none is an actor. The cluster slug is
  `<actor-slug>-<doc-stem>`.
- Members are in document order. A cluster of fewer than two members derives no flow
  view; a lone behavior requirement is the flow placement check's finding
  (`unplaced-behavior`), which feeds `curate-view`.

Flow order is document order of the member requirements: document link level, then path,
then section order, then id. An explicit member list on a curated view overrides it.
Reordering judgment belongs to `curate-view`.

Recompute rules keep curation and derivation from fighting:

- A default view is keyed by its id. On every commit, for each rule instance: the id
  absent creates the view with `default: true`; the id present with `default: true`
  rewrites `title` and `members` from the rule; the id present without `default` is left
  alone.
- A default whose rule stops holding (a scope emptied, a cluster below two members, a
  machine gone, a type without instances) is removed at the same commit. A curated view
  is never removed by the recompute.
- Any mutation on a default view clears `default`: `update_view` (any field),
  `edit_fact`, a decree, a limit bump. From then on the view is curated: its `members`
  are the session's or the human's, its `query` (a default `class` view carries one) keeps
  recomputing membership at every commit, and its `excluded` and `collapse` stand. A
  session's `upsert_view` with a default's kind and title lands on the default and clears
  `default` the same way.
- `delete_view` on a default view is rejected: the next commit would derive it again.
  Exclude members, collapse subtrees, or curate it first (`update_view` clears `default`),
  then delete.

## Membership

- A view renders its members plus every relationship among them, direct or
  [lifted](../diagrams.md#lifting-and-collapse). A `collapse`d member hides its subtree
  and the subtree's relationships lift to it. A relationship touching a member's hidden
  descendant lifts to the member.
- Limits are satisfied by membership, `collapse`, and sub-views, never by silently
  omitting an arrow. Past a hard threshold the view renders with the largest subtrees
  auto-collapsed and a visible note until `split-view` resolves it. See
  [over-limit views](../diagrams.md#over-limit-views).
- Views nest. A collapsed node links to the sub-view detailing it: the view of the same
  kind whose `query.parent` is the collapsed entity or whose members are its children.
  `split-view` creates such views and links them.
- Query membership recomputes at every commit. On a default view a new match joins
  `members` silently: the harness owns the view. On a curated view a new match joins
  `members` and is the `query-match` change, which opens the optional
  [`curate-view` goal](../goals/curate-view.md): the session keeps the member or excludes
  it with a note.
- The flow placement checks feed `curate-view` too: `unplaced-behavior` (the
  `flow-unplaced` change) and `unrepresented-failure-mode`. See
  [checks](../compilation.md#checks).
- A member of a curated view that dies is the `view-member-gone` change and opens
  [`retrace`](../goals/retrace.md) on the view. Default views recompute instead.

## Identity

- The id is `view:<kind>/<slug>`, the kind segment being the catalog kind with its hyphens
  removed and the slug from the title at creation (`use-case` → `usecase`). Default views
  take the stable ids of [their rule](#default-views). An id is minted once and never
  changes. A retitle through `update_view` keeps the id, so the slug can go stale. See
  [identifiers](../model.md#identifiers).
- The natural key is `kind` plus `title`. `upsert_view` keys on it, so a retried create
  lands on the existing view, and a default derived under a title a session already used
  lands on that session's view.
- `delete_view` needs a `reason` and is rejected on a default view. See
  [view tools](../tools.md#view-tools).
