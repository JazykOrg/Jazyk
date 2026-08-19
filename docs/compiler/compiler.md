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

- The [tool registry](./tools.md), served as an MCP server (`jazyk mcp`). Read tools
  are the public query surface. Write tools mutate the graph and are used by
  compilation turns, or by an external agent given `--write`. The same serving is
  injected into every [ACP session](../frontends/acp.md), so the tools have one
  implementation whoever calls them.
- The [ACP bridge](../frontends/acp.md): the single AI path. Jazyk is the ACP client
  of one configured agent (an external coding agent, or the generic
  [embedded agent](../frontends/acp.md#the-embedded-agent)); every
  [turn](./turns.md) runs as a session against it, with the jazyk tools injected
  over MCP.
- The [reconciler](./reconciler.md): computes what is stale and schedules turns level
  by level with bounded parallelism.
- The [control plane](./control-plane.md): the workflow policy on disk. Modes,
  releases, workers, and leases decide whether anyone may act on the queued work and
  who is acting, the same answer for every frontend.

## Build lifecycle

Compilation is not a component: it is the process the components above run
together, and [compilation](./compilation.md) describes it stage by stage. One
build, with each component in its place:

- The [reconciler](./reconciler.md) parses the documents, diffs the section trees
  into the dirty set, and derives the [task queue](./reconciler.md#the-task-queue).
- The [control plane](./control-plane.md) says whether the queued work may run:
  everything in `auto` mode, only released work in `manual`.
- Turns run in [waves](./compilation.md#waves) as sessions over the
  [ACP bridge](../frontends/acp.md), with the [tool registry](./tools.md) injected:
  ingest, pair review, entity review. The [graph store](./graph.md) commits each
  turn's changeset atomically.
- Deterministic checks and the verdict close the build. See
  [convergence](./compilation.md#convergence).

The first build is not special: it is reconciliation against an empty graph. A
rebuild with no changes has an empty dirty set and makes zero LLM calls.

## Outputs

Everything lives in the out directory (default `jazyk-out/`). See
[storage layout](./graph.md#storage-layout).

- `graph/`: the semantic graph, the primary output.
- `docs/`: section trees and coverage per document.
- `docsgen/`: one human-readable requirements document per entity, rendered
  deterministically on every build. See
  [documentation generation](../consumers/docsgen.md#the-requirements-document).
- `gen/`: generation and verification metadata: the
  [ledger](../consumers/gen.md#the-ledger) and the criteria files for llm tests. The
  deliverable itself lives outside the out directory
  ([generation settings](./project-settings.md#generation)).
- `journal/`: the audit trail of every change.
- `status.yaml`: convergence verdict, budgets spent, parked work.

`jazyk check` exits non-zero when open diagnostics of severity `error` exist. See
[CLI](../frontends/cli.md).
