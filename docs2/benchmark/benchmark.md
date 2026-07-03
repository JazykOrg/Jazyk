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

- a verdict: `capable` when every case passes, `not capable` otherwise,
- a score: the fraction of checks passed across all cases,
- per-case results: pass or fail, with the first failing check named,
- throughput: a blended token rate (tokens/s), completion tokens over wall time across
  all rounds.

Throughput does not gate the verdict. It is reported so a correct but slow model is
visible before a full build is attempted.

## Graded skills

- Tool-call fidelity: every call is syntactically valid and schema-correct under the
  codec in use.
- Extraction sanity: the model finds the planted requirements and entities and creates
  no junk nodes. See [turn-extract](./cases/turn-extract.md).
- Declarative extraction: the model recognizes obligations stated without `shall` and
  rephrases them into EARS. See [turn-declarative](./cases/turn-declarative.md).
- Reuse discipline: the model searches before creating and reuses the existing entity.
  See [turn-reuse](./cases/turn-reuse.md).
- Repair: the model reads a rejection message and fixes the call. See
  [turn-repair](./cases/turn-repair.md).
- Convergence discipline: the model stages zero mutations on an already-reconciled
  section. See [turn-converge](./cases/turn-converge.md).
- Review judgment: the model flags a planted contradiction and stays quiet on clean
  input. See [turn-review](./cases/turn-review.md).

Tool-call fidelity has no dedicated case. Every case exercises it: a model that cannot
emit valid calls passes nothing.

## Deterministic grading

Every check is deterministic code over the staged mutations and the resulting sandbox
graph. There is no LLM judge. A benchmark graded by a model would inherit the weakness
it is supposed to measure.
