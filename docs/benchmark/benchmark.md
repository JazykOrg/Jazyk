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
- A case passes only when its turn completes. An aborted turn (endpoint error, a
  rejected-call streak, an exhausted round budget) fails the case with the abort
  reason. Its checks are skipped and count as failed in the score: an aborted turn
  stages nothing, and an untouched fixture satisfying a check is not evidence. In
  particular, cases whose expected outcome is zero staged mutations must not pass
  vacuously on a turn that never ran.

## Report

The grade is a scale, not a boolean: a model that finds four of five planted
requirements is worth knowing about, and a pass/fail verdict would erase the
difference. Per codec, the benchmark reports:

- per-case scores: checks passed over checks total (0 to 1), with the first failing
  check named. An aborted turn scores 0 with the abort reason.
- per-tier scores: the mean case score for `extraction`, `review`, `generation`, and
  `verification`. These are the scale results; a tier at 0.8 says most of the skill
  is there and names what is missing.
- a verdict per workflow, routing work instead of rejecting models:
  - compilation: `not-capable`, `extraction` (can drive `reconcile-doc` turns), or
    `review` (can also be trusted with review judgment). A tier is held when every
    one of its cases scores 1.
  - generation: `capable` when every generation-tier case scores 1, else
    `not-capable`. A capable model writes real files with the file tools, records an
    honest manifest, and its tests are falsifiable.
  - verification: `capable` when every verification-tier case scores 1: the model
    judges a satisfied criteria file `pass` AND a violated one `fail`. A model that
    says pass to everything is exactly what this tier exists to catch.
- efficiency: per case, the rounds and completion tokens spent, and the ratio of the
  case's `par_rounds` (the rounds a competent model needs, part of the case
  definition) to the rounds used, capped at 1. The codec's efficiency is the mean
  over completed cases. Efficiency never gates a verdict; a correct but wasteful
  model is routed with open eyes, and the token numbers say what a build will cost.
- throughput: a blended token rate (tokens/s), completion tokens over wall time
  across all rounds.
- A codec where no turn ever produced a completion (e.g. the endpoint rejects every
  call) is reported as `unmeasured`, not `not capable`. Nothing was graded, so the
  codec gets no verdict and no results entry. When both codecs are unmeasured the
  run writes no results and exits non-zero.

## Results file

Every run writes `<out>/benchmark/results.yaml`, one entry per model:

- the workflow verdicts, tier scores, efficiency, throughput, and each case's score,
  rounds, tokens, and first failing check, per codec,
- `caseSetHash`: a hash over every embedded case definition. Two results compare only
  when their hashes match; a verdict quoted without its hash is stale after any case
  edit,
- `gradedAt`: unix seconds of the run.

The entry updates in place per model. History lives in the scorecard
(`bootstrap/VALIDATION.md`), not the artifact.

Unmeasured codecs are omitted from the entry, and a run where both codecs are
unmeasured does not touch the file. A dead endpoint never overwrites a real grade.
See [report](#report).

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
- Code-block extraction: a fenced pseudo-code block yields one requirement per
  behavioral step. Prose skill does not predict this one; the sort fixture failed at
  0% under a model the prose cases scored at 0.93. See
  [turn-steps](./cases/turn-steps.md).
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

Generation tier (a real `generate-entity` [turn](../compiler/turns.md#generation-turns)
against a fixture graph and a temp deliverable):

- Product and manifest honesty: the turn writes real files with the file tools,
  records the manifest, and every recorded file exists with its marker sites anchored.
  See [gen-basic](./cases/gen-basic.md).
- Test falsifiability: each recorded programmatic command passes as recorded, and
  fails after the harness plants a break (a mandated exact value replaced in the
  product). A test that passes either way scores nothing.
  See [gen-basic](./cases/gen-basic.md#checks).

Verification tier (the llm-judge path over a planted ledger row):

- Judged pass: criteria whose implementing file satisfies the statement come back
  `pass`. See [verify-judge](./cases/verify-judge.md).
- Judged fail: criteria whose implementing file violates the statement come back
  `fail`. The pair is the point: a sycophant passes the first and flunks the second.

Tool-call fidelity has no dedicated case. Every case exercises it: a model that cannot
emit valid calls passes nothing.

## Deterministic grading

Every check is deterministic code over the staged mutations and the resulting sandbox
state: the graph for compilation cases, the ledger, the files on disk, and the exit
codes of recorded commands for generation cases, the recorded verdict for
verification cases. There is no LLM judge grading answers. A benchmark graded by a
model would inherit the weakness it is supposed to measure; the verification tier
grades a model's judgment against planted ground truth, which is judgment measured
by code.
