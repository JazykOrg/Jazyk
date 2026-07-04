# MCP

`jazyk mcp graph` serves the [tool registry](../compiler/tools.md) over stdio as an MCP server:
line-delimited JSON-RPC per the Model Context Protocol. It dispatches the same tool
implementations the compiler's own [turns](../compiler/turns.md) use. There is one registry, not
a second API beside it.

## Default serving

By default the server exposes the [read tools](../compiler/tools.md#read-tools), the
[generation tools](../compiler/tools.md#generation-tools), and the
[verification tools](../compiler/tools.md#verification-tools):

- `context`, `expand`, `search`, `read_section`, `get_entity`
- `gen_instructions`, `gen_pending`, `gen_task`, `gen_mark`
- `verify_pending`, `verify_task`, `verify_mark`
- `await_changes` (a server tool, below)

This is the public query, generation, and verification surface. An agent can look up an
entity, pull a bounded [context pack](../compiler/context.md), follow
[expansion handles](../compiler/context.md#expansion-handles), and act as a generation
or verification worker, with no way to mutate the graph.

## External workers

The server adds one tool of its own:

- `await_changes({timeout_seconds?, lang?})`: a long poll. It returns when the graph's
  generation counter moves, a documentation file changes on disk, a manifest or test
  file in the deliverable changes, or the ledger changes, or at the timeout (default
  300 seconds). The reply carries the changed documents, whether the graph is stale
  (documents changed but not yet reconciled), the pending generation work, and the
  pending verification work grouped by reason.

Three workflows ride this surface, each an LLM harness that any agent can replace:

1. Compilation (docs → graph) is jazyk's own harness: `jazyk compile` or `jazyk watch`.
   Workers do not mutate the graph; when `await_changes` reports `graphStale`, the
   reconciler needs to run.
2. Generation (graph → deliverable + tests): drain `gen_pending`; for each entity fetch
   `gen_task`, write the files, `gen_mark` with the manifest.
3. Verification (tests → verdicts): drain `verify_pending`; for each row fetch
   `verify_task`, run the command or judge the criteria, `verify_mark` the verdict.

The loop this enables: a human edits documentation in an editor while `jazyk watch`
reconciles beside it. The external agent sits in `await_changes`; when it returns, the
agent generates, verifies, and fixes until both pending lists drain, and the deliverable
appears in the same editor the human is writing prose in. E.g.:

```
await_changes → {changedDocs: [docs/orders.md], graphStale: false,
                 genPending: [{entity: ent:order, changed: [req:orders-4 (added)]}],
                 verifyPending: {requirement-changed: 1}}
gen_task {entity: ent:order} → instructions + context + diff + deliverable + factHash
(worker writes src/order.rs and tests/order.rs, runs the tests)
gen_mark {entity: ent:order, factHash, manifest} → ledger updated
verify_task {requirement: req:orders-4} → run command
(worker runs it; exit 0)
verify_mark {requirement: req:orders-4, verdict: pass} → verified; back to await_changes
```

A fix-fail-reverify cycle is self-terminating: editing a deliverable file re-stales
exactly the rows whose files hash moved, and `verify_pending` shrinks monotonically once
tests pass.

## Write mode

`--write` adds the [write tools](../compiler/tools.md#write-tools). With it, an external agent
(e.g. a coding agent) can drive the same toolset a compilation turn uses, for debugging and
manual compilation:

- same tools and schemas (see [task toolsets](../compiler/tools.md#task-toolsets)),
- same [validation gates](../compiler/graph.md#validation-gates), with the same repair-oriented
  errors,
- same [journal](../compiler/graph.md#journal), so manual changes are audited like any turn.

## Reads and locking

Reads load the persisted graph from the out directory (see
[storage layout](../compiler/graph.md#storage-layout)). The server never compiles. If no graph
exists yet, every tool answers with guidance to run `jazyk compile` first.

Readers do not lock. They read the generation counter, load shards, and retry if the counter
moved mid-read. Writers respect the store lock, so one changeset commits at a time even with a
compile running next to the server. See [concurrency](../compiler/graph.md#concurrency).
