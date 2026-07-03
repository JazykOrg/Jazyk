# Code generation

Code generation turns the semantic graph into code. It reads the graph through the
[context engine](../compiler/context.md) and the [read tools](../compiler/tools.md#read-tools),
never the raw source files. The graph, not the prose, is the spec.

## The entity is the unit of generation

Each [entity](../compiler/model/entity.md) generates one code unit. The generator assembles
the entity's [context pack](../compiler/context.md#request): its `definition`, its
requirements across all documents, and its relationships. That pack is the entire input to
one bounded generation task. Nothing outside the pack leaks in, so each task is small,
repeatable, and auditable.

## Order from relationships

[Relationships](../compiler/model/relationship.md) give structure and order:

- `composition` → ownership and nesting.
- `association` → references.
- `dependency` → imports or injection.

Generation runs in topological order over the relationship edges: leaf entities (value
objects) first, then the entities that compose or depend on them. Each task can reference
already generated units by entity id.

## Incremental regeneration

Entity ids are stable ([identifiers](../compiler/model.md#identifiers)), so the generator
keeps a map from entity id to code unit. On rebuild:

- Only entities whose facts changed regenerate. The [journal](../compiler/graph.md#journal)
  names them; nothing else is touched.
- A merged entity leaves a redirect ([mutations](../compiler/graph.md#mutations)). The
  generator follows it and folds the absorbed unit into the survivor's.
- A renamed entity keeps its id, so its code unit is migrated in place, not rewritten.

## Dense entities generate in parts

A stringent component legitimately carries 50 or more requirements, and one generation
call has an output ceiling. The unit stays one file; the generation divides:

- The first part generates the module's types, state, and the first group of
  requirements.
- Each further part receives the code generated so far and the next group of
  requirements, and returns only additional code (a further `impl` block) to append.
- Parts concatenate into the one code unit; traceability comments per requirement are
  unaffected.

The group size defaults to 20 requirements per part. The `entity-too-dense` check warns
the author when an entity approaches the configured ceiling
([limits](../compiler/project-settings.md#limits)), so splitting the documentation into
subsections stays a choice, not an emergency.

## Incremental regeneration in practice

`codegen/state.yaml` in the out directory maps each generated entity to a hash of its
facts (definition plus its requirements). A rerun skips entities whose hash is
unchanged, so a docs edit regenerates only the entities it touched. `--force`
regenerates everything.

## Command

`jazyk codegen [entity...]` generates code units into `<out>/codegen/`. See
[CLI](../frontends/cli.md).

- With no arguments it generates every entity that has at least one requirement, in
  topological order over the relationship edges.
- Each unit is one file named by the entity slug, e.g. `codegen/graph-store.rs`. The
  file opens with a header comment carrying the entity id and the requirement ids it
  implements: the traceability key downstream tooling binds to.
- `--lang` picks the target language; the default is `rust`.

## Pluggable workers

Generation is defined by the [generation tools](../compiler/tools.md#generation-tools),
not by the built-in command. `jazyk codegen` is one worker: it asks for the pending
list and the task packages in-process, calls the configured model, and marks units
done. An external agent connected to [`jazyk mcp graph`](../frontends/mcp.md) is
another worker with the same contract: same instructions, same context, same change
diffs, same state. Whichever worker runs, the unit and its traceability comments come
out the same shape.

## Forced decisions

Generation sometimes must choose a value the documents never stated (a default, a limit, a
format). Each forced decision is recorded as a diagnostic on the entity and fed back to the
docs by [documentation generation](./docsgen.md), so the spec converges toward stating what
the code does. Generated code is verified by [test generation](./testgen.md), which derives
tests from the same requirements.
