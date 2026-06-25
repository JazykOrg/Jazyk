# Project Management

Project management maps the entity graph onto a project management system (e.g. issues in an external
tracker). It allows planning and tracking of work, and it can feed effort back into the docs.

## Technical design

### Mapping the graph to work

- Each [entity](./compiler/model/entity.md#entity) maps to a unit of work. The entity id is the stable
  key that links a task back to the documentation that justifies it.
- [Relationships](./compiler/model/relationship.md#relationship) map to task hierarchy and
  dependencies. `composition` and `aggregation` become parent and child items. `association` and
  `dependency` become blocking or related links.

### Tracking change

Re-running over an updated build creates work only where entities or requirements changed. Tasks are
keyed by entity id, so re-syncing is idempotent and creates no duplicates.

### Driving status from other usages

Project management reads the [code generation](./codegen.md#code-generation) and
[test generation](./testgen.md#test-generation) state for an entity to drive ticket status. E.g. when
an entity is implemented and its tests pass, the ticket is moved to done. The ticket reference is
stored so creation is not repeated, and the last observed status is recorded so external drift can be
reconciled.

### Effort

Effort can flow both ways. Estimated or actual effort from the tracker can be imported back into the
docs, and historical effort can estimate future work.

The downstream system can then assign and track tasks for human developers or AI coding agents.
