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

## Forced decisions

Generation sometimes must choose a value the documents never stated (a default, a limit, a
format). Each forced decision is recorded as a diagnostic on the entity and fed back to the
docs by [documentation generation](./docsgen.md), so the spec converges toward stating what
the code does. Generated code is verified by [test generation](./testgen.md), which derives
tests from the same requirements.
