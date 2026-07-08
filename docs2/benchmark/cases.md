# Cases

A case is one predefined test: a fixture, one turn to run, and deterministic assertions
about what the turn did. Each case lives under [`cases/`](./cases) as a markdown file:
an H1, a paragraph stating the skill it grades, and a fenced `yaml` block holding the
case definition per [`case.schema.yaml`](./case.schema.yaml). A file may hold more than
one `yaml` block; each block is one case.

## Case format

- `name`: unique case name, usually the file stem.
- `description`: one sentence stating the skill the case grades.
- `tier`: `extraction` (the default) or `review`. The verdict is the highest tier whose
  cases all pass. See [report](./benchmark.md#report).
- `task`: the turn to run. `type` is `reconcile-doc` or `review-entity`, `target` is a
  document path or an entity id. See [task types](../compiler/turns.md#task-types).
- `given`: the fixture.
  - `docs`: map of document path → markdown text. These are the only source files the
    case sees.
  - `graph` (optional): nodes pre-seeded into the sandbox store before the turn runs:
    `entities` and `requirements` maps keyed by id, and a `coverage` map of section
    reference → state.
  - `lint` (optional): project [lint rules](../compiler/project-settings.md#linting)
    the turn runs under, as `warnings` and `errors` lists.
- `assert`: an array of checks. All must pass. Each check is deterministic and runs over
  the staged mutations and the resulting graph. Patterns are regular expressions,
  matched case-insensitively. E.g.:
  - an entity named `Cart` exists,
  - no entity whose name matches `^--`,
  - zero mutations staged,
  - at least 6 requirements referencing `ent:frontend`,
  - a `composition` relationship between two named entities,
  - a diagnostic with rule `contradiction` and subject `ent:abc` exists (`subject` is
    optional: without it, any open diagnostic with the rule passes).

## Execution

- Each case runs in a fresh sandbox store seeded from `given`. The project graph is
  never touched.
- The harness runs exactly one turn: `task.type` on `task.target`, with the standard
  [task toolset](../compiler/tools.md#task-toolsets),
  [budgets](../compiler/turns.md#budgets), and
  [validation gates](../compiler/graph.md#validation-gates).
- Checks run after the turn commits or aborts. An aborted turn stages nothing, so the
  checks see the fixture as-is.

## Index

Extraction tier:

- [turn-extract](./cases/turn-extract.md): extraction sanity.
- [turn-declarative](./cases/turn-declarative.md): declarative extraction.
- [turn-density](./cases/turn-density.md): extraction density on plain declarative
  prose.
- [turn-edges](./cases/turn-edges.md): edge declaration from a sub-system list.
- [turn-reuse](./cases/turn-reuse.md): reuse discipline.
- [turn-converge](./cases/turn-converge.md): convergence discipline.
- [turn-repair](./cases/turn-repair.md): repair.

Review tier:

- [turn-review](./cases/turn-review.md): review judgment, one planted contradiction and
  one clean entity.
- [turn-review-duplicate](./cases/turn-review-duplicate.md): rephrase-duplicate
  collapse.
- [turn-review-lookalike](./cases/turn-review-lookalike.md): lookalike entity merge.
- [turn-review-lint](./cases/turn-review-lint.md): lint application.
