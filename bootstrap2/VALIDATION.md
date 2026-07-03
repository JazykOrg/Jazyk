# Proof-of-concept validation

Results of running the turn-based compiler (docs2 design) on the fixtures, against the
success criteria set before implementation. All runs used local models through Ollama
(`qwen3:4b-instruct` unless noted). Full traces live in the session scratchpad; the
persisted evidence is each fixture's `jazyk-out/` (journal, status, graph shards).

## Scorecard

| Criterion | Result | Notes |
| --- | --- | --- |
| Zero junk entities | PASS | No file paths, CLI flags, or markdown terms in any run. The admin.md junk bait (`--port`, `/etc/orderly/config.toml`) produced none. |
| Entity precision within 20% of hand count | PARTIAL | F2: 18 vs ~10 hand-counted. The overshoot is real concepts from glossary/roadmap prose the model normativized, not syntax junk. |
| Cross-doc identity: one node used across 3+ docs | PASS | `ent:order` is one node; requirements reference it from 6 documents. Mentions accumulate only on upsert, so the mention list is thinner than the requirement evidence. |
| buyer/Customer duplicate trap | PARTIAL | No separate `buyer` entity was ever created (the old design's failure mode). The explicit buyer=Customer link was not surfaced either. |
| Planted contradiction caught, right subjects | FLAKY at 4B | Run 2: caught exactly (`req:payment-2` vs `req:orders-3`, severity error). Run 3: missed. This is model judgment variance, the capability the benchmark is designed to gate. |
| Diagnostic noise | IMPROVED, not at target | F2: 26 open diagnostics for 18 entities (about half trace to the normativized roadmap/glossary). Old design: 534 diagnostics for 564 entities with 109 errors on clean docs. |
| No-op rebuild: zero LLM calls, under 1s | PASS | 0 turns, ~10 ms, on F1 and F2. |
| One-sentence edit touches only its region | PASS | 1 reconcile turn plus 3 review turns; graph diff was purely additive (`ent:shop`, `req:catalog-3`). |
| Convergence | PASS | Every build converged or parked cleanly and converged on resume. No wedged builds. |
| MCP read surface | PASS | `context` returns budget-bounded packs with working expansion handles; "requirements between Order and Payment" is answerable from one call. |

None of the failed-again triggers fired (junk >5%, duplicates >20% after review,
non-convergence, incremental cross-contamination, local model unable to complete F1).

## Findings worth keeping

- `gemma4:e4b-mlx`, the model whose truncated JSON broke the old one-shot pipeline,
  converges cleanly through the turn loop: 0 errors, 0 warnings, 100% coverage on F1.
  The environment-side gates do the work the model cannot.
- The text codec (JSON action per message, for models without native tool support)
  drives the full loop: 55 actions parsed in one F1 run, with repair messages correcting
  bad arguments, unknown relationship types, and id typos.
- Repair-oriented errors work as designed: models read `nearest existing: ent:shop` and
  fix the call.
- Sticky diagnostics work by construction: cleared findings flip to `resolved`, ids
  never churn, triage is never touched.
- Parked work resumes: a failed turn parks its item with an `incomplete-build`
  diagnostic and the next build picks it up first.

## Fixes discovered by running (each documented in docs2 first)

- Implicit `done`: a model that goes silent with staged work commits through the same
  gates instead of losing a valid changeset (docs2/compiler/turns.md#budgets).
- Whitespace-insensitive quote location: sentences wrapped across source lines were
  failing verbatim match, 88 rejections in one run (docs2/compiler/model.md).
- Requirement natural key (source section + normalized statement): kills exact-duplicate
  requirements from retries and reruns (docs2/compiler/graph.md#mutations).
- The covered-claim gate: a section containing `shall` cannot be claimed covered without
  a requirement sourced from it; stops silent requirement loss
  (docs2/compiler/graph.md#validation-gates).
- One bounded fix-up pass re-enqueues documents with uncovered sections
  (docs2/compiler/reconciler.md#waves).
- Review turns run grouped: entities sharing requirements review in order, groups in
  parallel, so judgments see their neighbors' merges (docs2/compiler/reconciler.md#waves).
- `report_diagnostic` accepts only the cataloged review rules; free-form rule names were
  producing incomparable findings (docs2/compiler/tools.md#write-tools).

## Known weaknesses (open)

- A 4B model rewrites non-normative prose (glossaries, roadmaps) into shall statements
  despite prompt doctrine. Semantic judgment quality is the model's, not the harness's;
  `jazyk benchmark` (documented, not yet implemented) is the gate, and a stronger-model
  comparison is the next experiment.
- Rephrase-duplicates (same statement, different word order) pass the requirement
  natural key; catching them is review-turn work, which weak models skim.
- Models rarely declare `edges`, so typed relationships stay sparse; reachability
  falls back to shared requirements.
- Entity mentions accumulate only on upsert; cross-doc presence is currently better read
  from requirement sources.

## F3: the docs2 dogfood

The compiler compiled its own documentation (30 documents, benchmark fixtures excluded)
with `gemma4:e4b-mlx` through LocalRouter. Mechanics: 5 link-graph levels, ~55 turns
across two passes (parked work resumed automatically), 94% section coverage, 0 error
diagnostics, ~54k completion tokens. Two documents stayed parked: their prose cites
sibling documents heavily and the model kept citing the linked document instead of the
target, which the `wrong-document` gate rejects by design.

The graph came out clean but sparse: 10 entities, 4 requirements, zero junk. All four
requirements are the explicit `shall` statements the docs contain, extracted verbatim,
including "The graph store shall mint every id at node creation and never change it."
The finding: docs2's declarative house style ("The store mints ids...") plus the
strict EARS doctrine ("never rewrite non-normative prose into shall statements") leads
a cautious model to classify most sections as non-normative. The deterministic
`suspicious-non-normative` check pushed back on 6 of those marks. Options, deliberately
left open: loosen the extraction doctrine for declarative statements of system
behavior, push the prompt to rephrase declaratives into EARS, or adopt `shall` phrasing
in spec-grade sections. The lint rule ran (4 findings) but the 4B judge misapplied it,
consistent with its failed benchmark verdict.

## The density experiment: doctrine and capability

Three dogfood runs over the same 30 documents isolate the two variables:

| run | entities | requirements | relationships | junk | errors |
| --- | --- | --- | --- | --- | --- |
| gemma4:e4b, strict doctrine | 10 | 4 | 0 | 0 | 0 |
| gemma4:e4b, declarative doctrine | 48 | 1 | 0 | 0 | 0 |
| gpt-5.5, declarative doctrine | 157 | 415 | 305 | 0 | 0 |

The declarative doctrine ("declarative prose states obligations, rephrase into EARS")
amplifies model capability rather than substituting for it. gemma minted entities but
could not land the statements behind them (47 `unused-entity` warnings); gpt-5.5
produced a dense spec graph with typed relationships. Both models were graded by
`jazyk benchmark` beforehand: gemma not capable, gpt-5.5 18/19 checks under native
tools. The harness invariants held in every run: zero junk names, zero spurious error
diagnostics.

Operational findings from the same runs: a dense extractor exhausts a 12-round turn
budget before claiming coverage (default raised to 24, and models are told to batch
tool calls); concurrent gpt-5.5 workloads through one provider hit rate limits, so
generation jobs are sequenced, not parallelized.

## Cost

F2 (11 documents, cold build): ~30 turns, ~220 rounds, ~18k completion tokens, roughly
8 minutes wall-clock on a local `qwen3:4b-instruct`. Incremental single-edit builds run
4 turns. No-op builds are free.
