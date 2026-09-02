# Identity

The graph store mints every id at node creation and never changes it. An id is immutable
for the node's lifetime. See [identifiers](../model.md#identifiers).

## Operations preserve identity

- A rename is an update that keeps the id. `ent:shopping-cart` stays `ent:shopping-cart`
  after a rename to `Basket`. The slug goes stale; the identity does not.
- A merge keeps one id. The absorbed id leaves a redirect to the survivor, so anything
  holding the old id still lands on the right node. See
  [mutations](../graph.md#mutations).
- Re-parenting keeps the id. An [`abstract-entity`](../goals/abstract-entity.md) goal
  that introduces a parent and moves detail under it edits `parent` on the children and
  `entities` on the requirements; no node is recreated. See
  [containment](../model/entity.md#containment).
- A provenance change keeps the id. A ratified fact flips from `derived` or `decree` to
  `quote` in place, and a dual write rewrites the sentence and the fact under the same
  id. See [provenance](../model.md#provenance) and
  [edit paths](../compilation.md#edit-paths).
- Diagnostics keep their ids the same way. A triage decision made against
  `diag:contradiction-1` survives every rebuild.

## Why identity is state

Identity is state, not something recomputed per build. Anything recomputed would churn
under a nondeterministic extractor: the same documents could yield slightly different
names, splits, or orderings on each run, and ids derived from that output would shift with
it. Minting an id once, at creation, and editing the node in place removes that failure
mode. The graph carries its identity forward; the model only proposes changes to nodes
that already have one.

## Identity of derived data and goals

Derived data is the deliberate exception. Relationships, state machines, and default
views are recomputed on every commit, and their ids are functions of their inputs:
`rel:<a>~<b>` from the member pair, `sm:<entity-slug>` from the subject, and default
views from their rule (`view:class/<node-slug>` or `view:component/<node-slug>` for a
level view, `view:class/scope-<scope>` or `view:component/scope-<scope>` for the scope
root's, `view:usecase/<node-slug>-<cluster-slug>`,
`view:sequence/<node-slug>-<cluster-slug>`, `view:state/<entity-slug>`,
`view:object/<type-slug>`), per
[default views](../model/view.md#default-views). A curated view slugs its title once,
at creation, and keeps that id like any authored node. Recomputation reproduces the
same id without minting, so a consumer holding `rel:customer~shopping-cart` still lands
on the same relationship after any number of commits. See
[derived data](../graph.md#derived-data).

Goals are derived and never stored. `g:<kind>:<target>` names one, and its `change`
record is its identity across re-derivations: the board recomputed from disk matches a
goal to its predecessor by the change, and a goal whose change record is gone is gone.
See [goal derivation](../reconciler.md#goal-derivation).

## The natural key under containment

Upserts key on a natural key, not an id, so a retried call lands on the node its first
attempt created instead of minting a duplicate. This is what makes retries harmless. The
keys per authored kind:

- Entity: `name` plus `scope`, and `parent` joins the key when the caller supplies it.
  See [entity identity](../model/entity.md#identity) and
  [scope in the natural key](./scopes.md#scope-in-the-natural-key).
- Requirement: the source section plus the punctuation-insensitive `statement`.
  Derived and decreed requirements key on `statement` alone within their `from` set.
  See [requirement identity](../model/requirement.md#identity).
- View: `kind` plus `title`. See [view identity](../model/view.md#identity).

Containment is why `parent` joins the entity key. Two parents may each contain a
same-named child (two modules, each with a `Config`), both in one scope. Keyed on `name`
and `scope` alone, the second upsert would land on the first child and fold two concepts
into one. A wrong merge is the failure to avoid: it destroys information, and the graph
cannot tell afterwards that two concepts were ever there. A duplicate is the cheaper
failure: it leaves two nodes and a finding, which a
[`review-entity`](../goals/review-entity.md) or
[`dedupe-candidates`](../goals/dedupe-candidates.md) goal repairs with `merge_entities`,
redirect left behind.

The rule for `upsert_entity`:

- An upsert that supplies `parent` matches only an entity with that `name`, `scope`, and
  `parent`. It never matches an entity with a different or missing `parent`; placing an
  existing entity under a parent is `update_entity` with `parent`, on the id.
- An upsert without `parent` matches when exactly one entity with that `name` and
  `scope` exists, whatever its `parent`.
- When several exist, the store rejects the call with an error naming the candidates
  and asking for `parent`. The caller answers by naming the parent it means. See
  [validation gates](../graph.md#validation-gates).

The natural key is also what flip detection watches: a key deleted and recreated across
builds is `unstable-extraction`, and a key that compile and GC goals flip between two
shapes is `unstable-derivation`. See [flip detection](../reconciler.md#flip-detection).

## Downstream binding

Downstream consumers (generated code, tests, tickets) bind to ids and stay bound. A
function generated for `ent:shopping-cart` still traces to the same entity after renames,
merges, re-parenting, and any number of rebuilds; the [ledger](../../consumers/gen.md#the-ledger)
keys on the same ids. Redirects cover the one case where an id retires: a consumer
holding an absorbed id follows `redirects.yaml` to the survivor. See
[storage layout](../graph.md#storage-layout).
