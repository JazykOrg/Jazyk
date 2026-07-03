# TODO

This scratch space is a live document to outline the work for this documentation.

## IDEAS

- Non-code usages (e.g. writing a book, hardware, CAD, 3d printing).

## LATER

- Viewer for the graph: browse entities, requirements, and diagnostics from
  `jazyk-out/graph/`.
- Embeddings-backed search behind the same [`search` tool](./compiler/tools.md#read-tools),
  same interface, no schema change.
- Relationship cardinality on [derived edges](./compiler/graph.md#derived-data).
- Per-entity file sharding for the [graph store](./compiler/graph.md#storage-layout), when
  `entities.yaml` grows too large.
- [Journal](./compiler/graph.md#journal) rotation: cap `journal/` growth, compact old
  changesets.
- Custom [format handlers](./compiler/parsing.md#format-handlers); Markdown is the only
  handler today.
- Checks-side sweep of the project [lint rules](./compiler/project-settings.md); they run
  in review turns only.

## NEXT

- Entity mentions accumulate only on `upsert_entity`; a turn that reuses an entity in a
  requirement adds no mention, so cross-doc presence reads better from requirement
  sources. Consider a derived mention on reuse.
- Models rarely declare requirement `edges`, so typed relationships stay sparse and
  reachability leans on shared requirements. Consider stronger prompting or a review
  pass that proposes edges.
- Rephrase-duplicates (same statement, different word order) pass the requirement
  natural key; review turns should catch them, weak models skim.
- Documents whose prose cites sibling documents heavily can park on the
  `wrong-document` gate (`compiler/model/diagnostic.md`, `compiler/concepts/identity.md`
  in the dogfood). Decide whether reconcile turns may cite linked documents read-only.
- Every graded model fails [turn-converge](./benchmark/cases/turn-converge.md) by staging
  harmless coverage re-claims. Decide whether the case is over-strict (allow idempotent
  re-claims) or the discipline is worth keeping.

## NOW

- [x] `bootstrap2` proof of concept against the example projects (see
  `bootstrap2/VALIDATION.md`).
- [x] Benchmark local models for capability (`qwen3:4b-instruct`, `gemma4:12b-mlx`:
  not capable; the harness holds regardless).
- [ ] Measure the declarative-obligation doctrine: re-dogfood density vs the strict run.
- [ ] Stronger-model comparison through LocalRouter.
