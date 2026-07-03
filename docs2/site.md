# Site

The public site for the project is hosted at [jazyk.org](https://jazyk.org).

## Style

- Static HTML with inline CSS. No build step, no framework.
- One hand-editable HTML file per page.

## Hosting

The site is hosted on [GitHub Pages](https://pages.github.com/) with the custom domain
pointed at [jazyk.org](https://jazyk.org).

## Pages

### Home (/)

- Hero: "Jazyk", pronunciation "/ˈjazɪk/", subtitle "Compiler for natural language".
- The thesis pitch from the [preamble](./main.md#preamble): open-ended prompts are
  unreliable, small well-defined ones are not, so treat documentation as source code.
- The graph as the build artifact: a persistent [semantic graph](./compiler/model.md)
  edited in place, never regenerated, queryable by tools.
- A compile trace snippet: a few [trace events](./compiler/turns.md#trace-events) from a
  real run, showing tool calls and staged mutations round by round.

### Compilation (/compilation)

- How reconciliation works, in order:
  - parse and diff the documents into the [dirty set](./compiler/reconciler.md#dirty-set),
  - run [turns](./compiler/turns.md) that mutate the graph through
    [tools](./compiler/tools.md),
  - repeat in [waves](./compiler/reconciler.md#waves) until
    [convergence](./compiler/reconciler.md#convergence) at a fixed point.
- One diagram of the [build lifecycle](./compiler/compiler.md#build-lifecycle).

### Graph (/graph)

- What the semantic graph holds: the five [node types](./compiler/model.md#node-types)
  and the [edge axes](./compiler/model.md#edge-axes).
- Example YAML of one entity and one requirement, verbatim from the
  [graph store](./compiler/graph.md#storage-layout).
