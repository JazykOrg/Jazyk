# CLI

The CLI is the command line frontend over the [compiler](../compiler/compiler.md). It runs the
build, inspects the [graph store](../compiler/graph.md), and hosts the other frontends.

## Commands

### jazyk compile

`jazyk compile [path...]` runs the [reconciler](../compiler/reconciler.md) to a
[fixed point](../compiler/reconciler.md#convergence).

Live trace:

- Default: one line per [turn](../compiler/turns.md) round, showing the tool calls with
  condensed arguments, the condensed results, and the model's reasoning text. See
  [trace events](../compiler/turns.md#trace-events).
- `--verbose`: additionally prints the full [context packs](../compiler/context.md) and raw
  payloads.
- `--quiet`: prints only the final summary.

Exit code: `0` on convergence, non-zero when work was parked.

### jazyk check

Compile, then exit non-zero if open [diagnostics](../compiler/model.md#node-types) of severity
`error` exist. The CI gate.

### jazyk watch

Recompile on file change. The same loop as `compile`: each change feeds the
[dirty set](../compiler/reconciler.md#dirty-set), so unchanged documents are not revisited. See
[incremental builds](../compiler/reconciler.md#incremental-builds).

### jazyk status

Summarize `status.yaml` (see [storage layout](../compiler/graph.md#storage-layout)):

- generation counter,
- [coverage](../compiler/reconciler.md#coverage) percentage,
- open diagnostics by severity,
- parked work.

### jazyk context

`jazyk context <target> [--focus parents=2,mentions=1,requirements=2] [--budget N]` prints the
rendered [context pack](../compiler/context.md) for a target, with its
[expansion handles](../compiler/context.md#expansion-handles). This is the debug window into the
context engine: what this command prints is exactly what a turn sees.

`<target>` is a section reference, an entity id, or a requirement id. See
[request](../compiler/context.md#request). E.g.:

```
jazyk context ent:shopping-cart --focus mentions=1,requirements=2 --budget 8000
```

### jazyk query

`jazyk query <text>` runs the [search tool](../compiler/tools.md#read-tools) and prints the
matches, one `{id, name, definition}` line each.

### jazyk codegen

`jazyk codegen [entity...]` generates one code unit per entity from its assembled
context pack, into `<out>/codegen/`. With no arguments it generates every entity that
has at least one requirement, leaf entities first. `--lang` picks the target language
(default `rust`). See [code generation](../consumers/codegen.md#command).

### jazyk testgen

`jazyk testgen [entity...]` derives tests from requirements into `<out>/testgen/`, one
file per entity, one or more tests per requirement, quotes embedded as the trace.
`--lang` picks the target language (default `rust`). See
[test generation](../consumers/testgen.md#command).

### jazyk viewer

`jazyk viewer [--out FILE]` renders the graph into one self-contained HTML file
(default `<out>/graph.html`). See [viewer](./viewer.md).

### jazyk mcp graph

Start the [MCP server](./mcp.md) on stdio. Read tools by default; `--write` adds the
[write tools](../compiler/tools.md#write-tools).

### jazyk benchmark

Grade whether the configured model is good enough to compile Jazyk. See
[benchmark](../benchmark/benchmark.md).

## Common options

- `--llm-base-url URL`: the LLM endpoint.
- `--model M`: the model to use.
- `--api-key K`: sent as a bearer token.
- `--out DIR`: the out directory (default `jazyk-out/`).

## Project discovery

The CLI walks up from the working directory to the nearest `jazyk.toml` and treats that
directory as the project root. Explicit `[path...]` arguments skip discovery and run ad hoc on
those files. The out directory defaults to `<root>/jazyk-out/`.
