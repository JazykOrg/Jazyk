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
  Links `storage.md`, `audit.md`, and `compliance.md` (level 2). The services carry
  one-word names (`Auth`, `Billing`) on purpose: a shared generic token such as
  `Service` would score every pair of them as lookalikes once the storage rules mention
  them in a second document (tokens 0.5, documents 1, score 0.63), 120 dedupe goals
  the fixture does not want.
- `storage.md`: six stores under the Data Layer, then one access sentence per service
  (`The Auth service reads the User Store.`). Each access sentence names two entities and
  opens `declare-edges`; the declared edges make every service an outside member of the
  Data Layer's level view, so `view:class/data-layer` holds 6 children plus 16 outside
  entities (22 > 20) and an optional `split-view` goal derives. The script moves the
  outside entities to a sub-view and excludes them from the level view with notes
  naming it. The Platform's own level view crosses the same limit meanwhile (16 children
  plus the stores) and its `split-view` goal is blocked with `fan-out first`; the
  grouping brings it back under the limit without a session. The stores share the
  token `Store` but are mentioned in this document alone, so no pair of them spans two
  documents and none is a lookalike candidate.
- `audit.md` and `compliance.md`: `Order Audit Log` and `Order Audit Trail`, one per
  document, three name tokens sharing two (`order`, `audit`). The lookalike score is
  0.5 (tokens 0.67, documents 0), the threshold, so a `dedupe-candidates` goal derives
  once both reviews close. Neither name is a qualifier variant of the other, so the
  near-duplicate gate lets both through without a note. The script keeps them separate.

## After the burst

- `ent:platform` has two children, the groupings, each `derived` from exactly its
  members with the Platform as `parent`; the sixteen services sit under them.
- `view:class/data-layer` is curated, shows its six stores, and excludes the sixteen
  services with notes naming the sub-view that lists them; the sub-view and the level
  views of the two groupings render to `.puml` and `.svg`.
- Both audit entities survive; the pair's goal is in the journal's resolved goals and
  does not derive again.
- The build is `converged` with the two `ratify` goals and the `bind` goals riding as
  `blocked`; a rebuild derives zero goals and runs no session.
