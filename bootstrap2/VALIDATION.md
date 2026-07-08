# Proof-of-concept validation

Results of running the turn-based compiler (docs2 design) on the fixtures, against the
success criteria set before implementation. All runs used local models through Ollama
(`qwen3:4b-instruct` unless noted). Full traces live in the session scratchpad; the
persisted evidence is each fixture's `jazyk-out/` (journal, status, graph shards).

## Scorecard

| Criterion | Result | Notes |
| --- | --- | --- |
| Zero junk entities | PASS | No file paths, CLI flags, or markdown terms in any run. The admin.md junk bait (`--port`, `/etc/orderly/config.toml`) produced none. |
| Entity precision within 20% of hand count | PARTIAL | F2: 18 vs ~10 hand-counted. The overshoot is real concepts from glossary/roadmap prose the model normativized, not syntax junk. |
| Cross-doc identity: one node used across 3+ docs | PASS | `ent:order` is one node; requirements reference it from 6 documents. Mentions accumulate only on upsert, so the mention list is thinner than the requirement evidence. |
| buyer/Customer duplicate trap | PARTIAL | No separate `buyer` entity was ever created (the old design's failure mode). The explicit buyer=Customer link was not surfaced either. |
| Planted contradiction caught, right subjects | FLAKY at 4B | Run 2: caught exactly (`req:payment-2` vs `req:orders-3`, severity error). Run 3: missed. This is model judgment variance, the capability the benchmark is designed to gate. |
| Diagnostic noise | IMPROVED, not at target | F2: 26 open diagnostics for 18 entities (about half trace to the normativized roadmap/glossary). Old design: 534 diagnostics for 564 entities with 109 errors on clean docs. |
| No-op rebuild: zero LLM calls, under 1s | PASS | 0 turns, ~10 ms, on F1 and F2. |
| One-sentence edit touches only its region | PASS | 1 reconcile turn plus 3 review turns; graph diff was purely additive (`ent:shop`, `req:catalog-3`). |
| Convergence | PASS | Every build converged or parked cleanly and converged on resume. No wedged builds. |
| MCP read surface | PASS | `context` returns budget-bounded packs with working expansion handles; "requirements between Order and Payment" is answerable from one call. |

None of the failed-again triggers fired (junk >5%, duplicates >20% after review,
non-convergence, incremental cross-contamination, local model unable to complete F1).

## Findings worth keeping

- `gemma4:e4b-mlx`, the model whose truncated JSON broke the old one-shot pipeline,
  converges cleanly through the turn loop: 0 errors, 0 warnings, 100% coverage on F1.
  The environment-side gates do the work the model cannot.
- The text codec (JSON action per message, for models without native tool support)
  drives the full loop: 55 actions parsed in one F1 run, with repair messages correcting
  bad arguments, unknown relationship types, and id typos.
- Repair-oriented errors work as designed: models read `nearest existing: ent:shop` and
  fix the call.
- Sticky diagnostics work by construction: cleared findings flip to `resolved`, ids
  never churn, triage is never touched.
- Parked work resumes: a failed turn parks its item with an `incomplete-build`
  diagnostic and the next build picks it up first.

## Fixes discovered by running (each documented in docs2 first)

- Implicit `done`: a model that goes silent with staged work commits through the same
  gates instead of losing a valid changeset (docs2/compiler/turns.md#budgets).
- Whitespace-insensitive quote location: sentences wrapped across source lines were
  failing verbatim match, 88 rejections in one run (docs2/compiler/model.md).
- Requirement natural key (source section + normalized statement): kills exact-duplicate
  requirements from retries and reruns (docs2/compiler/graph.md#mutations).
- The covered-claim gate: a section containing `shall` cannot be claimed covered without
  a requirement sourced from it; stops silent requirement loss
  (docs2/compiler/graph.md#validation-gates).
- One bounded fix-up pass re-enqueues documents with uncovered sections
  (docs2/compiler/reconciler.md#waves).
- Review turns run grouped: entities sharing requirements review in order, groups in
  parallel, so judgments see their neighbors' merges (docs2/compiler/reconciler.md#waves).
- `report_diagnostic` accepts only the cataloged review rules; free-form rule names were
  producing incomparable findings (docs2/compiler/tools.md#write-tools).

## Known weaknesses (open)

- Semantic judgment quality (extraction density, review calibration, lint application)
  is the model's, not the harness's. `jazyk benchmark` now gates density and review
  judgment behind tiered verdicts; generation quality is still ungated. No model has
  reached the `extraction` tier against the current case set.
- Cross-document near-duplicate entities (`backend` vs `backend-system`) remain
  review-turn work, which weak models skim. Same-doc rephrase-duplicates are now caught
  deterministically (`duplicate-requirement`), and a reworded re-extraction of the same
  sentence refreshes in place.
- Weak models declare few requirement `edges` despite the prompt guidance and the review
  repair pass, so their typed relationships stay sparse and reachability falls back to
  shared requirements. Dense models land them (305 relationships in the gpt-5.5 dogfood).

## F3: the docs2 dogfood

The compiler compiled its own documentation (30 documents, benchmark fixtures excluded)
with `gemma4:e4b-mlx` through LocalRouter. Mechanics: 5 link-graph levels, ~55 turns
across two passes (parked work resumed automatically), 94% section coverage, 0 error
diagnostics, ~54k completion tokens. Two documents stayed parked: their prose cites
sibling documents heavily and the model kept citing the linked document instead of the
target, which the `wrong-document` gate rejects by design.

The graph came out clean but sparse: 10 entities, 4 requirements, zero junk. All four
requirements are the explicit `shall` statements the docs contain, extracted verbatim,
including "The graph store shall mint every id at node creation and never change it."
The finding: docs2's declarative house style ("The store mints ids...") plus the
strict EARS doctrine ("never rewrite non-normative prose into shall statements") leads
a cautious model to classify most sections as non-normative. The deterministic
`suspicious-non-normative` check pushed back on 6 of those marks. Options, deliberately
left open: loosen the extraction doctrine for declarative statements of system
behavior, push the prompt to rephrase declaratives into EARS, or adopt `shall` phrasing
in spec-grade sections. The lint rule ran (4 findings) but the 4B judge misapplied it,
consistent with its failed benchmark verdict.

## The density experiment: doctrine and capability

Three dogfood runs over the same 30 documents isolate the two variables:

| run | entities | requirements | relationships | junk | errors |
| --- | --- | --- | --- | --- | --- |
| gemma4:e4b, strict doctrine | 10 | 4 | 0 | 0 | 0 |
| gemma4:e4b, declarative doctrine | 48 | 1 | 0 | 0 | 0 |
| gpt-5.5, declarative doctrine | 157 | 415 | 305 | 0 | 0 |

The declarative doctrine ("declarative prose states obligations, rephrase into EARS")
amplifies model capability rather than substituting for it. gemma minted entities but
could not land the statements behind them (47 `unused-entity` warnings); gpt-5.5
produced a dense spec graph with typed relationships. Both models were graded by
`jazyk benchmark` beforehand: gemma not capable, gpt-5.5 18/19 checks under native
tools. The harness invariants held in every run: zero junk names, zero spurious error
diagnostics.

Operational findings from the same runs: a dense extractor exhausts a 12-round turn
budget before claiming coverage (default raised to 24, and models are told to batch
tool calls); concurrent gpt-5.5 workloads through one provider hit rate limits, so
generation jobs are sequenced, not parallelized.

## Foreign prose: the mdBook corpus

The full mdBook guide (35 documents of someone else's documentation, heavy with code
blocks, TOML snippets, and CLI invocations) compiled with the same local gemma default:
28 entities, 6 requirements, coverage 66%, 7 documents parked, zero error diagnostics.

- The doctrine transfers: every extracted entity is a sane mdBook domain concept
  (`ent:book`, `ent:chapter-file`, `ent:html-renderer`, `ent:handlebars-template`), and
  the requirements are correctly EARS-ified from declarative prose, e.g. "The system
  shall generate a 404 page to be used for broken links."
- The junk gates held on completely foreign content: no paths, flags, or markdown terms
  became entities despite the corpus being full of them.
- Density again tracked model capability, not corpus origin: low requirement recall and
  parked documents mirror the gemma dogfood, not anything mdBook-specific.

## End to end: docs to green tests

The full chain ran on the F2 fixture with `gpt-5.5`: `jazyk compile` built the graph,
`jazyk codegen` generated 12 Rust modules (compiling to within 4 errors on the first
shot), `jazyk testgen` derived 22 tests from the 22 requirements, and a bounded repair
loop (the model fixing its own files given rustc output) reached a green `cargo test`:
43 tests passing, 0 failures.

- Everything is machine-generated: modules, tests, and repairs. Assembly (module naming,
  `crate::` to crate-name rewrites in integration tests, one test-framework dependency)
  is deterministic harness work.
- Repair converged in 9 iterations total once run at full width; per-file repairs
  preserved the requirement-id traceability comments, so every passing test still names
  the requirement and quote it verifies.
- The failure mode of narrow repair (two files per pass) was oscillation; repairing every
  failing file per pass converged monotonically: 45 errors → 3 files → green.

## Self-hosting: the compiler regenerated from its own documentation

`jazyk codegen` ran against the gpt-5.5 docs2 graph for the 18 most requirement-dense
core entities (`graph-store`, `entity`, `ears-requirement`, `derived-relationship`,
`diagnostics`, `section`, `semantic-graph`, `turn`, `context-engine`, `reconciler`,
`changeset`, `tool-registry`, and peers). The result assembled into `jazyk3`, a
~14,800-line crate that compiles with zero errors and passes its 23 embedded unit
tests, after bounded model repair plus deterministic salvage.

- The generated module map mirrors the design vocabulary, not bootstrap2's file layout:
  `store` ↔ `graph_store`, `context` ↔ `context_engine` + `context_pack`,
  `reconcile` ↔ `reconciler`, `tools` ↔ `tool_registry`, with the model split into
  per-node modules. The docs, not the old code, shaped the architecture.
- Boundary finding, now measured: an entity whose requirement set is dense enough
  (`semantic-graph` carries 51 requirements) exceeds the model's output ceiling and
  truncates as a single generation unit; the same ceiling breaks whole-file repair.
  Snippet-scoped repair and deterministic salvage (lexing brace depth outside strings
  and comments, cutting at the last complete item) close the gap; the real fix is
  decomposing dense entities into several generation tasks.
- Scope honesty: 18 core entities, not the whole system. Frontends, the LLM client, and
  the markdown parser were not selected. This is the self-hosting direction proven, not
  a drop-in replacement.

## Stability under edits

Controlled edits to the converged F2 fixture, gpt-5.5:

- Cosmetic (a comma added to unquoted prose): one turn, one mutation (the section's
  coverage re-claim), and the graph shards byte-identical to the pre-edit snapshot.
  The no-op doctrine holds: the model read the section, marked it non-normative again
  with sound reasoning, staged nothing else.
- Punctuation inside a quoted requirement sentence: every requirement id survived (no
  delete and recreate churn). The quote re-anchor and the third experiment (a new
  sentence trickling into exactly one new requirement and an incremental codegen of
  only the touched entities) were cut short when the remote provider began refusing
  authentication mid-run; the machinery for both is in place (natural-key quote
  refresh, `codegen_pending` diffs) and resumes with provider access.

The run also exposed and fixed a gate gap: `update_requirement` accepted relationship
types `upsert_requirement` would reject.

## Pluggable generation workers over MCP

The generation contract (`codegen_instructions` / `codegen_pending` / `codegen_task` /
`codegen_mark`) was exercised by an external agent speaking only the MCP wire protocol
over stdio against `jazyk mcp graph`, from the F2 fixture:

- `codegen_pending` returned 6 entities with requirement-level diffs (added ids per
  entity, plus one `(reworded)` case where only statement text changed).
- Per entity, `codegen_task` supplied the context pack, requirement groups, change
  diff, unit path, and `factHash`; the worker made minimal edits guided by `changed`
  (co-cited duplicate requirements at existing sites, added the one genuinely new
  behavior, refreshed a reworded comment) instead of regenerating whole units.
- `codegen_mark` with the package's `factHash` drained pending to 0.
- The assembled crate builds clean and its unit tests pass after the update.

Codex CLI as the worker is blocked by Codex itself, not the contract: `codex exec`
(v0.140.0) fires an interactive approval elicitation for every MCP tool call and
auto-cancels it in non-interactive mode (`ResolveElicitation { decision: Cancel }`),
regardless of `approval: never`, sandbox mode, or project `trust_level`. The same
server answers the same calls in 9ms when driven directly. Running Codex
interactively (approve the `jazyk` server once in the TUI) or with its explicit
bypass flag are the two ways in.

## The verification ledger: requirements to verdicts

The gen workflow (one task per entity producing deliverable files plus tests, tracked in
`gen/ledger.yaml`) ran end to end on F2 with gemma generating and a claude agent as the
fix-and-verify worker:

- `jazyk gen` produced 12 entities' product and test files into `product/` and seeded 27
  requirement rows: 20 `programmatic` (each with an exact `cargo test req_<id>_<hash8>`
  command), 7 `llm` (criteria files; gemma omitted tests it could not write
  programmatically and the harness recorded them as llm rows, as designed).
- `jazyk test` honestly recorded all 20 programmatic rows as failing (the gemma crate
  did not compile). The worker agent fixed the product, and all 27 rows reached
  `verified`, including a genuine llm FAIL: a refund requirement whose implementation
  was only a `println!`. Fixing it flipped the row to `stale-code` by hash, and a
  re-judgment verified it. The full fail, fix, re-stale, re-verify loop worked without
  any human bookkeeping.
- The cascade experiment: rewording one requirement (14 days to 21) recompiled in place
  (id stable), flipped exactly its rows to `stale-requirement (requirement-changed)`,
  surfaced the new requirement as `missing (not-generated)`, and listed exactly the
  three affected entities in `gen_pending` with precise diffs. Regeneration minted a
  new test name from the new statement hash, mechanically retiring the old run command.
- The llm rows were verified over MCP (`verify_task`, judgment, `verify_mark` with the
  package's `factHash`), proving the external-worker contract for verification the same
  way it was proven for generation.
- Dogfood started: `docs2/jazyk.toml` points `[gen] deliverable` at `project2/`; three
  core entities generated 106 requirement rows (79 programmatic, 27 llm) with
  `graph_store.rs`, `reconciler.rs`, `context_engine.rs` and their test files. The full
  run waits for a capable model.

Known weakness, consistent with every gemma result: regeneration quality. A fresh gemma
pass over an entity can re-break compilation, which the ledger reports truthfully as
failing rows; a capable model or an agent worker closes the loop.

## Prompt iteration: declarative extraction on a weak model

Four measured iterations against a 5-document warehouse-ERP fixture, gemma throughout.
The target: a weak model must extract atomic requirements from plain declarative prose
("The frontend is a web application built using React and TypeScript"), never wave
whole documents through as non-normative.

| iteration | requirements | tech-choice reqs | entities | note |
| --- | --- | --- | --- | --- |
| baseline | 4 | 0 | 11 | every top-level section non-normative |
| v1 sentence test | 11 | 2 | 27 | extraction unlocked; tech names became entities |
| v2 per-section + strict gate | 19 | 4 bundled | 8 | entities clean; gate bounces parked 3 turns |
| v3 batching + quote rules | 17 | 5 bundled | 8 | 78% coverage; bundling survived prompting |
| v4 bundle gate | 18 | 6 atomic | 18 | "shall be built using React." at last |

Findings that changed the harness, docs first as always:

- The sentence test ("does it say what the system is, does, uses, allows, requires, or
  limits") plus a worked technology-choice example moved gemma from 0 to reliable
  extraction. Non-normative is now docmented as the exception with a closed list.
- The covered-claim gate no longer keys on the word `shall`: any `covered` claim
  requires a requirement sourced from that section, so prose without spec grammar
  cannot be skimmed past silently.
- Atomicity would not land by prompting: three phrasings failed. A deterministic shape
  gate (`bundled_tech_list`) rejecting "built with X and Y" statements with a
  repair message succeeded on the first run. Prompt doctrine teaches; gates enforce.
- Residual gemma variance: coverage swings run to run (42% to 78%), and cross-document
  near-duplicate entities (`backend` vs `backend-system`) remain review-turn work.

## Harness iteration: the example-erp corpus

Continuation of the prompt iteration on the same warehouse-ERP fixture (grown to 6
documents with planted traps: an MD5 password line, a link to a nonexistent document,
an empty file), gemma throughout. Each round is a fresh from-scratch compile resumed to
convergence; numbers are the converged graph.

| round | requirements | entities | open warnings | note |
| --- | --- | --- | --- | --- |
| baseline | 15 | 9 | 5 | all of user.md waved through non-normative; MD5 trap missed; one concept minted as three entities |
| leniency + budgets | 27 | 22 | 32 | user.md extraction unlocked; operations minted as junk entities; review wave starved the fix-up and the verdict claimed converged at 68% coverage |
| honesty + dedupe | 28 | 15 | 8 | budget overflow parks instead of vanishing; duplicate pollution reached zero |
| final | 32 | 17 | 5 | per-bullet extraction incl. the MD5 password; two of the five warnings are the planted doc bugs |

Findings that changed the harness, docs first as always:

- Enumerations were dropped everywhere: the model extracted the colon lead-in and
  ignored the list under it, losing operations, properties, roles, and the sub-system
  containment. One requirement per item, quoting the item's own line, is now doctrine
  (docs2/compiler/concepts/ears.md#enumerations); items that are links still count.
- Stale anchors are a contract: the `done` gate rejects a turn that leaves one
  untouched (re-anchor via natural key, revise, or delete). Before the gate, a turn
  marked coverage around a stale anchor twice and converged with the phantom left in.
- Gates resolve intent instead of bouncing it: prefix-less ids (`user-management`),
  unique display names, and markdown-escaped quotes (`` \` ``) all resolve. A 4B model
  burned entire turns retrying identical calls the error message had already answered.
- Provenance validates first in `upsert_requirement`: a quote that does not locate is
  the clearest signal a statement was invented, and it was being masked by entity-id
  errors. The stuck case: gemma extracting the prompt's own gateway example into
  backend.md in a loop. The prompt now says the examples are illustrations.
- The round budget scales with dirty-section count, and an implicit `done` drops
  dishonest covered claims instead of discarding the whole changeset: one bad mark was
  sinking 23 rounds of good staging.
- Coverage outranks review: the fix-up pass runs before the review wave, and any work
  that no longer fits the turn budget parks, so converged means converged. The failure
  it fixes: 22 no-op review turns exhausted the cap, the fix-up was silently skipped,
  and 6 uncovered sections hid behind a converged verdict.
- One sentence, one fact: a re-extraction from the same source sentence whose statement
  subsumes the existing one refreshes in place instead of minting a twin.
  `duplicate-requirement` now separates intent: the same sentence extracted twice is a
  warning; the same fact restated across documents is intentional redundancy, kept and
  noted as info; parallel enumeration items are not flagged at all.
- Operations (`createUser`) are requirement detail, never entities: camelCase code
  identifiers are rejected by the junk-name gate. A 4B model was minting one entity per
  operation, flooding reachability with islands.
- Empty files and broken links were invisible by construction (no sections means no
  turn; links only feed scheduling), so `empty-file` and `broken-link` are now
  deterministic checks. Both planted bugs are found with zero LLM calls.
- `suspicious-non-normative` keys on obligation verbs and definition-list bullets, not
  the word `shall` (which documentation rarely uses); a lead-in-only body whose items
  live in child sections is exempt.
- Review turns repair missing references: the pack lists requirements whose statement
  names the entity without referencing it. One such miss ("user accounts" naming
  `ent:user`) had stranded a five-entity cluster unreachable.

Residual, and correctly surfaced rather than hidden: run-to-run judgment variance of
the 4B model (an orphaned field entity, an operation-as-subject island) lands as
`unused-entity` and `unreachable-entity` warnings, which is the product behavior, not a
defect. The two open non-planted warnings in the final run are exactly that class.

## The benchmark gates what the project depends on

The benchmark grew from 7 to 12 cases and the binary verdict became tiers that route
work instead of rejecting models (`not capable`, `extraction`, `review`), per
docs2/benchmark/benchmark.md. New graded skills: extraction density on plain
declarative prose, edge declaration from a sub-system list, rephrase-duplicate
collapse below the deterministic token-overlap threshold, lookalike entity merge, and
project lint application. Check patterns are real regular expressions now (the schema
always said so; the hand-rolled matcher fell short). Results persist to
`<out>/benchmark/results.yaml` keyed by model with a case-set hash; a verdict quoted
without its hash is stale by definition.

`gemma4:e4b-mlx`, re-graded against case set `fee6d1af`:

| codec | score | verdict | notable |
| --- | --- | --- | --- |
| native | 29/41 | not capable | fails extract, declarative, density, edges, review, duplicate |
| text | 30/41 | not capable | passes declarative and edges; density misses one fact (5 of 6); duplicate skimmed |

- The new review cases measure judgment the old set could not see: gemma merges the
  lookalike entities and applies the lint rule on both codecs, but never collapses
  the rephrase-duplicate pair, the exact weakness the dogfood recorded.
- Density fails as designed: under text gemma lands 5 of the 6 planted facts; under
  native it waves the frontend section through unprocessed. The F3 failure mode is
  now caught before a build instead of inside one.
- No model has reached the `extraction` tier under the extended set. The prior
  gpt-5.5 18/19 score predates the case-set hash and does not compare; re-grading
  stronger models is the open next step.

## Cost

F2 (11 documents, cold build): ~30 turns, ~220 rounds, ~18k completion tokens, roughly
8 minutes wall-clock on a local `qwen3:4b-instruct`. Incremental single-edit builds run
4 turns. No-op builds are free.
