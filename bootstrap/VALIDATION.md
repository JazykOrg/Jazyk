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
| f2 on the levels branch, snippets | qwen3.8:27b | not converged by design: 3 advance sessions (0 failures) then 7 goal snippets | 7 of 25 sections in the advance | 3 + 7, tokens not recorded | 6 pass (of 6) |
| example-org on the levels branch, snippets | qwen3.8:27b | not converged by design: 3 advance sessions (0 failures) then 9 goal snippets, the root fan-out among them | 11 of 32 sections in the advance | 3 + 9, tokens not recorded | 8 pass (of 8; trap 5 on five of its eight sites) |

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
- The completion cap trade-off (levels chain). On `qwen3.8:27b` the default 4096-token
  cap cuts a fan-out's deliberation (the pre-namesake root fan-out aborted on it and
  resolved at 16384), while 16384 lets an extraction over-think past the 600 s call
  timeout (f2's root batch failed twice that way). The `length` nudge closes the gap
  for extraction (five of five stalls acted on at once in the regressions); a fan-out
  that needs a long deliberation still wants the larger cap, so the cap is a
  per-run choice, not a setting the harness picks.
- Invented stereotypes. Extraction labels what the documents only name: f2's root
  ingest set «component» on Order, Payment, Shipment, Returns, Catalog, Inventory, and
  the Admin CLI and «entity» on Product from one sentence; org set «component» on
  Department and Team; the mini chain's checkout ingest set «entity» on seven leaves
  and «process» on Checkout. A structural label on one child flips its level's
  diagram to a component view, so the labels shape the pictures. The extraction skill
  now refuses defaults; whether a gate should demand a supporting quote is open (org's
  EXPECTED says stereotypes are the model's judgment).
- The frame as a sequence participant. A level's lifted sequence draws the level's
  own parent as a participant when a flow names that node itself: the Funds level's
  sequence in the mini chain draws Funds, Checkout, and Order because "A Gift Card is
  a prepaid balance a customer spends at Checkout" names Checkout, the node whose level
  Funds sits in. The derivation is faithful to the requirement and the picture reads as
  if the frame were a peer of its own children. The level view keeps the frame out
  (`e7f781d`); the sequence does not, and whether it should is a design question for
  the owner (`derive.rs`).
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

Stage 7, validation, runs by the owner's short-loop method: a project is snapshotted
at one moment, one goal's session runs from the copy (`jazyk benchmark --project
<dir> --goal <id> --force`), the transcript is read, the docs and the harness are
fixed, and the same snippet reruns. Model: qwen3.8:27b-mlx on Ollama, one session at a
time. Every number below is one snippet on one snapshot, never a converged corpus
graded hours later.

### The mini chain (three documents plus a checkout glossary, twelve root entities)

- The root fan-out resolves to the groupings the documents name (Orders, Shipping,
  Platform) and rejects the coupling candidate that mixed two areas; a namesake
  collision (a front door naming Checkout beside a stated Checkout process) resolves
  by moving the document's entities under the stated node, stated once; the node-form
  fan-out under Checkout mints Pricing, Funds (Payment was taken), and Selection. End
  state four levels deep, shape 5 / 17 / 9 / 1, every fan-out under nine, the top
  sequence drawing Checkout talking to Orders. Five derivation defects surfaced from
  re-rendered snapshots along the way (an ancestor admitted as a level's peer, a level
  owning every flow that touched an interactor, sequence endpoints never lifted,
  internal flows drawn as self-messages, the first edge collapsing where another
  reached), all fixed on the branch.
- The forced root fan-out on the pre-namesake snapshot aborted once on the default
  4096 completion cap (seven empty turns, each 17k characters of reasoning cut
  mid-deliberation, the right conclusion every time) and resolved in 9 rounds with
  `JAZYK_MAX_COMPLETION_TOKENS=16384`. A reasoning model that overruns the cap looked
  like a silent stop to the agent loop, which nudged it seven times at three minutes
  each; the loop now reads the finish reason and a `length` stall gets its own nudge
  (the cap is named, the model is asked for the call alone), measured in
  [the regressions](#f2-and-example-org-regressions) below.

### The owner's side of a grouping (accept and retract)

Measured on the mini chain's end state (three pending proposals, `81 blocked`):

- No terminal path existed. The answer engine served the LSP code action, the GUI
  questions panel, and chat's `answer_diagnostic` (the `chat` MCP serving only; `jazyk
  mcp graph --write` does not expose it), and `jazyk explain` and `preview` rendered
  the proposal without naming a command. `jazyk answer <diag|goal> [--option N |
  --text "..."]` now lists a prompt's options and applies one.
- Accept (`--option 0` on `g:ratify:ent:funds`): the sentence lands as the last
  paragraph of the target section, the entity's provenance flips to a mention quoting
  it, the journal carries one `ratify` entry (`edit_doc_prose`, `ratify_provenance`,
  `resolve_diagnostic`, `answer_diagnostic`), the change record clears, the goal is
  gone, blocked drops by one, and one `review-entity` opens on the entity. A session
  advanced afterwards derives no `reconcile-section` on the edited document: the
  hashes were absorbed at accept.
- Retract (`--option 1` on `g:ratify:ent:selection`) was unreachable: the store
  refused any entity still a parent, which every grouping is, and the refusal landed
  as a partial commit (the `answer_diagnostic` op applied while the retract was
  skipped), leaving the proposal answered forever with its goal open. Fixed: a
  retracted grouping dissolves as `dissolve_entity` would (children back under its
  parent, a tombstone redirect, the `dissolve_entity` mutation journaled in the
  `ratify` entry so the reparent flip replays), quoted requirements re-point to the
  parent, and the retract changeset is all or nothing (`Commit.all_or_nothing`). After
  the fix: the grouping is gone, both children sit under Checkout with their reviews
  open, blocked drops by one.

### A level's picture yields to its fan-out

A `split-view` goal on a sequence view of eleven honest participants failed honestly
(every split invents structure) while the level's fan-out was parked beside it. The
goal now blocks on `fan-out first: g:abstract-entity:<level>` while the fan-out is
open or parked (`jazyk benchmark` refuses it without `--force`), and the forced fan-out
lifts the same flow to two participants. The participant count behind the threshold
record was taken over the members' raw entities, never lifted to the level's members,
so the goal outlived the picture it was filed for; it now counts what the level draws
after lifting (`25f6a00`).

### example-saas, the API Server and its data model

Snapshot at generation 13 (44% coverage, nineteen of the twenty-one API Server
children stated, the mandatory fan-out open behind three hundred pair judgments),
forced:

- Loop 1: resolved in 9 rounds but wrong. The model staged Collaboration and Rendering,
  then "Billing" over the coupling candidate that mixed the Billing Handler with the
  billing classes; the near-duplicate gate refused the name against the handler with
  the advice to reparent the members under it, and the model obeyed: three classes
  under a handler, no Data model, no Request handling, the `## Data model` section
  never read. Trap 4: fail.
- Fixed docs first: headings first (the node's own document's headings are the
  partition, one grouping per heading), a peer that carries the area's word is not the
  area, the member hints carry the listing section of the node's document beside the
  dominant document, and the gate's advice follows where the lookalike sits (a sibling
  with children is reused, a childless sibling is a peer, another level's entity
  qualifies the name).
- Loop 2: resolved in 6 rounds, 22 mutations: Data model (12 classes), Request
  handling (4 handlers), Rendering (3 renderers), each derived from exactly its
  members with the heading in its reasoning; shape 8 / 5 / 3 / 19;
  `component/api-server` drills into `class/data-model`, `component/request-handling`,
  `component/rendering`; six ratification proposals. Traps 4, 7, 8: pass.
- The data model fan-out on that state: resolved in 8 rounds: Identity (3),
  Collaboration (6), and Billing classes (3; "Billing" was refused against the Billing
  Handler, now at another level, and the model qualified the name as the doctrine
  says). Shape 8 / 5 / 3 / 10 / 12, every fan-out 2 to 9, `class/data-model` drilling
  into the three. Trap 5: pass (the good run).
- Not measured here: traps 1, 2, 3, 6, 9, 10 depend on the ingest and the judgment
  goals that the snapshot's remaining sessions have not run.

### example-saas, the caps split on the API Server

The caps variant of `abstract-entity` had never run under any model. Snapshot at
generation 29 (98% coverage, 57 entities, 418 requirements; `ent:api-server` referenced
by 96 requirements, over hard 80, with seven stated children: four handlers and three
renderers), the goal mandatory and blocked behind ~1400 pair and bind goals in its cone,
forced as a snippet each time on a fresh copy. Model `qwen3.8:27b-mlx` via Ollama, about
three minutes per round; the initial loaded set was 37k characters against the 24k
budget in every run, the 96-requirement target alone.

- Attempt 1 (binary as-is): fail, nothing committed. The model minted the four
  api-server.md heading parts and five cross-document areas, two of them twins of the
  stated Deployment and Monitoring under another parent (the near-duplicate gate keys on
  the parent), re-pointed 55 statements with `entities: [<sub-entity>]` alone, dropping
  Session, Database, and Account from each (accepted), never re-parented the seven
  children, exhausted the 64-mutation budget with three sub-entities staged empty,
  could not withdraw them (`dissolve_entity` refused `staged-entity`), and failed the
  goal honestly. The prompt's `## Loaded` had listed eleven items the snippet session
  did not hold, so every `unload` bounced `bad-target`; the `group` hints read
  `persist`, `clas`, `every`, `from`; the defining section was the README's list item;
  and the gate the page states first (count within soft) had no code.
- Fixed docs first, then code: the session's mutation budget grows by the count a caps
  goal splits; `dissolve_entity` on an entity staged this session withdraws it; a
  re-point is a swap refused as `entities-dropped` with the list to send;
  `mark_goal_done` faces `count-over-limit` against soft; the hints show written words
  with function words dropped; the defining section is the document headed with the
  entity's name; the skill and contract say an actor's statement about the whole stays,
  existing children re-parent under their part, and a sub-entity and its moves go in
  one reply.
- Attempt 2: partial, committed (generation 30). Exactly the four heading parts, no
  twins; 15 `entities-dropped` refusals and every retry right; 38 statements moved; the
  handlers and renderers re-parented under theirs with one whole-part sentence each;
  then `count-over-limit` at 58 over soft 50, and the model deliberated past its
  completion cap into silence, so the implicit `done` committed a de-escalating split
  and the goal re-derived optional. The honest close was `mark_goal_failed`
  recommending a bump to 58; the model never said it.
- Fixed: the snippet session now holds the loaded set its prompt lists (the benchmark
  path built an empty one); the count refusal names both honest exits and says the
  staged split commits either way.
- Attempt 3: pass, resolved (generation 30). 20 rounds, 60 mutations: Data Model (28),
  Request Handling (11 plus the four handlers), Rendering (3 plus the three renderers),
  Errors (5), one whole-part statement per re-parented part with composition edges, 20
  `entities-dropped` refusals all repaired, 49 left on the API Server, the record
  cleared, level views `class/request-handling` and `class/rendering` derived, six
  ratification proposals blocked on a human, fan-out 2 to 9 everywhere. One judgment
  concession: Data Model absorbed seven collaboration behavior statements (the API
  Server creating a Notification when a Task is assigned) to reach the count.
- Left standing: a 96-requirement target overflows the 24k context budget by itself, so
  `load` and `expand` are refused for the whole session unless stubs are unloaded first;
  the model still sends the truncated `entities` list on its first batch every time
  (the gate carries the repair); a snippet journals `tokens: 0`.

### f2 and example-org regressions

Both from generation-0 copies (`f2-levels`, `org-levels`), advanced with `compile
--sessions 3` on the default completion cap and then snippeted goal by goal; a
judgment goal that needs two documents was chained (a snippet's sandbox copied as the
next snapshot, so the pair the second ingest opens is snippeted on the third). Every
transcript is in the session scratchpad (`snippet-f2-*.json`, `snippet-org-*.json`).

- The cap nudge, measured. Five ingest sessions across the two corpora (f2's root,
  admin, and catalog batches; org's departments and operations batches) each hit
  exactly one reasoning-only turn cut at the 4096-token cap after their searches, and
  every one answered the `length` nudge on the next turn with "I need to act."
  followed by the batched upserts: five of five acted on the first nudge, zero
  repeats. The previous binary's two attempts at f2's root died on the idle watchdog
  and the 600 s call timeout, and org's table-of-contents section repeated the same
  cut reasoning four times. The trade-off stands: `JAZYK_MAX_COMPLETION_TOKENS=16384`
  let an extraction over-think past the call timeout; 4096 with the nudge finished
  every session (19 to 37 rounds).
- f2, 6 of 6 traps pass. Trap 1: one `ent:order`, reused by every ingest (system,
  admin, orders, shipping). Trap 2: "to its buyer" recorded on `ent:customer`, no buyer
  entity, 7 rounds. Trap 3, chained: orders lifecycle (9 rounds; placed→cancelled with
  trigger `not paid` and guard `21 days after placement`, EXPECTED to the letter), then
  payment rules on that sandbox (7 rounds; placed→paid on `Payment confirmed`,
  placed→on hold on `Payment fails` [three times]; `sm:order` renders EXPECTED's
  machine exactly, and the commit opened the deadline pair), then the `rejudge-pair`
  on the third copy: 4 rounds, one `contradiction` error quoting both sentences with a
  two-option prompt carrying the exact edit for either document. Trap 4: the Examples
  rule extracted verbatim as a constraint in 9 rounds after reading the sibling
  section to check it is stated nowhere else, the 40 units named illustration, no SKU
  entity. Trap 5 inside the advance: 11 entities, none for a flag, path, or command,
  both commands and both flags verbatim in statements on the Admin CLI. Trap 6:
  glossary and roadmap non-normative in 5 and 7 rounds, nothing minted, no suspicious
  finding.
- example-org, 8 of 8 traps pass on the sites measured. Trap 1: Employee and Manager
  reused across README, departments, expenses, and policies. Trap 2: `update_entity
  ent:finance add_aliases ["Finance department"]`, no second node, 8 rounds. Trap 3,
  chained: spending (the 250 rule), then expenses steps on that sandbox (the approver
  table as one statement; the commit opened the exact pair), then the `rejudge-pair`:
  4 rounds, one `contradiction` error whose reasoning names the 250 to 500 band, the
  gpt-5.5 miss now a pass under the cycle-2 band rule. Trap 4: the salary-band rule
  verbatim from Background, the history named as history, 6 rounds. Trap 5: no entity
  for EX-12, the `S:\Finance\Claims` path, `CC-4410`, the expense tool, the ledger, the
  tracker, the spreadsheet, or Pearl Street (five of the eight junk sites snippeted;
  HR-04, HR-21, and the careers URL were not). Trap 6: the glossary non-normative in 4
  rounds. Trap 7: `sm:application` is EXPECTED's machine exactly (six arrows, one
  guard, no hired state, no exit from offered), and `compile --sessions 0` on the
  sandbox files `dead-end-state` naming offered. Trap 8: the quality facet with no
  `measure` and the reasoning "'promptly' is a bare adverb with no stated bound";
  `quality-unmeasured` filed on it.
- Levels on the regressions. f2 derives `component/public` (Customer, Operators,
  Orderly, Product) and `component/orderly` (seven children plus the lifted
  outsiders), every element carrying its card link, flow views per level beside the
  root ones. org crossed the root's soft limit at ten parentless entities after the
  hiring ingest, and the forced root fan-out resolved in 8 rounds to Hiring pipeline
  ("Hiring" bounced against Hiring Manager and was qualified from the document's own
  word) and Departments (the Department type, Employee, Manager, Team), root 10 → 4,
  the coupling candidate that mixed the company into the hiring cluster declined as an
  artifact. The second name is the finding: it sits beside the company whose level
  holds the real departments, and the near-duplicate gate folds no plurals, so
  "Departments" passed against `Department`. The abstraction skill now forbids a
  grouping named as the plural of a member or with a word another level uses for
  stated children (`e96199d`); the gate's plural fold is open (`near_name`). Loop 2
  on the rebuilt binary, same snapshot: 10 rounds, Hiring pipeline again, and the
  four moved under Ridgeline Outfitters with the Employee Handbook in one bundled
  whole-part statement, the justification citing the skill's warning by name; root
  10 → 2.
- Two more findings from the same runs. Extraction invented stereotypes: f2's root
  ingest labeled seven domain concepts «component» and Product «entity» from one
  sentence that names them; the extraction skill now says a concept the section only
  names carries none and `component`, `entity`, `class` are never defaults
  (`c1b490d`). The near-duplicate gate's token-containment rule cost five rounds on org (Employee vs
  Employee Handbook, Store Manager vs Manager, Account Manager vs Manager, Sales
  Associate vs Sales, People Partner vs People), every one repaired with a note; three
  of the five calls already carried the lookalike's department as `parent`, so a call
  whose `parent` is the lookalike could pass without the note (open).
