# Model

The compiler maintains one semantic graph per project. The graph is the build artifact.
It is stored in the [graph store](./graph.md), read through the [context engine](./context.md),
and modified only through [write tools](./tools.md#write-tools) during [turns](./turns.md).

The graph is persistent. Nodes are created once and edited in place. Nothing is regenerated
from scratch. See [identity](./concepts/identity.md).

## Node types

The graph has five node types. The first is structural, the other four are semantic.

- [Section](./model/section.md): a unit of document structure. Sections form a tree per
  document. They carry no semantic meaning.
- [Entity](./model/entity.md): a domain concept. Entities carry a single living `definition`
  that the compiler refines as documents are reconciled.
- [Requirement](./model/requirement.md): an EARS statement about one or more entities.
- [Relationship](./model/relationship.md): a typed edge between two entities. Relationships
  are derived from requirements. They are never written directly.
- [Diagnostic](./model/diagnostic.md): a recorded judgment about the graph or the documents.
  Diagnostics are nodes, so they are sticky by construction.

## Edge axes

Context loading traverses the graph along fixed axes. Each axis is a distinct kind of edge.

- `parents`: Section → parent Section. The document tree, both up and down.
- `mentions`: Entity ↔ Section. An entity's `mentions` list the sections that talk about it.
- `requirements`: Entity ↔ Requirement ↔ Entity. A requirement references entities, and
  through it entities reach other entities.

Two more connections exist but are not traversal axes:

- Relationship `members`: the two entities a derived edge connects.
- Diagnostic `subjects`: the nodes a diagnostic is about.

The [context engine](./context.md#axes) takes a hop quota per axis.

## Identifiers

- The graph store mints every id at node creation. Ids are immutable. See
  [identity](./concepts/identity.md).
- Entity: `ent:<slug>` where the slug comes from the name at creation time, with a numeric
  suffix on collision. E.g. `ent:shopping-cart`, `ent:shopping-cart-2`. A rename does not
  change the id, so the slug can go stale. That is expected.
- Requirement: `req:<doc-stem>-<n>`. E.g. `req:catalog-3`.
- Relationship: `rel:<slug-a>~<slug-b>`, the two member entity slugs in lexical order.
  Derived, recomputed on commit.
- Diagnostic: `diag:<rule>-<n>`. E.g. `diag:contradiction-1`.
- Section: a document path plus an internal reference, joined by `#`.
  E.g. `docs/cli.md#/cli/commands`. See [parsing](./parsing.md#references).

Ids are short and readable on purpose. Models copy readable ids into tool calls more
reliably than opaque tokens.

## Shared fields

- Provenance is a verbatim `quote`: the exact sentence or phrase copied from the source
  section. Quotes are located by whitespace-insensitive string search (any run of
  whitespace matches any other, so a sentence wrapped across source lines still
  locates), never by character offsets. Offsets break on every edit; quotes survive
  unrelated edits and fail loudly when their text changes. Entity `mentions` and
  requirement `source` both carry a `quote`.
- `confidence`: a number from 0 to 1 on extracted facts. High confidence facts can be acted
  on automatically. Low confidence facts drive review.
- `reasoning`: the recorded why behind a judgment. See [judgment](./concepts/judgment.md).
- `created` and `updated`: build markers set by the store on commit.
