# Cases

A case is one predefined test: a fixture, one turn to run, and deterministic assertions
about what the turn did. Each case lives under [`cases/`](./cases) as a markdown file:
an H1, a paragraph stating the skill it grades, and a fenced `yaml` block holding the
case definition per [`case.schema.yaml`](./case.schema.yaml). A file may hold more than
one `yaml` block; each block is one case.

## Case format

- `name`: unique case name, usually the file stem.
- `description`: one sentence stating the skill the case grades.
- `task`: the turn to run. `type` is `reconcile-doc` or `review-entity`, `target` is a
  document path or an entity id. See [task types](../compiler/turns.md#task-types).
- `given`: the fixture.
  - `docs`: map of document path → markdown text. These are the only source files the
    case sees.
  - `graph` (optional): nodes pre-seeded into the sandbox store before the turn runs:
    `entities` and `requirements` maps keyed by id, and a `coverage` map of section
    reference → state.
- `assert`: an array of checks. All must pass. Each check is deterministic and runs over
  the staged mutations and the resulting graph. E.g.:
  - an entity named `Cart` exists,
  - no entity whose name matches `^--`,
  - zero mutations staged,
  - a diagnostic with rule `contradiction` and subject `ent:abc` exists.

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

- [turn-extract](./cases/turn-extract.md): extraction sanity.
- [turn-declarative](./cases/turn-declarative.md): declarative extraction.
- [turn-reuse](./cases/turn-reuse.md): reuse discipline.
- [turn-converge](./cases/turn-converge.md): convergence discipline.
- [turn-repair](./cases/turn-repair.md): repair.
- [turn-review](./cases/turn-review.md): review judgment, one planted contradiction and
  one clean entity.
