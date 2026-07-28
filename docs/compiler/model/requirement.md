# Requirement

A requirement is an EARS statement about one or more [entities](./entity.md).
Requirements are the primary semantic content of the graph: entities exist because
requirements need them, and [relationships](./relationship.md) are derived from
requirement edges.

E.g.:

```
The system shall ensure each User email is unique.
When the customer checks out, the system shall empty the Shopping Cart.
```

## Fields

- `ears`: the statement text, in EARS form. Patterns per [EARS](../concepts/ears.md).
- `entities`: the entity ids the statement is about. At least one.
- `edges`: list of `{a, b, type?}`, the entity pairs this statement ties together, with
  an optional [relationship type](./relationship.md#types). Only entities listed in
  `entities` may appear.
- `source`: `{doc, section, quote}`. The `quote` is the verbatim source sentence, located
  by string search. See [shared fields](../model.md#shared-fields).
- `confidence` and `reasoning`.
- `created` and `updated`: build markers.

The id is `req:<doc-stem>-<n>`, minted by the store. E.g. `req:catalog-3`. See
[identifiers](../model.md#identifiers).

## Behavior vs constraint

Behavior vs constraint is derived from the EARS pattern, not stored. A `When ...`
statement is a behavior. A ubiquitous `shall ensure ...` is a constraint. Consumers read
it off the pattern, so there is no separate requirement taxonomy to maintain.

## Edges

A requirement that references two or more entities may declare `edges`. On every commit
the store recomputes relationships from these edges. See
[derived data](../graph.md#derived-data).

Requirements are the only source of edges. A diagram arrow or a structural sentence ("A
is part of B") is captured as a requirement, which then yields the edge. This keeps every
edge backed by a statement and provenanced through the requirement's `source`.
