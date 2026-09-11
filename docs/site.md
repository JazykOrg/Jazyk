# Site

The public site for the project is hosted at [jazyk.org](https://jazyk.org).

## Style

- Static HTML, one hand-editable file per page, styled with Tailwind classes from its
  CDN script. No build step, no framework, no bundler.
- Every image the site shows is a real rendering, copied into `site/` as a static asset
  from a built example's `jazyk-out/`. A page never links into an example directory or
  into an out directory; links between pages and to assets are relative to the site.
- Code blocks are real output: trace events, shard YAML, cards, and terminal lines are
  copied from a run, condensed where a line would wrap, never invented.

## Hosting

The site is hosted on [GitHub Pages](https://pages.github.com/) with the custom domain
pointed at [jazyk.org](https://jazyk.org). A push to `master` that touches `site/`
deploys it.

## Pages

### Home (/)

- Hero: "Jazyk", pronunciation "/ˈjazɪk/", subtitle "Compiler for natural language".
- The thesis pitch from the [preamble](./main.md#preamble): open-ended prompts are
  unreliable, small well-defined ones are not, so treat documentation as source code.
- The graph as the build artifact: a persistent [semantic graph](./compiler/model.md)
  edited in place, never regenerated, queryable by tools. Its authored kinds (entities,
  free-form requirement [statements](./compiler/concepts/statements.md) with verbatim
  quotes, views) and its derived kinds (relationships, state machines), with every fact
  carrying [provenance](./compiler/model.md#provenance).
- A compile trace snippet: a few [trace events](./compiler/sessions.md#trace-events) from
  a real session, showing the tool calls, the staged mutations with the goals they open,
  the goal resolved with its justification, and the commit.
- Levels in one paragraph: entities summarize into groupings, every level gets its own
  diagrams, and digging into an entity shows the level below it
  ([levels](./compiler/concepts/levels.md)), with the top diagram of a real project and a
  link to the levels page.
- One rendered diagram from a real project, with the sentence behind one of its arrows.

### Compilation (/compilation)

- How reconciliation works, in order:
  - parse and diff the documents into the [dirty set](./compiler/reconciler.md#dirty-set)
    and derive the [goal board](./compiler/reconciler.md#goal-derivation),
  - run [sessions](./compiler/sessions.md) that resolve goal batches through
    [tools](./compiler/tools.md) behind gates,
  - repeat in [bursts of compile and GC](./compiler/compilation.md#compile-and-garbage-collection)
    until [convergence](./compiler/compilation.md#convergence) at a fixed point.
- One diagram of the [build lifecycle](./compiler/compiler.md#build-lifecycle).
- The fan-out goal as the GC example: a level over its children limit draws an
  [`abstract-entity`](./compiler/goals/abstract-entity.md) goal, and the session names
  the groupings the documents suggest.

### Levels (/levels)

- What a level is: a node's direct children, the scope root as the top level, the tree
  as deep as the documents and the model's judgment make it
  ([levels](./compiler/concepts/levels.md#levels)).
- The drill-down story on one real project, the ledger example, as three diagrams in
  order: the top level (the scope root's component view), the checkout level (the class
  view of Checkout's children, with the groupings the fan-out minted), and the funds
  level (the class view of one grouping's members). Each diagram embedded as the asset
  the compiler wrote, its boxes linking down the page as the real rendering links to
  cards ([drill-down](./compiler/concepts/levels.md#drill-down)). The top level's
  sequence diagram beside it: the flows among leaves lift to the level's members
  ([level views](./compiler/diagrams.md#level-views)).
- One [entity card](./consumers/docsgen.md#entity-cards) shown as text, the card of the
  grouping, so a reader sees the walk: up, in context, inside, sideways, and the
  requirements one click away.
- The grouping's proposal: the compiler proposes the architecture chapter the documents
  never wrote, and the owner accepts or retracts it
  ([groupings](./compiler/concepts/levels.md#groupings),
  [ratification proposals](./consumers/docsgen.md#ratification-proposals)), with the
  `jazyk answer` lines ([CLI](./frontends/cli.md#jazyk-answer)).
- The walk's surfaces: the cards and [diagram pages](./consumers/docsgen.md#diagram-pages)
  on disk, the [LSP hover](./frontends/lsp.md#capabilities), and the
  [GUI explorer](./frontends/gui.md#explore), all reading one model.
- The measured facts of the example, from `bootstrap/VALIDATION.md`: the number of
  documents, the two fan-outs, the depth, and the shape.

### Graph (/graph)

- What the semantic graph holds: the [node kinds](./compiler/model.md#node-kinds)
  and the [edges](./compiler/model.md#edge-summary) between them.
- Example YAML of one entity, one requirement, and one view, verbatim from the
  [graph store](./compiler/graph.md#storage-layout), beside the
  [diagram](./compiler/diagrams.md) the view renders.
- `/artifact`, the page's earlier route, redirects here so old links keep resolving.
