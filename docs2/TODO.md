# TODO

This scratch space is a live document to outline the work for this documentation.

## IDEAS

- Non-code usages (e.g. writing a book, hardware, CAD, 3d printing).

## LATER

- [Language Server](./frontends/lsp.md) implementation.
- Viewer for the graph: browse entities, requirements, and diagnostics from
  `jazyk-out/graph/`.
- Embeddings-backed search behind the same [`search` tool](./compiler/tools.md#read-tools),
  same interface, no schema change.
- Relationship cardinality on [derived edges](./compiler/graph.md#derived-data).
- Per-entity file sharding for the [graph store](./compiler/graph.md#storage-layout), when
  `entities.yaml` grows too large.
- [Journal](./compiler/graph.md#journal) rotation: cap `journal/` growth, compact old
  changesets.

## NEXT

## NOW

- [ ] `bootstrap2` proof of concept against the example project.
- [ ] Benchmark a local model for capability.
