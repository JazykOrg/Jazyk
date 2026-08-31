# MCP

`jazyk mcp <toolsets>` serves the [tool registry](../compiler/tools.md) over stdio as an
MCP server: line-delimited JSON-RPC per the Model Context Protocol. It dispatches the
same tool implementations the compiler's own [sessions](../compiler/sessions.md) use.
There is one registry, not a second API beside it.

The serving has two audiences: an agent that connects on its own (the standalone
loops below), and an [ACP session](./acp.md) that gets the serving injected per
session ([MCP into ACP sessions](#mcp-into-acp-sessions)). Same tools, same gates.

## Toolsets

The argument names what the serving is for. One serving can carry several, comma
separated; each adds its tools to the union:

- `jazyk mcp compile`: the [compilation lifecycle](../compiler/tools.md#compilation-tools),
  the [write tools](../compiler/tools.md#write-tools), the
  [view tools](../compiler/tools.md#view-tools), and the
  [goal tools](../compiler/tools.md#goal-tools). The agent claims goal batches
  from [the board](../compiler/compiler.md#components) and resolves them.
- `jazyk mcp generate`: the [binding tools](../compiler/tools.md#binding-tools) and
  the [generation tools](../compiler/tools.md#generation-tools). The agent searches
  and writes the deliverable with its own editor and shell; jazyk holds the ledger.
- `jazyk mcp verify`: the [verification tools](../compiler/tools.md#verification-tools).
  Small enough to hand a subagent whose only job is judging one row.
- `jazyk mcp decompile`: the
  [decompilation tools](../compiler/tools.md#decompilation-tools). The agent reads
  the code with its own tools and submits prose drafts; jazyk validates and lands
  them in the docs tree. Decompilation stays outside the board.
- `jazyk mcp benchmark`: the agent under test performs the
  [benchmark cases](../benchmark/benchmark.md#agent-run-benchmarks) against sandbox
  stores and is graded by the same deterministic checks as an endpoint run. Built to
  hand a subagent: the serving is self-contained and touches no project state beyond
  the machine-wide history its report appends.
- `jazyk mcp graph`: the read tools only, for retrieval consumers. `--write` adds the
  raw write tools and view tools without the lifecycle, for debugging and manual
  graph surgery; each write commits as its own changeset, and the commit writes its
  [change records](../compiler/graph.md#change-records) so the next build derives
  the goals the write opened.
- `jazyk mcp chat`: the serving for [chat sessions](./acp.md#chat-sessions): read
  tools, the [compilation](../compiler/tools.md#compilation-tools),
  [binding](../compiler/tools.md#binding-tools),
  [generation](../compiler/tools.md#generation-tools), and
  [verification](../compiler/tools.md#verification-tools) lifecycles, the
  [dual-write tools](./acp.md#dual-write-tools), `update_diagnostic` and
  `answer_diagnostic` ([questions in chat](./acp.md#questions-in-chat)), and the
  [project tools](./acp.md#project-tools). No raw write tools: a chat edit moves the
  prose and the graph together or not at all.

Every serving includes the [read tools](../compiler/tools.md#read-tools) (`load`,
`expand`, `unload`, `graph_status`, `search`, `read_section`, `get_entity`,
`get_view`, `diagnostics`), [`report_feedback`](../compiler/tools.md#feedback-tool),
and `await_changes` (below). The reads maintain the
[loaded set](../compiler/context.md#the-loaded-set) per open batch, and every
mutating reply carries its condensed status block.

On a compile serving, `done` is the one finishing call: the session prompt says
`done`, so the model never translates a verb.

## MCP into ACP sessions

When jazyk creates an [ACP session](./acp.md#worker-sessions), it injects one MCP
server into it: `jazyk mcp` with the batch's toolsets and flags. The flags exist for
this spawning path and are not for standalone servings:

- `--ephemeral`: the serving belongs to one session. It does not register in the
  [worker registry](../compiler/control-plane.md#workers-and-leases) (the session is not
  a peer worker, it is part of a run that already holds its lease), and end of input
  with an open batch runs the implicit finish: staged work commits under the same
  gates the [budget path](../compiler/sessions.md#budgets) uses, so an agent that dies
  mid-batch still lands its valid extractions. It does not serve `await_changes`:
  the session exists for one batch, and a long poll is a stall wearing a tool's name
  (a weak model was observed idling a build on it instead of calling `done`).
- `--only <batch|goal>`: `begin_goals` accepts only the named batch
  (`b<generation>-<n>`), or the batch the named goal (`g:...`) belongs to. A
  session's serving cannot claim work beyond its batch, so a session that finishes
  early never wanders onto the rest of the board.
- `--build-token <id>`: the serving is part of the running internal build. The
  build-lease refusal and the release gate do not apply to its batch; without this,
  a build's own sessions would deadlock against the build's own lease.
- `--packaged`: the bridge already delivered the
  [assembled prompt](../compiler/sessions.md#the-prompt) as the session prompt;
  `begin_goals` answers with a short ack instead of repeating it.
- `--serve-files`: adds the file and command tools
  ([generation tools](../compiler/goals/generate.md#tools)) for agents whose
  profile sets [`serve_files`](../compiler/project-settings.md#acp). Agents with
  their own editor never get them.
- `--edit-sink <path>`: delegate document and settings writes to the spawning
  process ([doc edit delegation](./acp.md#doc-edit-delegation)). Absent, or with
  nothing listening, writes land on disk directly.

The `initialize` reply carries server instructions describing the work loop for the
toolsets served, so an agent that reads nothing else still knows the entry point, the
order of calls, and that a tool error names its own repair. A serving ends when its
input ends: close stdin to tear it down. `shutdown` is answered with null for agents
that send it out of LSP habit; it does nothing. The prompting lives in three places,
most specific wins: server instructions carry the loop, each `begin_*` reply carries
the batch's own assembled prompt (or the package, for binding and generation), and
each tool description names what to call next.

## The work loop

The agent-facing loop, common to the compile, generate, and verify toolsets:

1. Ask for work: `goals`, `binding_tasks`, `generation_tasks`, or
   `verification_tasks`. An empty answer is an answer, not an error; the board
   carries the build verdict when no goal is open.
2. Begin: `begin_goals` claims the next ready batch, the named batch, or the named
   goals as one batch; `begin_binding`, `begin_generation`, `begin_verification`
   claim one row each. The reply is the batch's contract: the assembled prompt as
   `instructions` and the loaded set as `package` for goals, the package for the
   rest.
3. Do the work: a goal batch stages graph writes and marks each goal
   `mark_goal_done` with a one-line justification (or `mark_goal_failed` with a
   reason); binding searches the deliverable and writes the missing test; generation
   edits deliverable files with the agent's own tools; verification runs or judges
   tests.
4. Finish: `done` runs the batch gates and commits the changeset, `record_binding`
   records the row, `record_generation` records the manifest, `run_tests` and
   `record_verdict` record verdicts. The reply names the next ready batch or row, so
   the loop chains without re-listing.

To watch for new work instead of polling, either run
[`jazyk monitor`](./cli.md#jazyk-monitor) as a background process and act on each
notice it prints (`--once` for a single blocking wake-up), or call `await_changes`:

- `await_changes({timeout_seconds?})`: a long poll. It returns when the graph's
  generation counter moves, a documentation file changes on disk, a manifest or test
  file in the deliverable changes, the ledger changes, or the
  [control plane](../compiler/control-plane.md) changes (a mode
  toggle or a release, so the user's click in the GUI is what wakes the agent), or
  at the timeout (default 300 seconds). `timeout_seconds: 0` waits indefinitely. The
  default returns because most MCP clients bound a tool call with their own timeout
  and would report an indefinite block as an error; a client configured without that
  bound passes 0 and holds the call open. The reply carries the changed documents,
  whether the graph is stale (documents changed but not yet reconciled), the board
  counts (open, blocked, parked, failed, per class), the open diagnostic counts, the
  pending generation work, the pending verification work grouped by reason, and the
  workflow modes with the gated counts.

## The control plane over MCP

The serving is a worker among workers, and says so:

- `initialize` registers the serving in the
  [worker registry](../compiler/control-plane.md#workers-and-leases) under the client's
  name (kind `agent`), heartbeats while the process lives, and deregisters on exit.
- `begin_goals` takes the batch's lease; `done` and `abandon_goals` release it.
  `begin_binding` and `begin_generation` take the row's lease; `record_binding` and
  `record_generation` release it, and a row lease nothing records against expires
  by the lease TTL
  ([workers and leases](../compiler/control-plane.md#workers-and-leases)), so a
  row is never abandoned by a tool call, only recorded or left to expire.
  A batch another worker holds is refused with `claimed` naming the holder; a live
  internal build refuses every begin with `build-running` (compilation is
  sequential: one build, one session at a time). Any tool call on the open batch
  refreshes its lease.
- In `manual` mode a gated batch is refused with `awaiting-release`: the reply names
  the release that opens it (`jazyk release compile`, or the GUI). The board carries
  `gated` and `claimedBy` per goal, so an agent sees the whole board before
  claiming.

## Compilation over MCP

Goal batches mutate the graph, so the serving holds an open changeset between calls,
exactly one at a time. The lifecycle has the same begin and done shape as every other
serving; what it claims is a batch:

- `goals({})` derives [the board](../compiler/reconciler.md#goal-derivation) from
  disk: every goal with its kind, class, `mandatory`, target, `change`, `cause`,
  state, hints, and [readiness](../compiler/reconciler.md#readiness) (`ready`, or the
  blocking reason as a sentence), plus `gated` and `claimedBy`. The reply groups the
  ready goals into the batches the scheduler would form, each with its id and its
  resolved [executor](./acp.md#executors). A batch id is `b<generation>-<n>`: the
  generation the board derives from and the batch's index within it. Batch ids are
  derived with the board, so a commit re-derives both and an id from an earlier
  generation names nothing. When no goal is open the reply carries the verdict with
  its counts instead.
- `begin_goals({batch?, goals?, full?})` claims the named batch, the named goals as
  one batch when they share a locality and their kinds' executors agree, or (with
  neither) the next ready batch (the highest ready tier, one locality, filled to the
  context budget: [batching](../compiler/reconciler.md#batching)). A `batch` the
  current board does not hold is refused, and the reply carries the current
  batches so the agent picks again without a second `goals` call. The claim reloads
  the store, syncs the section trees in memory, opens a changeset, assembles the
  [session prompt](../compiler/sessions.md#the-prompt) for the batch as
  `instructions`, and returns the batch id, its goals, and the initially
  [loaded set](../compiler/context.md#the-loaded-set) as `package`. A
  [`place-anchors`](../compiler/goals/place-anchors.md) goal carries the document's
  alignment proposals in its loaded set; a
  [`reconcile-section`](../compiler/goals/reconcile-section.md) goal stays blocked
  while its document has pending alignment. The serving's toolset for the batch is
  the union of its goal kinds' [toolsets](../compiler/sessions.md#toolsets).
- The write tools stage into the changeset, validated by the same
  [gates](../compiler/graph.md#validation-gates) a session faces, scoped to the
  batch's locality the same way. A mutating reply previews the goals the mutation
  will open ([bubbling](../compiler/reconciler.md#bubbling)) and re-renders the
  condensed status block.
- `mark_goal_done({goal, justification, evidence?})` is validated against the goal
  kind's gate when staged and again at commit; a claim the gate refuses comes back
  with the gate named. `mark_goal_failed({goal, reason})` is always accepted.
  `load_skill({name})` brings a [skill](../compiler/sessions.md#skills) into the
  batch. The goal must belong to the open batch; a goal from elsewhere on the board
  is rejected toward `done`, because the next batch is where it gets claimed.
- Write tools and goal tools outside an open batch are rejected toward
  `begin_goals`. Identity, provenance, and scope rules hold because they are the
  same code path.
- `done({summary, beginNext?})` runs the batch gates (every goal resolved or failed,
  coverage contract, stale anchors), commits atomically, writes the journal entry
  with `resolved_goals` and `opened_goals`, and re-derives the board. A gate failure
  leaves the changeset open and names the repair. The reply is `{committed, next}`,
  `next` naming the next ready batch by id with its goals. `beginNext: true` claims
  that batch in the same call and carries its `instructions` and `package` in the
  reply, saving a round trip per batch; the default reply only names it. The first
  batch of a serving ships the agent contract and the active skills in full; later
  batches elide the contract and any skill already delivered (the agent saw them
  earlier in the same session), while the project block, the goals block, and the
  loaded block always ship whole. `begin_goals` with `full: true` repeats everything
  for a client that lost its context.
- A goal the batch neither resolved nor failed when `done` commits parks; a parked
  goal resumes first in the next batch that fits it
  ([parked and failed](../compiler/reconciler.md#parked-and-failed)).
- `abandon_goals({reason})` drops the staged work. An abandoned batch leaves no
  trace, the same contract as an aborted session; its goals return to `open`. A
  server that dies mid-batch loses only staging; any process recomputes the board
  and the goals reappear.
- GC goals become ready as their cones settle
  ([compile and garbage collection](../compiler/compilation.md#compile-and-garbage-collection)).
  The board offers them in the same order the internal scheduler would, so an
  external agent runs the same bursts: a GC goal whose locality matches the batch
  just committed is the next batch.
- When a commit leaves no open goal on the board (blocked and optional goals aside),
  the serving runs the deterministic tail itself
  ([checks](../compiler/compilation.md#checks), rendering, docsgen, verdict),
  because none of it needs a model. The final `done` reply carries the
  [verdict](../compiler/compilation.md#convergence) with its counts and, under
  `ready`, the generation rows that became ready (`ready: {generate: [...]}`);
  `generation` stays the counter in every reply.

## Generation and verification over MCP

Binding and generation are stateless on the server: `begin_binding` and
`begin_generation` return the package, the agent searches, edits files, and runs
commands with its own tools, `record_binding` records the row and
`record_generation` the manifest. The binding package carries the requirement's
`statement` and `quote`. Jazyk deliberately serves no file-editing tools here; a
coding agent brings its own, and the in-process worker's file tools
([generation tools](../compiler/goals/generate.md#tools)) are not served over MCP.
The `bind`, `generate`, and `verify` goals on the board are the same rows these
lifecycles claim: resolving a row resolves its goal.

Verification: `run_tests` executes recorded programmatic tests (the build first, then
the commands) and records verdicts as a side effect. `llm` rows go through
`begin_verification` for the criteria and `record_verdict` for the judgment.

E.g. the loop after a docs edit:

```
await_changes → {changedDocs: [docs/orders.md], graphStale: true, board: {open: 2, blocked: 0}, ...}
goals → {generation: 412,
         goals: [{goal: g:place-anchors:docs/orders.md, kind: place-anchors, class: compile,
                  mandatory: true, ready: true},
                 {goal: g:reconcile-section:docs/orders.md#/orders/holds, kind: reconcile-section,
                  class: compile, mandatory: true, ready: false,
                  blocked: "alignment pending for docs/orders.md"}],
         batches: [{batch: b412-1, goals: [g:place-anchors:docs/orders.md], executor: embedded}]}
begin_goals {batch: b412-1}
  → {batch: b412-1, goals: [g:place-anchors:docs/orders.md],
     instructions: <the assembled prompt>, package: <section changes, proposals with candidates>}
(agent stages place_anchor / orphan_anchor, then mark_goal_done ...)
done → {committed: true, generation: 413,
        next: {batch: b413-1, goals: [g:reconcile-section:docs/orders.md#/orders/holds]}}
begin_goals {batch: b413-1}
  → {batch: b413-1, goals: [...], instructions: <...>,
     package: <the dirty section with its diff, its entities as stubs, stale anchors>}
(agent stages upsert_requirement / set_coverage, then mark_goal_done ...)
done → {committed: true, generation: 414,
        opened: [g:rejudge-pair:req:orders-6~req:payment-9, g:review-entity:ent:order],
        next: {batch: b414-1, goals: [g:rejudge-pair:req:orders-6~req:payment-9,
                                      g:review-entity:ent:order]}}
... judgment batches the same way; the cone settles and its GC goal is next:
done → {committed: true, generation: 415, next: {batch: b415-1, goals: [g:declare-edges:req:orders-6]}}
... last done → {committed: true, generation: 418,
                 verdict: {state: converged, blocked: 0, optional: 0},
                 ready: {generate: [ent:order]}}
binding_tasks → [{requirement: req:orders-6, reason: requirement-changed}]
begin_binding {requirement: req:orders-6} → package (statement, quote, entity, context)
(agent finds the test, sees it assert the old value, rewrites tests/orders.rs)
record_binding {requirement: req:orders-6, files: [src/orders.rs], test: {...}, verdict: fail}
  → row failing; ent:order is generation work
begin_generation {entity: ent:order} → package (carries the bound tests)
(agent edits src/orders.rs with its own tools until the bound test passes)
record_generation {entity: ent:order, factHash, manifest} → ledger updated
run_tests → build + commands run, verdicts recorded
```

A fix-fail-reverify cycle is self-terminating: editing a deliverable file re-stales
exactly the rows whose files hash moved, and the pending list shrinks monotonically
once tests pass. [`jazyk ripple docs/orders.md`](./cli.md#jazyk-ripple) replays the
whole cascade afterward, one line per journal entry.

## Transcripts

Every serving leaves a transcript: one JSON-lines file under `<out>/trace`
(`<ts>-mcp-cli<pid>.jsonl`; the file opens at server start, before the client
introduces itself, so the stem carries the process id, not the client name), one
`toolCall`/`toolResult`/`toolError` event per call
with condensed payloads, labeled by the open batch's id when one exists (`b412-3`)
and by the serving otherwise. The same format the compile trace writes
([trace events](../compiler/sessions.md#trace-events)), so an MCP session is
reviewable beside a build: what an agent asked, what it was told, and where it
stumbled.

## Reads and locking

Reads load the persisted graph from the out directory (see
[storage layout](../compiler/graph.md#storage-layout)). The server never compiles on
its own. If no graph exists yet, the compile toolset offers the board (a first build
is a board where every section is unprocessed); the other toolsets answer with
guidance to compile first.

Readers do not lock. They read the generation counter, load shards, and retry if the
counter moved mid-read. Writers respect the store lock, so one changeset commits at a
time even with a compile running next to the server. See
[concurrency](../compiler/graph.md#concurrency).
