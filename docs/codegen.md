# Code Generation

Code generation turns the [reviewed artifact](./compiler/artifacts/reviewed-artifact.md#reviewed-artifact)
into code. It consumes the entity graph, not the raw documentation.

## Technical design

### Entity as the unit of generation

Each [entity](./compiler/model/entity.md#entity) is a unit of generation. The generator builds one
entity at a time from its assembled spec: the [global definition](./compiler/linking/synthesize-definitions.md#synthesize-definitions)
plus every [requirement](./compiler/model/requirement.md#requirement) that references it. This keeps
each generation task small and well defined.

### Relationships drive structure and order

The [relationship](./compiler/model/relationship.md#relationship) graph gives both the structure and
the order:

- `composition` → ownership and nesting.
- `association` → references.
- `dependency` → imports or injection.

Generation runs in topological order. Leaf entities (value objects) are generated before the
aggregates that compose them.

### Incremental generation and migration

Entities have [stable ids](./compiler/concepts/stable-identity.md#stable-identity), so the generator
maps each entity to its generated code unit. When an entity's requirements change, only that unit is
regenerated, and existing code is migrated in place rather than rewritten.

### Forced decisions

Generation sometimes has to choose a value the documentation never stated. Those decisions are fed
back into the docs by [Documentation Generation](./docsgen.md#documentation-generation).

### Verification

Generated code is verified by [Test Generation](./testgen.md#test-generation), which derives tests
from the same requirements. Because code and tests come from the same entity graph, they stay
coupled.
