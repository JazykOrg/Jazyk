# Benchmark

[Sessions](../compiler/sessions.md) are agent-agnostic: any
[ACP agent](../frontends/acp.md#agents) can drive them. The benchmark decides whether
a specific configuration (an agent profile, and for the embedded agent a model and
endpoint) is capable of doing so. A weak model does not fail loudly, it fills the
graph with junk. The benchmark catches that before a build does.

`jazyk benchmark` runs every [case](./cases.md) against the configured agent, the
same configuration a build would use. See [CLI](../frontends/cli.md#jazyk-benchmark).

## Deferred cases

Benchmark cases per goal kind are deferred until after the landing: real use shows
what needs grading before cases are written for it. The case set grades four kinds:
`reconcile-section`, `review-entity`, `generate`, and `verify`. Every other kind in the
[goal catalog](../compiler/reconciler.md#goal-derivation) (`place-anchors`,
`rejudge-pair`, `retrace`, `conform-instance`, and the GC kinds) has no case, and no
case grades transition facets, attributes, edge cardinality, or view curation. Verdict
quality is the first thing the deferred cases measure: a judgment gate checks that a
verdict with reasoning exists, never that the verdict is true, so only a planted ground
truth can grade it.

The harness keeps compiling meanwhile: a case keys on a goal kind and a target, one
session resolves that goal in a sandbox, and requirement checks match the `statement`
field. See [cases](./cases.md#case-format).

## Runs

- `jazyk benchmark` grades the `embedded` profile: every case runs under both
  codecs, `native` first, then `text` (the embedded agent's two ways of speaking to
  an endpoint, see [the embedded agent](../frontends/acp.md#the-embedded-agent)),
  on the same generic loop the agent runs. A model can be capable under one codec
  and not the other. An external agent has no codec axis; it is graded through the
  [agent-run serving](#agent-run-benchmarks) (`jazyk mcp benchmark`), one column per
  agent.
- A case run is one real [session](../compiler/sessions.md#anatomy) in a sandbox
  store, holding exactly one goal: the case's kind on the case's target. Same tool
  registry, same [validation gates](../compiler/graph.md#validation-gates), same
  [budgets](../compiler/sessions.md#budgets), same one mid-batch reminder when the
  session ends in prose without finishing
  ([worker sessions](../frontends/acp.md#worker-sessions)). Only the store and the
  fixture differ from a build.
- Cases never touch the project graph. See [execution](./cases.md#execution).
- A case passes only when its session completes. An aborted session (endpoint error,
  a rejected-call streak, an exhausted round budget) fails the case with the abort
  reason. Its checks are skipped and count as failed in the score: an aborted session
  stages nothing, and an untouched fixture satisfying a check is not evidence. In
  particular, cases whose expected outcome is zero staged mutations must not pass
  vacuously on a session that never ran.
- A goal the model marks failed (`mark_goal_failed`) fails its case. Every fixture is
  resolvable by design, so a failure claim measures the model, not the fixture.

## Running a subset

`jazyk benchmark [case...]` grades only the named cases (e.g. `jazyk benchmark
density reuse`), for iterating on one failure without paying for the full set. An unknown case name is an error that lists the available names. Verdicts still
print but a filtered run is never a capability grade; the history entry is skipped.
A tier with no graded case prints `-` for its score and verdict instead of
claiming a result it never measured.

Before grading, one tiny completion probes the endpoint: a dead or misrouted
endpoint fails one probe, not every case under both codecs.

Every case session writes its full message transcript to
`<out>/benchmark/trace/<codec>-<case>.json`, so a failed check is diagnosed from
what the model actually saw and said, not from the score line.

## Report

The grade is a scale, not a boolean: a model that finds four of five planted
requirements is worth knowing about, and a pass/fail verdict would erase the
difference. Per codec, the benchmark reports:

- per-case scores: checks passed over checks total (0 to 1), with the first failing
  check named. An aborted session scores 0 with the abort reason.
- per-tier scores: the mean case score for `extraction`, `review`, `generation`, and
  `verification`. These are the scale results; a tier at 0.8 says most of the skill
  is there and names what is missing.
- a verdict per workflow, routing work instead of rejecting models:
  - compilation: `not-capable`, `extraction` (can drive
    [`reconcile-section`](../compiler/goals/reconcile-section.md) sessions), or
    `review` (can also be trusted with judgment kinds; the graded one is
    [`review-entity`](../compiler/goals/review-entity.md)). A tier is held when every
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
- A codec where no session ever produced a completion (e.g. the endpoint rejects
  every call) is reported as `unmeasured`, not `not capable`. Nothing was graded, so
  the codec gets no verdict and no results entry. When both codecs are unmeasured the
  run writes no results and exits non-zero.

A local endpoint's grade varies run to run: a loaded server starts truncating or
answering prose, and a codec that scored high an hour earlier collapses to aborts. A
collapsed grade beside a recent healthy one is endpoint trouble first, model second;
rerun before concluding. The scorecard keeps history for exactly this comparison.

## Results file

Every run writes `<out>/benchmark/results.yaml`, one entry per graded configuration
(agent profile, plus the model under the `embedded` profile):

- the workflow verdicts, tier scores, efficiency, throughput, and each case's score,
  rounds, tokens, and first failing check, per codec,
- `caseSetHash`: a hash over every embedded case definition. Two results compare only
  when their hashes match; a verdict quoted without its hash is stale after any case
  edit,
- `gradedAt`: unix seconds of the run.

The entry updates in place per model. Analysis history lives in the scorecard
(`bootstrap/VALIDATION.md`); the raw run history is machine-wide (below).

## Agent-run benchmarks

A coding agent is graded the same way an endpoint is: by performing the cases. The
`jazyk mcp benchmark` [toolset](../compiler/tools.md#toolsets) serves each case as a
claimable goal against a throwaway sandbox store, and the agent under test does the
work with the same write tools a compile session holds:

- `benchmark_cases()`: the case list with each case's tier and state (pending, scored,
  open), and the run's progress.
- `begin_case({case?})`: claim the named case or the first pending one. The reply has
  the shape a `begin_goals` reply has
  ([compilation over MCP](../frontends/mcp.md#compilation-over-mcp)): top-level
  `instructions` (the assembled session prompt for the case's goal) plus a `package`
  (the loaded set), built from the case fixture (including the case's own lint rules,
  not the serving project's). A generation case names the sandbox deliverable
  directory by absolute path: the agent writes there with its own file tools, records
  with `record_generation`, and proves the work with `run_tests`, all against the
  sandbox. A verification case's package carries the statement, the quote, and the
  implementing file paths; the agent judges and passes its verdict to `finish_case`.
- The write tools stage into the open case's sandbox exactly as compilation over MCP
  stages into the project, gated by the case's goal kind.
- `finish_case({summary?, verdict?, evidence?})`: run the goal's gate and the `done`
  gates, apply the staged work to the sandbox, grade with the case's deterministic
  checks, and return the score with the first failing check named. The sandbox is
  discarded either way.
- `benchmark_report({model})`: after the last case, compute tier scores and workflow
  verdicts, append the run to the [machine-wide history](#machine-wide-history), and
  return the report. `model` names the agent honestly (e.g. `claude-sonnet-4.6
  (agent)`); the client name from `initialize` is the default. The reply carries a
  legend for the verdict scale (`not-capable < extraction < review` for compilation,
  `review` being the highest), so a driver reads the ordering without consulting
  these docs.

Grading is identical to an endpoint run with two substitutions, both recorded on the
entry: the codec is `agent` (a third column beside `native` and `text`), and rounds
count the agent's tool calls per case (tokens are unknowable from outside the agent
and stay null). Efficiency against par therefore compares call discipline, not
context cost, and is comparable only within the `agent` codec: an in-process session
batches several calls into one round, so `par_rounds` undercounts what honest
sequential MCP calls need. The checks, the scores, and the verdicts are the same code
path, so an agent's grade sits in the same table as an endpoint's.

## Machine-wide history

Every run also appends one entry to `~/.jazyk/benchmarks/history.yaml`, keyed by
nothing and never overwritten: model, endpoint, `gradedAt`, `caseSetHash`, and the
per-codec report. Grades outlive the project that produced them, so a model graded
once is comparable everywhere, and endpoint variance shows up as history instead of
overwriting itself.

Known results ship in the binary: `docs/benchmark/known-results.yaml` is embedded at
compile time, so a fresh install compares its local model against curated grades
(popular models, both codecs) before running anything. An embedded entry is marked
`source: embedded`; a locally graded model with the same `caseSetHash` sits beside it
and never overwrites it. Curation is manual: a run worth publishing is copied into the
file by hand.

The comparison surface is the [GUI benchmarks tab](../frontends/gui.md#benchmarks):
configure the endpoint, pick a model, kick off a run, and compare grades across
models and codecs.

Unmeasured codecs are omitted from the entry, and a run where both codecs are
unmeasured does not touch the file. A dead endpoint never overwrites a real grade.
See [report](#report).

## Graded skills

Extraction tier (one `reconcile-section` goal batch covering one fixture document):

- Tool-call fidelity: every call is syntactically valid and schema-correct under the
  codec in use.
- Extraction sanity: the model finds the planted requirements and entities and creates
  no junk nodes. See [extract](./cases/extract.md).
- Declarative extraction: the model recognizes obligations stated without `shall` and
  restates them as statements
  ([declarative prose states obligations](../compiler/concepts/statements.md#declarative-prose-states-obligations)).
  See [declarative](./cases/declarative.md).
- Extraction density: plain declarative prose (technology choices, enumerations,
  access rules) yields atomic requirements at recall, not a non-normative pass.
  See [density](./cases/density.md).
- Edge declaration: a sub-system list becomes typed, directional edges, not just
  prose. See [edges](./cases/edges.md).
- Code-block extraction: a fenced pseudo-code block yields one requirement per
  behavioral step. Prose skill does not predict this one; the sort fixture failed at
  0% under a model the prose cases scored at 0.93. See
  [steps](./cases/steps.md).
- Restraint: a glossary of outside-world terms and a roadmap of wishes state no
  obligations; the model marks both sections `non-normative` and mints nothing (no
  `ent:glossary`, no term extracted as a requirement). See
  [navigation](./cases/navigation.md).
- Reuse discipline: the model searches before creating and reuses the existing entity.
  See [reuse](./cases/reuse.md).
- Repair: the model reads a rejection message and fixes the call. See
  [repair](./cases/repair.md).
- Convergence discipline: the model stages zero mutations on an already-reconciled
  section. See [converge](./cases/converge.md).

Review tier (one `review-entity` goal):

- Review judgment: the model flags a planted contradiction and stays quiet on clean
  input. See [review](./cases/review.md).
- Rephrase-duplicates: two requirements stating one fact in different words collapse
  to one. The deterministic `duplicate-requirement` check catches reordered tokens;
  this case plants a pair below its overlap threshold, so only judgment finds it.
  See [review-duplicate](./cases/review-duplicate.md).
- Lookalike entities: two entities that are one concept merge, aliases kept.
  See [review-lookalike](./cases/review-lookalike.md).
- Lint application: a project lint rule is reported where it fires and nowhere else.
  See [review-lint](./cases/review-lint.md).

Generation tier (a real [`generate`](../compiler/goals/generate.md) goal session
against a fixture graph and a temp deliverable):

- Product and manifest honesty: the session writes real files with the file tools,
  records the manifest, and every recorded file exists with its marker sites anchored.
  See [gen-basic](./cases/gen-basic.md).
- Test falsifiability: each recorded programmatic command passes as recorded, and
  fails after the harness plants a break (a mandated exact value replaced in the
  product). A test that passes either way scores nothing.
  See [gen-basic](./cases/gen-basic.md).

Verification tier (the llm-judge path of the [`verify`](../compiler/goals/verify.md)
goal over a planted ledger row):

- Judged pass: criteria whose implementing file satisfies the statement come back
  `pass`. See [verify-judge](./cases/verify-judge.md).
- Judged fail: criteria whose implementing file violates the statement come back
  `fail`. The pair is the point: a sycophant passes the first and flunks the second.

Tool-call fidelity has no dedicated case. Every case exercises it: a model that cannot
emit valid calls passes nothing.

The case names under `cases/` are fixture file stems (`extract`, `gen-basic`,
`verify-judge`); `jazyk benchmark [case...]` and `begin_case` take them as written.

## Deterministic grading

Every check is deterministic code over the staged mutations and the resulting sandbox
state: the graph for compilation cases, the ledger, the files on disk, and the exit
codes of recorded commands for generation cases, the recorded verdict for
verification cases. There is no LLM judge grading answers. A benchmark graded by a
model would inherit the weakness it is supposed to measure; the verification tier
grades a model's judgment against planted ground truth, which is judgment measured
by code.
