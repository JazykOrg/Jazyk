# MCP

`jazyk mcp <toolsets>` serves the [tool registry](../compiler/tools.md) over stdio as an
MCP server: line-delimited JSON-RPC per the Model Context Protocol. It dispatches the
same tool implementations the compiler's own [turns](../compiler/turns.md) use. There is
one registry, not a second API beside it.

## Toolsets

The argument names what the serving is for. One serving can carry several, comma
separated; each adds its tools to the union:

- `jazyk mcp compile`: the [compilation lifecycle](../compiler/tools.md#compilation-tools)
  plus the [write tools](../compiler/tools.md#write-tools). The agent performs
  compilation tasks from [the task queue](../compiler/reconciler.md#the-task-queue).
- `jazyk mcp generate`: the [generation tools](../compiler/tools.md#generation-tools).
  The agent writes the deliverable with its own editor and shell; jazyk holds the
  ledger.
- `jazyk mcp verify`: the [verification tools](../compiler/tools.md#verification-tools).
  Small enough to hand a subagent whose only job is judging one row.
- `jazyk mcp graph`: the read tools only, for retrieval consumers. `--write` adds the
  raw write tools without the task lifecycle, for debugging and manual graph surgery;
  each write commits as its own changeset.

Every serving includes the [read tools](../compiler/tools.md#read-tools),
[`report_feedback`](../compiler/tools.md#feedback-tool), and `await_changes` (below).

The `initialize` reply carries server instructions describing the work loop for the
toolsets served, so an agent that reads nothing else still knows the entry point, the
order of calls, and that a tool error names its own repair. The prompting lives in
three places, most specific wins: server instructions carry the loop, each
`begin_*` package carries the task's own contract, and each tool description names
what to call next.

## The work loop

The agent-facing loop, common to all three toolsets:

1. Ask for work: `compilation_tasks`, `generation_tasks`, or `verification_tasks`.
   Zero tasks is an answer, not an error; the compilation list carries the build
   verdict when the queue is empty.
2. Begin the first ready task: `begin_compilation`, `begin_generation`,
   `begin_verification`. The reply is the task package: instructions plus everything
   the task needs.
3. Do the work: compilation stages graph writes, generation edits deliverable files
   with the agent's own tools, verification runs or judges tests.
4. Finish: `finish_compilation` commits the changeset, `record_generation` records the
   manifest, `run_tests` and `record_verdict` record verdicts. The reply names the
   next ready task, so the loop chains without re-listing.

To watch for new work instead of polling, either run
[`jazyk monitor`](./cli.md#jazyk-monitor) as a background process and act on each
notice it prints, or call `await_changes`:

- `await_changes({timeout_seconds?})`: a long poll. It returns when the graph's
  generation counter moves, a documentation file changes on disk, a manifest or test
  file in the deliverable changes, or the ledger changes, or at the timeout (default
  300 seconds). The reply carries the changed documents, whether the graph is stale
  (documents changed but not yet reconciled), the pending generation work, and the
  pending verification work grouped by reason.

## Compilation over MCP

Compilation tasks mutate the graph, so the serving holds an open changeset between
calls, exactly one at a time:

- `begin_compilation` claims a task from the queue, reloads the store, syncs the
  section trees in memory, and opens a changeset. The write tools stage into it,
  validated by the same [gates](../compiler/graph.md#validation-gates) a compilation
  turn faces, scoped to the task's document the same way.
- Write tools outside an open task are rejected toward `begin_compilation`. Identity,
  provenance, and scope rules hold because they are the same code path.
- `finish_compilation` runs the `done` gates (coverage contract, stale anchors),
  commits atomically, and updates [the task queue](../compiler/reconciler.md#the-task-queue).
  A gate failure leaves the changeset open and names the repair. `beginNext: true`
  claims the next ready task in the same call and carries its package in the reply,
  saving a round trip per task; the default reply only names the next task.
- `abandon_compilation` drops the staged work. An abandoned task leaves no trace, the
  same contract as an aborted turn. A server that dies mid-task loses only staging;
  any process recomputes the queue and the task reappears.
- When a commit empties the compile queue, the serving runs the deterministic tail
  itself (checks, docsgen, verdict), because none of it needs a model. The final
  `finish_compilation` reply carries the verdict and the generation tasks that became
  ready.

## Generation and verification over MCP

Generation is stateless on the server: `begin_generation` returns the package, the
agent edits files and runs commands with its own tools, `record_generation` records
the manifest. Jazyk deliberately serves no file-editing tools here; a coding agent
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
finish_compilation → {committed: true, next: {kind: review-entity, target: ent:order}}
... reviews the same way; last finish → {verdict: converged, generation: [ent:order]}
begin_generation {entity: ent:order} → package
(agent edits src/order.rs, tests/order.rs with its own tools)
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
