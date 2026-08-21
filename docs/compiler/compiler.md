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

- [parsing](./parsing.md) and [alignment](./alignment.md) (section diffing and anchor
  relocation),
- identifiers and the [graph store](./graph.md) with its validation gates,
- the [dirty set](./reconciler.md#dirty-set) (what is stale),
- [context assembly](./context.md),
- derived relationships and [garbage collection](./graph.md#garbage-collection).

The model owns everything that requires judgment:

- reading a section and extracting requirements and entities,
- deciding whether a concept already exists in the graph (search before create),
- writing and refining definitions,
- judging severity, and marking sections covered or non-normative.

## Build lifecycle

Compilation is one build: bring the graph in line with the documents.
[Compilation](./compilation.md) describes the stages; these are the components that
carry them, in the order they act:

- The [reconciler](./reconciler.md) drives the build from start to verdict. It
  parses the documents, diffs the section trees into the dirty set, and derives the
  [task queue](./reconciler.md#the-task-queue): what work exists, in what order,
  with bounded parallelism.
- The [control plane](./control-plane.md) decides whether anyone may act on that
  work and who is acting: everything in `auto` mode, only released work in
  `manual`. Modes, releases, workers, and leases are files in the out directory,
  so every frontend reads the same policy.
- Each scheduled [turn](./turns.md) runs as one session over the
  [ACP bridge](../frontends/acp.md): jazyk is the ACP client of one configured
  agent (an external coding agent, or the generic
  [embedded agent](../frontends/acp.md#the-embedded-agent)). All AI work takes this
  one path.
- The session's tools come from the [tool registry](./tools.md), served as an MCP
  server (`jazyk mcp`) and injected into every session, so the tools have one
  implementation whoever calls them. Read tools are the public query surface; write
  tools mutate the graph and are used by compilation turns, or by an external agent
  given `--write`.
- The [graph store](./graph.md) commits each turn's changeset atomically, behind
  validation gates, and journals it.
- When the last turn lands, deterministic checks run and the verdict closes the
  build. See [convergence](./compilation.md#convergence).

The reconciler repeats [waves](./compilation.md#waves) (ingest, pair review, entity
review) until the graph and the documents agree. The first build is not special: it
is reconciliation against an empty graph. A rebuild with no changes has an empty
dirty set and makes zero LLM calls.

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
