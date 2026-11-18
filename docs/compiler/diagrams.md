# Diagrams

A diagram has semantic facts, an authored view, and disposable rendering files.
Entities and requirements supply facts. Relationships and machines are computed
from them. A view records the question, membership and presentation intent chosen
by a session or human. Geometry remains renderer-owned.

The catalog is in [view kinds](./model/view.md#kinds). Nothing renders without an
authored view, including a state machine with no state diagram.

## Rendering

view → `.puml` → `.svg` (→ `.png` on demand)

- On every commit the renderer emits one PlantUML file per view and renders it to SVG
  beside it. Emission is deterministic: the same store snapshot and the same view produce
  byte-identical `.puml`.
- The renderer is skipped for a view whose emitted `.puml` equals the file already on
  disk and whose `.svg` exists. Most commits touch few views, so most renders are
  skipped.
- A deleted view loses its files at the commit that deleted it.
- `.png` is rasterized from the `.svg` only when a surface asks for one (at most 8192
  pixels on the longer side, scaled down past that), and is written beside it. [Docsgen](../consumers/docsgen.md) embeds `.svg`. The
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
  name, the emitter derives an alias from the entity id, stable across builds. Distinct
  ids never share an alias: `ent:a-b` and `ent:a_b` are two elements.
- Text from the graph is inert. A line that would open a preprocessor directive (`!`)
  or end the diagram (`@enduml`), a builtin call (`%getenv`), and image markup (`<img`)
  anywhere get a zero-width space after the trigger character, so no renderer reads a
  file or the environment on a fragment's behalf.
- A `stereotype` renders as `<<stereotype>>` on the element that carries it.
- A `title` line is emitted only when the emitter has something to say beyond the view's
  title: the [over-limit](#over-limit-views) note, or a timing view's governing
  requirement.
- A relationship among the drawn members renders as one arrow per direction-and-type
  group, cardinality at the `b` end. An `association` is the exception: its line carries
  no arrowhead, so both directions of a pair draw as one line (a `Customer -- Shipment`
  and a `Shipment -- Customer` contribution are one association, count two), never two
  parallel lines. Notation, `a` acting on `b`:

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
  cardinality. A class view is authored for a specific structural question. E.g.:

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
  from the relationships among the instances. An object view selects known instances of the type.
- `package`: the member entities that contain other members render as packages holding
  those children as classes; a member with no shown children renders as the one-line
  form `package "X" as X`, never an empty body. Arrows are the lifted cross-package
  relationships only; relationships among the classes inside one package belong to the
  class view. Grouping follows containment (`parent`) or, for a view with a `scope`
  query, the scope.
- `component`: the member entities by stereotype: «actor» as `actor`, «interface» as
  `interface`, everything else as `component`. A stereotype other than the keyword it
  became still renders on the element (`component "Ledger" as ledger <<backend>>`),
  so the top diagram keeps saying what each box is. A `realization` toward an interface draws
  the lollipop; a `dependency` from a component toward an interface draws the socket; a
  dependency from an actor draws a dashed arrow. The authored area controls the abstraction boundary. E.g.:

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
  parts (`[Part]`, a bracket in a part's name rendered as a parenthesis) inside
  `component "Name" { }`, connectors from the relationships among the parts and from
  parts to members outside the boundary.
- `deployment`: the member entities as `artifact`s. Every attribute with a `value` on a
  member is a placement: the artifact sits inside a `node` labeled `<value> <name>`
  (`region = EU` → `node "EU region"`), and members sharing a placement share the node.
  A member without valued attributes is a bare artifact. Nothing synthesizes topology:
  the picture holds only what prose stated. There is no default deployment view;
  membership is curated, so what counts as a placement is the curator's choice of
  members.
- `use-case`: the view itself is the `usecase`, titled by the view; the actors are the
  «actor» entities among the members' `entities`, each linked `Actor -- UseCase`. The
  membership and intent are authored for the question being explained. A view whose members name no «actor» draws a note
  under the bubble saying so (`no actor: none of the N member statement(s) names an
  «actor» entity`), since a bare bubble reads as a rendering defect where it is a
  curation gap (an erp session placed "Manage the ACME warehouse" with no actor in the
  graph).
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
  the edge the [message rule](./model/view.md#fields) picks (the subject's edge first,
  then a dependency, then any): the sender is `a`, the receiver is `b`, and a receiver that is an «interface» resolves to its
  provider (the entity with a `realization` toward it). A member with no edges is a
  self-message on its first entity. «actor» participants render as `actor`. A
  member with a transition guard or trigger can draw an `alt` fragment labeled by
  that stated condition. Authored sequence fragments take precedence. No branch
  condition is invented.
- `communication`: the same view in its second layout: participants as `actor` or
  `rectangle`, messages numbered `1.`, `2.`, in member order.
- `timing`: measured criteria of the member requirements or the subject's
  requirements, and the member entities that have a derived machine. The `title` is the first such
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
  type (an `association` collapses both directions, since it draws without an arrowhead). When several types land on the same shown pair in the same direction, the arrow
  shows the strongest type ([ranking](./model/relationship.md#rendering): generalization,
  realization, composition, aggregation, association, dependency) and carries the count of
  concrete edges beneath it as its label. A count of one draws no label. A cardinality
  is drawn only when the arrow's one concrete edge joins the two shown nodes
  themselves: a lifted arrow speaks for a hidden pair (`Order o-- "1..*" Product` lifts
  to `Order o-- Catalog` when Product hides under Catalog), and a count that belonged
  to the leaf would be false on the ancestor, so the lifted arrow drops it. `instantiation`
  is dropped before grouping and never drawn as an arrow: it names the instance's type
  in the object view.
- An edge whose two ends lift to the same shown node is not drawn.
- Lifting stores nothing. It is render-time aggregation over `parent` chains, so it
  cannot drift. The lifted arrow's justification is the set of concrete edges beneath it:
  the GUI inspector lists them on click, each walking to its requirement and quote.
- Views nest. A collapsed node carries the same PlantUML link every rendered entity
  carries, the link to its card (`[[../../docsgen/entities/<slug>.md]]`,
  [drill-down](#drill-down)); the card's `Inside` section is the level the collapse
  hid. A view carries no link to another rendering, the `overview` kind's `:ref:` steps
  to the sequence views they reference being the one exception.

E.g.: `ent:a` contains `ent:a1`, `ent:b` contains `ent:b1`, and a requirement declares
`{a: ent:a1, b: ent:b1, type: dependency}`. A view showing only `a` and `b` draws one
arrow, and clicking it lists the one concrete edge:

```
A ..> B
```

## Levels

A node's level is its set of direct children, and the scope root (the parentless
entities of a scope) is the top level ([levels](./concepts/levels.md#levels),
[the scope root](./concepts/levels.md#the-scope-root)). Each level gets its own diagrams,
and digging into a member shows the level below it
([drill-down](./concepts/levels.md#drill-down)). The tree is as deep as the documents and
the model's judgment make it; nothing here fixes a depth.

### Level views

Containment defines levels without requiring diagrams. An authored view names
its area and question. Multiple diagrams may explain one level. No renderer
synthesizes state diagrams or other kinds when a view is absent.

## Drill-down

Every rendered entity links to its [card](../consumers/docsgen.md#entity-cards):

- In the `.puml`, the element carries a PlantUML `[[...]]` hyperlink to the entity's
  card: `[[../../docsgen/entities/<entity-slug>.md]]`, the same form as the
  collapsed-node link under [lifting and collapse](#lifting-and-collapse). The link is
  part of the emitted text, so it takes part in determinism and in the skip rule.
- In the `.svg`, the renderer emits the link as an anchor on the element. The path is
  relative under the out directory, so a rendering opened from the out directory
  navigates on click with no server behind it, and a card served beside it links back.
  The card is where the drill-down lives: its `Inside` section is the level below, its
  `Sits in` line the level above. One link per node, and the card decides the rest.
- A participant of a flow view links the same way; a message does not.
- The view carries a `children` list beside its members, in `get_view`
  ([read tools](./tools.md#read-tools)) and in the GUI API: the containment levels reachable
  from its drawn entities. Each entry carries the target and zero or more authored
  diagram IDs.

What consumes the links:

- [Docsgen](../consumers/docsgen.md) renders a card per entity and a page per diagram
  beside the level pages: a reader lands on a card, sees the level it sits in and the
  level below it, and moves up, down, or sideways one link at a time. A diagram page
  carries a legend, one link per drawn entity, because an image embedded in a markdown
  preview does not click; the `.svg` anchors work wherever the file itself is opened.
- The [GUI](../frontends/gui.md#map) draws from the graph, not from the files. It
  shows the containment tree beside the diagram panel and a breadcrumb over it, and
  follows `children` to open the level below a clicked member.
- The [terminal viewer](../frontends/viewer.md) prints the containment tree with each
  node's view ids, so the level views are addressable without a browser.

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
  `(collapsed: n subtrees over limit)`, or `(over limit: n members)`, `(over limit: n
  edges)`, or `(over limit: n participants)` when nothing could collapse, naming the
  limit that was crossed.

## Output layout

```
<out>/derived/diagrams/<kind>/<slug>.puml    the emitted PlantUML
<out>/derived/diagrams/<kind>/<slug>.svg     the rendering
<out>/derived/diagrams/<kind>/<slug>.png     on request only
```

- The path is the view id with the `view:` prefix removed: `view:usecase/checkout`
  renders to `diagrams/usecase/checkout.puml`. The `<kind>` directory is the id's kind
  segment as [identifiers](./model/view.md#identity) spell it.
- The directory mirrors the set of views. A commit that deletes a view deletes its
  files. A `.png` is removed whenever its `.puml` changes and is rasterized again on the
  next request.
- The `.svg` files are what other outputs link: an entity page under `docsgen/` embeds
  `../diagrams/<kind>/<slug>.svg`.
- Links out of a rendering are relative paths under the out directory: a
  [drill-down](#drill-down) link in `diagrams/component/shop.svg` reads
  `../../docsgen/entities/order-service.md`. Nothing in a rendering names the out
  directory or the project root, so the out directory can be copied or served whole
  and every link holds.
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
  No Java, no external tool, nothing to install. The conversion runs the crate's
  parser, layout, and SVG stages directly, never its filesystem preprocessor, so no
  `!include` or `%load_json` reads a file.
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
stdin and the SVG on stdout; a non-zero exit is a `RenderError`. It runs under the
command timeout ([budgets](./graph.md#budgets-and-thresholds)): a binary still running
at the deadline is killed, and the kill is a `RenderError`. It is the cross-check,
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
- In a component diagram the crate draws no `actor` (declared with an alias it drops the
  element and draws a link to it as the bare alias) and, when no `interface` is
  declared, no link but `-->` and `..>`: `--`, `*--`, `o--`, `--|>`, and `..|>` between
  components vanish. The emitted `.puml` stays standard, so the official binary draws the
  true notation; the in-process conversion alone adapts it: an actor converts as a
  `component` with the `«actor»` stereotype, and without an interface each link converts
  to `-->` (`..>` for the dotted forms) in the relationship's stated direction, its label
  kept. The ERP's top view had drawn three bare boxes where its source declared an actor
  and three associations.
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

Renderings under `<out>/derived/diagrams/` are never input. The out directory is excluded from the
docs glob by construction, so a diagram the compiler drew cannot become a quote.

## Authored intent

Views store purpose, area, ordered members and relationship selection. Labels
change display text only. Notes cite graph sources. Presentation groups form an
acyclic forest and never change entity containment. Layout accepts relative
direction and grouping hints, never raw renderer syntax or coordinates. Sequence
fragments cite member requirements and sourced guards; each message occurs once.
Explicit detail and phase links connect diagrams without matching titles.

All catalog kinds are authored through the same tools. Text remains inert through
the existing escaping pipeline. The same snapshot produces the same PlantUML.

Presentation packages are supported for class, component and object diagrams.
Other kinds retain their own semantic frames. Unsupported group and `together`
hints are refused. Direction hints remain relative, with renderer-owned geometry.

Labels target entities or requirements. Relationship groups use their canonical
labels. Notes target graph IDs; structural entity targets receive connectors.
Other notes appear in the diagram legend. Their sources remain inspectable in
the definition. Sequence `ref` fragments name a sequence in `view` and identify
its replaced member interval through operands. Nested fragment placement follows
member order. Explicit overview phase links render in their authored order.

Relationship limit counts respect authored relationship selection before lifting.
