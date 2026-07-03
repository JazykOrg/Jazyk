# LSP

`jazyk lsp` starts the language server over stdio, with Content-Length framed JSON-RPC.

The server is thin and read-only. It reads the [graph store](../compiler/graph.md) and maps
graph nodes to editor positions. It runs no analysis of its own and never calls the LLM.

## Capabilities

- Diagnostics: open [diagnostics](../compiler/model.md#node-types) are published inline,
  anchored by locating each `quote` in the open document (see
  [shared fields](../compiler/model.md#shared-fields)). Quotes survive unrelated edits, so
  anchors stay put while typing.
- Go to definition: entity → its defining mention.
- References: entity → all mentions across documents.
- Hover: the entity's definition, requirements, and relationships from the graph. Hover content
  is a rendered pack from the [context engine](../compiler/context.md), so it matches what the
  compiler and the [MCP server](./mcp.md) show.
- Completion: entity names and aliases, from the name index (see
  [derived data](../compiler/graph.md#derived-data)).

## Rebuilds and refresh

The server does not compile. Rebuilds run through the same
[reconciler](../compiler/reconciler.md) as `jazyk compile` and `jazyk watch`. The store's
generation counter tells the server when to refresh: when the counter moves, the server reloads
the graph and republishes (see [concurrency](../compiler/graph.md#concurrency)).