- Not measured: a converged verdict on either corpus (the snippets grade goals, not
  builds), the remaining trap 5 sites, and the stereotype rule on a rerun (the one
  ingest snippeted after it landed, f2's stock rules, minted no entity, so the rule
  never came into play). The plural rule was measured (loop 2 above).

Scorecard for the levels snippets: 6 of 6 fan-outs graded resolve to the documents'
own names after the fixes (root, namesake, node form, API Server, data model, the org
root); the accept and retract paths pass; the split-view yield passes; f2 6 of 6 and
org 8 of 8 traps pass on the levels binary; 11 defects found and fixed on the branch
(the retract of a grouping, the partial commit on refusal, the missing terminal path,
the split-view yield, the member hints without sections, the gate's advice on a peer
and on another level, the earlier toolset, auto-load, and parked-list fixes, the
default stereotype, the plural grouping name); 2 findings open in the gate (the plural
fold, the parent shortcut).

### Claude Code as the session model

Every example compiled with Claude Code driving the sessions: a loop script previews
the next batch's prompt (`jazyk preview`, the exact prompt a session receives) and
hands it to `claude -p --strict-mcp-config` with `jazyk mcp compile` as the only MCP
server, one batch per call, up to a cap per project (2026-09-04 and 05; the ACP path,
`jazyk compile --agent claude`, works too since MCP over HTTP landed, and takes more
rounds per batch). Transcripts are the MCP traces under each project's `jazyk-out/trace/`
in the session scratchpad (`self-<example>/`).

