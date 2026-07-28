# Scopes

Same-named mentions across documents usually name the same concept, but not always. Two
bounded contexts may both define `Order` and mean different things. A scope lets the
documentation itself say which same-named entities are one concept and which are
deliberately distinct.

Scope is a property of the [entity](../model/entity.md), captured from the documents
during reconciliation. It is not a project setting. Values:

- `public` (the default): the concept resolves across the whole project.
- `private`: the entity stays within its own document.
- a named context (e.g. `billing`, `fulfillment`): the entity resolves only against
  entities in the same named context.

## Scope in the natural key

The natural key for entity upserts is `name` plus `scope`. See
[mutations](../graph.md#mutations). An `upsert_entity` call for `Order` in scope `billing`
matches only an existing `Order` in scope `billing`; a public `Order` elsewhere is a
different node.

Two same-named entities in different named contexts shall stay distinct, with no
diagnostic. The separation is intentional and recorded in the documents, so it is not
ambiguity to flag.

## Authoring

- To keep same-named concepts apart, state the scope in the prose. E.g. "this `Order` is
  internal to the billing service." The model captures it during a `reconcile-doc` turn.
- To make two mentions one concept, leave them public and let the documents agree. A
  duplicate that slips through under two names is repaired in the review wave with
  `merge_entities`. See [task types](../turns.md#task-types).
