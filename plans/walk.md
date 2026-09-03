# Plan: the walk

Status: landed on the `levels` branch (2026-09-03): the spec in the docs, the shared
model (`bootstrap/src/card.rs`), the pages (docsgen cards and diagram pages, every
drawn entity in a rendering linking to its card), the LSP (hover card, type
definition to the card, the VS Code extension routing the links into the preview),
and the GUI explorer (card and page endpoints, detail expansion, history behind an
addressable URL), and the GUI markdown preview (generated pages, deliverable
markdown, and source documents render with headings, links, and the diagrams inline,
served read-only from the out directory; a live preview beside the editor; the
diagrams inline as live SVG, so a box click lands on its card). Verified over stdio: the hover and the type definition on example-ledger. Not yet
exercised by a human in an editor or a browser: the definition of done below is the
checklist for that. Read this file, then the spec
sections named below.

## The owner's ask

Hovering an entity in the editor shows a quick description, then a high-level diagram
where it is used, then a diagram of its internals. Clicking an entity opens something
one can walk: click a box in a diagram and land on that entity, dig deeper into the
levels, come back out, see more or less detail, move sideways to other modules and
classes. A graph of diagrams to click through. Small generated pages, one per entity
or diagram, so the reader decides where to go next. The LSP links an entity to its
page. What the LSP cannot do, the GUI does.

## The design

- One small **card** per entity at `<out>/docsgen/entities/<slug>.md`: the definition,
  `Sits in` (up), `In context` (the parent level's structural view, where it is used),
  `Inside` (its own level view, down), relationships (sideways along edges), flows,
  siblings (sideways along the level), children, and `More` (the requirements
  document, the level pages, the proposal). Spec:
  [entity cards](../docs/consumers/docsgen.md#entity-cards).
- One **page** per diagram at `<out>/docsgen/diagrams/<kind>/<slug>.md`: its level,
  the image, a clickable legend (an image in a markdown preview does not click; the
  legend does), the steps as drawn for flow kinds, and `Around` (same level, above,
  below). Spec: [diagram pages](../docs/consumers/docsgen.md#diagram-pages).
- Every drawn entity node in a rendering links to its card
  (`[[../../docsgen/entities/<slug>.md]]`); the card holds the drill-down. Spec:
  [drill-down](../docs/compiler/diagrams.md#drill-down).
- The LSP hover renders the card in short with the in-context image and the inside
  link; go to type definition opens the card. Spec:
  [LSP capabilities](../docs/frontends/lsp.md#capabilities).
- The GUI serves the card and the page as JSON and adds live nodes, history with an
  addressable URL, a detail zoom over groupings, and sideways chips. Spec:
  [explore](../docs/frontends/gui.md#explore) and the two endpoints under
  [API](../docs/frontends/gui.md#api).
- One model behind all three: `card.rs` (`Walk::new`, `entity_card`, `view_page`,
  `breadcrumb`), so docsgen, the hover, and the GUI never disagree about what sits
  where.

## Why this shape

- Small pages, one level in every direction: a level is under its children limit by
  construction, so every list on a card is short, and the reader clicks for the next
  level instead of scrolling past it.
- Cards, not requirements documents, as the landing page: the long read stays one
  click away and keeps its anchors (the LSP and the GUI open requirement headings
  there).
- One link per diagram node, to the card: a node cannot say both "drill down" and
  "tell me about this"; the card says both, and every surface intercepts the same
  href.
- Legends beside images: the walk must work in a markdown preview with no server,
  which is where the LSP sends the reader.

## Definition of done

- `jazyk docsgen` on the finished mini project (`levels-mini-final` in the session's
  scratchpad) writes cards and diagram pages whose links all resolve; opening
  `entities/funds.md` in a markdown preview lets a reader reach the top level, the
  funds level, its siblings, and every requirement in under four clicks each.
- The hover on `Funds` in `docs/checkout.md` shows the definition, the breadcrumb, the
  checkout level's diagram, the inside link, and the three links; type definition
  opens the card.
- In the GUI, clicking a box in an overlaid level view opens its card; back returns;
  `?entity=` reloads to the same place; more detail expands a grouping in place.
- Tests for each, the suite green, docs links resolving, no em dashes.

## Open questions for the owner

- Should a level's frame (the node itself) draw as a participant in its own
  sequences when a flow starts there? Today it does; the alternative drops the
  message.
- Should the front door's "its concerns are described in" sentence place the
  groupings under the stated backend (Ledger)? Today the navigation sentence is
  non-normative and the groupings sit beside Ledger.
