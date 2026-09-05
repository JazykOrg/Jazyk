# TODO

This scratch space is a live document to outline the work for this documentation.

## IDEAS

- Adjacent notations as style options over the same views: a C4-styled rendering of
  the containment tree, ER on the class projection
  ([diagrams](./compiler/diagrams.md#the-emitters)).
- Expose the [limits registry](./compiler/graph.md#limits) as configuration once
  dogfooding settles the values. Per-node bumps with decree provenance cover the
  need until then.
- Glossary promotion: every noun phrase in a section resolves to a known entity, an
  undefined term draws a diagnostic. Entities plus scopes already form a per-context
  glossary; the check is cheap and the leverage is high.

## LATER

- Per-entity file sharding for the [graph store](./compiler/graph.md#storage-layout), when
  `entities.yaml` grows too large.
- [Journal](./compiler/graph.md#journal) rotation: cap `journal/` growth, compact old
  changesets.
- Custom [format handlers](./compiler/parsing.md#format-handlers); Markdown is the only
  handler today.
- Checks-side sweep of the project [lint rules](./compiler/project-settings.md#docs); they
  run in `review-entity` sessions only.
- Per-goal-kind executor defaults in [`[executors]`](./compiler/project-settings.md#executors):
  which kinds a cheap executor handles well and which need the strongest model.
- Ratification review granularity: aggregate
  [ratification proposals](./compiler/model/diagnostic.md#ratification-proposals) per
  target document (one reviewed draft carrying many facts) instead of one prompt per
  fact.
- A cheap dirtiness probe beside full [goal derivation](./compiler/reconciler.md#goal-derivation),
  serving `await_changes` and `jazyk watch`; and whether
  [change records](./compiler/graph.md#change-records) outgrow `status.yaml` into
  sibling files.

## NEXT

- Benchmark cases per goal kind
  ([deferred cases](./benchmark/benchmark.md#deferred-cases)): the kinds without a case
  (`place-anchors`, `rejudge-pair`, `retrace`, `conform-instance`, the GC kinds), plus
  transition facets, attributes, cardinality, and view curation. Verdict quality of the
  judgment gates is the first target: a gate checks that a verdict exists, only a
  planted ground truth grades whether it is true. Refresh `known-results.yaml` on the
  new case set hash; remote runs cost money, approve per run.
- Parallel sessions. Compilation is sequential by design (one build under the build
  lease, one session at a time, [execution](./compiler/sessions.md#execution)); nothing
  depends on parallelism, so it is an optimization measured on the dogfood, not a
  correctness change.
- Embeddings: one index over docs and graph behind the same
  [`search` tool](./compiler/tools.md#read-tools) (same interface, no schema change),
  feeding `dedupe-candidates` lookalike scoring and the flow clustering behind
  [default views](./compiler/model/view.md#default-views). A similarity signal inside
  deterministic machinery, never the context path.
- OpenTelemetry export, off by default: one span per build, session, and tool call,
  GenAI semantic-convention attributes on session spans, OTLP endpoint from config.
  The attribute set is pinned in one module; the journal stays the source of truth
  and OTel is an export ([trace events](./compiler/sessions.md#trace-events)).
- Cost views beyond this build's totals in `status.yaml` `costs`
  ([storage layout](./compiler/graph.md#storage-layout)), which the
  [board sidebar](./frontends/gui.md#board) and `jazyk status` already show by kind and
  by class: per document, and across builds.
- The natural-key experiment: measure the locked entity key
  ([the natural key under containment](./compiler/concepts/identity.md#the-natural-key-under-containment))
  on the corpora. How often an upsert without `parent` hits several candidates, how
  often the model supplies `parent` unprompted, whether a wrong merge ever lands.
- Docs-split vs graph-split: the same size pressure is answered by splitting a
  section (`section-too-large`, `doc-too-large` in the
  [checks](./compiler/compilation.md#checks)) or by splitting an entity
  ([`abstract-entity`](./compiler/goals/abstract-entity.md)). Where the line sits is a
  declared experiment; record what the dogfood and `example-erp` teach.
- Cross-class flip detection has no measured thresholds: it starts at two flips park
  ([flip detection](./compiler/reconciler.md#flip-detection)). Measure on the corpora
  before loosening.
- Edge declaration under weak models stays sparse; measure how much
  [`declare-edges`](./compiler/goals/declare-edges.md) recovers as a GC burst and
  whether cross-document lookalikes reach
  [`dedupe-candidates`](./compiler/goals/dedupe-candidates.md) instead of staying
  duplicates.

## NOW

Validation of the landing, in order. Every run uses a capable model routed through
LocalRouter; the default local model fails judgment-heavy goals.

- [ ] `cargo test` green, including the unit tests for goal derivation and identity
  across re-derivations, cone readiness, escalation, per-direction relationship
  recompute, state machine derivation and its checks, default views, lifting (a
  multi-type pair collapsing under it), the render emitters against the showcase,
  justification closure, flow placement, cross-class flip parking, change records,
  and gate rejections.
- [ ] `bootstrap/example/f1` converges.
- [ ] `bootstrap/example/f2` converges and its planted traps still trip; its
  `EXPECTED.md` states free-form statements and the new kinds.
- [ ] The example corpora converge: `example-sort`, `example-erp`, `example-slides`.
- [ ] The two new fixtures: `example-org` (an organization corpus) and `example-novel`
  (a narrative corpus), beside `example-slides`, each converging with views and
  diagrams that read as an org chart and as scenes.
- [ ] The dogfood with diagrams: `cd docs && jazyk compile` reaches `converged`
  (blocked and advised counts are acceptable, silence is not); `jazyk-out/diagrams/`
  holds `.puml` and `.svg` per view ([output layout](./compiler/diagrams.md#output-layout));
  [docsgen](./consumers/docsgen.md) pages embed the images; `jazyk preview`,
  `jazyk explain`, and `jazyk ripple` answer ([CLI](./frontends/cli.md)); LSP hover
  shows a diagram ([capabilities](./frontends/lsp.md#capabilities)); the GUI board
  renders goals live ([GUI](./frontends/gui.md)).
- [ ] The zero-call rebuild: an immediately repeated `jazyk compile` derives zero
  goals and makes zero LLM calls ([incremental builds](./compiler/compilation.md#incremental-builds)).
- [ ] The dual-write roundtrip: an edit through the inspector rewrites the sentence and
  the graph in one changeset without re-dirtying the document; a decree queues a
  ratification proposal, and accepting it flips the fact's provenance to `quote`
  ([edit paths](./compiler/compilation.md#edit-paths)).
- [ ] The store version: an out directory without `version: 2` archives to
  `jazyk-out.bak` and the next build reconciles from the empty graph, on `f1` and on
  the dogfood.
- [ ] Docs tree and binary agree: no page describes machinery the binary lacks, no
  behavior lacks a page, prompts and skills are `include_str!` bytes, and the root
  `CLAUDE.md` describes the landed design.
- [ ] Full docs gen and verify run to verdicts. The archive drops the old ledger, so
  binding runs fresh ([the ledger](./consumers/gen.md#the-ledger)).
- [ ] `jazyk benchmark` under the landed harness on the four graded kinds, both
  codecs, with the `known-results.yaml` entry refreshed.

## Docs versus code audit (2026-09-04)

A read-only audit compared every doc area with the code that mirrors it and found
42 discrepancies; the full ranked list is in the session's scratchpad
(`docs-audit-2026-09-04.md`). The ones to fix first, each docs-first with a test:

- The GC sweep runs only inside a build; every other commit path (MCP writes, decree,
  dual-write, ratify, answer, triage) skips it. `graph.md` promises the sweep at commit.
- Verify sessions never receive the file tools `serve_files` promises them
  (`mcp.rs` adds them for generate only).
- `docsgen.md` documents glossary, fragmentation, and staleness sections the index
  never writes: implement or strike.
- `jazyk mcp chat` lacks the documented lifecycle tools; `bump_limit` and
  `retract_decree` are GUI-only, not chat tools.
- Mandatory GC goals do not sort before optional ones within a burst (the tier bit is
  dropped for GC batches).
- Documented section kinds `list-item`, `code-block`, `blockquote`, `diagram` are never
  parsed, so diagrams as input cannot work.
- `delete_entity` and `merge_entities` defer two gates to commit while reporting
  success at staging.
- Entity pages lack the documented link to the parent's level page; the level page
  and entity header orders differ from the doc; `compile --help` omits `--sessions`;
  unknown flags are absorbed instead of exiting 2; several `via` values are documented
  but never produced, and `reparent-flip`, `flip-detection`, and `requirements` are
  produced but undocumented.
