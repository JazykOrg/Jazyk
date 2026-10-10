# e2e-gc expected outcomes (not part of the docs glob)

A fixture for the end-to-end GC burst test in `bootstrap/src/reconcile/e2e.rs`. Each
document plants one crossing of the limits registry (`bootstrap/src/limits.rs`), so a
scripted compile runs every GC kind the registry resolves them with. Nothing here is
graded by a model: the scripted runner extracts one requirement per sentence and the
harness derives the goals.

## What each document plants

- `system.md` (the root, link level 0): the Platform «system» and the Data Layer. Links
  `services.md`, so the services exist before the storage rules name them.
- `services.md` (level 1): sixteen `The Platform contains the X service.` sentences
  under two headings, `Storefront` (eight) and `Back Office` (eight). The Platform's
  level crosses `children-per-entity` hard (16 > 15), so a mandatory `abstract-entity`
  goal derives on `ent:platform` in its fan-out form. The headings are the partition the
  script groups by: two groupings named `Storefront` and `Back Office`, each with
  derived provenance from its eight members and a `ratify` goal blocked on a human.
  Links `storage.md` (level 2). The services carry two-word names (`Auth Service`,
  `Billing Service`) on purpose: all sixteen share the token `Service`, and once the
  storage rules mention them in a second document a plain Dice overlap would score every
  pair of them as lookalikes (tokens 0.5, documents 1, score 0.63), 120 dedupe goals.
  The inverse document frequency weight on `service` (held by 16 of the graph's 25
  entities, weight 0.42) brings each pair to tokens 0.3, score 0.47, under the
  threshold, so none derives: the audit pair below is the only dedupe goal of the build.
- `storage.md`: five stores under the Data Layer, then one access sentence per service
  (`The Auth Service reads the User Store.`). Each access sentence names two entities and
  opens `declare-edges`; the declared edges make every service an outside member of the
  Data Layer's level view, so `view:class/data-layer` holds 5 children plus 16 outside
  entities (21 > 20) and an optional `split-view` goal derives. The script moves the
  outside entities to a sub-view and excludes them from the level view with notes
  naming it. The Platform's own level view crosses the same limit meanwhile (16 children
  plus the stores) and its `split-view` goal is blocked with `fan-out first`; the
  grouping brings it back under the limit without a session. The stores share the
  token `Store` but are mentioned in this document alone, so no pair of them spans two
  documents and none is a lookalike candidate.
- The lookalike pair: `Audit Log` and `Audit Trail`. The Billing service appends to the
  Audit Log in `services.md` and `storage.md`; the Reporting service exports the Audit
  Trail in `services.md` alone. Two name tokens sharing one (`audit`): tokens 0.5,
  documents 0.5 (one shared document of two), score 0.5, the threshold, so a
  `dedupe-candidates` goal derives once both reviews close. One shared token of two is
  under the review hint's bar, so neither review lists the other as a lookalike and the
  pair reaches the goal instead of being remembered by a review's `separate` verdict.
  Neither name is a qualifier variant of the other, so the near-duplicate gate lets both
  through without a note. The script keeps them separate.

## After the burst

- `ent:platform` has two children, the groupings, each `derived` from exactly its
  members with the Platform as `parent`; the sixteen services sit under them.
- `view:class/data-layer` is curated, shows its five stores, and excludes the sixteen
  services with notes naming the sub-view that lists them; the sub-view and the level
  views of the two groupings render to `.puml` and `.svg`.
- Both audit entities survive; `status.yaml` remembers the pair under `lookalikes` with
  its score 0.5, no `lookalike` record stands, and the pair does not derive again.
- The build is `converged` with the two `ratify` goals and the `bind` goals riding as
  `blocked`; a rebuild derives zero goals and runs no session.
