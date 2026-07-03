# bootstrap2

A proof-of-concept implementation of the turn-based Jazyk compiler specified in
[`docs2/`](../docs2/main.md). It exists to validate the design: a persistent semantic
graph, edited by LLM turns through validated tools, driven by a deterministic reconciler.

Strict docs-first: every behavior here is documented in `docs2/` before it is coded.
When implementation work reveals a missing or wrong behavior, `docs2/` changes first.

## Build

```
cargo build --release        # binary at target/release/jazyk
cargo test                   # store, context, tools, md, codec unit tests
```

Dependencies are minimal (`serde`, `serde_json`, `serde_norway`); HTTP, JSON-RPC, and
MCP are hand-rolled.

## Run

```
cd example/f1                # or example/f2 (the trap fixture)
jazyk compile                # reconcile the graph, live trace
jazyk compile --verbose      # full context packs and payloads
jazyk status                 # last build summary
jazyk context ent:order      # print a context pack
jazyk query customer         # search entities
jazyk codegen [entity...]    # one code unit per entity from its context pack
jazyk testgen [entity...]    # tests derived from requirements, quote as trace
jazyk viewer                 # the graph as one self-contained HTML page
jazyk mcp graph [--write]    # the graph MCP server on stdio
jazyk lsp                    # the language server on stdio (read-only)
jazyk benchmark              # grade the configured model under both codecs
```

LLM config resolves flag → env (`JAZYK_LLM_BASE_URL`, `JAZYK_MODEL`, `JAZYK_API_KEY`) →
`~/.jazyk/config.toml` → project `[llm]` → default. See
[`docs2/compiler/project-settings.md`](../docs2/compiler/project-settings.md).

## Implemented subset

Implemented per docs2: parsing and section diffing, the graph store (natural-key
upserts, atomic changesets, journal, redirects, GC, sticky diagnostics), the context
engine (focus, budget, expansion handles), the tool registry with validation gates, the
turn harness (native and text codecs with the capability probe, implicit done), the
reconciler (BFS levels, parallel ingest, grouped review wave, deterministic checks
including the flip detector, one fix-up pass, parked-work resume),
`compile`/`check`/`watch`/`status`/`context`/`query`, the MCP server (read tools;
`--write` for mutations), the read-only LSP, and `jazyk benchmark` (cases embedded from
[`docs2/benchmark/cases/`](../docs2/benchmark/cases), graded deterministically under
both codecs).

Not yet implemented: custom format handlers (Markdown is the only handler), and the
checks-side sweep of settings lint rules (they run in review turns only).

## Fixtures

- `example/f1`: two small docs, the smoke fixture.
- `example/f2`: ten docs with planted traps; see `example/f2/EXPECTED.md` for the
  hand-labeled expectations.
