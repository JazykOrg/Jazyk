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

- Multi-project runtime and portable workspace contracts for hosted compilation.
  Accounts, project selection, and the complete editor are owned by the private
  [Jazyk Cloud plan](https://github.com/JazykOrg/jazyk-cloud/blob/master/plans/cloud/README.md).
  The native [API](./frontends/gui.md), [browser engine](./frontends/wasm.md), and
  [capability descriptor](./frontends/gui.md#backends-and-capabilities) stay here.
- [Journal](./compiler/graph.md#journal) rotation: cap `journal/` growth, compact old
  changesets.
- Custom [format handlers](./compiler/parsing.md#format-handlers); the built-in ones
  are Markdown, plain text, and natively DOCX and PDF.
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
  (`place-anchors`, `retrace`, `conform-instance`, the caps variant of
  `abstract-entity`), plus transition facets, attributes, cardinality, and the
  `flow-unplaced` path of `curate-view`. Verdict quality of the
  judgment gates is the first target: a gate checks that a verdict exists, only a
  planted ground truth grades whether it is true. Refresh `known-results.yaml` on the
  new case set hash; remote runs cost money, approve per run.
- A verified ledger with no product. A programmatic row's verdict is its exit code
  ([verify](./compiler/goals/verify.md)), so a test that imports a descriptor object
  (an object literal naming the framework, the database, and the endpoints) and asserts
  its fields passes, and a deliverable with no server can read verified on every row.
  The criteria skill and the bind contract now say an observation uses the product as
  its user does ([criteria](./compiler/skills/criteria.md)), and the
  `criteria-observation` case grades it. What is still missing is a deterministic
  signal. One candidate: the [medium](./consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)
  records a start or smoke command for a software deliverable, and a row cannot read
  verified while that command fails. Owner decision.
- The fixed cost of a round. A three-sentence document compiled on the local model
  billed 401k tokens across three sessions; tool results resent were about a tenth of
  it. Each round resends roughly 48k characters before any history: the active skill
  (extraction, 22.5k), the batch's tool schemas (the reconcile toolset, 24 tools,
  16.4k; the whole catalog of 52 is 37.7k), the agent contract (about 6k), and the goal
  block with the loaded set (3k to 4k). The prefix is byte-identical across a
  session's rounds, so a provider's prompt cache applies to it; the journal records
  cached input as `cachedTokens` where the endpoint reports it (Ollama reports none,
  and whether LocalRouter forwards `prompt_tokens_details` is unmeasured). Shorter tool descriptions or skills would cut
  the rest, against the explicitness the gates and benchmarks were tuned on: an owner
  decision, to be measured case by case with `jazyk status --costs` and the benchmark's
  token figures, never guessed.
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
- The journal's cost report in the GUI: the [board sidebar](./frontends/gui.md#board)
  shows this build's `costs` by kind and by class; the per document and across builds
  view is on the CLI only ([`jazyk status --costs`](./frontends/cli.md#jazyk-status)).
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

- [x] `cargo test` green (611 on 2026-10-09), including the unit tests for goal
  derivation and identity across re-derivations, cone readiness, escalation,
  per-direction relationship recompute, state machine derivation and its checks,
  default views, lifting (a multi-type pair collapsing under it), the render emitters
  against the showcase, justification closure, flow placement, cross-class flip
  parking, change records, and gate rejections.
- [ ] `bootstrap/example/f2` converges and its planted traps still trip; its
  `EXPECTED.md` states free-form statements and the new kinds.
- [ ] The example corpora converge: `example-sort`, `example-erp`, `example-slides`.
- [ ] The two new fixtures: `example-org` (an organization corpus) and `example-novel`
  (a narrative corpus), beside `example-slides`, each converging with views and
  diagrams that read as an org chart and as scenes.
- [ ] The dogfood with diagrams: `jazyk compile` from the repository root reaches
  `converged` (blocked and advised counts are acceptable, silence is not);
  `.jazyk/ds_main/diagrams/`
  holds `.puml` and `.svg` per view ([output layout](./compiler/diagrams.md#output-layout));
  [docsgen](./consumers/docsgen.md) pages embed the images; `jazyk preview`,
  `jazyk explain`, and `jazyk ripple` answer ([CLI](./frontends/cli.md)); LSP hover
  shows a diagram ([capabilities](./frontends/lsp.md#capabilities)); the GUI board
  renders goals live ([GUI](./frontends/gui.md)).
- [ ] The zero-call rebuild on the dogfood: an immediately repeated `jazyk compile`
  derives zero goals and makes zero LLM calls
  ([incremental builds](./compiler/compilation.md#incremental-builds)). `f1` holds it.
- [ ] The dual-write roundtrip: an edit through the inspector rewrites the sentence and
  the graph in one changeset without re-dirtying the document; a decree queues a
  ratification proposal, and accepting it flips the fact's provenance to `quote`
  ([edit paths](./compiler/compilation.md#edit-paths)).
- [ ] The store version on the dogfood: an out directory without `version: 4`
  archives to `<out>.bak` and the next build reconciles from the empty graph. `f1`
  holds it.
- [ ] Docs tree and binary agree: no page describes machinery the binary lacks, no
  behavior lacks a page, prompts and skills are `include_str!` bytes, and the root
  `AGENTS.md` describes the landed design.
- [ ] Full docs gen and verify run to verdicts. The archive drops the old ledger, so
  binding runs fresh ([the ledger](./consumers/gen.md#the-ledger)).
- [ ] `jazyk benchmark` under the native runtime on the current case set, with the
  `known-results.yaml` entry refreshed.

## Docs versus code audit (2026-09-04)

A read-only audit compared every doc area with the code that mirrors it and found
42 discrepancies; the full ranked list is in the session's scratchpad
(`docs-audit-2026-09-04.md`). The eight ranked first are closed (2026-10-09): the
sweep runs behind every commit, verify servings carry the read-only file tools,
mandatory GC batches rank first, the delete and merge gates hold at staging, the
section kinds the Markdown handler produces are the ones the docs name, the CLI
refuses unknown options with exit `2`, the `via` lists agree, the chat serving
carries `retract_decree` and `bump_limit`, and `docsgen.md` states the entity header
and the report files as rendered.

## Pair judgment volume (2026-09-05)

On `example-saas` at 30% coverage the board held 479 goals, 358 of them
`rejudge-pair`, against 144 requirements and 36 entities; the pairwise judgments
dominate a corpus by an order of magnitude over ingest. Measured under Claude Code:
a pair session settles four pairs (org) to eight (f2) in six to twenty calls with
nothing loaded, so the batch is far under the budget. First lever, landed: a pair batch
packs its locality to the context budget (four to eight pairs on the org snapshot;
the org relaunch settles eight to twenty goals per batch). Measured after it on
example-saas at 98 percent coverage: 418 requirements, 1336 pairs over 384
requirements; each created requirement picks its best six, but a broad statement is
picked by many (one sits in 40 pairs, another in 37; 150 requirements sit in more
than six). Second lever, landed: the degree cap
([pairs](./compiler/reconciler.md#pairs)), `pairs-per-requirement` at 8, pinned
pairs first and the rest by score, the waiting pairs standing on their records
(`detail.judged` on the created and revised records, the record cleared at the
sweep when no candidate remains). Measured on the same snapshot with no LLM call
(`jazyk status`, generation 29): the board holds 880 open pairs where it held 1336,
456 wait under the cap and none is lost, the top requirement's open degree is 8
where it was 40 (`req:collaboration-2`: 8 open, 32 waiting), 74 requirements sit at
the cap and none above it, 381 requirements hold an open pair where 384 did. Third,
landed: `settle_pairs`, a batch-level verdict list so one claim settles a batch's
pairs, one entry per goal under the single calls' gates
([goal tools](./compiler/tools.md#goal-tools)); measured under Claude Code on the
saas snapshot, a six-pair batch went from one claim per pair to one `settle_pairs`
call plus `done`. Next: a `pairs-per-entity` limit in the registry (the lexically
closest pairs, the rest left to `review-entity`, which reads every statement anyway).

## The dogfood's rate (2026-09-08)

Measured on `docs/` under Claude Code with the pair cap: a `reconcile-section` batch
carries two or three sections and lands in about four minutes; 639 sections need some
five hundred such batches, and each landed section opens pairs faster than a pair
session settles them (2323 open goals at 47 sections covered). Levers, in order: a
section batch packed by section size instead of a flat estimate (a short section costs
little; a batch should hold ten of them); scopes gated like names (a scope the
documents never name as a bounded context is refused, and a case variant of an
existing scope folds into it; the run minted `Benchmark` beside `benchmark` and
`Jazyk` beside `public`); the `pairs-per-entity` limit from the pair volume entry.

## Evidence audit (2026-09-25)

Every model-executed kind's contract was read against its gate: each duty the hints make
concrete is now a verdict the evidence names, checked against the staged work
([goal tools](./compiler/tools.md#goal-tools)). Left open:

- `conform-instance` documents that an `update_requirement` on a requirement not
  naming the instance is refused; the code does not refuse it, since a `review-entity`
  goal on the type in the same batch legitimately updates the type's statements. The
  gate needs the owning goal of each staged op before it can hold.
- `curate-view` asks for the same decision on the twin view (the use-case and sequence
  views sharing a title); nothing checks it, because a default twin derives its members
  and the rule for when a curated twin must mirror is not stated.
- `done` re-runs each resolution's gate but not its evidence checks, so work staged
  after a mark (an `update_view` that re-adds a node claimed excluded) is not re-read
  against the verdicts.
- Advisory, judgment only: `derive-criteria`'s observer rule (a criterion uses the
  product, never inspects a contract or a specification: a lexical refusal would hit a
  sales contract or a slide deck's page), `review-entity`'s same-document duplicate
  delete (the pair's own `rejudge-pair` goal carries it), `reconcile-section`'s
  extraction duties (attributes, transitions, the page subject), and the placement and
  split judgments of `curate-view`, `split-view`, and `abstract-entity`.

## Stability review (2026-10-09)

A code review of every module for crashes, corruption, hangs, and lost updates, each
finding verified in code before a change. What landed, all docs-first with tests:

- The disk boundary. Every store, ledger, and drafts write lands through a sibling
  temporary file renamed into place; a file that exists but does not parse makes its
  owner read-only instead of reading as empty; a writer re-reads the generation under
  an OS advisory lock and refuses `store-moved`
  ([writes and unreadable files](./compiler/graph.md#writes-and-unreadable-files),
  [concurrency](./compiler/graph.md#concurrency)). The stored hash is pinned to
  SipHash-1-3 ([section](./compiler/model/section.md)).
- Bounds. Every shell command runs under the command timeout and reads exit `124`
  ([runners](./consumers/gen.md#a-test-that-could-not-run-says-nothing)); the ACP
  handshake, session calls, and the embedded agent's MCP calls wait at most the idle
  timeout; `read_text_file` is capped; a goal resolved and re-opened with the same
  change twice parks ([parked and failed](./compiler/reconciler.md#parked-and-failed)).
- Input. An unreadable document keeps its stored tree; repeated heading slugs take
  numeric suffixes; headings follow CommonMark spacing
  ([parsing](./compiler/parsing.md)).
- Cost. Derived indices are built once per derivation
  ([derived data](./compiler/graph.md#derived-data)); journal reads are bounded and the
  pair scorer's inputs are computed once per pass ([journal](./compiler/graph.md#journal),
  [pairs](./compiler/reconciler.md#pairs)); the local API keeps disk work off the async
  runtime, coalesces board derivations, and bounds the jobs it holds
  ([jobs](./frontends/gui.md#jobs)).
- Panic boundaries around GUI jobs, embedded agent turns, and proxy commands.

Left open, each an owner decision or a measurement:

- The validation ladder under NOW has run only on `f1` since the landing; the dogfood
  row of `bootstrap/VALIDATION.md` still reads "no result". Every change above is
  verified by unit tests and synthetic stores, not by a converging build of the
  dogfood.
- Pair judgment volume ([pair judgment volume](#pair-judgment-volume-2026-09-05)): the
  derivation is cheap now, the sessions are not; the degree cap and locality batching
  want measuring on `example-saas`.
- The fixed cost of a round ([NEXT](#next)): measure cached input through LocalRouter
  before cutting any payload.
