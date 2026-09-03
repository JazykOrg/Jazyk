# Documentation generation

Documentation generation feeds the graph back into the documentation. The compiler reads
prose and builds a graph; this consumer reads the graph, through the
[loaded set](../compiler/context.md#the-loaded-set) and the
[read tools](../compiler/tools.md#read-tools), and produces pages, rendered diagrams,
reports, and proposals that improve the prose. Nothing is written to a source file
without human review. Every output on this page is deterministic (no LLM runs), so it is
always as fresh as the graph: after every commit, during builds, and on builds that park
work alike.

## The requirements document

Every committed changeset renders one human-readable document per entity into
`<out>/docsgen/<entity-slug>.md`. `jazyk docsgen` renders the same pages on demand
([CLI](../frontends/cli.md#jazyk-docsgen)). A page whose entity is absent from the graph
is pruned, so a link never lands on a dead page; a merged entity's page is pruned with it
and the redirect names the survivor ([mutations](../compiler/graph.md#mutations)).

The page, in order:

- The header: the name, aliases, `stereotype`, `scope`, one link to the entity's
  [card](#entity-cards), the `definition`, the `parent` and the children as links to
  their pages ([containment](../compiler/model/entity.md#containment)), and the
  `attributes` (name, type, value, each with its provenance). An entity whose
  provenance is `derived` or `decree` says so here and links to its proposal. An
  entity with a level (two or more children) links to its [level page](#level-pages);
  every entity links to the level page of its parent, or of the scope root when it has
  no parent. The requirements documents link among themselves (parent, children, ties,
  relationships); the card is the one link out of that web.
- `## Diagrams`: the rendered views the entity appears in
  ([diagrams on entity pages](#diagrams-on-entity-pages)).
- `## Requirements`: one block per requirement that names the entity, under a `###`
  heading whose text is the requirement id (e.g. `### req:orders-6`). The block carries
  the `statement`, the facets (with the `measure` of a `quality` facet), the edges as
  `a → b (type, cardinality)`, the transition when one is declared, and the provenance:
  the verbatim quote with its source section for a quote-provenanced requirement, the
  upstream nodes and reasoning for a derived one, the author, time, and note for a decree.
  The heading is the anchor the [LSP](../frontends/lsp.md) and the GUI open
  (`jazyk.openRequirement`), so the id is the heading text, verbatim.
- `## Relationships`: the derived [relationships](../compiler/model/relationship.md),
  one line per direction-and-type group, with the other member linked to its page, the
  cardinality, and the contributing requirements by id.
- `## Proposals`: the pending [ratification proposals](#ratification-proposals) on the
  entity, its attributes, and its requirements, and the open `invented-choice` prompts
  generation filed on the entity ([invented choices](./gen.md#invented-choices)), which
  propose sentences the same way.
- `## Goals`: the parked and failed goals whose target is the entity, one of its
  requirements, or a view the entity is a member of: the kind, the state with its
  reason, and the cause. Open and blocked goals live on the board surfaces; the page
  shows what a build left behind.
- `## Open diagnostics`: every open diagnostic whose subjects include the entity or one of
  its requirements, with severity, message, and triage state.
- `## Mentioned in`: the sections that mention the entity, with the verbatim quotes.

This is the reading surface between prose and graph. The [LSP](../frontends/lsp.md) links
every entity occurrence in a source document to its requirements document, so a reader
clicks a concept in a source page and lands on everything the project says about it,
with each statement pointing back at the exact source sentence.

When the [ledger](./gen.md#the-ledger) holds a row for a requirement, its block carries a
verification line: the derived status, the test name and kind, the last run time, and the
evidence. A requirement with no row reads the derived status `missing` (reason
`not-generated`, [statuses](./gen.md#status-is-derived-never-stored)). The status derives at render
time ([status is derived](./gen.md#status-is-derived-never-stored)), so the document never
shows a stored stale flag.

## Diagrams on entity pages

Every view renders on every commit to `<out>/diagrams/<kind>/<slug>.svg` beside its
`.puml` ([output layout](../compiler/diagrams.md#output-layout)). An entity page embeds
the renderings of the views relevant to the entity as relative image links from
`<out>/docsgen/` into `<out>/diagrams/` (`../diagrams/<kind>/<slug>.svg`), so the whole
out directory can be copied or served anywhere and the links still resolve. Docsgen uses
the `.svg`; it never requests a `.png`. E.g.:

```markdown
## Diagrams

![Commerce](../diagrams/class/commerce.svg)

`view:class/commerce` (class, 12 members): [Customer](./entities/customer.md),
[Shopping Cart](./entities/shopping-cart.md), [Order Item](./entities/order-item.md), ... · [source](../diagrams/class/commerce.puml) · [page](./diagrams/class/commerce.md)

![Order](../diagrams/state/order.svg)

`view:state/order` (state, 3 states: placed, paid, held) · [source](../diagrams/state/order.puml) · [page](./diagrams/state/order.md)

![Customer: Orders](../diagrams/sequence/customer-orders.svg)

`view:sequence/customer-orders` (sequence, 2 steps): [Customer](./entities/customer.md),
[Order Service](./entities/order-service.md), [Inventory Service](./entities/inventory-service.md) · [source](../diagrams/sequence/customer-orders.puml) · [page](./diagrams/sequence/customer-orders.md)
```

The views an entity page embeds, in this order
([default views](../compiler/model/view.md#default-views)):

- The level neighborhood: the [level view](../compiler/diagrams.md#level-views) of
  the entity's parent (`view:class/<parent-slug>` or `view:component/<parent-slug>`),
  or of the scope root (`view:class/<scope>` or `view:component/<scope>`)
  when the entity has no parent, and the entity's own level view when it has two or
  more children.
- The entity's state machine: `view:state/<entity-slug>`, when any `transition` names
  the entity as subject ([state machine](../compiler/model/state-machine.md#rendering)).
  The caption lists the states.
- The flows it appears in: every `use-case`, `activity`, `sequence`, and
  `communication` view whose member requirements name the entity, default and curated.
- The object view of its type (`view:object/<type-slug>`), when the entity is an
  instance or a type with instances.
- Every other curated view that lists the entity in `members` or `collapse`.

Each image carries a caption line: the view id, its kind and member count, the drawn
entities as links to their [cards](#entity-cards) (a flow view's participants as the
diagram lifts them), a link to the `.puml` source, and a link to the view's
[diagram page](#diagram-pages). The caption is the same on every page that embeds a
view (a requirements document, a level page, a card, the index), and its links all
point into the walk: a reader moves from one entity to its neighbors through the views
they share. The [LSP](../frontends/lsp.md#capabilities) hover shows one view, the most
relevant by its own deterministic rule, and links to the card.

An over-limit view renders with auto-collapse and the renderer's note in its title
([over-limit views](../compiler/diagrams.md#over-limit-views)); the caption names the
`split-view` goal the view carries. Docsgen never omits a view for size. A view whose
`.svg` is missing (the renderer failed on its `.puml`) renders as its caption line with
the `.puml` link and no image; nothing is invented.

The rendering is the third layer of a diagram, build output: the page embeds it, never
reads it back ([rendering](../compiler/diagrams.md#rendering)).

## Level pages

A [level](../compiler/concepts/levels.md#levels) is a node's direct children. Every
node with at least two children has a level page, and so does the
[scope root](../compiler/concepts/levels.md#the-scope-root), the parentless entities of a
scope. The pages nest as the containment tree does: a reader starts at the scope root,
reads one level, and digs into a member to read the level below it
([drill-down](../compiler/concepts/levels.md#drill-down)). The pages render with the
entity pages, on every commit and on `jazyk docsgen`, into
`<out>/docsgen/levels/<slug>.md` with the node's slug; the scope root's page is
`levels/scope-<scope>.md` (`levels/scope-public.md` for the default scope). A level
page whose node lost its level (fewer than two children, or the node dissolved) is
pruned like an entity page.

The page, in order:

- The breadcrumb: the chain from the scope root down to the node, each ancestor a link
  to its level page (an ancestor with one child has no level page and links to its
  [card](#entity-cards) instead), the node itself last and unlinked. This is the link
  up.
- The header: the node's name, `stereotype`, and `definition` (the scope root: the
  scope name), with a link to the node's card and one to its requirements document. A
  node with `derived` provenance (a grouping) says so and links to its ratification
  proposal.
- `## Diagrams`: the node's [level views](../compiler/diagrams.md#level-views)
  embedded as on entity pages: the structural level view first, then the flow views
  per level (`use-case` and `sequence`), each with the caption line described above.
  The embedded `.svg` carries the renderer's drill-down anchors to the cards
  ([drill-down](../compiler/diagrams.md#drill-down)); the links between level pages
  are the members list below.
- `## Members`: the direct children in document order, one line each: the name as a
  link to its card, its `stereotype`, its `definition`, and, when the member has a
  level of its own, a link to its level page with its child count. This is the link
  down. An outside entity a level view includes through a lifted edge is not a member;
  it appears in the diagram and its caption only.

Only the entity page carries requirements. A grouping holds no requirements of its own;
its level page shows what its members relate to through lifting, and each member's
entity page shows the statements.

## Entity cards

The requirements document is the long read. The card is where a reader lands: one
small page per entity at `<out>/docsgen/entities/<entity-slug>.md`, rendered with the
other pages on every commit and on `jazyk docsgen`, pruned with the entity. Every link
from a diagram, a hover, or another card points at a card, and the card lets the reader
pick a direction: up, down, sideways, or into the detail. The card carries no
requirement blocks; it links to them. The shared model behind it (`card.rs`) is the one
the [LSP hover](../frontends/lsp.md#capabilities) and the
[GUI explorer](../frontends/gui.md#explore) read, so the three surfaces walk the same
graph.

The card, in order:

- The header: the name, the `stereotype`, and the `definition` in one paragraph. A
  derived or decreed entity says so in one line with a link to its proposal.
- `Sits in`: the breadcrumb from the scope root down to the parent, each a link to its
  card (the scope root links to its [level page](#level-pages)), the entity itself
  last and unlinked. This is the link up.
- `In context`: the structural [level view](../compiler/diagrams.md#level-views) of the
  parent's level (the scope root's for a parentless entity), embedded as an image, with
  the caption line under it: every drawn entity linked to its card, and a link to the
  view's [diagram page](#diagram-pages). The entity is one of the boxes; this is where
  it is used.
- `Inside`: when the entity has a level of its own, its level view embedded the same
  way, and the flows of that level, one line per flow cluster: the cluster's title,
  its `use-case` and `sequence` pages as links, and the step count. A leaf says
  `a leaf` and nothing more. This is the link down.
- `Relationships`: one line per other entity: the relationship type as the
  [relationship](../compiler/model/relationship.md) summarizes it, the direction, the
  other entity as a link to its card, and the number of contributing requirements.
  The lines are lifted as the in-context diagram lifts its arrows: a grouping's card
  lists the relationships its subtree carries, each other end lifted to the member
  of the parent's level it sits under, so a grouping is never `none` while its box
  has arrows. This is the link sideways along the edges.
- `Flows`: the flow clusters at the parent's level in which the entity is a drawn
  participant, one line per cluster: the title, the `use-case` and `sequence` pages
  as links, and the step count. A cluster is one line, never one per kind.
- `Siblings`: the other children of the same parent, each a link to its card, with a
  child count where one has a level. This is the link sideways along the level.
- `Children`: the direct children, each a link, with child counts. Absent on a leaf.
- `More`: the requirements document (with the requirement count), the level page of
  the parent's level, the entity's own level page when it has one, and the proposal
  when it is pending.

Every list is short by construction: a level is under its
[children limit](../compiler/graph.md#limits), and the card lists one level in each
direction. A reader who wants the next level clicks; nothing is inlined two levels
deep.

## Diagram pages

Every view has a page at `<out>/docsgen/diagrams/<kind>/<view-slug>.md`, rendered and
pruned with the view. A diagram page is the same size as a card and is the other node
of the walk: cards link to diagrams, diagrams link to cards.

The page, in order:

- The title and the view id, with the kind and the member count.
- `Level`: for a level view or a lifted flow view, the breadcrumb of the level it
  belongs to, each ancestor a link to its card, ending in a link to the level page.
  This is the link up. A view with no level (a curated view, an object or state view)
  says which entity or machine it belongs to instead.
- The image, embedded as on every page, with the source link.
- `Drawn`: one line per drawn entity in drawing order: a link to its card, its
  `stereotype`, and, when it has a level view, a link to that view's page marked
  `level below`. This is the legend: the image does not click inside a markdown
  preview, the legend does.
- `Steps` (flow kinds only): the members in order, each the statement with its id
  linked to its heading in the requirements document, and `initiator → receiver` as
  links to their cards, lifted as the diagram draws them.
- `Around`: the other views of the same level (`use-case`, `sequence`, the structural
  view), the level view of the level above, and the level views of the drawn entities
  that have one. This is the link sideways and down.

## Ratification proposals

A fact with `derived` or `decree` provenance is invented until the documents state it
([provenance](../compiler/model.md#provenance)). The commit that lands such a fact files
one `ratification-pending` diagnostic on it and writes the `provenance-pending` change
record the blocked [`ratify` goal](../compiler/goals/ratify.md) derives from
([ratification proposals](../compiler/model/diagnostic.md#ratification-proposals)). The
diagnostic's `prompt` is the proposal: the sentence the documents should gain, and where
it goes. Docsgen owns the composition, run in the same commit, by these rules:

- The sentence. When the fact's author staged one, that sentence stands: a session that
  stages a derived fact writes its statement to be read as prose (the `abstract-entity`
  goal proposes sentences for the structure it introduces), and a decree carries the
  human's `note`. Otherwise docsgen composes it from the fact: a requirement's
  `statement` verbatim; an entity's name and `definition` as one sentence; an attribute
  as the entity, the attribute name, and its type or value.
- The target section. For a derived fact, the section quoted by most of its upstream
  nodes (`from`), nearest the fact's first entity on a tie; for a decree, the section of
  the first entity's first mention. A decree that replaced a quoted fact targets the
  fact's former source section, and `old_text` is the former quote, so the accepted
  sentence overwrites the one it overrules. The target is always an existing section:
  an oversized section or document keeps its `section-too-large` or `doc-too-large`
  advice to split, and the proposal follows the section wherever a split moves it.
- The options. One `edit` option that inserts the sentence at the end of the target
  section (`{doc, section, old_text, new_text}`, `old_text` empty for an insertion), and
  one `answer` option, `retract`. Freeform is accepted: a reply that rewords the sentence
  is the accepted sentence.

The proposal renders on the entity page under `## Proposals`: the sentence, the target as
`doc.md#/ref`, the upstream nodes with their reasoning (or the decree's author and note),
and the two options. The index carries a `## Ratification` report grouping every pending
proposal by target document, so an owner reviews one document's proposals together, as
one draft.

Accepting a proposal (the GUI questions panel, the LSP code action, or
`answer_diagnostic` in chat) is a dual write journaled as `ratify`: the sentence lands in
the section, the section hashes are absorbed, the fact's provenance flips to `quote` in
the same changeset, the diagnostic resolves, and the `ratify` goal is gone
([edit paths](../compiler/compilation.md#edit-paths)). Retracting runs `retract_decree`
([mutations](../compiler/graph.md#mutations)). A derived node or a node created by decree
is deleted with reason `retracted`, and the facts derived from it with it; a field
decreed over a quoted fact returns to the prior value and source the decree's journal
entry recorded. The next build re-derives whatever the documents still support
([the human path](../compiler/goals/ratify.md#the-human-path)).
The owner may also write the sentence by hand, anywhere: the next build's
`reconcile-section` refreshes the fact's provenance to `quote` when the natural key
matches. The compiler never writes a source document without an accepted proposal.

## Relationships view

The index (`<out>/docsgen/index.md`) opens with the walk: for each scope, a `Start
here` line linking the scope root's [level page](#level-pages) and listing the top
level's members as links to their [cards](#entity-cards), each with its child count
where it has a level. Then it lists every entity with a link to its requirements
document, then embeds the top level of each scope rendered from the graph: one image per scope, the
scope root's level view (`view:class/<scope>` or `view:component/<scope>`
by the kind rule), each with the caption line described above, default or curated
([default views](../compiler/model/view.md#default-views)). The scope root's level view
is the per-scope view ([level views](../compiler/diagrams.md#level-views)), so the
index's picture of a scope is its top level, and its caption links to the scope root's
[level page](#level-pages). An edit turns a
default view curated without changing its id, so the index never drops a scope's view
over an edit. The images are generated on
every run like everything else here, so they cannot drift from the graph the way a
hand-drawn architecture diagram drifts from the code. A scope over its member or edge
limit renders collapsed with the renderer's note and carries a `split-view` goal on the
board; the index never omits a scope. Each entity's own Relationships section carries the
detail either way.

Below the class views the index lists every view in the graph, default and curated: the
id, the kind, the title, the member count, links to its `.svg` and `.puml`, and the goal
it carries when it is over a limit. The glossary and the reports below render as sections
of the index after the views.

## Glossary

The glossary is generated from the graph: every entity's name, aliases, `stereotype`,
and `definition`, sorted by name, linked to its page and to its defining sections through
its mentions. The graph is the only input, so a term missing from the glossary is a term
missing from the graph.

## Fragmentation reports

An entity whose mentions span many documents may deserve its own page. The report ranks
entities by mention spread (documents touched, sections per document), so an owner can
decide what to consolidate. Fragmentation is a query over the `mentions`
[axis](../compiler/context.md#axes), nothing more.

## Staleness reports

Open [diagnostics](../compiler/model/diagnostic.md) grouped by section give a staleness
map of the docs: which pages accumulate contradictions, stale anchors, and low-confidence
facts. Sections marked `non-normative` whose `note` looks weak are listed for re-review.
See [coverage](../compiler/compilation.md#coverage).

## Plain-English lint

Projects declare lint rules in prose in
[project settings](../compiler/project-settings.md#docs). E.g.:

- terminology bans: never call it a `basket`, the term is `shopping cart`,
- style rules: a requirement names its actor, no passive voice.

The rules ride along in `review-entity` sessions
([the review-entity goal](../compiler/goals/review-entity.md)). Findings are ordinary
diagnostics under the
`lint` rule (e.g. `diag:lint-1`), so each carries a `quote` and `reasoning`, lands in the
same triage queue, and is resolved like any other diagnostic.
