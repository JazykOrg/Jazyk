# Authored diagrams

A view is an explicitly authored diagram node. Only a model session or a human
creates it. Relationships and state machines remain computed facts. No default
view is created at commit. Rendering files are disposable output.

## Fields

- `kind` is one of the catalog below. Kind is immutable.
- `title` and kind form the natural key. IDs remain stable when a title changes.
- `purpose` states the reader's question. `description` adds optional context.
- `area` identifies `{target, extent, document?}`. The target is an entity or
  scope root. Extent is `area` or `selection`.
- `members` is ordered. `query` expands structural membership. `excluded` names
  omitted members with reasons. `collapse` names shown ancestors hiding detail.
- `relationships` selects canonical directional `{a, b, type}` groups before
  lifting. Mode is `all` or `selected`; excluded selectors require reasons.
- `labels`, `notes`, `groups`, `layout`, `fragments`, and `links` carry authored
  presentation intent. They add no semantic facts. See [diagrams](../diagrams.md).
- `provenance` is mandatory. Model choices use derived sources and reasoning;
  human choices use decree; source diagram blocks use verbatim quote provenance.
- `limits` retains per-view human limit bumps.
- `children` is computed navigation, never authored. A member can have children
  with zero, one, or multiple diagrams.

## Kinds

| kind | members | what the emitter draws | limits |
| --- | --- | --- | --- |
| `class` | entities | one class per member with its typed attributes; one arrow per relationship group among the members, lifted | structural, edges |
| `object` | entities (instances) | one object per member, named `instance : Type` through its `instantiation` group, with its attribute values; links from every relationship group among the members except `instantiation` | `instances-per-object-view`, edges |
| `package` | entities (the packages) | one package per member holding its children as classes; relationships between packages lifted to the packages | structural, edges |
| `component` | entities | members as components; entities the members realize as interfaces (the lollipop); `dependency` toward an interface as the socket; members labeled `actor` as actors | structural, edges |
| `composite` | one entity (the boundary) | the first member as the boundary, its children as parts, connectors from relationships among the parts and crossing the boundary | structural, edges |
| `deployment` | entities | each member as an artifact; every attribute with a `value` on a member is a placement, the artifact inside a node labeled `<value> <name>` (`region = EU` → `node "EU region"`), members sharing a placement share the node; a member without valued attributes is a bare artifact; nothing synthesizes topology | structural, edges |
| `use-case` | requirements (ordered) | one use case for the view; the members' actors | flow |
| `activity` | requirements (ordered) | one action per member in order; a member whose transition leaves a state another member's transition also leaves draws as a branch | flow |
| `state` | one entity | the subject's derived [state machine](./state-machine.md) | states (on the subject) |
| `sequence` | requirements (ordered) | one message per member from its initiator to its receiver; the participants are the union | flow, participants |
| `communication` | requirements (ordered) | the same messages, numbered in order, the participants as boxes | flow, participants |
| `timing` | one entity and requirements | the subject's state machine as a lane, the members' `quality` measures as the timeline | flow |
| `overview` | requirements (ordered) | an activity frame whose steps reference the sequence views containing the members | flow |

Terms the flow kinds share:

- A member's message follows one of its edges. The initiator is `a`. The receiver is `b`, resolved through
  `realization` when `b` is an «interface»: the one entity realizing it is the receiver.
  Several realizers draw `provider-ambiguous`. A member with no edges is a self-message
  on its first listed entity. The message follows the statement's
  subject: the first edge whose `a` is the requirement's first listed entity (the
  entity the statement is about), then dependencies, then the other edges, so "When a
  Customer submits an Order, the Shop sends a confirmation email" with `entities`
  `[shop, customer, order]` draws the Shop's message, not the Customer's trigger. In a
  [level's](../concepts/levels.md#levels) flow view the ends lift to the level's members, and the
  message follows the first such edge whose lifted ends differ; when every edge
  collapses into one member and an `actor` among the statement's entities lifts
  elsewhere, the actor initiates the message toward that member (the Operator
  running the Admin CLI draws as Operator to Orderly at the top level, not Orderly to
  itself); otherwise the self-message stays.
- The actors of a flow are the members' entities labeled `actor`. Where no member's entity
  carries that label, the use case draws no actor: a flow among the subject's own parts
  has no outside initiator, and drawing the first component as a stick figure (a
  Shipment "acting" on Inventory) misreads the picture.

The limits: `structural` is `members-per-structural-view` (20 soft, 30 hard), `edges` is
`edges-per-view` (40, 60), `flow` is `members-per-flow-view` (12, 20), `participants` is
`participants-per-sequence-view` (8, 12), `instances-per-object-view` is (15, 25), all
resolved by `split-view`. `states` is `states-per-state-machine` (12, 20), resolved by
`abstract-entity` on the subject. See [limits](../graph.md#limits). Notation and what each
emitter reads in full are in [the emitters](../diagrams.md#the-emitters).

## Membership

Queries expand only an existing authored node. Query matches ask curate-view for
judgment. Explicit membership persists until edited, including references that
need retrace after deletion. Hidden endpoints lift to shown ancestors. Flow
lifting uses the view's explicit area rather than an assumed default view ID.

Every diagram is editable and deletable through view tools. Deleting a diagram
never deletes its semantic facts. Missing coverage may reopen diagram review.

## Identity

- The ID is `view:<kind>/<slug>`, with hyphens removed from the kind segment.
  The slug comes from the title once. Retitling keeps the ID.
- The natural key is `kind` plus normalized `title`. A retried create updates the
  existing authored node.
- `delete_view` requires a reason. All diagrams can be deleted. See
  [view tools](../tools.md#view-tools).
