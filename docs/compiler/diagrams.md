# Diagrams

A diagram has three layers. The graph stores the first two:

- The semantic facts: [entities](./model/entity.md),
  [requirements](./model/requirement.md), and what derives from them
  ([relationships](./model/relationship.md), [state machines](./model/state-machine.md)).
  The only editable truth.
- The [view](./model/view.md): which facts one diagram includes, never how it looks.
- The rendering: build output under `<out>/diagrams/`, diffable, never hand-edited,
  never read back. Deleting it loses nothing.

There are no diagram elements in the graph. Geometry, layout, and styling are stored
nowhere. A rendering cannot drift from the graph because every build recomputes it from
the graph. The catalog of view kinds is in [view kinds](./model/view.md#kinds); the
renderer covers every kind in it.

## Rendering

view → `.puml` → `.svg` (→ `.png` on demand)

- On every commit the renderer emits one PlantUML file per view and renders it to SVG
  beside it. Emission is deterministic: the same store snapshot and the same view produce
  byte-identical `.puml`.
- The renderer is skipped for a view whose emitted `.puml` equals the file already on
  disk and whose `.svg` exists. Most commits touch few views, so most renders are
  skipped.
- A deleted view loses its files at the commit that deleted it.
- `.png` is rasterized from the `.svg` only when a surface asks for one, and is written
  beside it. [Docsgen](../consumers/docsgen.md) embeds `.svg`. The
  [LSP](../frontends/lsp.md) hover embeds `.svg` through a `file://` link. The
  [GUI](../frontends/gui.md) draws its projections from the graph directly and reads
  none of these files.
- Rendering is part of the build. There is no degraded mode: the renderer is always
  present. An error from the renderer is a harness defect, not a documentation finding:
  the `.puml` stays on disk, the stale `.svg` is removed, the trace carries the error as
  a `note` event, and no diagnostic is filed.

## The emitters

One emitter per view kind. Each is a pure function of the store snapshot and the view:
no LLM, no state, no reads outside the snapshot. Rules shared by every emitter:

- The file opens with `@startuml` and closes with `@enduml`.
- Display names are entity `name`s. Where PlantUML needs an identifier beside a quoted
  name, the emitter derives an alias from the entity id, stable across builds.
- A `stereotype` renders as `<<stereotype>>` on the element that carries it.
- A `title` line is emitted only when the emitter has something to say beyond the view's
  title: the [over-limit](#over-limit-views) note, or a timing view's governing
  requirement.
- A relationship among the drawn members renders as one arrow per direction-and-type
  group, cardinality at the `b` end. Notation, `a` acting on `b`:

  | type | arrow |
  |---|---|
  | `generalization` | `a --\|> b` |
  | `realization` | `a ..\|> b` (lollipop `a -- b` toward an «interface» in component views) |
  | `composition` | `a *-- b` |
  | `aggregation` | `a o-- b` |
  | `association` | `a -- b` |
  | `dependency` | `a ..> b` (socket `a --( b` toward an «interface» in component views) |
  | `instantiation` | never an arrow; it names the instance's type in the object view |

- In flow views a member requirement's label is its `statement` followed by its id in
  parentheses.
- Members are the view's explicit list plus its query's matches at the snapshot, minus
  `excluded` ([membership](./model/view.md#membership)). A listed member the graph no
  longer holds is skipped; the [`retrace`](./goals/retrace.md) goal repairs the view.
- Hidden descendants and collapsed members follow [lifting and collapse](#lifting-and-collapse).

The reference output is one small graph drawn in every kind: a «system» `shop`
(attribute `region = EU`) containing the «service» entities `order-service` and
`inventory-service`; `order-service` contains the «interface» `checkout-api` and the
concepts `shopping-cart`, `order` (attributes `total`, `currency`), and `order-item`;
`inventory-service` contains the «interface» `stock-api`; an «actor» `customer`
(attribute `tier : string`); the instances `ana` (`tier = gold`) and `anas-cart`. Its
requirements state that the customer submits the cart through the checkout API
(`req:shop-1`), that the order service provides the checkout API (`req:shop-2`) and
reserves stock through the stock API (`req:shop-3`), that the inventory service provides
the stock API (`req:shop-4`), that a cart holds one or more order items (`req:shop-6`,
composition `1..*`), and that an order moves from `placed` to `paid` on
`payment succeeds` (`req:shop-7`) and to `held` on `payment declined` (`req:shop-8`).
Unit tests build this graph and compare every emitter's output to the reference blocks
after normalizing whitespace, aliases, and free-text labels (the reference abbreviates
step and message labels by hand; the emitters print statements). The package emitter's
output differs from its reference block only in never emitting an empty body.

The kinds, with what each reads from the graph:

- `class`: the member entities, each `attributes` entry as `name : type` (the type when
  stated), the stereotype, and every relationship among the members with type and
  cardinality. The default is one per scope
  ([default views](./model/view.md#default-views)). E.g.:

  ```
  @startuml
  class Customer <<actor>> {
    tier : string
  }
  class "Shopping Cart" as Cart
  class Order {
    total
    currency
  }
  class "Order Item" as Item
  Customer -- Cart
  Cart *-- "1..*" Item
  @enduml
  ```

- `object`: the member instances. An instance is an entity with an `instantiation` edge
  toward its type (`a` the instance, `b` the type). Each renders as
  `object "<name> : <Type>"` with its attribute values as `name = value`; links come
  from the relationships among the instances. The default is one per type, holding the
  type's instances.
- `package`: the member entities that contain other members render as packages holding
  those children as classes; a member with no shown children renders as the one-line
  form `package "X" as X`, never an empty body. Arrows are the lifted cross-package
  relationships only; relationships among the classes inside one package belong to the
  class view. Grouping follows containment (`parent`) or, for a view with a `scope`
  query, the scope.
- `component`: the member entities by stereotype: «actor» as `actor`, «interface» as
  `interface`, everything else as `component`. A `realization` toward an interface draws
  the lollipop; a `dependency` from a component toward an interface draws the socket; a
  dependency from an actor draws a dashed arrow. The default is one per system (a
  containment root with at least one child), over its subtree. E.g.:

  ```
  @startuml
  actor Customer
  component "Order Service" as OS
  component "Inventory Service" as IS
  interface "checkout API" as C
  interface "stock API" as S
  Customer ..> C
  OS -- C
  IS -- S
  OS --( S : use
  @enduml
  ```

- `composite`: one member entity as the boundary, its children among the members as
  parts (`[Part]`) inside `component "Name" { }`, connectors from the relationships among
  the parts and from parts to members outside the boundary.
- `deployment`: the member entities as `artifact`s. Every attribute with a `value` on a
  member is a placement: the artifact sits inside a `node` labeled `<value> <name>`
  (`region = EU` → `node "EU region"`), and members sharing a placement share the node.
  A member without valued attributes is a bare artifact. Nothing synthesizes topology:
  the picture holds only what prose stated. There is no default deployment view;
  membership is curated, so what counts as a placement is the curator's choice of
  members.
- `use-case`: the view itself is the `usecase`, titled by the view; the actors are the
  «actor» entities among the members' `entities`, each linked `Actor -- UseCase`. The
  default is one per flow cluster.
- `activity`: the same use case view. Each member in order is an action. A run of
  consecutive members whose `transition` fields share `subject` and `from` renders as one
  decision: the condition is the first member's `trigger` with `?`, the first member's
  action on the `yes` branch, the rest on the `no` branch. `start` and `stop` frame the
  flow.
- `state`: the derived state machine of the view's subject entity: `[*] -->` the initial
  state, one `from --> to : trigger [guard]` line per transition. The default is one per
  entity that has a machine ([derivation](./model/state-machine.md#derivation)). E.g.:

  ```
  @startuml
  [*] --> placed
  placed --> paid : payment succeeds
  placed --> held : payment declined
  @enduml
  ```

- `sequence`: the ordered member requirements as messages. A member's message follows
  its first `dependency` edge (its first edge when it has no dependency): the sender is
  `a`, the receiver is `b`, and a receiver that is an «interface» resolves to its
  provider (the entity with a `realization` toward it). A member with no edges is a
  self-message on its first entity. «actor» participants render as `actor`. The default
  is one per flow cluster, over the members that carry a message edge.
- `communication`: the same view in its second layout: participants as `actor` or
  `rectangle`, messages numbered `1.`, `2.`, in member order.
- `timing`: the member requirements that carry a `quality` facet with a `measure`, and
  the member entities that have a derived machine. The `title` is the first such
  requirement's statement and id. Each entity is a `robust` lane: `@0` its initial state,
  `@<measure>` the state after the machine's first transition out of the initial state.
  The lane shows only what the measure governs. A timing view is always curated; over a
  subject with no time measure the lane draws with no marks.
- `overview`: the use case view as an activity frame. Each step is a `:ref:` to the
  sequence view whose members are all among this view's members, in order of first
  member; a step with no such sequence view renders as its activity action.

## Lifting and collapse

A view draws its members plus every relationship among them, direct or lifted. Nothing
else: a relationship to an entity outside the view with no shown ancestor is not drawn.

A member's descendants (through `parent`) are hidden when the view does not list them, or
when the member is in the view's `collapse` list. Lifting keeps the coarse picture true
without drawing every leaf:

- Every relationship touching a hidden descendant lifts to the nearest shown ancestor.
- Contributions landing on the same shown pair collapse to one arrow per direction and
  type. When several types land on the same shown pair in the same direction, the arrow
  shows the strongest type ([ranking](./model/relationship.md#rendering): generalization,
  realization, composition, aggregation, association, dependency) and carries the count of
  concrete edges beneath it as its label. A count of one draws no label. `instantiation`
  never promotes with the others and keeps its own group.
- An edge whose two ends lift to the same shown node is not drawn.
- Lifting stores nothing. It is render-time aggregation over `parent` chains, so it
  cannot drift. The lifted arrow's justification is the set of concrete edges beneath it:
  the GUI inspector lists them on click, each walking to its requirement and quote.
- Views nest. A collapsed node links to the sub-view detailing it: the view of the same
  kind whose `query.parent` is the node, or whose members are all children of the node.
  The rendered node carries a PlantUML link to that sub-view's rendering
  (`[[../<kind>/<slug>.svg]]`).

E.g.: `ent:a` contains `ent:a1`, `ent:b` contains `ent:b1`, and a requirement declares
`{a: ent:a1, b: ent:b1, type: dependency}`. A view showing only `a` and `b` draws one
arrow, and clicking it lists the one concrete edge:

```
A ..> B
```

## Over-limit views

The [limits registry](./graph.md#limits) bounds views: `members-per-structural-view`,
`edges-per-view`, `members-per-flow-view`, `participants-per-sequence-view`,
`instances-per-object-view`. Crossing a limit's soft value derives the
[`split-view`](./goals/split-view.md) goal as optional; crossing the hard value makes it
mandatory. A view is over a limit, for rendering, when its drawn count (the members and
edges left after the authored collapse) exceeds the hard value, or the hard threshold
its own bump sets (`limits: {<limit>: n}` on the view, decree provenance,
[per-node bumps](./graph.md#per-node-bumps)). From there the renderer intervenes until
the goal resolves. The goal counts the other way: `split-view` derives from the listed
members ([limits](./graph.md#limits)), so a well-collapsed view can render cleanly while
the goal stands. The renderer never truncates silently:

- A structural view over a limit renders with auto-collapse: the renderer collapses the
  shown member with the most hidden descendants, then the next, until the view is within
  the limit or no subtree is left to collapse. Auto-collapse is render-time only. The
  view's `collapse` field stays as authored; the durable answer belongs to `split-view`.
- A view whose members have no subtrees to collapse (a flow view, an object view) renders
  every member.
- Both cases mark the picture. The `title` line carries the view's title with the suffix
  `(collapsed: n subtrees over limit)`, or `(over limit: n members)` when nothing could
  collapse.

## Output layout

```
<out>/diagrams/<kind>/<slug>.puml    the emitted PlantUML
<out>/diagrams/<kind>/<slug>.svg     the rendering
<out>/diagrams/<kind>/<slug>.png     on request only
```

- The path is the view id with the `view:` prefix removed: `view:usecase/checkout`
  renders to `diagrams/usecase/checkout.puml`. The `<kind>` directory is the id's kind
  segment as [identifiers](./model/view.md#identity) spell it.
- The directory mirrors the set of views. A commit that deletes a view deletes its
  files. A `.png` is removed whenever its `.puml` changes and is rasterized again on the
  next request.
- The `.svg` files are what other outputs link: an entity page under `docsgen/` embeds
  `../diagrams/<kind>/<slug>.svg`.
- The out directory is never docs input ([never-input paths](./project-settings.md#glob)).

## The renderer

One renderer for the whole catalog, in process.

- `plantuml-little = "=1.2026.2-4"`: a pure-Rust reimplementation of PlantUML with
  byte-exact SVG parity against the Java release, verified by its reference suite,
  covering every kind in the catalog. Graphviz is linked as the
  `graphviz-anywhere = "=0.2.5"` prebuilt static archive. The archive comes through the
  crate's own opt-in path: `bootstrap/.cargo/config.toml` sets
  `[env] GRAPHVIZ_ANYWHERE_ALLOW_DOWNLOAD = "1"`, and the crate version pins the download
  to the `v0.2.5` release assets (macOS universal, Linux x86_64 and aarch64, Windows).
  No Java, no external tool, nothing to install.
- `resvg = "0.48"` rasterizes `.svg` to `.png`, with system fonts loaded
  (`fontdb.load_system_fonts()`) so text measures as a viewer would draw it.
- `plantuml-little` and `graphviz-anywhere` are pinned exactly (`=`); `resvg` is pinned
  to its `0.48` line. A bump is a deliberate change: the reference graph and the
  fixtures re-render and the diffs are reviewed.

The seam is one function:

```
render::render_svg(puml: &str) -> Result<String, RenderError>
```

Every caller (the commit, `.png` on request, the CI cross-check) goes through it, and
nothing else in the binary knows which implementation sits behind it.

The official PlantUML native binary (a GraalVM build per platform, no Java runtime) is
the authorized swap behind the seam: `JAZYK_PLANTUML=<path to native binary>` selects it
for the process. The binary runs once per diagram with `-tsvg -pipe`, the `.puml` on
stdin and the SVG on stdout; a non-zero exit is a `RenderError`. It is the cross-check,
not the default: CI renders the reference graph and the fixture views
(`bootstrap/example/f1`, `f2`) through both implementations and diffs the SVGs, so a gap
in the crate is measured before a reader meets it.

Known gaps, and the emitter rules they dictate:

- A `package` with an alias and a body (`package "X" as X { ... }`) fails in
  `plantuml-little` with `invalid DOT input`. The package emitter therefore never
  aliases a package: packages go by their quoted name, and the classes inside keep
  their aliases.
- The crate rejects a diagram with no elements, so a view with no members renders as
  one `rectangle "<title>: no members"`.
- A renderer panic (observed on non-ASCII characters in the edge labels of
  graphviz-routed kinds) is caught at the seam and reported as a render failure like
  any other: the `.puml` stays, the stale `.svg` is removed, the build continues.

## Diagrams as input

A PlantUML block inside a source document is the opposite thing: input. The parser makes
it a `diagram` section ([section tree](./parsing.md#section-tree)), and
[`reconcile-section`](./goals/reconcile-section.md) reads it as prose
([statements](./concepts/statements.md)): the obligations it states become requirements
with verbatim quotes located in the block's text, its named boxes become entities or
mentions like any other noun phrase, and its arrows are the edges those statements
declare. The block is never rendered back or compared to the graph's own renderings. What
the graph then draws is a projection of the extracted facts, and the two pictures agree
exactly as far as the extraction did.

Renderings under `<out>/diagrams/` are never input. The out directory is excluded from the
docs glob by construction, so a diagram the compiler drew cannot become a quote.
