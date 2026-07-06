# Entity

An entity is a domain concept. Each entity carries one living `definition` that the
compiler refines as documents are reconciled. One concept, one node: there is never a
per-document copy of an entity.

## What is an entity

Entities are domain concepts: a component, a type, a field, a product, an actor. They are
not syntax artifacts:

- not file paths or directory names,
- not CLI flags or option names,
- not markdown constructs (a heading, a table, a link),
- not generic fragments ("the system", "the input"),
- not technologies, languages, or third-party tools the system is built with (React,
  Go, PostgreSQL). Those belong in the requirement's statement text: "The gateway shall
  be built with Go" references the entity `gateway` only.

Entities exist because requirements need them. If no statement is about a concept, it is
not an entity. [`reconcile-doc` turns](../turns.md#task-types) extract requirements first
and mint entities only as those requirements need them.

Granularity guidance: attach detail to a requirement before minting a sub-entity. "The
Shopping Cart shows a line-item count" is a requirement on `ent:shopping-cart`, not a new
`ent:line-item-count`. Mint the sub-entity only when statements are about it directly.

The [validation gates](../graph.md#validation-gates) reject names that look like syntax
rather than a concept, unless the call carries an explaining `note`. They also reject a
name that is a variant of an existing entity's name ("backend" beside "backend
system"): one concept, one node; the wording joins the existing entity's `aliases`
instead. A `note` saying how the concepts differ overrides when the resemblance is
coincidental.

## Fields

- `name`: the primary handle. `aliases`: alternate names seen in the documents.
- `definition`: the one living definition, refined as documents are reconciled. It is
  never forked per document.
- `scope`: keeps distinct same-name concepts apart. See [scopes](../concepts/scopes.md).
- `mentions`: list of `{doc, section, quote}`, the sections that talk about the entity.
  Each `quote` is verbatim. See [shared fields](../model.md#shared-fields).
- `confidence` and `reasoning`.
- `created` and `updated`: build markers.

## Identity

- The natural key is `name` plus `scope`. `upsert_entity` keys on it, so a retried or
  parallel create lands on the existing node instead of duplicating it. See
  [mutations](../graph.md#mutations).
- The id `ent:<slug>` is minted at creation and never changes. A rename keeps the id, so
  the slug can go stale. That is expected. See [identifiers](../model.md#identifiers).
- A merge keeps one entity and leaves a redirect from the absorbed id to the survivor, so
  downstream consumers holding the old id still resolve.
