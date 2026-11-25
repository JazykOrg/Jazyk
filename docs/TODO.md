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
  `abstract-entity`), plus transitions, attributes and cardinality. Verdict quality of the
  judgment gates is the first target: a gate checks that a verdict exists, only a
  planted ground truth grades whether it is true. Refresh `known-results.yaml` on the
  new case set hash; remote runs cost money, approve per run.
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
  [default views](./compiler/model/view.md#membership). A similarity signal inside
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

- [x] `cargo test` green (1046 on 2026-11-07), including the unit tests for goal
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

## Field report: a doc-first project (2026-11-22)

A Claude Code session compiled an external project of 18 documents, 116 sections,
about 330 requirements, and 50 entities, written docs first. Reported, not yet
reproduced here:

- Judgment volume blocks generation. After extraction the board held 634 mandatory
  goals: 535 `rejudge-pair`, 182 `declare-edges`, 49 `review-entity`, 49
  `derive-criteria`, 31 `diagram-coverage`. Generate goals wait behind every one
  ("compile goal(s) open in the cone"). Rows of one table on a shared entity pair
  with each other (about 40 pairs from one networks table). Candidate levers: the
  `pairs-per-entity` limit from [pair judgment volume](#pair-judgment-volume-2026-09-05),
  a similarity floor on pair candidates instead of a shared entity alone, and sibling
  rows of one table or list left to `review-entity` by default. `settle_pairs` already
  settles a batch in one call; the report does not say whether the session used it.
- Oversized tool replies. `begin_goals` on a pair batch returned 151k characters and
  `goals` 155k, past the client's tool output limit, so the agent could not read its
  own batch contract. Each `reconcile-section` batch also repeated about 8k characters
  of the per-kind contract after the "(same agent contract ... unchanged)" marker.
  Wanted: a size bound with paging on `begin_goals` and `goals`, and the per-kind
  contract sent once per session.
- Tool list flapping. The MCP server disconnected and reconnected several times,
  dropping `write_document`, `release_work`, and `save_document_set` mid-session; the
  agent fell back to plain file writes. Tools unlocked by `edit-documentation` did not
  appear until the client's tool search ran.
- `release_work compile` covered only the sections dirty at its call. Documents
  written afterwards needed a second release, and nothing said so.
- 17 `broken-link` warnings from a README written before its targets stayed open after
  the targets existed. Link findings resolve when the deterministic checks re-run; a
  build that never closes may never re-run them. Unconfirmed.
- Long headings ("Phase 0 ... a go/no-go gate") mint long section and requirement IDs
  (`phase-0-de-risk-spike-1-2-weeks-a-go-no-go-gate`). A slug length bound is a
  candidate.
- Working well: the `upsert_entity` mention-unnamed refusal was clear.

## Field report: a stuck tier-0 goal (2026-11-25)

The same project's orchestrator ran about 15 parallel agents editing docs first, then
one compile from generation 72 to 96 on 0.2.2+203.g3f718b9, store version 7, about
600 sections. Reported, not yet reproduced here:

- A `place-anchors` goal outlived its alignment and blocked the board. At generation 72,
  `g:place-anchors:views.md` aligned with 23 proposals and sat parked. At generation 94 a
  README `reconcile-section` commit deleted 9 of those anchors' requirements ("source
  section gone"), and the `views.md` snapshot moved, so the pending alignment vanished.
  The goal stayed in `local/run.yaml` `parked:` with its frozen `change.proposals`. Then:
  `load alignment:views.md` reported no pending proposals; `place_anchor` on a surviving
  requirement refused `unknown-anchor`; `orphan_anchor` refused with a message listing the
  rejected ID among the batch's proposals; `mark_goal_failed` was accepted, but `done`
  (plain and `checkpoint`) still refused `undecided-proposal` for all 23. Only
  `abandon_goals` released the batch. Being tier 0, the goal held about 620 mandatory
  goals in every document; 40 of 602 sections were covered. Wanted: a commit never
  deletes anchors a pending `place-anchors` goal holds, or derivation drops or
  re-derives a goal whose alignment is gone; `done` accepts a goal marked failed; the
  anchor tier gates its own document only. Workaround found later: `read_open_work`
  showed the goal's expired open work (epoch 1, no mutations), and
  `discard_open_work` on it let the board re-derive fresh goals. Every refusal named
  the batch's proposals and none named that work or that tool, so the escape exists
  but went undiscovered for a whole run; the refusals should point at it.
- Placing the same anchor twice in one batch (to switch `reevaluate`) made `done` refuse
  `commit-skipped` "source not found" until the first placement was unstaged.
- A missing model. `~/.jazyk/config.toml` named a model the endpoint does not serve.
  `jazyk compile` got HTTP 404, logged it as "session stopped: refusal", parked about
  800 goals on the session limit, and advanced the generation. `compile_document_sets`
  over MCP closed the server twice with no error returned, likely the same cause.
  Wanted: a model preflight before the first session that fails fast and names the
  served models, and the tool returning the error instead of exiting. `--agent claude`
  ran cleanly.
- Reply size again: `done` with `beginNext` returned 60k to 110k characters and
  `open_work state=all` 54k of committed history. Wanted: replies under about 20k by
  default, and active work as the default filter.
- Status drift: `graph_status` read generation 72 while `jazyk status` read 70;
  "awaiting release" reasons stayed until the next derivation after `release_work`.
- `jazyk status` listed 31k unclaimed files, Android build output among them; the
  default source glob should honour `.gitignore`.
- `report_diagnostic` edit options refuse an empty `edit.new_text`, so a proposed
  deletion cannot be expressed.
- Doc churn outruns compilation. With many agents editing and compilation released
  rarely, hundreds of sections waited. Asked for: cheap incremental per-document
  reconciliation, independent document batches in parallel, and a short "N sections
  pending in these documents" status agents can cite.
- A compile wrote uncommitted `.jazyk/` changes into the shared main checkout while
  another agent merged and pushed there. The release agent saw hundreds of modified
  and deleted graph files plus about 200 new untracked diagnostic files
  (`stale-provenance`, `unjustified-fact`, `section-too-large`), burying every status
  check; a `git add -A` would have swept up a half-finished compile. Graph commits
  landed on the checked-out main between the
  release's merge and its push, which carried them unseen. The engine runs no `git
  commit` outside tests, so those commits came from the compiling agent. Asked for:
  guidance (in the `compile` skill and the generated project block) to compile in its
  own worktree and land graph state as its own merge, with document edits in separate
  commits from graph state; and fewer diagnostic files per run so status and diffs
  stay readable.
- Asked for: an integrity re-check after an external history rewrite of `.jazyk/`
  files, a configurable PII lint at compile time (the graph mirrors doc text), and bulk
  review of ratification proposals (394 pending `derive-criteria` proposals; group by
  document or ratify a section at once).
- Diagnostics mix kinds: about 74 `unjustified-fact` and `stale-provenance` findings
  that reconciliation clears sit beside findings that need a human. The stale
  `broken-link` findings recurred here.
- Working well: agents kept to docs-first scope and said "not reconciled" instead of
  faking it; skill-gated tools; a real contradiction found between two documents and
  confirmed against the shipped APK; gated anchor tools with named rules; atomic
  commits with overwrite notices; `jazyk explain <goal>` for diagnosing a block.

## Field report: a docs and code change (2026-11-25)

A coding agent on the same project wrote four documents first, committed them, then
implemented the code. Reported:

- Docs first settled questions the agent would have left vague (a button's wording,
  when a client may close, why a part cannot resume after a restart). The cost was
  another docs pass for each requirement added mid-task.
- The agent skipped every workflow skill (no `read_jazyk_skill`, `compile`, `bind`, or
  `verify`), for three reasons: the user was blocked, a docs edit does not authorize
  compilation, and the cost of each workflow was unknown. The graph stayed behind the docs and code,
  reported only as "pending reconciliation".
- The project guidance is long and abstract for a plain docs-then-code task. It does
  not say whether an authorized code change obliges compilation, whether `bind` works
  without a compiled graph, or whether `verify` needs one.
- Asked for: a model-free command listing sections changed since the last compile and
  requirements lacking code or test evidence (`jazyk status` may already cover part
  of this; check what it prints); a one-line cost note per workflow skill in
  `list_jazyk_skills`; a short minimal path for a docs and code change at the top of
  the generated project guidance.
- A second agent (map layers and exact-address geocoding) reported the same: docs first
  settled privacy wording, a cache key, and off behaviour, and surfaced three
  contradicting sentences elsewhere; it skipped every skill and tool for the same
  reasons, plus a task brief that never mentioned Jazyk. New asks: find the section
  that owns a feature by name (its brief named the wrong document); say whether a
  docs edit must be compiled in the same task or can wait for a separate pass; say
  whether code and tests cite requirement IDs; flag sentences elsewhere that a new
  section contradicts, cheaply and before a full compile. The MCP `search` tool may
  cover the first; check whether it returns sections for a feature name.
- A third agent, working in a git worktree, found the MCP server bound to the main
  checkout (`project_context` showed another base commit and 206 local changes), so
  the graph and guarded file tools could not reach its own tree; it used plain file
  tools and left reconciliation pending. Asked for: a worktree-aware project root, or
  `project_context` stating plainly which path the server serves; and a read-only
  "requirements affected by this diff" so an agent can report sync precisely without
  compiling. It found the docs-first and safety rules clear despite the length.
- A fourth agent (profile routing and a full-resolution viewer) also used no skill or
  tool, for the same reasons. New points: it could not tell whether a small behaviour
  addition to an existing section is a behaviour change or a repair, or whether pending
  reconciliation is a blocker or a note to report; the generated project block repeats
  the MCP server instructions almost verbatim. Asked for: a way to link code to a doc
  section without a full compile, and a short table of fix, doc tweak, and new
  behaviour with the tools each needs.
- A fifth agent (a docs-first change across three documents plus Kotlin and Go code)
  read only the `change-code` skill. The skill lists about 28 tools but says nothing
  for an agent editing with ordinary file tools in a git worktree: whether skipping
  `begin_generation` and `record_generation` is acceptable or leaves the graph
  inconsistent. Asked for one line there: after external edits, report pending
  reconciliation; the next compile picks it up. It also noted the duplicated project
  block and MCP instructions.
- Across the five agents: almost none ran a workflow skill (one read `change-code`
  only), and the asks converge on a short task-type recipe at the top of the
  generated project block (deduplicated against the MCP server instructions), a cost
  note per workflow, an external-edit note in `change-code`, and one model-free view
  mapping a docs diff to affected requirements and evidence.

## Evidence audit (2026-09-25)

Every model-executed kind's contract was read against its gate: each duty the hints make
concrete is now a verdict the evidence names, checked against the staged work
([goal tools](./compiler/tools.md#goal-tools)). Left open:

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
