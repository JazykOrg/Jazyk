# Viewer

`jazyk viewer [--out FILE]` renders the [graph store](../compiler/graph.md) into one
self-contained HTML file, by default `<out>/graph.html`. No server, no external assets.
The file works offline and can be attached to a review or a ticket as-is.

The viewer reads the same shards every frontend reads. See
[storage layout](../compiler/graph.md#storage-layout). It renders what is on disk; it
never compiles.

## What it shows

- A header with the build stats: entity, requirement, and relationship counts, open
  [diagnostics](../compiler/model/diagnostic.md) by severity, and the coverage fraction.
- The tree: the containment tree the `parent` field makes
  ([levels](../compiler/concepts/levels.md#levels)), one root per scope
  ([the scope root](../compiler/concepts/levels.md#the-scope-root)), printed with
  indentation. Each node prints its name, its child count when it has a level (a link
  to the node's level section), and the ids of its
  [level views](../compiler/diagrams.md#level-views) (the structural view and the flow
  views per level), each id linked to its view card. The scope root prints as
  `scope:<scope>` with its own level view ids. This is the terminal viewer's
  drill-down ([drill-down](../compiler/concepts/levels.md#drill-down)): reading down
  the tree is reading down the levels.
- Levels: one anchored section per level (every node with a level view, and the scope
  root), the file's form of the docsgen [level page](../consumers/docsgen.md#level-pages).
  A section carries, in order:
  - The breadcrumb: the chain from the scope root down to the node, each ancestor a
    link to its level section (an ancestor with one child has no level and links to
    its card instead), the node itself last and unlinked. This is the link up.
  - The header: the node's name, `stereotype`, and `definition`, with a link to its
    card (the scope root: the scope name). A `derived` or `decree` node says so.
  - `diagrams`: the level's views as links to their view cards, the structural view
    first, then the flow views per level. The rendering lives on the view card.
  - `members`: the direct children in document order, each a link to its card with
    its `stereotype` and `definition`, and, when the member has a level of its own, a
    link to its level section with its child count. This is the link down. An outside
    entity a level view includes through a lifted edge is not a member.
- Entities: id, `name`, `scope` when not `public`, `definition`, `aliases`, mentions
  (document, section, and the located `quote`), the requirements referencing the
  entity, and the sections of the [entity card](../consumers/docsgen.md#entity-cards)
  (the shared card model, `card.rs`), every link an anchor to another card in the same
  file:
  - `sits in`: the breadcrumb from the scope root down to the entity, each ancestor a
    link to its card (the scope root to its level section), the entity itself last
    and unlinked. This is the link up.
  - `in context`: the structural level view of the parent's level (the scope root's
    for a parentless entity) as a link to its view card, where the rendering is. The
    entity is one of the boxes there. `no level view` when the level above holds the
    entity alone.
  - `inside`: when the entity has a level of its own, its level view as a link, and
    the flows of that level, one line per flow cluster (the title, the `use case` and
    `sequence` view cards as links, the step count). A leaf says `a leaf`. This is the
    link down.
  - `relationships`: one line per other entity: the type as the
    [relationship](../compiler/model/relationship.md) summarizes it, the direction,
    the other entity as a link, and the number of contributing requirements, lifted
    as the card lifts them. This is the link sideways along the edges.
  - `flows`: the flow clusters at the parent's level in which the entity is a drawn
    participant, one line per cluster.
  - `siblings`: the other children of the same parent, each a link, with a child
    count where one has a level. This is the link sideways along the level.
  - `children`: the direct children, each a link, with child counts. Absent on a leaf.
  - `levels`: a link to the level section of the parent's level, and to the entity's
    own level section when it has one.
- Requirements: id, the `statement`, the entities it references, the provenance (the
  `source` quote, or the `derived` or `decree` record), its `edges` when declared, and
  its `transition` and `facets` when present.
- Relationships: id, members, and each contribution group (direction, `type`,
  cardinality, the contributing requirement ids). Derived nodes, shown as stored. See
  [derived data](../compiler/graph.md#derived-data).
- Views: id, `kind`, `title`, ordered members, a link to the rendered `.svg` under
  `<out>/diagrams/` when it exists, and the sections of the
  [diagram page](../consumers/docsgen.md#diagram-pages), in the file:
  - `level`: for a level view or a lifted flow view, the breadcrumb of the level it
    belongs to, each ancestor a link to its card, ending in a link to the level
    section. A view with no level (a curated view, an object or state view) says
    which entity or machine it belongs to instead.
  - The rendering, inline. The viewer reads `<out>/diagrams/<kind>/<slug>.svg` at
    render time and embeds it as an `<svg>` element: sanitized (`script` and
    `foreignObject` elements, `on*` attributes, `javascript:` links, the XML
    declaration and processing instructions stripped), the `viewBox` kept and the hard
    `width` and `height` dropped (the attributes and the same declarations in the root
    `style`), the rendering's own width kept as the figure's maximum so a small diagram
    scales down to the card and never up. Every [drill-down](../compiler/diagrams.md#drill-down)
    anchor is rewritten from `../../docsgen/entities/<slug>.md` to the entity's card
    anchor in the file, and a collapsed node's link to another rendering to that view's
    card, so a box click jumps to the card. A missing `.svg` degrades to the card
    without the image, as before; the file stays self-contained and offline either way.
  - `drawn`: the legend under the diagram, the same one docsgen writes: one line per
    drawn entity in drawing order, a link to its card, its `stereotype`, and, when the
    entity has a level view, a link to that view's card marked `level below`.
  - `steps` (flow kinds only): the members in order, each the requirement id linked to
    its card, the statement, and `initiator → receiver` as links to their cards,
    lifted as the diagram draws them.
  - `around`: the other views of the same level, the level view of the level above,
    and the level views of the drawn entities that have one, each a link to its view
    card.
- Diagnostics: id, `rule`, a severity chip, `lifecycle`, subjects, `message`, and
  `reasoning`.
- Coverage: one row per document with covered, non-normative, and unprocessed section
  counts.

## Verification overlay

When the [ledger](../consumers/gen.md#the-ledger) exists, the viewer overlays
verification state, derived at render time exactly as
[`verification_tasks`](../compiler/tools.md#verification-tools) derives it:

- The header gains a verification summary: verified, failing, stale, and unverified
  counts, plus not-generated requirements.
- Each requirement card carries a status chip (`verified`, `failing`,
  `stale-requirement`, `stale-test`, `stale-code`, `unverified`, `missing`) with the
  test kind, the recorded run command, and the last evidence line.
- Each entity card aggregates its requirements: all verified reads green, any failing
  reads red, any stale reads amber, none generated reads gray.

The overlay is read-only and deterministic. Rerunning `jazyk test` and re-rendering the
viewer is the whole refresh loop.

## Navigation

- One text filter narrows every card at once. Matching is case-insensitive over ids,
  names, and text.
- Every node id links to its card. Clicking an id anywhere jumps to it.
- The walk of the docsgen pages runs inside the one file: a card's sections link to
  the cards one level away in every direction, a view card links to the cards of what
  it draws and to the views around it, a level section links down to its members and
  up along its breadcrumb, and a click on an entity drawn in an inline diagram lands on
  its card. Every link is an anchor (`#n-<id>` for a node's card, `#l-<target>` for a
  level section), so the file works from disk with no server behind it.
- Severity chips color-code diagnostics: red for `error`, amber for `warning`, blue for
  `info`, grey for `none`.
