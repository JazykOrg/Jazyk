# TODO

This scratch space is a live document to outline the work for this documentation.

## IDEAS

- Non-code usages (e.g. writing a book, hardware, CAD, 3d printing). The gen design is
  already medium-agnostic; no non-code corpus has been run yet.

## LATER

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

- Typed relationships stay sparse under weak models despite the prompt guidance and the
  review repair pass. Add a benchmark case that gates edge declaration.
- Cross-document near-duplicate entities (`backend` vs `backend-system`) remain
  review-turn work; weak models skim. Same-doc rephrase-duplicates are caught
  deterministically (`duplicate-requirement`), and a reworded re-extraction of the same
  sentence refreshes in place.
- Grade stronger models against the extended benchmark (results persist to
  `jazyk-out/benchmark/results.yaml` with a case-set hash). No model has reached the
  `extraction` tier yet; `gemma4:e4b-mlx` scores 29-30 of 41 checks. Remote runs cost
  money, approve per run.
- A generation capability probe (does a generated unit compile and pass its named
  test) remains unbuilt; the [gen workflow](./consumers/gen.md) is gated only by the
  ledger after the fact.

## NOW

- [x] `bootstrap2` proof of concept against the example projects (see
  `bootstrap2/VALIDATION.md`).
- [x] Benchmark local models for capability (`qwen3:4b-instruct`, `gemma4:12b-mlx`:
  not capable; the harness holds regardless).
- [x] Measure the declarative-obligation doctrine: re-dogfood density vs the strict run
  (see `bootstrap2/VALIDATION.md`, the density experiment).
- [x] Stronger-model comparison through LocalRouter (`gpt-5.5`: density, end to end,
  self-hosting, stability; see `bootstrap2/VALIDATION.md`).
- [x] Converge the docs2 dogfood: 185/185 sections covered, 0 errors, 0 parked. The
  `wrong-document` gate held without a read-only citation relaxation; the decision is
  recorded in [validation gates](./compiler/graph.md#validation-gates).
- [ ] Full docs2 gen and verify run to verdicts with a capable model
  (`jazyk-out/gen/ledger.yaml` holds 106 rows with no verdicts).
