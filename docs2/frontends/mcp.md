# MCP

`jazyk mcp graph` serves the [tool registry](../compiler/tools.md) over stdio as an MCP server:
line-delimited JSON-RPC per the Model Context Protocol. It dispatches the same tool
implementations the compiler's own [turns](../compiler/turns.md) use. There is one registry, not
a second API beside it.

## Default serving

By default the server exposes the [read tools](../compiler/tools.md#read-tools) and the
[generation tools](../compiler/tools.md#generation-tools):

- `context`, `expand`, `search`, `read_section`, `get_entity`
- `codegen_instructions`, `codegen_pending`, `codegen_task`, `codegen_mark`
- `await_changes` (a server tool, below)

This is the public query and generation surface. An agent can look up an entity, pull a
bounded [context pack](../compiler/context.md), follow
[expansion handles](../compiler/context.md#expansion-handles), and act as a generation
worker, with no way to mutate the graph.

## External generation workers

The server adds one tool of its own:

- `await_changes({timeout_seconds?})`: a long poll. It returns when the graph's
  generation counter moves or a documentation file changes on disk, or at the timeout
  (default 300 seconds). The reply carries the changed documents, whether the graph is
  stale (documents changed but not yet reconciled), and the pending generation work.

The loop this enables: a human edits documentation in an editor while `jazyk watch`
reconciles the graph beside it. The external agent sits in `await_changes`; when it
returns, the agent fetches `codegen_task` for each pending entity, writes the unit into
the workspace, verifies it with its own build and tests, and calls `codegen_mark`. The
generated code appears in the same editor the human is writing docs in. E.g.:

```
await_changes → {changedDocs: [docs/orders.md], graphStale: false,
                 pending: [{entity: ent:order, changed: [req:orders-4 (added)]}]}
codegen_task {entity: ent:order} → instructions + context + diff + unit path
(worker writes src/order.rs, runs its tests)
codegen_mark {entity: ent:order} → recorded; back to await_changes
```

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
