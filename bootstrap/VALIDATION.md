# Validation

The measured record of the `uml` landing (the goal-based reconciler, the loaded set, the
in-process renderer, the full generation pipeline) and the early results of the `levels`
landing. Earlier records (the turn-based design, the benchmark grades of the 4B-class
models) live in git history under this file's previous revisions; nothing below depends
on them.

Two models ran everything here:

- Local: `qwen3.8:27b-mlx` via Ollama (free, slow, judgment-limited).
- Remote: `gpt-5.5` via LocalRouter on a subscription with a quota window (see
  [known weaknesses](#known-weaknesses)).

The persisted evidence per corpus is its `jazyk-out/` (journal, status, graph shards,
diagrams, ledger). The grading history and the two prompt-loop reports are the session
scratchpad's `validation-notes.md`, `iterate-cycle-1.md`, and `iterate-cycle-2.md`.
"Not recorded" below means the number is not in those sources; nothing here is estimated.

## Scorecard

| corpus | model | verdict | coverage | sessions, tokens | traps |
| --- | --- | --- | --- | --- | --- |
| f1 (2 docs, 5 sections) | qwen3.8:27b | `converged`; rebuild 0 goals, 0 LLM calls | 100% (5 of 5) | 6, 28k | none planted |
| f2 baseline (10 docs, 25 sections) | qwen3.8:27b | `converged, 59 blocked`, 4 builds | 100% (25 of 25) | not recorded | 5 pass, 1 half (of 6) |
| f2 cycle-1 A/B, tuned payloads | qwen3.8:27b | `converged, 53 blocked`, generation ~23 | not recorded | not recorded | 6 pass (of 6) |
| f2 | gpt-5.5 | `converged, 75 blocked, 2 optional advised`, generation 29 | 100% (25 of 25) | 27, not recorded | 4 pass, 2 half (of 6) |
| f2 generation and verification | gpt-5.5 | 53 of 53 binds, 20 of 20 entities, every requirement verified | n/a | 3 quota windows | see below |
| example-org (8 traps) | gpt-5.5 | `converged, 174 blocked, 2 optional advised` | 100% (32 of 32) | 14 + 23 across two recorded windows, 32k + 22k; the converging third window not recorded | 5 pass, 1 half, 2 miss (of 8) |
| example-erp | qwen3.8:27b | `converged`, generation 16, 51 goals resolved, 43 blocked, 0 session failures | 100% (19 of 19) | 13, 75k | not graded this landing |
| example-slides | qwen3.8:27b | `converged`, generation 8, 17 goals resolved, 16 blocked, no diagnostics | 100% (9 of 9) | 7, 28k | none planted |
| example-sort | qwen3.8:27b | incomplete, generation 5, 1 parked, 13 blocked; died on `acp host is gone` | 87% (7 of 8) | not recorded, 18k | not graded (2 planted perturbations) |
| example-novel | qwen3.8:27b | incomplete, 3 parked, 38 warnings | 5% (2 of 37) | 3, 51k | not graded |
| example-org | qwen3.8:27b | refused (a dead orphan held the lease); superseded by the gpt run | n/a | n/a | n/a |
| dogfood (`docs/`) | gpt-5.5 | no result: the first live window lost 262 sessions to the host death (fixed since, not rerun) | not recorded | not recorded | n/a |

The f2 traps: 1 cross-document identity (`ent:order`), 2 buyer lookalike, 3 the 21 versus
30 day contradiction, 4 a rule hidden in an Examples section, 5 junk bait in `admin.md`,
6 genuinely non-normative pages (roadmap, glossary). The org traps: 1 cross-document
Employee/Manager, 2 the lookalike Finance department, 3 the 500 versus 250 threshold
contradiction, 4 a rule hidden in Background, 5 junk bait, 6 the glossary, 7 a dead-end
hired stage, 8 a quality statement without a measure.

## f1: incrementality and the store version

- Fresh compile: 5 `reconcile-section` goals, then `rejudge-pair`, `review-entity`,
  `declare-edges`, and a `curate-view` burst; 4 diagrams (class/public,
  component/catalog, use case plus sequence for customer-system). The failure-mode
  branch landed after the right step in the flow view.
- Immediate rebuild: 0 goals, 0 LLM calls, `converged`.
- A version-0 out directory archived to `jazyk-out.bak` and reconciled from empty.
- Found during the run: the internal build never recorded the compile release, so
  default manual mode blocked everything (`6fc98ed`).

## f2 on the local model: the baseline

- Converged across 4 effective builds. One build was interrupted by a killed shell; the
  orphaned build kept its lease honestly and the resumed build continued from committed
  state, parked goals first. 15 entities, 18 diagrams.
- Trap 1 pass: one `ent:order` across six documents, one `sm:order`, one state view.
  Trap 2 pass: no buyer entity, folded into Customer. Trap 3 pass: exactly one
  `contradiction` error on the pair, surfacing as the one mandatory `answer` goal.
  Trap 4 pass: "The Stock count shall never go below zero" extracted from the Examples
  section. Trap 5 pass: no flag, path, or command entities; Admin CLI carries the
  identifiers verbatim in statements.
- Trap 6 half: roadmap non-normative, glossary marked `covered` with definitional
  entries extracted (SKU, Picking, Reorder point). Cycle 1 graded this a prompt gap,
  not a wrong expectation.
- The 59 blocked: 1 `answer` (the trap contradiction, the human seam) and 58 bind,
  generate, and verify goals awaiting `jazyk release generate`.
- Judgment gap: 2 transition facets in the whole corpus (`sm:order` carried
  placed→cancelled only; "When a Payment is confirmed, the system marks the Order as
  paid" carried none). The derived machines, their checks, and the renderer were
  faithful to the store.
- GC read well: `declare-edges` declined behaviors with per-sentence reasoning and did
  not duplicate existing edges; `curate-view` placed 8 unplaced behaviors into the right
  flows.

## f2 on gpt-5.5: the full pipeline

Compile: generation 29, 21 entities, no Buyer, no junk-bait entities, an `SKU-1042`
instance with `conform-instance` diagnostics fired. Trap 3 was the showcase: one
`contradiction` error naming both deadline quotes, a two-option prompt carrying exact
doc edits for either resolution, freeform allowed. Trap 6 half again (roadmap
non-normative, glossary covered). Trap 4 half: the rule extracted, but an instance was
minted from the illustration. The run tripped two harness bug families live, both fixed
before generation (see [harness defects](#harness-defects-exposed-and-fixed)).

Generation and verification on the converged graph, across three quota windows:

- Window 1: all 53 bind goals resolved with 0 failures; 6 of 19 entities generated (17
  files) before the window died; the verify sweep ran 36 goals, 21 verified, 15 failing
  (the ungenerated half).
- Window 2: generation completed (14 resolved, 6 unchanged); the verify sweep went
  green, 29 of 29 requirements verified by real `cargo test` runs. One standing failure
  was a gate catch, not a loss: `ent:sku-1042`'s session recorded files that did not
  exist under the deliverable and the harness refused the claim.
- Window 3: `ent:sku-1042` landed on retry with real files. 20 of 20 entities, 0
  failures, every requirement verified by `cargo test`. Docs, graph, code, and tests are
  green end to end.
- The grading found the ledger's `medium` reading "Python command-line application"
  while every generated file is Rust and every test row is `cargo test`. Root cause:
  f2's docs never state the medium, `decide_medium` saw statements only, and the
  workers wrote Rust steered by the existing crate. Fixed in cycle 2 (`decide_medium`
  now sees the deliverable tree). A divergence check between the ledger medium and the
  produced file types is still open.
- Other generation findings from the same grading: 164 doubled-prefix markers
  (`req:req:`) silently unparsed; grep-the-source tests accepted as programmatic
  verification; three parallel Order implementations; generation ran green over an
  open contradiction error. The first three are fixed in cycle 2; the last is deferred.
- Standing reminder: `bootstrap/example/f2/product/` is tracked fixture code and
  generation overwrites parts of it. Revert `product/` before any commit; never commit
  generated fixture output.

## example-org on gpt-5.5: the trap misses

Converged at 100% coverage, `converged, 174 blocked, 2 optional advised`. The 59% share
of `rejudge-pair` sessions in the recorded window is the multi-entity pair rule at work.
Graded about 5.5 of 8.

- Trap 1 pass: one Employee entity, mentions across 3+ documents. Trap 2 pass: a single
  `ent:finance`, stereotype department, parented under Ridgeline. Trap 4 pass: the
  salary-band rule extracted verbatim from Background. Trap 6 pass: one non-normative
  section in `policies.md`. Trap 7 pass: `sm:application` derived, `dead-end-state`
  fired on it (plus a legitimate second on `sm:expense-claim`).
- Trap 3 miss, a judgment miss on the strong model. Both statements were extracted
  (`req:expenses-13`, `req:policies-2`), the exact pair was derived and judged
  (`g:rejudge-pair:req:expenses-13~req:policies-2` is in the journal), and no
  contradiction was filed. The two harmonizing justifications, verbatim:
  - `expenses-13~policies-2` judged consistent: "expenses-13 covers manager-then-Finance
    above 500 while policies-2 imposes Finance in addition for the broader above-250
    range".
  - `expenses-12~policies-2` judged consistent: "policies-2 adds Finance approval above
    250 rather than removing manager approval".
  - Both read the `expenses.md` approver table (500 or less: the manager) as additive
    when it is exhaustive. Cycle 2 added the band rule to the judgment skill and the
    `rejudge-pair` contract: compare band by band across the union of thresholds; a
    table assigning an outcome per band is exhaustive, never a floor.
- Trap 5 half (regraded in cycle 2). The form-number, path, policy-number, and
  cost-center half held; `ent:hiring-tracker` and `ent:expense-tool` exist against
  EXPECTED's must-not list, and `pearl-street-store` and `ledger` were minted from
  single mentions in history.
- Trap 8 miss, instructive. The model recorded the quality facet and laundered the
  vague word into the field: `measure: "promptly"`. The `quality-unmeasured` check is
  correct and was defeated semantically. Cycle 2: a measure is a number, duration,
  count, or rate; a bare adverb is the unmeasured case, leave `measure` absent.
- Machine overshoot: `sm:expense-claim` carried 10 states including a compound
  "approved or returned", an invented `created`, duplicate arrows from restating
  sentences, and an example-sourced approved→paid. Cycle 2 tightened transitions
  (trigger versus guard, either-A-or-B is two transitions, restated pairs record once).
- Harness smell: 45 sticky `incomplete-build` warnings accumulated from the starved
  builds and 32 stale `uncovered-section` warnings at 100% coverage. The sweep should
  resolve or dedupe both when their condition clears (deferred).

## The local chain: erp and slides converge, novel collapses

- example-erp is the first full local convergence on a root example corpus: 51 goals
  resolved with `rejudge-pair` verdicts landing, 4 infos and 2 warnings, 0 session
  failures.
- example-slides, the non-software corpus, reconciles cleanly with no diagnostics.
- example-sort's near-code prose extracted well; the run died on `acp host is gone`
  against direct Ollama, so that death was the host seam under load, not rate limiting.
- example-novel: the local model loops on narrative prose. It re-reads sections, plans
  in text instead of calling tools, trips the repeated-call guard, and idles out. The
  harness held: gates bounced everything malformed, the board and verdict stayed
  honest. Model weakness, not a prompt gap.

## The cycle-1 A/B on qwen

Cycle 1 turned 43 graded findings from the twin f2 runs into payload edits
(`4d12b3a`). A fresh f2 compile on the same local model with the tuned payloads, against
the baseline:

- Glossary trap: pass (was half). Glossary and roadmap both non-normative, zero glossary
  entities or requirements minted. The counterweight worked on the weak model.
- Transitions: 4 facets (was 2). `sm:order` carries three arrows from `placed`
  (cancelled on the 21-day trigger, paid on Payment confirmed, on hold after three
  failures). The Payment-confirmed facet was the baseline's named miss.
- Parents: 2 set (was 0); `view:component/orderly` derives (was absent).
- Pair coverage: 17+ `rejudge-pair` goals fired under the multi-entity rule, all judged
  consistent with specific justifications. Cycle 2 later measured this as pair flooding
  on qwen (49 goals, one real) and deferred a scoring discount.
- Queries: the one stored query is a real `scope: public` filter; no hollow queries.
- `report_feedback`: 2 entries filed, the first ever (see the next section).
- Unchanged: `sm:product`, the invented Product visibility machine, appears in both
  runs (see [known weaknesses](#known-weaknesses)).
- New on qwen in the cycle-2 grading: a whole obligation vanished behind a false
  justification (the stock-decrease sentence), the recurrence cycle 1 had held a
  reserve edit for; aggregation direction inverted (`product o-- catalog`). Both went
  into cycle 2's extraction edits.

## The feedback channel's first closed loop

In cycle 1's grading, `report_feedback` was never called in either f2 run, even where a
session correctly diagnosed a real tool flaw in its private reasoning. Cycle 1 rewrote
`feedback-note.md` (a refusal that seems wrong is exactly the `report_feedback` case, in
the moment) and ended the context-full and repeated-call refusals with a nudge.

The qwen A/B run then filed two entries, both real harness gaps:

- `report_diagnostic` returned no id, and staged diagnostics were invisible to reads
  within the session.
- The `done` justification-length rejection did not name the offending goal.

Both landed as fixes in `10551ca`: `report_diagnostic` answers with the finding's id
resolved at stage time by the commit fold's natural key, staged findings are visible to
`diagnostics`, `update_diagnostic`, and `resolve_diagnostic` under read-your-writes, and
the rejection names its goal. In cycle 2's three graded runs, `report_feedback` fired in
the moment on all three, every entry a real harness defect.

## Harness defects exposed and fixed

Each found by a run, documented first, then fixed. The commit gists:

- Empty means absent (`75439f9`, `dea6e23`, `660b21d`): a model that fills every schema
  field with empty strings, lists, and objects makes the same call as one that omits
  them, so hollow provenances stop counting as a second provenance, hollow transition
  objects stop bouncing every requirement, and an adversarial sweep's 33 confirmed
  empty-versus-absent traps collapsed into 12 fixes (empty members or edges no longer
  wipe judged work, a match-everything view query no longer floods flow views,
  `run_tests` with a blank target runs everything, manifests drop blank rows).
- The host death (`8c0a091`): every `acp host dropped the prompt` failure since the
  first starved window traced to one seam, the embedded agent answering a failed turn
  with a JSON-RPC error that the client library treats as fatal to the whole
  connection, so one rate-limited call silently killed the host driver, dropped every
  pending reply, and closed the child's stdin; reproduced on f1 against the closed rate
  limit in two minutes, and the agent now answers a refusal stop with the error as a
  message chunk while the host driver says its death out loud.
- The context-budget ceiling (`fe9b43c`): the initially loaded set treated the
  high-water mark as a suggestion (measured at 1.8x budget on org, the top round-waster
  on every run), and now treats it as a ceiling, batch sizing counts the skills' payload
  bytes, and the `rejudge-pair` estimate follows the statements it will load.
- The multi-call codec (`6c65e98`): a text-codec reply packing several action objects
  executed the first and silently dropped the rest, leaving a qwen session believing
  26 goals were marked while one landed; every object now executes in order with one
  result each.
- The dead-endpoint breaker (`ede2221`): five consecutive failed sessions that spent no
  tokens park what remains with the last error in the reason, because window four
  showed 252 futile refusals against a rate limit that had already answered.
- The pair rule (`55198c7`): a restatement built from the same entities can share every
  noun and no other token, so two shared entities now qualify a neighbor on their own
  (the missed f2 glossary pair is the regression test); the same commit evicts a dead
  ACP host so it stops poisoning later batches and makes the rounds budget say its real
  bound (48 model round-trips, `AGENT_MAX_ROUNDS`).
- The compile release (`6fc98ed`): a typed command is its own approval, so a fresh
  manual-mode project compiles instead of reporting converged with every goal blocked.
- Cycle 1's harness half (`4d12b3a`): the context-full refusal prints real numbers and
  leads with the unload imperative, `unload` clears the repeat key, a hollow view query
  parses as no query and flow views refuse entity queries, token spend folds as a delta
  instead of clobbering the meter, `unhandled-event` stays silent under two
  transitions, condensed requirement lines carry their statements.
- Cycle 2's harness half (`4894407`): the sentence counter stops counting the dot in
  `customer.md` (a gate that had trained models to strip evidence), scoped servings
  answer `session-complete`, info-severity observations stop deriving mandatory human
  answers, `unhandled-event` stops restating dead ends, doubled `req:` prefixes fold and
  marker warnings surface, `decide_medium` sees the existing deliverable tree.

## Known weaknesses

- Local models on narrative prose. example-novel collapsed at 5% coverage on
  `qwen3.8:27b`; software-shaped and slide-shaped prose converged on the same model.
  The novel's traps (the nondeterministic arc transition, instance conformance) are
  ungraded until a strong-model run.
- The subscription's quota cadence. A gpt-5.5 window (roughly 3.5 hours apart) fits
  about one f2-sized workload. Outside a window sessions fail fast and park (the
  breaker); a window-chaining loop (up to 8 windows) resumes incremental work. The
  dogfood has not yet completed a live window.
- The `sm:product` question. Two qwen runs and the written subject rule failed to stop
  a Product visibility machine (in stock → hidden) grounded in `catalog.md`'s explicit
  shown/hidden pair. Cycle 2 tolerates it as optional in f2's EXPECTED (the original
  rationale was written for Shipment/Return, which stay wrong). Whether to strengthen
  the subject rule further is open.
- Strong-model over-minting: account and warehouse on f2, history and channel entities
  on org (the cycle-2 counterweights are in; the regrade is pending).
- Per-session token usage is not plumbed into the journal for ACP worker sessions
  (`tokens: 0` per entry); `status` totals are correct.
- Two open design questions for the owner: whether a component view should include
  outside entities with any edge to a child, and whether a non-actor flow cluster needs
  an actor before deriving a use-case view.
- The deferred harness list (cycle 2, still open after the ceiling, multi-call, and
  breaker fixes): a build-lease heartbeat and takeover (a wedged holder survived 3.5
  hours); a terminal trace line per session attempt; GC-sweep normalization of
  committed hollow queries and flooded flow members; pair-candidate boosts for
  same-subject same-transition restatements and a discount for highest-degree-only
  overlap; merging derived transitions with identical from/to into one arrow;
  sequence `message_of` preferring the edge touching the cluster's actor and lifted
  arrows dropping generalization type; auto-resolving `incomplete-build` and
  `uncovered-section` diagnostics when their condition clears; a contradicted flag on
  ledger rows so generation cannot run green over an open error; the ledger medium
  versus produced file types divergence check; three staging gates considered and not
  taken (coverage-note id cross-check, exclude-note view-id verification, a
  composition-owner guard).

## Levels

The `levels` branch (from `uml`) adds the top half of the containment tree: a
`children-per-entity` limit (soft 9, hard 15) driving the fan-out variant of
`abstract-entity` with deterministic coupling hints, `group_entities` and
`dissolve_entity` behind the documented gates, one structural level view per node with
lifted flow views and drill-down links, the level-shape check and the shape line, and
the frontends nesting, tree, and breadcrumb. Landed on the branch: stage 1 (docs), 2
(harness), 3 (tools, level views, lifted flows, drill-down links), 4 (docsgen, GUI,
viewer), 5 (the abstraction skill and the fan-out contract), and 6 (`example-saas`, a
fixture written to the owner's picture with ten traps). One deterministic scenario pins
the whole loop with no model in it (`8babb01`).

Stage 7, validation, is running: `example-saas` on the local model, then `f2` and
`example-org` regressions on the levels binary, then the dogfood, with grading cycles
to follow. No level result is measured yet. The next update fills in, per corpus: the
verdict and shape line, the groupings minted and their names against the fixture's
headings, the level views derived and the drill-down chain, and the `example-saas` trap
tally (of 10).
