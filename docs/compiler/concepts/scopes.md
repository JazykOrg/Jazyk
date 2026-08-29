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

The natural key for entity upserts is `name` plus `scope`, and `parent` joins the key
when the caller supplies it. See
[the natural key under containment](./identity.md#the-natural-key-under-containment)
and [mutations](../graph.md#mutations). An `upsert_entity` call for `Order` in scope
`billing` matches only an existing `Order` in scope `billing`; a public `Order`
elsewhere is a different node.

Two same-named entities in different named contexts stay distinct, with no diagnostic.
The separation is intentional and recorded in the documents, so it is not ambiguity to
flag.

## Scope and containment

Scope and `parent` are different axes. Scope says which same-named mentions resolve to
one concept; `parent` says which entity contains this one. See
[containment](../model/entity.md#containment). An entity's scope is stated for that
entity; containment does not set it, and a child does not inherit its parent's scope.

Both axes shape the default views: a class view derives per scope, a component view per
system, a containment root with at least one child. See
[default views](../model/view.md#default-views). The package
projection groups by scope or by containment subtree. See
[the emitters](../diagrams.md#the-emitters).

## Authoring

- To keep same-named concepts apart, state the scope in the prose. E.g. "this `Order` is
  internal to the billing service." The model captures it during a
  [`reconcile-section`](../goals/reconcile-section.md) session.
- To make two mentions one concept, leave them public and let the documents agree. A
  duplicate that slips through under two names is repaired with `merge_entities`: by a
  [`review-entity`](../goals/review-entity.md) goal when the entity's own neighborhood
  shows a lookalike, by a [`dedupe-candidates`](../goals/dedupe-candidates.md) goal
  when a cross-document lookalike scores high. Both leave a redirect from the absorbed
  id. See [identity](./identity.md#operations-preserve-identity).
