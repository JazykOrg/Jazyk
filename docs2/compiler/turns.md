# Turns

A turn is one focused LLM session with tools. It is the only place the model touches the
compilation process. The [reconciler](./reconciler.md) decides what turns to run; the turn
harness runs one.

## Anatomy

A turn is given:

- a work item: the task and its target, e.g. reconcile document `docs/cli.md`,
- an initial [context pack](./context.md) for that target,
- a task-scoped subset of the [tool registry](./tools.md#task-toolsets),
- budgets: maximum rounds, maximum staged mutations, context size.

A turn produces either a committed [changeset](./graph.md#changesets) or a parked work
item. Nothing in between. An aborted turn leaves no trace in the graph.

## Task types

- `reconcile-doc`: bring the graph in line with one document's dirty sections. The model
  reads the sections, extracts requirements and the entities they need, updates what
  drifted, and marks sections covered. The pack includes the dirty section bodies, the
  known entities of the document's neighborhood, coverage states, and stale anchors.
- `review-entity`: judge one entity whose facts changed. The model checks that the
  requirements form a coherent whole, refreshes the `definition`, merges lookalike
  duplicates, and reports [diagnostics](./model/diagnostic.md). The pack includes the
  entity, its requirements across all documents, and lookalike candidates.

Extraction order inside `reconcile-doc` is deliberate: requirements first, entities only
as requirements need them. An entity that no statement needs is noise. See
[entity](./model/entity.md#what-is-an-entity).

## Message loop

- The system message states the task, the graph invariants, and the finish contract: the
  turn ends by calling `done`.
- The first user message is the rendered context pack.
- Each model reply is either tool calls or text. Read tools answer immediately. Write
  tools stage mutations. Results go back as tool results.
- The transcript is append-only.

## Codecs

The loop speaks to the model through a codec. Two codecs exist:

- `native`: OpenAI-style `tools` and `tool_calls`. Used when the endpoint and model
  support it.
- `text`: tools are described in the system prompt. The model answers with exactly one
  JSON action object per reply, e.g. `{"tool": "upsert_entity", "args": {...}}`. Results
  come back as a plain message. One action per reply is deliberate: small models cannot
  reliably emit several.

The harness probes on the first round. If the endpoint rejects the `tools` parameter or
the model answers prose without tool calls, the run downgrades to `text` and stays there.
The [benchmark](../benchmark/benchmark.md) grades a model under both codecs.

## Staged mutations

Write tools never touch the store directly. They stage mutations into the turn's
changeset. Each call is validated the moment it is staged, against the store plus what is
already staged, and invalid calls are rejected with a repair message. See
[validation gates](./graph.md#validation-gates).

Three consecutive invalid rounds abort the turn. The work item is retried once with fresh
context, then parked with an `incomplete-build` diagnostic.

## Commit

Calling `done` triggers batch-level checks. Failures give the model up to two repair
rounds. A clean batch commits atomically. A batch that cannot be repaired parks the work
item. See [changesets](./graph.md#changesets).

## Budgets

- Rounds per turn: default 12.
- Staged mutations per turn: default 64.
- Context budget: per model profile, e.g. 24k characters for a 4B class model.
- Per build: a hard turn cap, so a stuck build stops instead of looping. See
  [convergence](./reconciler.md#convergence).

A model that stops replying with tool calls while mutations are staged is treated as
having called `done`: the same commit gates run, and a clean batch commits. Weak models
forget the finish contract more often than they stage bad work; discarding a valid
changeset over a missing `done` would punish the wrong thing. A turn with nothing staged
parks as usual.

## Trace events

The harness emits a structured event per round: the tool call with condensed arguments,
the condensed result, and any reasoning text the model produced. The `compile` command
renders these live. Verbose mode includes the full context pack and raw payloads. The
committed changeset with the same information persists in the [journal](./graph.md#journal).
