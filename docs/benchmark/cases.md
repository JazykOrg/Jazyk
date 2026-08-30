# Cases

A case is one predefined test: a fixture, one goal to resolve in one session, and
deterministic assertions about what the session did. Each case lives under
[`cases/`](./cases) as a markdown file: an H1, a paragraph stating the skill it grades,
and a fenced `yaml` block holding the case definition per
[`case.schema.yaml`](./case.schema.yaml). A file may hold more than one `yaml` block;
each block is one case.

Cases exist for four goal kinds. Cases for the other kinds are deferred until after
the landing; see [deferred cases](./benchmark.md#deferred-cases).

## Case format

- `name`: unique case name, usually the file stem.
- `description`: one sentence stating the skill the case grades.
- `tier`: `extraction` (the default), `review`, `generation`, or `verification`.
  Tier scores and workflow verdicts derive from these; see
  [report](./benchmark.md#report).
- `par` (optional): `rounds`, the rounds a competent model needs; the efficiency
  ratio compares against it. Defaults per tier when omitted.
- `goal`: the goal to run. `kind` is a goal kind and `target` is the goal's target:
  - [`reconcile-section`](../compiler/goals/reconcile-section.md): `target` is a
    document path. The session holds one batch: every section of that document, in
    document order, as it would in a build
    ([batching](../compiler/reconciler.md#batching)).
  - [`review-entity`](../compiler/goals/review-entity.md): `target` is an entity id.
  - [`generate`](../compiler/goals/generate.md): `target` is an entity id; the session
    generates into a temp deliverable.
  - [`verify`](../compiler/goals/verify.md): `target` is a requirement id; the
    llm-judge path over a planted ledger row.
- `given`: the fixture.
  - `docs`: map of document path → markdown text. These are the only source files the
    case sees.
  - `graph` (optional): nodes pre-seeded into the sandbox store before the session
    runs: `entities` and `requirements` maps keyed by id (a requirement carries
    `statement`, `entities`, and `source`, plus `edges`, `transition`, and `facets`
    where the case needs them), and a `coverage` map of section reference → state.
  - `deliverable` (optional, verification cases): map of deliverable-relative path →
    content, the implementing files the judged row names. The harness builds the
    ledger row and its criteria file from the target requirement.
  - `lint` (optional): project [lint rules](../compiler/project-settings.md#docs)
    the session runs under, as `warnings` and `errors` lists.
- `assert`: an array of checks. All must pass. Each check is deterministic and runs over
  the staged mutations and the resulting graph. Patterns are regular expressions,
  matched case-insensitively. E.g.:
  - an entity named `Cart` exists,
  - no entity whose name matches `^--`,
  - zero mutations staged,
  - at least 6 requirements referencing `ent:frontend`,
  - a requirement whose `statement` matches `empt(y|ies|ied)` and references a named
    entity,
  - a `composition` relationship between two named entities,
  - a diagnostic with rule `contradiction` and subject `ent:abc` exists (`subject` is
    optional: without it, any open diagnostic with the rule passes).

## Execution

- Each case runs in a fresh sandbox store seeded from `given`. The project graph is
  never touched.
- The harness runs exactly one session holding one goal: `goal.kind` on
  `goal.target`, with the kind's [toolset](../compiler/tools.md#toolsets), the
  session [budgets](../compiler/sessions.md#budgets), and the
  [validation gates](../compiler/graph.md#validation-gates). The session prompt is
  the one a build assembles ([the prompt](../compiler/sessions.md#the-prompt)); the
  goal's gate runs at `mark_goal_done` and at `done` exactly as in a build
  ([resolving, failing, parking](../compiler/sessions.md#resolving-failing-parking)).
- Checks run after the session commits. An aborted session fails the case with the
  abort reason; its checks are skipped and count as failed. A goal marked failed fails
  the case. See [runs](./benchmark.md#runs).

## Index

Case names are the fixture file stems and are used as written by
`jazyk benchmark [case...]`.

Extraction tier:

- [extract](./cases/extract.md): extraction sanity.
- [declarative](./cases/declarative.md): declarative extraction.
- [density](./cases/density.md): extraction density on plain declarative
  prose.
- [edges](./cases/edges.md): edge declaration from a sub-system list.
- [steps](./cases/steps.md): code-block extraction, one requirement per
  behavioral step.
- [navigation](./cases/navigation.md): restraint on a glossary and a
  roadmap, both non-normative.
- [reuse](./cases/reuse.md): reuse discipline.
- [converge](./cases/converge.md): convergence discipline.
- [repair](./cases/repair.md): repair.

Review tier:

- [review](./cases/review.md): review judgment, one planted contradiction and
  one clean entity.
- [review-duplicate](./cases/review-duplicate.md): rephrase-duplicate
  collapse.
- [review-lookalike](./cases/review-lookalike.md): lookalike entity merge.
- [review-lint](./cases/review-lint.md): lint application.

Generation tier:

- [gen-basic](./cases/gen-basic.md): product and manifest honesty, test
  falsifiability.

Verification tier:

- [verify-judge](./cases/verify-judge.md): judged pass and judged fail, as a pair.
