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
  requirements already sourced from each dirty section (so an unchanged statement is a
  no-op, not a re-extraction, and a coverage claim sees what the section already
  yielded), the known entities of the document's neighborhood, coverage states, and
  stale anchors. Stale anchors are a contract, with three outcomes: the fact still
  stands and the turn re-records it with a fresh verbatim quote (the natural key
  resolves to the anchor and updates it in place), the fact changed and the turn
  revises it with `update_requirement` carrying the new `ears` plus the new `quote`,
  or the fact is gone and the turn deletes it. The `done` gate rejects a turn that
  leaves a stale anchor untouched. The pack also carries the document's
  [incoming links](#incoming-links).
- `review-requirement`: judge one changed statement against its computed neighbors.
  The [reconciler](./reconciler.md#waves) picks the neighbors; the turn only judges.
  The pack shows the changed requirement (`ears`, `quote`, source section) and each
  neighbor side by side, plus any open diagnostic already tying a pair. The model gives
  one verdict per neighbor:
  - duplicate: the same obligation reworded. Delete the worse-sourced one, or report a
    `duplicate-requirement` info diagnostic when the restatement is intentional
    cross-document redundancy (both kept).
  - contradiction: the two statements cannot both hold. Report a `contradiction`
    diagnostic naming both.
  - consistent: no action.
  It also resolves a pair diagnostic whose condition no longer holds. Focused pairwise
  judgment is deliberate: weak models answer "can these two both hold?" far more
  reliably than "is this entity coherent?".
- `review-entity`: judge one entity whose facts changed. The model checks that the
  requirements form a coherent whole, refreshes the `definition`, merges lookalike
  duplicates, deletes requirements that restate a fact another requirement on the
  entity already carries (keeping the better-sourced one), and reports
  [diagnostics](./model/diagnostic.md). The pack includes the
  entity, its requirements across all documents, lookalike candidates, and requirements
  whose statement names the entity without referencing it: candidates for a missing
  reference the review adds with `update_requirement`. A missing reference is what
  strands an entity cluster unreachable from the roots.

Extraction order inside `reconcile-doc` is deliberate: requirements first, entities only
as requirements need them. An entity that no statement needs is noise. See
[entity](./model/entity.md#what-is-an-entity).

## Incoming links

A document set describes one subject by splitting it across files. A parent lists its
parts and links to a file per part; the file details the part. The link is what says
which entity the file is about, and the parent's turn has already recorded it: the list
item yielded a requirement, and that requirement introduced the part's entity.

So the `reconcile-doc` pack names every incoming link the graph already resolved:

```
## Linked from
- docs/slides.md#/slides "[Introduction](./slide-intro.md)" introduced ent:introduction (Introduction)
```

The turn reads that as the document's subject: statements in this document are about
`ent:introduction`, and its requirements reference that entity instead of minting a
second one for the same concept. Without it, a file linked as a part yields requirements
tied to nothing the parent knows, the part's entity keeps only the parent's one-line
mention, and [generation](../consumers/gen.md) sees an entity with a name and no
content.

A link is resolved by locating the target document in the verbatim quote of an existing
entity mention or requirement source, so the binding is deterministic and needs no model
judgment. Links the graph has not yet resolved are not listed: the
[level order](./reconciler.md#scheduling) runs a parent before the documents it links
to, so by the time the part's turn runs, the parent's requirement exists.

## Repeated calls

The same call with the same arguments has the same answer. A model that re-asks a
question it already asked is stuck, and the harness says so rather than letting the turn
spend its budget on it:

- The second identical call answers as usual, with a `repeat` field on the result saying
  the answer is unchanged and to act on it.
- The third is refused with a `repeated-call` error naming the tool and the way forward:
  record what the section states, mark its coverage, move on.

Identity is the tool name plus its arguments verbatim, counted per turn. `done` is
exempt; repairing a rejected `done` legitimately repeats it.

A refusal counts toward the invalid streak, so a model that keeps looping ends the turn
after three rounds instead of after the whole round budget, and the work it staged
before getting stuck still commits. The guard is what keeps one unanswerable question
from costing a document its later sections.

## Message loop

- The system message states the task, the graph invariants, and the finish contract: the
  turn ends by calling `done`.
- Directly under the role line, high in every turn's system message, sits the feedback
  contract: an instruction, a tool, an argument, or an error message that is ambiguous,
  wrong, or confusing goes to [`report_feedback`](./tools.md#feedback-tool), and the
  turn then continues with its best judgment. The note is one paragraph, shared by
  every task type, and says what feedback is not: a problem in the documents is a
  diagnostic, not feedback.
- The first user message is the rendered context pack.
- Each model reply is either tool calls or text. Read tools answer immediately. Write
  tools stage mutations. Results go back as tool results.
- The transcript is append-only.
- A reasoning model's reasoning rides on the assistant message: a `reasoning_content`
  or `reasoning` field, or inline `<think>` text in the content. The harness appends
  the message unchanged, so later rounds see the reasoning behind earlier calls. A
  streamed response accumulates its reasoning deltas into the same field before the
  message is appended, so streaming and non-streaming replies carry the same fields.
  Some providers reject reasoning fields echoed back in the request; the client then
  strips them from outgoing messages for the rest of the run (see
  [LLM settings](./project-settings.md#llm)), and the transcript and trace
  keep the text.

## Codecs

The loop speaks to the model through a codec. Two codecs exist:

- `native`: OpenAI-style `tools` and `tool_calls`. Used when the endpoint and model
  support it. The codec asks the model to batch one section's calls (searches, upserts,
  the coverage mark) into a single reply.
- `text`: tools are described in the system prompt. The model answers with exactly one
  JSON action object per reply, e.g. `{"tool": "upsert_entity", "args": {...}}`. Results
  come back as a plain message. One action per reply is deliberate: small models cannot
  reliably emit several.

Pacing guidance is the codec's to give, not the shared system prompt's: the two codecs
contradict each other on batching, so the instruction ships in the codec's own
system-prompt section.

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

An explicit `done` finishes the coverage contract: every dirty section carries a mark,
staged in this turn or already recorded. A turn that extracts requirements from a
section and never marks it is sent back to set the mark, not committed around. The
implicit path is exempt (see [budgets](#budgets)): it commits the staged work and the
unmarked sections stay unprocessed for the next build.

## Budgets

- Rounds per turn: default 24, raised for dense work items: a turn gets at least 8
  rounds per dirty section. A dense document stages one mutation per round under a
  model that calls one tool at a time, so the budget scales with extraction density, not
  caution. A model may batch several tool calls in one reply; each reply is one round.
- Staged mutations per turn: default 64.
- Context budget: per model profile, e.g. 24k characters for a 4B class model.
- Per build: a hard turn cap, so a stuck build stops instead of looping. See
  [convergence](./reconciler.md#convergence).

A model that stops replying with tool calls while mutations are staged is treated as
having called `done`: the same commit gates run, and a clean batch commits. Weak models
forget the finish contract more often than they stage bad work; discarding a valid
changeset over a missing `done` would punish the wrong thing. A turn with nothing staged
parks as usual.

The same reasoning bounds what one bad claim can sink. When the implicit `done` is
rejected over a dishonest `covered` claim, the harness drops the offending coverage
marks and commits the rest: the extracted requirements land, the miscovered sections
stay unprocessed, and the next build resumes them. Only the explicit `done` holds the
model to repairing its own claims.

An untouched stale anchor is different: the turn parks and stages nothing. Stale
anchors are a contract only the model can honor, and the harness never commits around
one. The next build lists the anchor again.

## Trace events

The harness emits a structured event per round: the tool call with condensed arguments,
the condensed result, and any reasoning text the model produced. Reasoning carried in a
`reasoning_content` or `reasoning` field is emitted as model text, the same as reasoning
prose in the content. The `compile` command
renders these live, and the [GUI](../frontends/gui.md) streams them to the browser. The
[generation](../consumers/gen.md) and verification workers emit their own kinds per
entity and per ledger row. The committed changeset with the same information persists
in the [journal](./graph.md#journal).

Every event carries a `label`: the work it belongs to (`reconcile-doc docs/main.md`,
`review-entity ent:cart`, `gen ent:cart`, `verify req:...`). The label is the grouping
key, so a reader can reassemble one turn from an interleaved parallel run. A `step`
names the position inside the label (`r3` for a turn round, `product 1/2` for a
generation part).

The event kinds:

- `turnStart`, `turnDone`, `turnFailed`: the turn lifecycle. `turnStart` carries where
  the turn is working: `task`, `target`, the `doc` when the task is `reconcile-doc`,
  the dirty `sections` it must process, and the stale anchor count.
- `toolCall`, `toolResult`, `toolError`: one row per tool call, condensed.
- `modelText`: prose or reasoning the model produced.
- `section`: the turn moved to a section. Emitted when an accepted tool call names a
  section (`set_coverage`, `upsert_requirement`, an entity mention, `read_section`)
  that differs from the last one, so the sequence of these events is the turn's path
  through the document. Carries `doc`, `section`, and the `tool` that named it.
- `llmRequest`, `llmResponse`, `llmRetry`: one model call. The request carries the
  whole outgoing message list (system prompt, context pack, and the conversation so
  far) plus the tool names offered; the response carries the raw assistant message,
  the elapsed milliseconds, and the completion tokens; a retry carries the attempt,
  the error, and how long the harness waits before trying again. Sticky fallbacks
  (codec downgrade, streaming, dropped `temperature`) are notes on the same label, so
  a run's whole conversation with the endpoint is in one place.
- `note`: a plain line. Verbose notes carry the full context pack and raw payloads.
- `waveStart`: the reconciler is about to run a wave. Carries the wave number, the
  `task` its items share (`reconcile-doc`, `review-entity`, `review-requirement`),
  and their targets, so a reader sees what is queued before any turn starts.
- `genEntityStart`, `genEntitySkipped`, `genEntityDone`, `genEntityFailed`;
  `verifyRowStart`, `verifyRowDone`, `verifyRowStale`, `verifyRowError`: the worker
  kinds.

Payloads are recorded in full and shipped condensed. A transcript keeps every prompt
and reply as it was sent (capped per message, so one runaway payload cannot fill the
disk); the live stream and the transcript listing carry the same events with long
strings elided to a preview plus a byte count. A reader that wants the whole payload
asks for that one event (see [GUI jobs](../frontends/gui.md#jobs)). Terminal rendering
follows the [trace level](../frontends/cli.md#jazyk-compile): model calls print one
timing line at `--verbose` and nothing at the default level, where the tool rows
already say what happened.
