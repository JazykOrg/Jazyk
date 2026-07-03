# Test generation

Test generation derives tests from the semantic graph. It reads the graph through the
[context engine](../compiler/context.md) and the [read tools](../compiler/tools.md#read-tools),
never the raw source files. Where [code generation](./codegen.md) turns entities into code,
test generation turns requirements into the checks that hold that code to its spec.

## The requirement is the unit of test generation

Each [requirement](../compiler/model/requirement.md) derives one or more tests, keyed by the
requirement id. A failing test names the requirement it verifies, and a changed requirement
regenerates exactly the tests keyed to it. The context pack for the task is the requirement,
the entities it references, and their definitions.

## EARS pattern to test shape

The [EARS](../compiler/concepts/ears.md) pattern of a requirement decides the test shape:

- event-driven (`When ...`) → a scenario test: arrange, trigger the event, assert the
  response.
- ubiquitous (`The <entity> shall ...`) → a property or invariant test.
- unwanted behavior (`If ..., then ...`) → a negative test.
- state-driven (`While ...`) → a stateful test: enter the state, assert the behavior holds
  throughout.

## The quote is the trace

Every requirement carries a verbatim `quote`
([shared fields](../compiler/model.md#shared-fields)). The generated test embeds it, so a
failing test shows the exact source sentence it checks. The trail is
test → requirement id → `quote` → section.

## Coverage as a graph query

Test coverage is a query over the graph, not over the code:

- requirements with no derivable test,
- entities with no behavior (no event-driven or state-driven requirement references them).

Both findings are ordinary [diagnostics](../compiler/model/diagnostic.md), so they land in
the same triage queue as everything else.