- Converged on their own: f1 (5 batches, 2 to 4 minutes each, coverage 100 percent,
  the planted confirmation-email contradiction filed with a two-option prompt, "the
  system" read as the Shop, no invented states), sort (7 batches; one contradiction,
  and it is genuine: the documented simple-case test expects unsorted output while the
  pseudo-code sorts ascending), slides (10 batches, clean).
- At full coverage with judgment goals left at the cap: f2 (every planted trap passes
  from the graph: the twelve expected entities, one `ent:order`, no buyer, no SKU, no
  flag or command minted, glossary and roadmap minted nothing, the 21 versus 30 day
  deadline filed as a contradiction plus a second real one in the catalog page),
  ledger (no derived grouping needed: the front door's listing sentence read as
  containment, Orders, Checkout, Shipping, and Platform under Ledger), erp.
- At the cap short of full coverage: org (90 percent, 36 entities, one Finance, one
  Employee, no junk, Employee Handbook beside Employee), novel (81 percent: Ines Marlow
  with alias Nessa across five documents and no Nessa entity, the chapter files never
  entities, the two ledgers and the Marlow pair apart; three Lantern Room entities,
  the titled-subject mistake the contract now forbids), saas (44 percent at batch 8:
  one Backend holding API Server, Cache, Database, Queue; Frontend and Backend under
  Trellis, the nested reading of the front door the fixture calls a failure and the
  owner's open question).
- Tool errors across the first 370 calls: one (Employee Handbook bounced as a variant
  of Employee, the gate since refined), plus the previewed batch id every session's
  first call tripped on (`b<n>-0` for the board's `b<n>-1`, since fixed). No nudges,
  no stalls.
- The cost that dominates a corpus under a strong model is the pairwise judgment:
  saas held 479 goals at 30 percent coverage (358 pairs) and over 1100 at batch 19;
  a pair session settled four pairs (org) to eight (f2) in six to twenty calls with
  nothing loaded. Batches now pack a locality to the budget (four to eight pairs on
  the org snapshot); the `pairs-per-entity` lever waits on a rerun. The batch claim
  `settle_pairs` (2026-09-08) collapses the per-pair calls: one pair batch of the
  saas snapshot (six pairs on `req:api-server-10`, generation 29) run under Claude
  Code over `jazyk mcp compile` spent three calls in all, `begin_goals`, one
  `settle_pairs` carrying the six verdicts, `done`, in 36 seconds; every entry was
  accepted and the journal records six resolutions with their own justifications,
  where the per-pair pattern spent one `mark_goal_done` per pair plus a
  `report_diagnostic` per finding.
- Pair volume under the degree cap (2026-09-08, `pairs-per-requirement` at 8,
  docs/compiler/reconciler.md#pairs): on the saas snapshot at 98 percent coverage
  (418 requirements, generation 29) the board derived with no LLM call holds 880
  open pairs where it held 1336, with 456 waiting under the cap and none dropped
  (the before and after id sets differ by exactly the waiting set). The top
  requirement's open degree fell from 40 to 8 (`req:collaboration-2`: 8 open, 32
  waiting, its best-scoring partners first); 74 requirements sit at the cap and none
  above it; the board line went from 1398 open goals to 942. `jazyk explain` on a
  waiting pair names the full side and its records, `preview` says no session runs
  for it yet. Derivation cost: `jazyk status` on the snapshot takes 0.68 seconds
  where it took 0.59, the plan scoring every candidate once per derivation.
- Judgment calls the runs settled or exposed: Order folded into the Orders area as an
  alias (the extraction skill now keeps the concept under the area; the gate folds
  plurals for groupings only); the novel minted as an entity beside the bookshop named
  after it (the contract's no-System rule now covers a titled subject).

### The walk through the GUI (2026-09-08)

The walk exercised by hand in the browser on `example-ledger` (`jazyk gui`, driven
through Chrome): the explorer's tree opens the Checkout card with its breadcrumb
(Public, Checkout), the flows it takes part in, its inside structure, lifted
relationships, siblings, children, and the requirements beneath; the editor underlines
every entity mention and its hover shows the card in short (stereotype, sentence,
breadcrumb, the level view, the card and level links, the diagnostics on it); the
preview renders the document with headings; the generated card page for Checkout
embeds the in-context diagram (`component/public`) and the inside diagram
(`class/checkout`) inline with every box a live link, and the diagram page lists the
level, the drawn members with their links down, and the arrows around. Following the
Order box from the Checkout card lands on the Order card with its own breadcrumb
(Public, Orders, Order). One defect surfaced and fixed: the map drew the composition
between a parent and a child nested inside it as a loop around the compound box (the
Checkout box carried four); a relationship between an entity and its drawn ancestor is
now the nesting itself and draws no arrow. Left as observed: the fcose layout lets
compound boxes overlap on a graph of thirty entities (Orders over Checkout, Shipping
inside the Checkout area), a layout-parameter question, not a data one. VS Code hover
and type definition were verified over stdio only, not by hand.

### example-novel under Claude Code, the judgment traps (2026-09-08)

Resumed from 86 percent coverage with twenty batches over `jazyk mcp compile`, then
graded on a copy whose checks ran against a dead endpoint (no model call): coverage
100 percent, 350 goals open (315 pairs, 40 of them waiting under the cap). Trap 3
passes: both transitions out of `acquaintance` stand, the state machine check files
`nondeterministic-transition` on the pair and the session had already filed a
`contradiction` beside it, neither transition dropped. Trap 4 passes the second way:
the research notes' hidden rule ("three days to produce, so Theo's report cannot
exist before chapter 3") is extracted as two requirements and the section is covered.
Trap 2 failed at extraction: "Theo is 36 when he arrives and has been moving from job
to job for twelve years" yielded one requirement carrying the twelve years and no
age, so the contradiction with "Age 34" never had a pair. The extraction skill now
states that a sentence carrying two facts joined by "and" yields one requirement per
fact; a short loop on a copy (one word changed after the trap sentence to dirty the
section, then three batches: anchors, the section, the pair) extracted the age and
filed the contradiction naming both ages. Still open on the copy: the three Lantern
Room entities await their eleven `dedupe-candidates` goals.

### The dogfood under the cap (2026-09-08)

Resumed on `docs/` (98 documents, 639 sections) with Claude Code over `jazyk mcp
compile`, the generate release withheld, the pair cap and `settle_pairs` in the
binary. Twenty-one batches landed in 88 minutes (mean 4.2 minutes, longest 7.3) before
the session limit cut the run; thirty-nine further attempts failed in two seconds each.
Every landed batch was `reconcile-section` (108 goals previewed, two `place-anchors`):
coverage went from 24 to 47 sections of 639, the board from 1248 open goals to 2323
(2308 compile, 747 pairs waiting under the cap beside the open ones, 217 of the 245
parked by the earlier quota death still queued for their documents' turn). The rate is
the finding: a section batch carries two or three sections at the context budget, so
ingest alone needs some five hundred batches at this shape, and the pairs each
landed section opens outgrow the sessions that settle them. The walk holds at this
size: `jazyk docsgen` writes 114 entity cards and 205 diagram pages in seconds. Two
shape defects show on the index: the model set scopes the documents never name as
bounded contexts (`Jazyk`, `compiler`, `Benchmark` beside `benchmark`, `public`), so
the index opens with five top levels, two of them case variants of one word; and the
public root holds 25 direct children, over the hard fan-out, its regrouping blocked
behind the compile goals in its cone.

### The prompt loop under Claude Code (2026-09-12)

An improvement loop with Claude Code as the session model over `jazyk mcp compile`,
driven by one line ("compile this project using the jazyk MCP server's tools, batch
after batch, then say what confused you"), each project a fresh copy, every iteration
rebuilt on the fixes the previous one exposed. The run records (graph, journal, trace,
diagrams, docsgen, the agent's transcript) live outside the repository under
`~/dev/jazyk-runs/iter-N/<project>/`; the subagents' own `report_feedback` entries and
closing notes were the main source of defects. Codex (`codex exec --approve-for-me`)
compiled f1 to the same graph in six batches.

| project | iteration 1 | iteration 4 |
| --- | --- | --- |
| f1 | 81 turns, $3.37, 6.4 min, 9 reads | 63 turns, $2.63, 4.9 min, 0 reads |
| example-sort | 94 turns, $4.29, 7.2 min | 63 turns, $3.27, 6.1 min, one invocation |
| f2 | 393 turns, $19.84, 34.7 min, 127 pair verdicts, 34 entity reviews | 199 turns, $7.98, 14.6 min, 17 pair verdicts, 15 entity reviews |

Every run converged at 100 percent coverage; f2's planted traps held on every
iteration (one `ent:order`, `buyer` an alias of Customer, the 21 versus 30 day
contradiction filed as an error with a two-option prompt, no SKU, no flag or path
entity, glossary and roadmap non-normative, `sm:order` as EXPECTED). Grading caveat:
the subagents have shell access and two f2 sessions read the fixture's EXPECTED.md
(a grep for duplicate hints, a `head -20`); later run copies exclude the file.

What the loop fixed, docs first then code, in the order found:

- The prompt. The protocol line a standalone serving returned said to call
  `begin_goals` again (`serving-protocol.md` now states the claim); the contract
  paragraph and the static hints repeated for every goal of a batch (once per kind
  now); the change line was raw JSON; `done` without `beginNext` returned the whole
  board, 169k characters; `goals` on a thirty-goal board returned 75k characters
  (compact by default, `full: true` for everything); the `## Loaded` block listed
  items whose text never arrived, so every batch opened with a round of reads (the
  rendered items ride under the block now, and the f1 reads went from nine to zero);
  the condensed block echoed unchanged on every mutating reply; `search` hid
  `queries`; `mark_goal_done` had no stated evidence shape; `update_view exclude`
  refused the list the skill promised.
- The store. Two statements from one sentence folded into one requirement (the
  same-quote fold now needs a tight rewording and reports `matched`); an update that
  touched only `entities` or `edges` reopened every entity's review (only the added
  entities, or the ends of a new composition); a review's own refresh reopened it (the
  page said it would not; the code now agrees); `upsert_entity` with a parent minted
  `admin-cli-2` beside a parentless `admin-cli` (`parentless-twin`); the hub counted
  as a shared entity and two shared entities qualified a pair with no word in common;
  a code block owned a reconcile goal and a coverage mark against parsing.md; skill
  text counted toward the load refusal, so two skills active left no room to load; the
  near-duplicate gate folded Employee Handbook into Employee.
- The pictures. Flow views clustered by whichever entity led a sentence (f2 drew
  `Product: Inventory` with Shipment as its actor); they cluster by document now,
  titled by the document, actors read off the members, none invented. A lifted arrow
  kept the leaf's cardinality; an association drew twice, once per direction; a
  sequence message followed a trigger clause instead of the subject; a root-level
  message collapsed to `Orderly -> Orderly` where the Operator initiates; the index
  linked a level page that did not exist and embedded a child level before the top.
- The skills. A condition naming another entity is context, never an edge; a plural
  area and its singular concept are two nodes, with the concepts an area's page
  defines under the area; a test case's data blocks belong to the case section; a
  contradiction prompt's `old_text` comes from the section, not only the quote.

The loop continued onto the org corpus and onto generation:

- example-org, no answer key in the copy: converged twice at 100 percent coverage
  with every one of its eight traps passing (one Employee, one Finance, the 250 versus
  500 threshold as an error with a two-option prompt, the salary-band rule out of
  Background, no form-number or path entity, the glossary quiet, `hired` never entered
  and `dead-end-state` filed, "promptly" flagged unmeasured). The first run spent 155
  pair verdicts and $23.38 because pairs qualified on shared tokens such as `s`, `at`,
  and `their`; with function words and short tokens dropped the rerun spent 106 on a
  richer graph (35 entities against 27) for $21.73.
- Generation over `jazyk mcp all` on the converged f2 (tracked fixture files only):
  three invocations bound 29 requirements and generated five entities (Admin CLI,
  Catalog, Customer, Inventory, Restock task) with every recorded row green, the
  medium recorded first by the agent through `record_medium`. On example-sort the
  agent recorded a Node script as the medium, bound all 17 requirements (two as `llm`
  rows), found nothing to generate, and left one test failing on purpose: it encodes
  the documented Simple case, which contradicts the ascending rule, the real defect
  the compile had already filed. What the runs fixed: an actor with no files of its
  own re-deriving forever as `files-gone`, the medium undecidable by an external
  worker, the crate root no entity could touch (`supportFiles`), a shared requirement
  tested once per entity, a test name matched literally where a Rust runner addresses
  `tests::req_x`, an earlier test keyed on an id that renumbers between compiles
  rather than the statement hash, and stale tests reconstructed by hand
  (`existingTests` in the binding package now names them).

- The corpora the loop had not run (no answer keys in the copies): example-slides
  converged in 68 turns and $2.40 with no diagnostics and no flow view, which is right
  for a deck whose statements are content. example-erp converged twice at 100 percent
  coverage: 218 turns and $8.74 with 36 pair verdicts, then 193 turns and $7.63 with
  18 after the fixes it exposed. Six operation bullets of one page had opened
  twenty-three pair goals on `supports` and `operation`, so an entity's boilerplate
  (a token on at least four and half of its statements) counts for nothing; the
  duplicate-requirement check had filed a one-subject twin beside the session's
  finding; three bare headings had reached a session as goals; `done(beginNext)` had
  declined the optional batch at convergence; and User Account's four attributes drew
  nowhere, since a sub-system holding one domain object had no level view (a one-child
  level with attributes derives its class view now). example-sort rerun on the same
  binary: 60 turns, $2.71, 18 pair verdicts against 36, and a contradiction filed
  before its pair opened could not be carried by `settle_pairs` (the reuse path had
  never run; it needs only the justification now). Regression runs on the fixed binary:
  f1 converged in 64 turns and $2.87, f2 in 199 turns and $7.83 with 16 pair verdicts
  and every planted trap holding (one Order, `buyer` an alias, the 21 versus 30 day
  contradiction as an error with two edit options, no SKU, glossary and roadmap
  non-normative, `sm:order`), and checklinks reports zero broken links on the
  generated pages after the docsgen anchor and quote-link fixes.

- example-org rerun on the fixed binary: converged in 405 turns and $19.71 with 81 pair
  verdicts (155, then 106, then 81 across the three runs), 38 entities, every trap
  holding. Its feedback: `update_entity` could not clear a parent (`remove_parent`
  now), the cardinality enum could not say "four departments" or "two or three" (a
  multiplicity pattern now), the skill's "the event that creates the subject names its
  initial state" read as a transition to record (the creating sentence records none),
  and two hint wordings ("its parent" for the level above, "this session" for the open
  batch).
- example-novel, no answer key in the copy: converged after two driver rounds (eleven
  invocations, 1085 turns, $67.87, 132 minutes) at 100 percent coverage with 251
  requirements, 267 pair verdicts, and 47 entity reviews. The traps held: one Ines
  Marlow with `Nessa` as an alias and no `ent:nessa`, Theo's 34 versus 36 filed as an
  error with Ines's 31 twice a quiet duplicate, both `acquaintance` transitions standing
  with `nondeterministic-transition` from the checks and a contradiction from a session,
  the research note's three-day rule extracted and the section covered, no chapter-file
  or directory entity, the two ledgers and the three Marlows apart, the notebook scene an
  instance of Scene with its four values. What it cost: the two leads carried 95 and 82
  statements, so `requirements-per-entity` (hard 80) raised mandatory `abstract-entity`
  goals on people, and the sessions moved each lead's arc into an `arc` sub-entity to
  get under the limit, taking the state machines with it; the limit now counts the
  statements an entity leads (its first-listed entity), never the ones that only name
  it. A session also grouped the four characters and the Marlow family under a derived
  Characters node along the front door's index line, and split the 30-member plot flow
  into one sequence view per chapter with an overview above them. The characters carry
  no `age` or `occupation` attributes: the contract had said an attribute goes on the
  entity "instead" of a requirement while the skill's example said requirement, and the
  sessions chose requirements (both now).

- example-novel again, capped at five invocations on the fixed binary ($31.82, 547
  turns, not converged by design): the four characters now carry `age` and
  `occupation`, the Lantern Room its hours and position, the harbor wall its dates, and
  the leads stay whole (the per-entity limit no longer counts the statements that only
  name them). Seven feedback entries fixed: the settings page still read the Scene as
  its subject (a link introduces an entity only when its sentence names it), the pair
  slice had no way to record an alias the quotes settle (`update_entity` now), the
  conform-instance gate and the conformance skill said nothing about links to
  entities that are no instances (they realize a declared attribute), the judgment
  skill did not resolve the same fact stated of a source artifact and the deliverable
  built from it (two subjects, consistent), `scope:public` read as a scope the session
  had set (the hint says it is the flat top level), and a 58k first reply spilled to a
  file (skills ship as `load_skill` pointers past 45k).

- Acceptance criteria (2026-09-13, `plans/acceptance-criteria.md`): on example-sort the
  first compile with the criterion tools served converged in 86 turns and $5.64 with
  14 requirements and 24 criteria, two quoted from the test-case sections and 22 derived
  by one `derive-criteria` session with a boundary or a negative for every conditional
  and every collecting step, none restating its requirement; the requirements document
  lists them and binding takes one test per criterion. The first attempt had found the
  tools named in the contract but absent from the MCP serving, which a test now holds.
  On f2 the criteria compile converged in 356 turns and $14.17 (7.83 without criteria)
  with 102 criteria for 53 requirements, three quoted and the rest derived, none
  missing; the first attempt had packed twelve entities' criteria goals against one
  64-mutation budget and failed three, so a criteria goal is a batch wall of its own
  and raises the budget per requirement. Generation over that ledger bound 34 rows in
  three invocations for $18.52, one test per criterion on most, and exposed that
  regeneration dropped the criterion tests from a row, that a partly implemented
  requirement had no status between unimplemented and failing (it reads `partial`
  now), and that a failing binding could reach no author (the `failing-binding` check
  files it). On example-erp the criteria compile converged with 86 criteria for 39
  requirements (54 positive, 32 negative, none quoted: the corpus states no cases or
  examples), eight criteria goals done, for about $20 across a run the usage limit cut
  and its resume. Generation over that ledger on an empty deliverable bound 34
  requirements with 73 criterion tests and generated the backend and the frontend as
  an npm workspace, every test green, with no tool error and no feedback entry in 148
  turns ($16.16). example-slides with a deliverable converged with 31 criteria for 14
  requirements ($5.30 against $2.40 without them); example-f1 without a deliverable
  derives no criteria goals and converges as before ($2.55), its missing criteria
  riding as information. Generation over the sort and slides criteria ledgers bound 14
  requirements each with 24 and 31 criterion tests ($3.25 and $6.38), sort's one red
  row being the documented contradiction and the slides deck built and judged green.
  example-org with a deliverable converged with 251 criteria for 126 requirements (13
  quoted from its examples), every trap holding, across a Claude run of $33.31 the owner
  stopped mid-way and a Codex finish of one invocation; from that point the loop runs
  on Codex and the embedded agent only.
- The embedded agent through LocalRouter (gemma4:e4b, text codec) converged f1 in 6
  sessions and 25k tokens, sort in a resumed run after a fix (the text codec hands
  `evidence` as a JSON string and the gate refused every shape), and f2 in 46 sessions
  and 289k tokens: 46 requirements, 14 entities, 68 criteria (8 quoted), the planted
  contradiction filed as an error, the Order state machine with its three transitions,
  100% coverage. Its gaps are judgment (Inventory, Catalog, and Admin CLI without the
  Orderly parent, Admin CLI without a stereotype) and stamina (twenty tool errors,
  eight of them three-sentence justifications; three optional GC goals parked after a
  truncated action object read as prose, which the codec now answers with a repair).
  It read the board's fresh `b24-1` beside its own `b24-1` and reported the two as
  inconsistent: the goals reply now marks the reader's own leased goals and numbers
  derived batches past the ids live leases hold. Codex generating over the erp criteria
  ledger exposed the sequencing hole: it bound one backend requirement with no files,
  generated the entity as an object literal describing the API (Express, PostgreSQL,
  four endpoint paths), and bound nine more rows against that description as verified,
  all green; `begin_generation` now refuses an entity whose goal is not ready (its
  requirements still owe a binding), the task lists carry readiness, the generation
  contract makes the bound criterion tests pass instead of deriving requirement tests,
  and the bind contract names a constant or object literal describing the artifact a
  description in another syntax. Under the fixed binary the next two invocations bound
  24 rows, every one unimplemented with a failing criterion test as its gate, with no
  tool error, and the run finished green: 39 rows, 42 criterion tests, all passing,
  over seven Codex invocations. The org handbook under Codex bound all 126
  requirements with 251 criterion tests (244 green) and generated five Markdown pages
  across nineteen invocations and two usage-limit pauses; the four rows left red are
  expense and hiring rules that the handbook-delivery requirements contradict, and the
  `failing-binding` check files them as findings because generation had its turn (a
  rule added when sort's contradictory example criterion regenerated its entity on
  every build). The embedded agent binding f2's 46 requirements against its Rust
  product wrote 66 criterion tests (44 green) and generated two entities before every
  later generation session died on a 400 about an unclosed object: a tool call cut at
  the model's output cap was dispatched as an empty call and stored verbatim, so every
  later request of the run carried the cut JSON; cut arguments are now a repaired
  action that never re-enters the history, and `write_text_file` appends so a long
  file is written in parts.

- The review of every collected run (2026-09-15; `jazyk-runs/review-2026-09-15.md`,
  1183 sessions audited) found the cost and the shallowness in one place: statement
  fragmentation. The doctrine's one-requirement-per-fact and per-bullet rules turned six
  novel documents into 294 statements (113 sharing a quote with another, 67 quoting a
  list item), a four-department list into nine statements, and 126 org requirements
  into 251 criteria, each statement then reviewed, paired, given a positive and a
  contrived negative criterion, tested, and dumped as a docsgen block (a novel lead's
  page: 54KB, 103 blocks). The judgment sessions themselves were cheap and sound, and
  errors were under 2% of calls on every executor. The changes: a parts list is one
  membership statement with an edge per part, a property list one statement with
  attributes, one criterion per requirement the norm; docsgen pages open with an index
  and group by document and section; ratification proposals count on their own line;
  a failed abstract-entity goal stands within a fifth of growth. Fixes are now verified
  by replays (`jazyk replay`, scripted sessions against the harness, seconds, no model)
  and by single-session benchmark cases; the embedded agent (qwen3.8:27b) scored the
  new `enumeration` and `properties` cases 1.00 in six and seven rounds.

- f2 under the new doctrine (2026-09-24, qwen3.8:27b through Ollama, the embedded
  agent): converged at 100 percent coverage with all six planted traps holding, 44
  requirements (46 before), none facetless (four before), and 60 criteria (68). The
  five `facets-missing` refusals were definitional statements the model recorded with a
  facet in the same session, one round each. Reading its sessions found five defects,
  each verified by a replay or a unit test proven to fail without the fix and never by a
  rerun: a criterion copying a sibling requirement word for word passed the gate (the
  `criterion-restates` gate now compares against every statement); reviews recited up
  to eight `criterion-missing` findings they could neither act on nor clear (a
  harness-owned rule now); every criteria session spent one to nine `read_section`
  rounds before its first criterion (it opens holding the source sections); 22 of 389
  recorded bindings across three runs listed their own tests as implementation (dropped
  and named now); 25 of 44 docsgen blocks printed the quote and a statement differing
  only by `shall` (the quote is omitted). Every benchmark case, snippet, and replay
  session had been told to call `begin_goals` with no such tool (an open-batch protocol
  line now).
- The erp generation Codex had finished green reads verified on 42 of 42 criteria with
  no server: `npm start` names a missing file, the frontend prints a constant, and
  nothing uses Express or PostgreSQL. The tests assert descriptor objects, rooted in a
  derived criterion "when a client examines the integration contract". Single-session
  probes on the local model wrote behavioral criteria under the old and new skill alike
  and bound to the descriptor test under three contract wordings, so the criteria skill
  now says an observation uses the product as its user does, and the
  `criteria-observation` case grades it (qwen3.8:27b 1.00 on both codecs, a client sending an HTTP request and a user seeing the listed products; Codex's recorded erp criterion fails it; Codex itself now scores it 1.00, observing the listing as a user and asking an `invented-choice` question for the technology-only statement). The same review cases exposed two serving defects, both fixed: a pretty-printed case list Codex's approval guard could not see a named case in, and `report_feedback` refused as `no-build` after a case closed. review-lint went from 0.50 to 1.00 on qwen3.8:27b once the review evidence named each project lint rule, and Codex scores it 1.00. A deterministic signal for a verified row whose
  files hold no executable code is still open.

- example-novel under Codex on the new doctrine (2026-09-24, 21 invocations across usage
  pauses, generation 99): converged at 100 percent coverage with every planted trap
  holding (one Ines with `Nessa` an alias and mentions in six documents, Theo's 34 versus
  36 filed as an error, the two `acquaintance` transitions kept with
  `nondeterministic-transition`, the research note's rule extracted, no file or
  directory entity, the ledgers and the Marlows apart, the notebook scene an instance of
  Scene with its four values). 261 statements against 294 on the old doctrine: the
  change targeted lists, and the novel's prose states a fact per clause. Where it departs
  from the answer key: the stance machines sit on minted stance entities under each lead
  rather than on the leads, Ines hangs under a derived Cast grouping with four
  aggregation edges from the Marlow family, and nothing ties the Lantern Room or Marlow
  House to Port Alder. 74 default views for 52 entities: a document's flow derives once
  per level it reaches (the root, Cast, Settings), the same members lifted to three
  zooms, which the level-view design intends and which is an owner question.
- The full benchmark on qwen3.8:27b (2026-09-24, both codecs) scored 124/127 native and
  120/127 text; reading each miss's session found defects in the harness, not the model,
  for most of them: a case batch listing a code block beside its holding section, a
  same-sentence reword minted as a second requirement over `shall`, a test row without
  its requirement dropped in silence, a repeat guard calling a changed answer unchanged, a
  split-view claim accepted with the view still over its limit, a multi-sentence quote
  merging two facts, a new entity quoting a sentence that never names it, a mid-turn codec
  downgrade leaving native calls in the history, and a reasoning plan cut by the
  completion cap on every round. Each landed with a unit test or replay that fails without
  it; the targeted rerun on the fixed binary scored edges, converge, split-view, and
  gen-basic 1.00 on both codecs.

Left standing: the pair volume on a one-entity corpus (best six per requirement, by
design, 32 verdicts for 17 requirements on sort); a curated use-case view over one
actorless behavior (erp's "Manage the ACME warehouse" draws one bubble and nothing
else, honest but empty); a statement repair inside a pair
session re-deriving its pairs; the front door's "Its concerns are described in"
leaving Ledger beside its areas rather than above them (an owner question); the
deterministic checks' findings appearing only at the tail while the project block
counts judgment diagnostics, which one session read as a contradiction.
