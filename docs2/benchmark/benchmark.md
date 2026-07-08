# Benchmark

The [turn harness](../compiler/turns.md) is model-agnostic: any endpoint that speaks one
of its [codecs](../compiler/turns.md#codecs) can drive compilation turns. The benchmark
decides whether a specific model is capable of doing so. A weak model does not fail
loudly, it fills the graph with junk. The benchmark catches that before a build does.

`jazyk benchmark` runs every [case](./cases.md) against the configured model and
endpoint, the same configuration a build would use. See [CLI](../frontends/cli.md).

## Runs

- Every case runs under both codecs: `native` first, then `text`. See
  [codecs](../compiler/turns.md#codecs). A model can be capable under one and not the
  other.
- A case run is one real [turn](../compiler/turns.md#anatomy) in a sandbox store: same
  tool registry, same [validation gates](../compiler/graph.md#validation-gates), same
  budgets. Only the store and the fixture differ from a build.
- Cases never touch the project graph. See [execution](./cases.md#execution).

## Report

Per codec, the benchmark reports:

- a verdict, the highest tier whose cases all pass. Tiers route work instead of
  rejecting models:
  - `not capable`: an extraction-tier case fails. The harness still holds (gates
    bounce junk), but graph density and judgment degrade.
  - `extraction`: every extraction-tier case passes. The model can drive
    `reconcile-doc` turns.
  - `review`: extraction plus every review-tier case passes. The model can also be
    trusted with `review-entity` judgment.
- a score: the fraction of checks passed across all cases,
- per-case results: pass or fail, with the first failing check named,
- throughput: a blended token rate (tokens/s), completion tokens over wall time across
  all rounds.

Throughput does not gate the verdict. It is reported so a correct but slow model is
visible before a full build is attempted.

Generation quality (the [gen workflow](../consumers/gen.md)) is not yet graded; the
verification ledger reports it truthfully after the fact.

## Results file

Every run writes `<out>/benchmark/results.yaml`, one entry per model:

- verdict, score, throughput, and each case's pass or first failing check, per codec,
- `caseSetHash`: a hash over every embedded case definition. Two results compare only
  when their hashes match; a verdict quoted without its hash is stale after any case
  edit,
- `gradedAt`: unix seconds of the run.

The entry updates in place per model. History lives in the scorecard
(`bootstrap2/VALIDATION.md`), not the artifact.

## Graded skills

Extraction tier:

- Tool-call fidelity: every call is syntactically valid and schema-correct under the
  codec in use.
- Extraction sanity: the model finds the planted requirements and entities and creates
  no junk nodes. See [turn-extract](./cases/turn-extract.md).
- Declarative extraction: the model recognizes obligations stated without `shall` and
  rephrases them into EARS. See [turn-declarative](./cases/turn-declarative.md).
- Extraction density: plain declarative prose (technology choices, enumerations,
  access rules) yields atomic requirements at recall, not a non-normative wave-through.
  See [turn-density](./cases/turn-density.md).
- Edge declaration: a sub-system list becomes typed relationships, not just prose.
  See [turn-edges](./cases/turn-edges.md).
- Reuse discipline: the model searches before creating and reuses the existing entity.
  See [turn-reuse](./cases/turn-reuse.md).
- Repair: the model reads a rejection message and fixes the call. See
  [turn-repair](./cases/turn-repair.md).
- Convergence discipline: the model stages zero mutations on an already-reconciled
  section. See [turn-converge](./cases/turn-converge.md).

Review tier:

- Review judgment: the model flags a planted contradiction and stays quiet on clean
  input. See [turn-review](./cases/turn-review.md).
- Rephrase-duplicates: two requirements stating one fact in different words collapse
  to one. The deterministic `duplicate-requirement` check catches reordered tokens;
  this case plants a pair below its overlap threshold, so only judgment finds it.
  See [turn-review-duplicate](./cases/turn-review-duplicate.md).
- Lookalike entities: two entities that are one concept merge, aliases kept.
  See [turn-review-lookalike](./cases/turn-review-lookalike.md).
- Lint application: a project lint rule is reported where it fires and nowhere else.
  See [turn-review-lint](./cases/turn-review-lint.md).

Tool-call fidelity has no dedicated case. Every case exercises it: a model that cannot
emit valid calls passes nothing.

## Deterministic grading

Every check is deterministic code over the staged mutations and the resulting sandbox
graph. There is no LLM judge. A benchmark graded by a model would inherit the weakness
it is supposed to measure.
