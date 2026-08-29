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
- A compile trace snippet: a few [trace events](./compiler/sessions.md#trace-events) from
  a real run, showing goals resolved, tool calls, and staged mutations round by round.
- One rendered diagram from the dogfood, with the sentence behind one of its arrows.

### Compilation (/compilation)

- How reconciliation works, in order:
  - parse and diff the documents into the [dirty set](./compiler/reconciler.md#dirty-set)
    and derive the [goal board](./compiler/reconciler.md#goal-derivation),
  - run [sessions](./compiler/sessions.md) that resolve goal batches through
    [tools](./compiler/tools.md),
  - repeat in [bursts of compile and GC](./compiler/compilation.md#compile-and-garbage-collection)
    until [convergence](./compiler/compilation.md#convergence) at a fixed point.
- One diagram of the [build lifecycle](./compiler/compiler.md#build-lifecycle).

### Graph (/graph)

- What the semantic graph holds: the [node kinds](./compiler/model.md#node-kinds)
  and the [edges](./compiler/model.md#edge-summary) between them.
- Example YAML of one entity, one requirement, and one view, verbatim from the
  [graph store](./compiler/graph.md#storage-layout), beside the
  [diagram](./compiler/diagrams.md) the view renders.
