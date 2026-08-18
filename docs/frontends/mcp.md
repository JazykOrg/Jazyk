# MCP

`jazyk mcp <toolsets>` serves the [tool registry](../compiler/tools.md) over stdio as an
MCP server: line-delimited JSON-RPC per the Model Context Protocol. It dispatches the
same tool implementations the compiler's own [turns](../compiler/turns.md) use. There is
one registry, not a second API beside it.

The serving has two audiences: an agent that connects on its own (the standalone
loops below), and an [ACP session](./acp.md) that gets the serving injected per
session ([MCP into ACP sessions](#mcp-into-acp-sessions)). Same tools, same gates.

## Toolsets

The argument names what the serving is for. One serving can carry several, comma
separated; each adds its tools to the union:

- `jazyk mcp compile`: the [compilation lifecycle](../compiler/tools.md#compilation-tools)
  plus the [write tools](../compiler/tools.md#write-tools). The agent performs
  compilation tasks from [the task queue](../compiler/reconciler.md#the-task-queue).
- `jazyk mcp generate`: the [binding tools](../compiler/tools.md#binding-tools) and
  the [generation tools](../compiler/tools.md#generation-tools). The agent searches
  and writes the deliverable with its own editor and shell; jazyk holds the ledger.
- `jazyk mcp verify`: the [verification tools](../compiler/tools.md#verification-tools).
  Small enough to hand a subagent whose only job is judging one row.
- `jazyk mcp decompile`: the
  [decompilation tools](../compiler/tools.md#decompilation-tools). The agent reads
  the code with its own tools and submits prose drafts; jazyk validates and lands
  them in the docs tree.
- `jazyk mcp benchmark`: the agent under test performs the
  [benchmark cases](../benchmark/benchmark.md#agent-run-benchmarks) against sandbox
  stores and is graded by the same deterministic checks as an endpoint run. Built to
  hand a subagent: the serving is self-contained and touches no project state beyond
  the machine-wide history its report appends.
- `jazyk mcp graph`: the read tools only, for retrieval consumers. `--write` adds the
  raw write tools without the task lifecycle, for debugging and manual graph surgery;
  each write commits as its own changeset.
- `jazyk mcp chat`: the serving for [chat sessions](./acp.md#chat-sessions): read
  tools, the task lifecycle, the
  [dual-write requirement tools](./acp.md#dual-write-tools),
  `update_diagnostic` and `answer_diagnostic`
  ([questions in chat](./acp.md#questions-in-chat)), and the
  [project tools](./acp.md#project-tools). No raw write tools: a chat edit moves the
  prose and the graph together or not at all.

Every serving includes the [read tools](../compiler/tools.md#read-tools),
[`report_feedback`](../compiler/tools.md#feedback-tool), and `await_changes` (below).

On a compile serving, `done` is the one listed finishing call: the task instructions
say `done`, so the model never translates a verb. `finish_compilation` stays
dispatchable as a compatibility alias for older clients but is not advertised.

## MCP into ACP sessions

When jazyk creates an [ACP session](./acp.md#worker-sessions), it injects one MCP
server into it: `jazyk mcp` with the task's toolsets and flags. The flags exist for
this spawning path and are not for standalone servings:

- `--ephemeral`: the serving belongs to one session. It does not register in the
  [worker registry](../compiler/control-plane.md#workers-and-leases) (the session is not
  a peer worker, it is part of a run that already holds its lease), and end of input
  with an open task runs the implicit finish: staged work commits under the same
  gates the [budget path](../compiler/turns.md#budgets) uses, so an agent that dies
  mid-task still lands its valid extractions. It does not serve `await_changes`:
  the session exists for one task, and a long poll is a stall wearing a tool's name
  (a weak model was observed idling a build on it instead of calling `done`).
- `--only <target>`: `begin_compilation` accepts only the named target. Parallel
  worker sessions each get their own serving, and none can grab a sibling's work.
- `--build-token <id>`: the serving is part of the running internal build. The
  build-lease refusal and the release gate do not apply to its target; without this,
  a build's own sessions would deadlock against the build's own lease.
- `--packaged`: the bridge already delivered the task's instructions and package as
  the session prompt; `begin_compilation` answers with a short ack instead of
  repeating them.
- `--serve-files`: adds the file and command tools
  ([generation turns](../compiler/turns.md#generation-turns)) for agents whose
  profile sets [`serve_files`](../compiler/project-settings.md#acp). Agents with
  their own editor never get them.
- `--edit-sink <path>`: delegate document and settings writes to the spawning
  process ([doc edit delegation](./acp.md#doc-edit-delegation)). Absent, or with
  nothing listening, writes land on disk directly.

The `initialize` reply carries server instructions describing the work loop for the
toolsets served, so an agent that reads nothing else still knows the entry point, the
order of calls, and that a tool error names its own repair. A serving ends when its
input ends: close stdin to tear it down. `shutdown` is answered with null for agents
that send it out of LSP habit; it does nothing. The prompting lives in
three places, most specific wins: server instructions carry the loop, each
`begin_*` package carries the task's own contract, and each tool description names
what to call next.

## The work loop

The agent-facing loop, common to all three toolsets:

1. Ask for work: `compilation_tasks`, `binding_tasks`, `generation_tasks`, or
   `verification_tasks`. Zero tasks is an answer, not an error; the compilation list
   carries the build verdict when the queue is empty.
2. Begin the first ready task: `begin_compilation`, `begin_binding`,
   `begin_generation`, `begin_verification`. The reply is the task package:
   instructions plus everything the task needs.
3. Do the work: compilation stages graph writes, binding searches the deliverable and
   writes the missing test, generation edits deliverable files with the agent's own
   tools, verification runs or judges tests.
4. Finish: `done` commits the changeset, `record_binding` records the
   row, `record_generation` records the manifest, `run_tests` and `record_verdict`
   record verdicts. The reply names the next ready task, so the loop chains without
   re-listing.

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
  whether the graph is stale (documents changed but not yet reconciled), the pending
  generation work, the pending verification work grouped by reason, and the workflow
  modes with the gated task counts.

## The control plane over MCP

The serving is a worker among workers, and says so:

- `initialize` registers the serving in the
  [worker registry](../compiler/control-plane.md#workers-and-leases) under the client's
  name (kind `agent`), heartbeats while the process lives, and deregisters on exit.
- `begin_compilation`, `begin_binding`, and `begin_generation` take the task's
  lease; `finish_*`, `record_binding`, and `abandon_*` release it. A task another worker holds is refused with `claimed`
  naming the holder; a live internal build refuses all begins with `build-running`.
  Any tool call on the open task refreshes its lease.
- In `manual` mode a gated task is refused with `awaiting-release`: the reply names
  the release that opens it (`jazyk release compile`, or the GUI). Task lists carry
  `gated` and `claimedBy` per task, so an agent sees the whole board before
  claiming.

## Compilation over MCP

Compilation tasks mutate the graph, so the serving holds an open changeset between
calls, exactly one at a time:

- `begin_compilation` claims a task from the queue, reloads the store, syncs the
  section trees in memory, and opens a changeset. The write tools stage into it,
  validated by the same [gates](../compiler/graph.md#validation-gates) a compilation
  turn faces, scoped to the task's document the same way.
- Write tools outside an open task are rejected toward `begin_compilation`. Identity,
  provenance, and scope rules hold because they are the same code path.
- `done` runs its gates (coverage contract, stale anchors),
  commits atomically, and updates [the task queue](../compiler/reconciler.md#the-task-queue).
  A gate failure leaves the changeset open and names the repair. `beginNext: true`
  claims the next ready task in the same call and carries its package in the reply,
  saving a round trip per task; the default reply only names the next task. The first
  task of each kind ships the full instructions text; later tasks of a kind the
  serving already delivered elide it (the agent saw it earlier in the same session),
  and `begin_compilation` with `full: true` repeats it for a client that lost its
  context.
- `abandon_compilation` drops the staged work. An abandoned task leaves no trace, the
  same contract as an aborted turn. A server that dies mid-task loses only staging;
  any process recomputes the queue and the task reappears.
- When a commit empties the compile queue, the serving runs the deterministic tail
  itself (checks, docsgen, verdict), because none of it needs a model. The final
  `done` reply carries the verdict and the generation tasks that became
  ready.

## Generation and verification over MCP

Binding and generation are stateless on the server: `begin_binding` and
`begin_generation` return the package, the agent searches, edits files, and runs
commands with its own tools, `record_binding` records the row and
`record_generation` the manifest. Jazyk deliberately serves no file-editing tools here; a coding agent
brings its own, and the in-process worker's file tools
([generation turns](../compiler/turns.md#generation-turns)) are not served over MCP.

Verification: `run_tests` executes recorded programmatic tests (the build first, then
the commands) and records verdicts as a side effect. `llm` rows go through
`begin_verification` for the criteria and `record_verdict` for the judgment.

E.g. the loop after a docs edit:

```
await_changes → {changedDocs: [docs/orders.md], graphStale: true, ...}
compilation_tasks → [{kind: reconcile-document, target: docs/orders.md, ready: true}]
begin_compilation → instructions + dirty sections + stale anchors + known entities
(agent stages upsert_requirement / set_coverage ...)
done → {committed: true, next: {kind: review-entity, target: ent:order}}
... reviews the same way; last finish → {verdict: converged, generation: [ent:order]}
binding_tasks → [{requirement: req:order-3, reason: unbound}]
begin_binding {requirement: req:order-3} → package
(agent searches src/, finds no implementation, writes tests/order.rs)
record_binding {requirement: req:order-3, files: [], test: {...}, verdict: fail}
  → row unimplemented; ent:order is generation work
begin_generation {entity: ent:order} → package (carries the bound tests)
(agent edits src/order.rs with its own tools until the bound test passes)
record_generation {entity: ent:order, factHash, manifest} → ledger updated
run_tests → build + commands run, verdicts recorded
```

A fix-fail-reverify cycle is self-terminating: editing a deliverable file re-stales
exactly the rows whose files hash moved, and the pending list shrinks monotonically
once tests pass.

## Transcripts

Every serving leaves a transcript: one JSON-lines file under `<out>/trace`
(`<ts>-mcp-<client>.jsonl`), one `toolCall`/`toolResult`/`toolError` event per call
with condensed payloads, labeled by the open task when one exists (`reconcile-doc
docs/api.md`) and by the serving otherwise. The same format the compile trace writes
([trace events](../compiler/turns.md#trace-events)), so an MCP session is reviewable
beside a build: what an agent asked, what it was told, and where it stumbled.

## Reads and locking

Reads load the persisted graph from the out directory (see
[storage layout](../compiler/graph.md#storage-layout)). The server never compiles on
its own. If no graph exists yet, the compile toolset offers the queue (a first build
is just a queue where every document is dirty); the other toolsets answer with
guidance to compile first.

Readers do not lock. They read the generation counter, load shards, and retry if the
counter moved mid-read. Writers respect the store lock, so one changeset commits at a
time even with a compile running next to the server. See
[concurrency](../compiler/graph.md#concurrency).
