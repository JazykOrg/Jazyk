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
`<out>/derived/docsgen/<entity-slug>.md`. `jazyk docsgen` renders the same pages on demand
([CLI](../frontends/cli.md#jazyk-docsgen)). A page whose entity is absent from the graph
is pruned, so a link never lands on a dead page; a merged entity's page is pruned with it
and the redirect names the survivor ([mutations](../compiler/graph.md#mutations)).

The page, in order:

- The header, in order: the name; one line with the id, aliases, `stereotype`,
  `scope`, and one link to the entity's [card](#entity-cards); the `definition`; the
  containment line, the `parent` and the children as links to their pages
  ([containment](../compiler/model/entity.md#containment)); the level line, a link to
  the level page of the parent (the scope root's for a parentless entity) when that
  level has a page, and, for an entity with a level of its own (two or more children),
  a link to its own [level page](#level-pages); the `attributes` (name, type, value, each with its
  provenance). An entity whose provenance is `derived` or `decree` says so last in
  the header and links to its proposal. The requirements documents link among
  themselves (parent, children, ties, relationships); the card is the one link out of
  that web.
- `## Diagrams`: the rendered views the entity appears in
  ([diagrams on entity pages](#diagrams-on-entity-pages)).
- `## Requirements`: one block per requirement that names the entity, under a `###`
  heading whose text is the requirement id (e.g. `### req:orders-6`). The section opens
  with an index when it holds more than five blocks: one line per source group, the
  document and section title linked to its first block, with the number of statements
  it holds, so a page like a novel lead's (103 requirements over fifteen sections)
  shows where its statements come from before it shows them. An index of every
  statement would repeat the page. The blocks are grouped by the source document
  and section they quote, in document order, each group opened by a bold line naming
  the document and the section title; derived and decreed requirements group last under
  their provenance. The block carries
  the `statement`, the facets (with the `measure` of a `quality` facet), the edges as
  `a → b (type, cardinality)`, the transition when one is declared, and the provenance:
  the verbatim quote with its source section for a quote-provenanced requirement (the
  quote is omitted when it says what the statement says, which is the common case: the
  words equal once case, punctuation, link targets, articles, `then`, the modal verbs
  (`shall`, `must`, `will`, `should`, `can`, with `cannot` read as `not`), and a verb's
  or plural's ending are set aside; on f2 25 of 44 blocks
  repeated their statement with only "shall" added), the
  upstream nodes and reasoning for a derived one, the author, time, and note for a decree.
  Under the block, `Criteria:` lists the requirement's [criteria](../compiler/model/criterion.md),
  one line each: id, `kind`, the statement, its quote's section or `proposed` for a derived
  one, and the test verdict from the ledger when a row exists; a requirement with none
  shows no criteria line.
  A relative link inside a quote is rewritten to resolve from the page (a bullet quoting
  `[User Management](./subsystems/user.md)` from `docs/backend.md` links to
  `../../docs/subsystems/user.md`; an anchor-only link such as `(#api-endpoints)` points
  into the source document), so the quote reads verbatim and its link still lands.
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
evidence. A requirement with no row shows no verification line (a project without a
ledger once read `missing` a hundred times on one page). The status derives at render
time ([status is derived](./gen.md#status-is-derived-never-stored)), so the document never
shows a stored stale flag.

## Diagrams on entity pages

Entity pages embed relevant authored diagrams only. Captions name the view ID,
kind, purpose, members and sources. Missing SVGs link to the definition and
PlantUML with rendering status. Flow lists link to diagrams that actually exist.
The output does not infer a diagram ID from an entity name or scope.

## Level pages

Every nonempty containment level has a page, including one-child levels. Its
breadcrumb, definition and direct child links remain useful with no diagram.
The page lists zero or more authored diagrams by area. Diagrams do not create
or remove containment levels.

## Entity cards

Cards provide Sits in, In context, Inside, Relationships, Flows, Siblings,
Children and More. Context and inside diagrams are optional. A node with children
and no diagram says that no authored diagram is available and retains child and
level links. Only a node with no children is a leaf. Relationship summaries remain
available without a picture. The card lists related authored diagram references
with their purpose, not a synthesized primary view.

## Diagram pages

A page exists for each authored diagram. It carries purpose, description,
provenance, rendering, member legend and explicit links to detail or phases.
Entity links open their cards. Deleting a view prunes its page and rendering.
Standalone output uses relative links under the output directory.

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
  one `answer` option, `retract`. Freeform replies instruct an answer session. They
  are never inserted as document text.

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

The index starts with containment navigation and entity cards for each scope.
It lists authored diagrams with kind, purpose, member counts and output links.
A scope with no diagrams still lists its entities and relationships. Counts and
links come from stored view nodes, never from default ID conventions.

## Glossary

The glossary, `<out>/derived/docsgen/glossary.md`, is generated from the graph: every entity's
name, aliases, `stereotype`, and `definition`, sorted by name, each linked to its
[card](#entity-cards) and to its defining sections through its mentions (relative links
into the source documents, anchored by GitHub's slug of the heading text:
`docs/main.md#/warehouse-erp/backend` links to `../../docs/main.md#backend`, since a
GitHub anchor is the heading alone, never the path, and a heading `main.ts` anchors as
`#maints`, punctuation dropped, where the section reference reads `main-ts`). The graph is the only input, so a term missing from the glossary is a term
missing from the graph. The index links the glossary.

## Fragmentation reports

An entity whose mentions span many documents may deserve its own page. The report,
`<out>/derived/docsgen/fragmentation.md`, ranks entities by mention spread (documents touched,
then sections touched), most spread first, one line per entity with the counts and the
documents, so an owner can decide what to consolidate. Fragmentation is a query over
the `mentions` [axis](../compiler/context.md#axes), nothing more. Entities mentioned in
one document only are left out; an empty report says so. The index links it.

## Staleness reports

Open [diagnostics](../compiler/model/diagnostic.md) grouped by section give a staleness
map of the docs: `<out>/derived/docsgen/staleness.md` lists, per section that any open
diagnostic's subjects anchor in (a requirement through its source, an entity through its
first mention), the count per rule, most burdened first, so a reader sees which pages
accumulate contradictions, stale anchors, and low-confidence facts. Below it, sections
marked `non-normative` whose `note` is empty or under twenty characters are listed for
re-review. See [coverage](../compiler/compilation.md#coverage). The index links it.

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

Generated source links account for the document set directory. A page directly
under `.jazyk/<set-id>/docsgen/` reaches project files with `../../../`. Links
within the same graph output remain relative to that set's store.

## Obligation locations

Render each requirement once and list all its source locations.

## Navigation without diagrams

Containment pages and entity cards exist independently of diagrams. Every nonempty
level has navigation, including single-child levels. Diagram sections list only
actual authored nodes. Empty diagram lists retain parent, children and relationship
links. Rendering failures retain links to definitions and source, not dead images.
