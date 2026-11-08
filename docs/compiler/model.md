# Model

The compiler maintains one semantic graph per project. The graph is the build artifact.
It is stored in the [graph store](./graph.md), read through the
[loaded set](./context.md#the-loaded-set), and modified only through
[write tools](./tools.md#write-tools) during [sessions](./sessions.md).

The graph is persistent. Nodes are created once and edited in place. Nothing is regenerated
from scratch. See [identity](./concepts/identity.md).

## Node kinds

The graph stores four authored kinds and derives the rest. Authored content enters through
write tools during sessions or through human edits. Derived content is recomputed by the
store on every commit and has no write tool.

| id | kind | authored or derived | natural key |
| --- | --- | --- | --- |
| `ent:<slug>` | [entity](./model/entity.md) | authored | `name` + `scope` (+ `parent` when supplied) |
| `req:<stem>-<hash>` | [requirement](./model/requirement.md) | authored | source section + statement |
| `crit:<stem>-<hash>` | [criterion](./model/criterion.md) | authored | owning requirement + statement |
| `view:<kind>/<slug>` | [view](./model/view.md) | authored (defaults derived) | `kind` + `title` |
| `rel:<a>~<b>` | [relationship](./model/relationship.md) | derived from requirement `edges` | member pair |
| `sm:<entity-slug>` | [state machine](./model/state-machine.md) | derived from requirement `transition` | subject entity |
| `doc.md#/ref` | [section](./model/section.md) | structural | document path + reference |
| `diag:<rule>-<hash>` | [diagnostic](./model/diagnostic.md) | judgment record | `rule` + `subjects` |

- Entity: a domain concept with one living `definition`, a free-form `stereotype`, a
  `parent` in one containment tree, and `attributes`.
- Requirement: one atomic obligation, a free-form `statement` about one or more entities,
  carrying the `edges`, the `transition`, and the `facets` the statement states.
- Criterion: the observable evidence one requirement is met, one condition and its
  outcome with concrete values, quoted from a test case or an example or derived with a
  proposal; what a test encodes.
- View: the stored half of a diagram: which nodes one diagram includes, never how it looks.
  Default views derive on every commit. Curated views are authored.
- Relationship: the typed connections between two entities, grouped per direction and type,
  recomputed from requirement `edges`.
- State machine: one per entity that a `transition` names as subject, recomputed from those
  transitions.
- Section: a unit of document structure. Sections carry no semantic meaning and are stored
  per document, not in `graph/`.
- Diagnostic: a recorded judgment (a contradiction, an ambiguity, a conformance finding, a
  pending decision). Diagnostics are nodes, so they are sticky by construction. Diagnostics
  record findings and questions. [Goals](./reconciler.md#goal-derivation) carry work and
  are never stored.

Everything that could be a kind of its own is an entity with a stereotype, a view, or a
derivation:

- A component is an entity (stereotype `service`, `container`, whatever the model judges
  the medium calls it), contained through `parent`, with its contracts as relationships.
  Its requirements attach directly to it.
- An interface is an entity (stereotype `interface`). Its operations are requirements on
  it. Provided is a `realization` toward it. Required is a `dependency` toward it.
- An instance is an entity tied to its type by an `instantiation` relationship, its values
  on `attributes`.
- A use case is a view: an ordered set of requirements (the flow) whose entities give the
  participants. An interaction is the same members in a `sequence` view.
- A decision is a diagnostic with a `prompt` (rule `decision`). A decision the documents
  state is prose, extracted like any other statement.
- A state machine is derived data: no write tool, recomputed at commit.
- A level is a node's children, and a grouping is an entity that holds one. See
  [levels](./concepts/levels.md).

The graph is medium-neutral. Nothing enumerates stereotypes, and the same kinds read a
software system, a slide deck, an organization, or a novel. The model adapts to what it
reads.

## Provenance

Every fact carries exactly one provenance. Three kinds exist:

```yaml
provenance: {quote: {doc, section, quote}}
provenance: {derived: {from: [ids], reasoning}}
provenance: {decree: {author, at, note}}
```

- `quote`: extracted from prose. The `quote` is the verbatim sentence or phrase, located in
  the section's text by whitespace-insensitive string search (any run of whitespace matches
  any other), never by character offsets. Offsets break on every edit. Quotes survive
  unrelated edits and fail loudly when their text changes. A quote that stops locating is a
  stale anchor: the [dirty set](./reconciler.md#dirty-set) picks it up, and a quote left
  dead draws `stale-provenance`.
- `derived`: synthesized from upstream nodes by a session or by the harness. `from` lists
  the upstream ids, `reasoning` says why. A section is not a node: a fact a section
  states enters as a `quote` of its sentence, and `from` naming a section reference is
  refused with that repair. A derived fact is invented until ratified: a
  [ratification proposal](./model/diagnostic.md#ratification-proposals) offers the
  sentence the documents should gain, the owner accepts it, and the fact flips to `quote`.
- `decree`: authored by a human directly on the graph (an inspector edit, a chat tool, a
  limit bump). A decree outranks derivation, not the documents: prose that contradicts it
  draws a diagnostic, and its ratification proposal stands until the decree is written into
  the documents or retracted. The compiler never overwrites a decree.

Where the provenance sits:

- Entity: `mentions` is the quote form, a list because a concept is mentioned in many
  places. An entity no document states carries `provenance` (`derived` or `decree`),
  whatever mentions its requirements add by reference. A mention that names the entity
  removes it: an `upsert_entity` whose `mention` names it, or an accepted ratification
  proposal. That is the flip to `quote`. See [entity fields](./model/entity.md#fields).
- Requirement: `source: {doc, section, quote}` is the quote form. `provenance` carries the
  other two kinds. A requirement has exactly one of `source` or `provenance`.
- Attribute: each attribute carries its own `provenance`, any of the three kinds.
- View: `provenance` is `derived` for default views and for views a session curates,
  `decree` for views a human creates. Views are projections and are not ratified. Their
  justification closes through their members.

Ratification pressure pushes every fact toward `quote`. A `derived` or `decree` fact on an
entity, a requirement, or an attribute opens a blocked [`ratify` goal](./goals/ratify.md)
and rides in the verdict's `blocked` count. Justification closure is a check: walking
provenance upward from any fact or rendered element ends in a verbatim quote in a live
section, or in a `derived` or `decree` fact with live upstream nodes and an open
ratification proposal. Anything else is `unjustified-fact`. See
[checks](./compilation.md#checks).

## Edge summary

| edge | stored on | points to |
| --- | --- | --- |
| `parents` | section | section |
| `parent` | entity | entity |
| `members` / `excluded` / `collapse` | view | entities, requirements |
| `mentions` | entity | sections |
| `entities`, `edges`, `transition` | requirement | entities |
| contribution groups | relationship (derived) | entities |
| transitions | state machine (derived) | requirements |
| `verifies` | ledger row | requirement |
| `subjects` | diagnostic | any node |
| `from` | derived provenance | upstream nodes |

The [loaded set](./context.md#axes) walks `parents`, `mentions`, `requirements`, `related`
(relationships, `instantiation` included), and `members` under a budget. The
[dirty set](./reconciler.md#dirty-set) walks the same edges plus view membership, so a
change reaches exactly the nodes with a justification path through it. The
[ledger](../consumers/gen.md#the-ledger) is not part of the graph. Its rows point into it.

## Identifiers

- The graph store mints every id at node creation. Ids are immutable. See
  [identity](./concepts/identity.md).
- An id is built from the node's natural key, never from a counter, so two branches
  that create different nodes never mint the same id, and two branches that extract the
  same statement mint the same one.
- The slug part of an id is the id slug of a text: the text transliterated to ASCII,
  lowercased, every run of characters other than letters and digits written as one
  `-`, leading and trailing `-` dropped, and cut to at most 32 characters at the last
  `-` that fits (a single word longer than 32 is cut at 32). An empty slug is `x`. E.g.:

  ```
  "aaa       #$%^  bbb"                        → aaa-bbb
  "Čajovňa U Zlatého"                          → cajovna-u-zlateho
  "Harbor redevelopment conflict: Characters"  → harbor-redevelopment-conflict
  ```

- The id hash of a key is the 64-bit FNV-1a hash of its UTF-8 bytes written in
  lowercase Crockford base 32 (`0123456789abcdefghjkmnpqrstvwxyz`), its first four
  characters. When the id is already taken by another node, the next longer prefix (six,
  eight, then all thirteen characters) is used, then `-2`, `-3`, and so on. Statements
  in keys are punctuation-insensitive, the same normalization as the
  [natural key](./graph.md#mutations).
- Entity: `ent:<slug>`, the id slug of the name at creation time. When that id is taken,
  `ent:<slug>-<hash>`, the hash of name, scope, and parent. E.g. `ent:shopping-cart`,
  `ent:shopping-cart-4f7a`. A rename keeps the id, so the slug can go stale. That is
  expected.
- Requirement: `req:<stem>-<hash>`. The stem is the id slug of the source document's
  file stem and the last segment of its section reference (the segment dropped when the
  section is the document's root or preamble). The hash is of the document, the section
  reference, and the statement. E.g. `req:catalog-search-9k2d` for a statement in
  `docs/catalog.md#/catalog/search`. A derived or decreed requirement has no document
  behind it: stem `x`, the hash of the `from` ids (sorted) and the statement.
- Criterion: `crit:<stem>-<hash>`, the stem of its requirement (whatever section its own
  quote sits in, so a requirement's criteria read as one family), the hash of the
  requirement id and the statement. E.g. `crit:catalog-search-m3qa`.
- View: `view:<kind>/<slug>`, the kind segment being the catalog kind with its hyphens
  removed (`use-case` → `usecase`) and the id slug of the title at creation time. When
  that id is taken, `-<hash>` of kind and title follows. E.g.
  `view:sequence/checkout`, `view:usecase/checkout`.
- Relationship: `rel:<slug-a>~<slug-b>`, the two member entity slugs in lexical order.
  Derived, recomputed on commit.
- State machine: `sm:<entity-slug>`, the subject entity's slug. Derived, recomputed on
  commit.
- Diagnostic: `diag:<rule>-<hash>`, the hash of the rule and the sorted subjects. E.g.
  `diag:contradiction-7hq2`.
- Change record: `c<generation>-<hash>`, the hash of kind and subject. See
  [change records](./graph.md#change-records).
- Ids minted before store version 4 (`req:catalog-3`, `diag:contradiction-1`) keep
  resolving and are never rewritten ([store version](./graph.md#store-version)).
- Section: a document path plus an internal reference, joined by `#`. E.g.
  `docs/cli.md#/cli/commands`. See [parsing](./parsing.md#references).
- Goal: `g:<kind>:<target>`, the target a node id, a section reference, a document path,
  or a pair `req:a~req:b`. Goals are derived, never stored. See
  [goal derivation](./reconciler.md#goal-derivation).
- Expansion handle: `h:<target>:<axis>`, minted by the loaded set. See
  [policy](./context.md#policy).

Ids are short and readable on purpose. Models copy readable ids into tool calls more
reliably than opaque tokens.

## Shared fields

- Provenance, in one of the three kinds above. Nothing enters the graph without it.
- `confidence`: a number from 0 to 1 on extracted facts. High confidence facts can be acted
  on automatically. Low confidence facts drive review.
- `reasoning`: the recorded why behind a judgment. See [judgment](./concepts/judgment.md).
- `created` and `updated`: generation markers set by the store on commit, the generation
  that created the node and the generation of its last mutation. They are local, in
  `markers.yaml`, never in the node file ([node files](./graph.md#node-files)). `jazyk ripple --back`
  walks `updated` to the edit that started a cascade.
- `limits`: `{<limit>: n}` on entities and views, a per-node bump above a built-in limit.
  Dismissing a size goal writes this field with decree provenance. The goal derives again
  only when the raised threshold is crossed. See [limits](./graph.md#limits).
