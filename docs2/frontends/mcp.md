# MCP

`jazyk mcp graph` serves the [tool registry](../compiler/tools.md) over stdio as an MCP server:
line-delimited JSON-RPC per the Model Context Protocol. It dispatches the same tool
implementations the compiler's own [turns](../compiler/turns.md) use. There is one registry, not
a second API beside it.

## Default serving

By default the server exposes the [read tools](../compiler/tools.md#read-tools) only:

- `context`
- `expand`
- `search`
- `read_section`
- `get_entity`

This is the public query surface. An agent can look up an entity, pull a bounded
[context pack](../compiler/context.md), and follow
[expansion handles](../compiler/context.md#expansion-handles), with no way to mutate the graph.

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
