# Test Generation

Test generation derives tests from the entity graph and ties them back to the documentation. Where
[Code Generation](./codegen.md#code-generation) turns entities into code, test generation turns the
same [requirements](./compiler/model/requirement.md#requirement) into the checks that keep that code
consistent with its spec.

## Technical design

Each requirement is something that can be asserted. Tests are keyed by requirement id, so a failing
test points at the requirement (and the section) it verifies, and a changed requirement regenerates
exactly the tests that cover it.

### Kinds of tests

The [EARS](./compiler/concepts/ears.md#ears) pattern of a requirement decides the test shape:

- event-driven (`When ...`) → a scenario test.
- ubiquitous invariant (`The system shall ...`) → a property test.
- unwanted behavior (`If ...`) → a negative test.

### Fixtures from examples

Examples in the docs (e.g. `john@acme.com`) are extracted as concrete instances and become test
fixtures.

### Coverage as a graph query

Coverage is a query over the graph: requirements with no derivable test, entities with no behavior.
See [checks](./compiler/linking/checks.md#checks).

### Contract tests across relationships

When two entities are tied by a relationship, a contract test can be derived from both ends. A
mismatch is caught before any code is generated.
