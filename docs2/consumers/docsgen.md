# Documentation generation

Documentation generation feeds the graph back into the documentation. The compiler reads
prose and builds a graph; this consumer reads the graph, through the
[context engine](../compiler/context.md) and the [read tools](../compiler/tools.md#read-tools),
and produces reports and proposals that improve the prose. Nothing is written to a source
file without human review.

## The requirements document

Every committed changeset renders one human-readable document per entity into
`<out>/docsgen/<entity-slug>.md`: the definition, every requirement with its verbatim
quote and source section, the derived relationships, and any open diagnostics. The
render is deterministic (no LLM), so it is always as fresh as the graph, during builds
and on builds that park work alike. `jazyk docsgen` renders on demand.

This is the reading surface between prose and graph. The [LSP](../frontends/lsp.md)
links every entity occurrence in a source document to its requirements document, so a
reader clicks a concept in a source page and lands on everything the project says about
it, with each statement pointing back at the exact source sentence.

## Glossary

The glossary is generated from the graph: every entity's name, aliases, and `definition`,
sorted by name, linked to its defining sections through its mentions. The graph is the only
input, so a term missing from the glossary is a term missing from the graph.

## Fragmentation reports

An entity whose mentions span many documents may deserve its own page. The report ranks
entities by mention spread (documents touched, sections per document), so an owner can
decide what to consolidate. Fragmentation is a query over the `mentions`
[axis](../compiler/model.md#edge-axes), nothing more.

## Staleness reports

Open [diagnostics](../compiler/model/diagnostic.md) grouped by section give a staleness map
of the docs: which pages accumulate contradictions, stale anchors, and low-confidence
facts. Sections marked `non-normative` whose `note` looks weak are listed for re-review.
See [coverage](../compiler/reconciler.md#coverage).

## Plain-English lint

Projects declare lint rules in prose in [project settings](../compiler/project-settings.md).
E.g.:

- terminology bans: never call it a `basket`, the term is `shopping cart`,
- style rules: a requirement names its actor, no passive voice.

The rules ride along in review turns and in the checks
([waves](../compiler/reconciler.md#waves)). Findings are ordinary diagnostics under the
`lint` rule (e.g. `diag:lint-1`), so each carries a `quote` and `reasoning`, lands in the
same triage queue, and is resolved like any other diagnostic.
