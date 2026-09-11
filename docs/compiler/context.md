# Context

The context engine maintains the loaded set: an explicit working set of graph nodes,
bounded by a budget, with a rendered status the model reads on every round. It is the
single answer to the question every consumer asks: give me just enough of the
[graph](./model.md) to work on this item, and a way to load more in the right direction.

Loading is pure computation over the graph store. No LLM runs in the loading path, so
loading is fast, deterministic, and cacheable. One engine serves every consumer:
[sessions](./sessions.md) during compilation, the [MCP servings](../frontends/mcp.md) for
external agents, the [LSP](../frontends/lsp.md) for hover and navigation, the
[`jazyk context`](../frontends/cli.md#jazyk-context) command, and the GUI's loaded-set
panel.

## The loaded set

The context is an explicit working set, not an accident of what was prompted. The
[serving](./compiler.md#the-serving) keeps one loaded set per open goal batch (per
process for the read-only `graph` serving and the CLI) and renders its status into every
round, so the model always knows what is loaded, what could be loaded next, and what it
costs. E.g.:

```text
## Loaded (14.2k/24k chars)
- view:class/commerce   12 entities, 18 edges shown; 9 members unloaded  [h:view:class/commerce:members]
- ent:order             full: 7 requirements, parent ent:order-service   [3 more edges: h:ent:order:related]
- ent:customer          stub (definition only)                           [5 edges loadable: h:ent:customer]
- docs/orders.md#/orders/holds   section body
Skills: extraction (active, 11.3k); judgment, flow-views, structural-views, abstraction, conformance (load_skill)
Consider unloading: ent:customer (not referenced in 6 rounds, no open goal touches it)
```

The set holds items of three depths: a node loaded in full, a stub (one line), and a
section body. Each item carries the handles at its frontier, the things that could be
loaded next with a size estimate. The [skills](./sessions.md#skills) rendered this
session ride in the same accounting.

What joins the set:

- the initially loaded set: the registry's `pack` computes it for a batch from its
  goals' hints, under the budget ([batching](./reconciler.md#batching)). Each goal
  kind's page says what its kind loads.
- `load` and `expand` calls,
- a read's subject, as a stub: `search` hits, the section `read_section` returned, the
  entity or view `get_entity` and `get_view` returned, the subjects of the diagnostics
  `diagnostics` listed.

What leaves it: `unload`. Nothing else shrinks the set; a session that loads freely and
never unloads reaches the high-water mark and is told so.

The status block renders in the [session prompt](./sessions.md#the-prompt) as the
`## Loaded` block, condensed on every mutating tool reply, and in full on
`graph_status`. The transcript records each rendering, so what the model saw is on
record.

## Tools

The loading tools are [read tools](./tools.md#read-tools), served everywhere:

- `load({target, depth?})`: load a target and its immediate neighborhood. The target is
  any node id (entity, requirement, view, diagnostic), a section reference, or
  `scope:<scope>`, the top level of a scope, which loads its parentless entities as
  stubs ([the scope root](./concepts/levels.md#the-scope-root)), each with the
  document it is mentioned in most (`[doc <path>]`) and, under them, the
  relationships among the members one line each, so a fan-out over the level reads
  what every member relates to without a `get_entity` per member. `depth`
  defaults to `1`: the target in full, its edges, each neighbor as a stub. `depth: 2`
  loads the neighbors in full too and their neighbors as stubs, still under the budget.
  The reply renders what was loaded and the status block.
- `expand({handle})`: load the frontier behind a handle. Every truncation emits a
  handle, so the model picks the direction that deserves the next slice of budget. The
  reply renders the frontier and re-renders the item's line with what remains.
- `unload({target})`: drop an item from the set. Its line leaves the status, its handles
  close, later replies stop rendering it, and its budget frees for the rest of the
  session.
- `graph_status({})`: re-render the status block in full on demand.
- `search`, `read_section`, `get_entity`, `get_view`, `diagnostics`: reads. A read's
  subject joins the set as a stub, so a follow-up `load` on it is one step away and the
  status shows what the session has seen.

Outside a session, [`jazyk context <target>`](../frontends/cli.md#jazyk-context) prints
exactly what `load` renders for a target, with `--expand` following named handles first.
It is the debug window into the loaded set.

## Policy

The policy is deterministic and budget-driven. Loading `A` brings:

- `A` in full. For an entity: `name`, `aliases`, `definition`, `scope`, `stereotype`,
  `parent`, `attributes`, `limits`, its mentions with their quotes, its requirements
  (statement, entities, edges, transition, facets), its relationships with their
  contribution groups, the views that include it, and its state machine in summary
  when it is a subject. For a requirement: `statement`, `entities`, `edges`,
  `transition`, `facets`, `source` or `provenance`, the pairs its edges contribute to,
  and the views it belongs to. For a view: `kind`, `title`, the ordered members as
  stubs, `excluded`, `collapse`, `query`, `limits`, and the relationships among the
  members with a count. For a section: the body (`raw`), the coverage state, the
  requirements sourced from it, and the entities it mentions; a dirty section renders
  with the diff against its last reconciled body marked. For a diagnostic: its fields
  and its subjects as stubs.
- `A`'s edges: every reference `A` stores or is stored on, by [axis](#axes), with a
  count per axis.
- Each neighbor as a stub: name, one definition line, the stereotype, and its own edge
  count for an entity; the statement and its entities for a requirement; the reference,
  title, and coverage state for a section; the kind, title, and member count for a
  view.
- Neighbors' neighbors as counts only (`5 edges loadable`).

The initially loaded set obeys the same ceiling as every later `load`: a goal's own
target loads full, and a supporting item that would land the set past the high-water
mark enters as a stub ([batching](./reconciler.md#batching)).

The walk is breadth-first per axis in a fixed order (`parents`, `mentions`,
`requirements`, `related`, `members`), each frontier in document order (document link
level, then path, then section order) with ties broken by id. Size accounting runs
during the walk: when the next item would exceed the budget, the walk stops on that
frontier and emits a handle instead. Overload is impossible by construction: the model
sees `9 members unloaded` and chooses.

The budget is 24000 characters, a registry constant
([budgets](./sessions.md#budgets)). The header counts every loaded item plus the skills
rendered this session; an inactive skill keeps its chars, because its text is still in
the conversation.

- High-water mark: at 90 percent of the budget, `load` and `expand` are refused with a
  `context-full` error naming the unload candidates, until something is unloaded. Reads
  still answer; their stubs are one line each and count like any item.
- Unload suggestions: the status names candidates that are least recently referenced
  (not named in any tool call's arguments for 6 rounds) and not named by any open goal
  in the batch, as target or hint. Suggestions are advice; the model decides.
- Repeats: a `load` of a target that is already loaded is a repeat whatever its `depth`,
  answered by the [repeated-call guard](./sessions.md#repeated-calls). Deepening a
  loaded node is `expand` on its handles. An `unload` clears the count for its target.
- Staged work: the set renders the committed snapshot the session began with. An item a
  staged mutation touches carries a `staged:` note on its line (`staged: delete`,
  `staged: statement revised`), so the model never mistakes its own pending work for a
  lost write ([staged mutations](./sessions.md#staged-mutations)).

### Expansion handles

Whatever the budget cut off is represented by a handle: a stable token naming the
omitted frontier and its size. The shape is `h:<target>:<axis>[:<start>]`, parsed from
the right, where `start` is the offset into the frontier when an earlier `expand` took
the first slice. E.g.:

```text
h:ent:shopping-cart:requirements     # 4 more requirements, ~900 chars
h:ent:shopping-cart:requirements:4   # the slice after the first four
h:view:class/commerce:members        # 9 members unloaded
h:ent:customer                       # a stub's whole neighborhood
```

The axis names are a closed set: the five walk axes plus three frontiers that only
handles name: `children` (the containment subtree below the direct children), `body`
(the rest of a truncated section body, also reachable with `read_section`), and `edges`
(the relationships among a view's members beyond those shown). A handle without an axis
names a stub's whole neighborhood. Handles are accepted by `expand`; a handle closes
when its item unloads, and `expand` on a closed or unknown handle errors
`unknown-handle` naming the open ones.

## Axes

The axes are the directions a load walks. They follow the
[edge summary](./model.md#edge-summary).

- `parents`: the containment trees. From a section: its ancestors up to the document
  root, and its direct subsections as stubs. From an entity: its `parent` chain, and its
  direct children as stubs. Deeper descendants sit behind a `children` handle.
- `mentions`: entity ↔ section. From an entity, the sections that mention it, each with
  its quote. From a section, the entities it mentions.
- `requirements`: from an entity, its requirements; from a section, the requirements
  sourced from it; from a requirement, the entities it names. Hop 2 continues to the
  other entities those requirements tie the target to, and their requirements.
- `related`: the derived data. From an entity, its relationships (every contribution
  group, `instantiation` included) with the entity on the other end, and its state
  machine when it is the subject. From a requirement, the relationship each of its
  edges contributes to.
- `members`: from a view, its members in order, its exclusions, and its collapse list.
  From an entity or requirement, the views that include it.

E.g., `load({target: "ent:order"})` brings `ent:order` in full, `ent:order-service` as
a stub (`parents`), the sections that mention the order with their quotes (`mentions`),
its seven requirements (`requirements`), `ent:shopping-cart` and `ent:customer` as stubs
with the relationship types between them and the order (`related`), and the lines
`view:class/commerce` and `view:state/order` (`members`). `depth: 2` would bring the
stubs in full.

## Rendering

Rendering is part of the engine, not the consumer, so every consumer sees the same
shape: compact markdown with node ids inline, so the model can name any id in a
follow-up call.

The status block is one line per item: the id, what is loaded (`full`, `stub`,
`section body`) with its counts, and the handles in brackets with their sizes. Under
the items, the skill index line, then the unload suggestion when there is one. The
header carries the used and total budget. The full form lists every item; the condensed
form, rendered on every mutating reply, keeps the header and lists only the lines that
changed since the last rendering (a new stub, a `staged:` note, a freed handle).

A `load` reply renders the item itself. E.g.:

```markdown
## ent:order (Order)  full
scope: commerce   parent: ent:order-service (Order Service)
definition: a customer's purchase, priced in one currency
attributes: total, currency
mentions: docs/orders.md#/orders "An order carries a total and a currency."
requirements (7):
- req:orders-1: An order carries a total and a currency.
- req:orders-6: When payment succeeds, the order becomes paid.
  transition: placed → paid (payment succeeds)   facets: behavior
  ...
related: ent:shopping-cart (composition 1..*, 1 requirement), ent:customer (association, 2 requirements)
  [3 more edges: h:ent:order:related]
views: view:class/commerce, view:state/order
state machine sm:order: 3 states, 2 transitions

neighbors:
- ent:order-service «service» owns checkout and orders  (6 edges)  [h:ent:order-service]
- ent:shopping-cart holds the items a customer intends to buy  (4 edges)  [h:ent:shopping-cart]
```

A section renders as its reference, its title, its coverage state, the body, then the
requirements already sourced from it and the entities it mentions; a dirty section marks
the diff. A view renders its members in order as stubs and the arrows among them as
`a → b (type, n)` lines, lifted the same way the [diagram](./diagrams.md#lifting-and-collapse)
draws them.

The [LSP](../frontends/lsp.md) hover and the GUI inspector render the same load for a
target; [`jazyk context`](../frontends/cli.md#jazyk-context) prints it.
