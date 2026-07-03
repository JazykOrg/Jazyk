# Identity

The graph store shall mint every id at node creation and never change it. An id is
immutable for the node's lifetime. See [identifiers](../model.md#identifiers).

## Operations preserve identity

- A rename is an update that keeps the id. `ent:shopping-cart` stays `ent:shopping-cart`
  after the entity is renamed to `Basket`. The slug goes stale; the identity does not.
- A merge keeps one id. The absorbed id leaves a redirect to the survivor, so anything
  holding the old id still lands on the right node. See
  [mutations](../graph.md#mutations).
- Diagnostics keep their ids the same way. A triage decision made against
  `diag:contradiction-1` survives every rebuild.

## Why identity is state

Identity is state, not something recomputed per build. Anything recomputed would churn
under a nondeterministic extractor: the same documents could yield slightly different
names, splits, or orderings on each run, and ids derived from that output would shift with
it. Minting an id once, at creation, and editing the node in place removes that failure
mode. The graph carries its identity forward; the model only proposes changes to nodes
that already have one.

## Downstream binding

Downstream consumers (generated code, tests, tickets) bind to ids and stay bound. A
function generated for `ent:shopping-cart` still traces to the same entity after renames,
merges, and any number of rebuilds. Redirects cover the one case where an id retires: a
consumer holding an absorbed id follows `redirects.yaml` to the survivor. See
[storage layout](../graph.md#storage-layout).
