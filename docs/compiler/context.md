# Context

The context engine assembles a bounded slice of the [graph](./model.md) around a target.
It is the single answer to the question every consumer asks: give me just enough of the
graph to work on this item, and a way to load more in the right direction.

Assembly is pure computation over the graph store. No LLM runs in the loading path, so
context is fast, deterministic, and cacheable.

One engine serves three consumers: [turns](./turns.md) during compilation, the
[MCP server](../frontends/mcp.md) for external agents, and the future
[LSP](../frontends/lsp.md) for hover and navigation.

## Request

A context request has three parts:

- `target`: the item that needs context. A section reference, an entity id, or a
  requirement id.
- `focus`: how far to walk each [edge axis](./model.md#edge-axes), as a hop quota per axis.
  E.g. `{parents: 2, mentions: 1, requirements: 2}`.
- `budget`: the maximum size of the returned pack, in characters. The engine never returns
  more than the budget.

Defaults exist for `focus` and `budget` per task type, so a plain
`context({target: "ent:shopping-cart"})` works.

## Axes

- `parents`: from a section, walk the document tree. Hop 1 is the parent and the direct
  children. Hop 2 adds grandparents and grandchildren.
- `mentions`: from an entity, the sections that mention it (with their quotes). From a
  section, the entities mentioned in it.
- `requirements`: from an entity, its requirements. Hop 2 continues to the other entities
  those requirements reference, and their requirements.

E.g. a request for entity `A` with `{parents: 2, mentions: 1, requirements: 2}` loads:

- the sections that mention `A` (hop 1 on `mentions`),
- each such section's parent chain, two levels up (hops on `parents`),
- `A`'s requirements, the entities they tie `A` to, and those entities' requirements
  (two hops on `requirements`).

## Assembly

- Traversal is breadth-first per axis, under that axis's quota, with the whole walk capped
  by `budget`.
- Ordering is deterministic. For an entity: its own record first, then its defining
  mentions, then requirements, then related entities by hop distance, ties broken by
  document order.
- Size accounting runs during the walk. When the next item would exceed the budget, the
  walk stops on that branch and emits an [expansion handle](#expansion-handles) instead.

## Expansion handles

Whatever the budget cut off is represented by a handle: a stable token naming the omitted
frontier and its size. E.g.:

```
h:ent:shopping-cart:requirements:2   # 4 more requirements, ~900 chars
```

Handles are accepted by the `expand` [read tool](./tools.md#read-tools). The model decides
which direction deserves the next slice of budget. This is the expansion mechanism: the
initial pack is a starting point, not the whole story.

## Rendering

The pack is rendered as compact markdown with node ids inline, so the model can reference
any id in a follow-up tool call. E.g.:

```markdown
## Entity ent:shopping-cart (Shopping Cart)
definition: holds items a customer intends to buy
mentions: docs/catalog.md#/catalog/cart "The shopping cart holds items..."

### Requirements
- req:catalog-2: When the customer checks out, the system shall empty the Shopping Cart.
  (ties: ent:customer)

### More
- h:ent:shopping-cart:requirements:2 (4 more requirements)
```

Rendering is part of the engine, not the consumer, so every consumer sees the same shape.
