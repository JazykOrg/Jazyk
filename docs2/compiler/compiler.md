# Compiler

The compiler maintains a persistent [semantic graph](./model.md) that mirrors the
project's documentation. Compiling means reconciling: bring the graph in line with the
documents, surface ambiguity and contradictions as [diagnostics](./model/diagnostic.md),
and leave everything queryable for downstream consumers.

The graph is the build artifact. It is edited in place, never regenerated. Entities,
requirements, and diagnostics keep their identity across builds, so everything downstream
(generated code, tests, tickets, triage) stays bound. See
[identity](./concepts/identity.md).

## Division of labor

The design splits work strictly between deterministic code and the model.

The harness owns everything that must never be wrong:

- [parsing](./parsing.md) and section diffing,
- identifiers and the [graph store](./graph.md) with its validation gates,
- the [dirty set](./reconciler.md#dirty-set) (what is stale),
- [context assembly](./context.md),
- derived relationships and [garbage collection](./graph.md#garbage-collection).

The model owns everything that requires judgment:

- reading a section and extracting requirements and entities,
- deciding whether a concept already exists in the graph (search before create),
- writing and refining definitions,
- judging severity, and marking sections covered or non-normative.

## Components

- The [tool registry](./tools.md), also served as an MCP server (`jazyk mcp graph`).
  Read tools are the public query surface. Write tools mutate the graph and are used by
  compilation turns, or by an external agent given `--write`.
- The [turn harness](./turns.md): one focused LLM session wired to the registry, staging
  mutations, committing atomically.
- The [reconciler](./reconciler.md): computes what is stale, schedules turns level by
  level with bounded parallelism, and decides when the build has converged.

## Build lifecycle

```
parse all docs → diff section trees → dirty set
  → ingest wave (reconcile-doc turns, root first, then levels in parallel)
  → review wave (review-entity turns)
  → checks (deterministic lint, coverage, reachability)
  → fixed point reached, or budget exhausted with work parked
```

The first build and every rebuild run the same lifecycle. The first build starts
from an empty graph, so everything is dirty. A rebuild with no changes has an empty dirty
set and makes zero LLM calls.

## Outputs

Everything lives in the out directory (default `jazyk-out/`). See
[storage layout](./graph.md#storage-layout).

- `graph/`: the semantic graph, the primary output.
- `docs/`: section trees and coverage per document.
- `docsgen/`: one human-readable requirements document per entity, rendered
  deterministically on every build. See
  [documentation generation](../consumers/docsgen.md#the-requirements-document).
- `journal/`: the audit trail of every change.
- `status.yaml`: convergence verdict, budgets spent, parked work.

`jazyk check` exits non-zero when open diagnostics of severity `error` exist. See
[CLI](../frontends/cli.md).
