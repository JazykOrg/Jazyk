# Turns

A turn is one focused agent session with tools. It is the only place a model touches the
compilation process. The [reconciler](./reconciler.md) decides what turns to run; a turn
executes as an [ACP worker session](../frontends/acp.md#worker-sessions) against the
configured [agent](../frontends/acp.md#agents). Jazyk owns the work item, the tools, the
gates, and the commit; the agent owns the model and the loop that drives it.

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
  [incoming links](#incoming-links). Extraction records the facts as stated, even when
  sections disagree: judging contradictions belongs to the review tasks that follow,
  and `report_diagnostic` is not in this task's toolset. A conflict noticed here is
  not lost; the review wave sees the statements side by side.
- `review-requirement`: judge one changed statement against its computed neighbors.
  The [reconciler](./reconciler.md#waves) picks the neighbors; the turn only judges.
  The pack shows the changed requirement (`ears`, `quote`, source section) and each
  neighbor side by side, plus any open diagnostic already tying a pair. The model gives
  one verdict per neighbor:
  - duplicate: the same obligation reworded. Delete the worse-sourced one, or report a
    `duplicate-requirement` info diagnostic when the restatement is intentional
    cross-document redundancy (both kept).
  - contradiction: the two statements cannot both hold. Report a `contradiction`
    diagnostic naming both, and when the repair is enumerable, attach a
    [prompt](./model/diagnostic.md#prompts): one suggested edit per side (rewrite
    the other document's sentence to agree), freeform allowed. The question reaches
    the owner inline in the file and in chat; the answer resolves the conflict
    without a fresh investigation.
  - consistent: no action.
  It also resolves a pair diagnostic whose condition no longer holds. An open
  diagnostic naming a requirement that was deleted shows the dead subject marked
  `(deleted)`: the turn resolves it when the finding died with that requirement, or
  refiles it against the surviving statements when it still stands. A verdict is owed
  only for the pairs shown, but a contradiction or duplicate found against a
  statement the pack did not pair is filed with `report_diagnostic` all the same,
  provided the evidence is in quotes the turn has read. Focused pairwise
  judgment is deliberate: weak models answer "can these two both hold?" far more
  reliably than "is this entity coherent?".
- `review-entity`: judge one entity whose facts changed. The model checks that the
  requirements form a coherent whole, refreshes the `definition`, merges lookalike
  duplicates, deletes same-document duplicate requirements (keeping the
  better-sourced one; duplicates quoting different documents are intentional
  redundancy, kept with a `duplicate-requirement` info diagnostic, the same policy
  the pairwise review applies), and reports
  [diagnostics](./model/diagnostic.md). The pack includes the
  entity, its requirements across all documents, lookalike candidates, and requirements
  whose statement names the entity without referencing it: candidates for a missing
  reference the review adds with `update_requirement`. The candidates are word
  matches, not judgments; leaving one alone is a correct outcome. A missing reference
  is what strands an entity cluster unreachable from the roots.

Both review kinds share one error asymmetry: a wrong delete or merge destroys
information, while a missed duplicate only leaves a finding for the next build. When
in doubt, keep both nodes and file a diagnostic instead of mutating.

- `generate-entity`: produce one entity's part of the deliverable and its tests. See
  [generation turns](#generation-turns).

Extraction order inside `reconcile-doc` is deliberate: requirements first, entities only
as requirements need them. An entity that no statement needs is noise. See
[entity](./model/entity.md#what-is-an-entity).

A turn's consumer is interchangeable: the same work item, pack, and toolset serve an
[ACP worker session](../frontends/acp.md#worker-sessions) and an external agent that
connects over [MCP](../frontends/mcp.md#compilation-over-mcp) on its own. The pack is
the prompt in both cases; the task contract rides in the `begin_*` package as
`instructions`, so the prompt has one source whoever consumes it.

## Generation turns

The `generate-entity` turn replaces the fixed file-reply pipeline: the model works the
task with tools instead of answering a fixed sequence of prompts. Its toolset adds
file and command tools, sandboxed to the deliverable directory. They are served into
the session only when the agent's profile sets
[`serve_files`](./project-settings.md#acp): a coding agent brings its own editor and
shell (see [MCP](../frontends/mcp.md#generation-and-verification-over-mcp)); the
[embedded agent](../frontends/acp.md#the-embedded-agent) has none, so jazyk serves
these:

- `read_text_file({path, line?, limit?})`: one file's content, path relative to the
  deliverable.
- `write_text_file({path, content})`: write one file. A path recorded for another
  entity is rejected with the owner named
  ([file ownership](../consumers/gen.md#file-ownership-and-conventions)).
- `list_files({path?})`: the deliverable tree.
- `run_command({command, cwd?})`: execute a shell command under the deliverable,
  bounded by a timeout; the exit code and output tail come back. This is how the turn
  runs the build it wrote, reads the traceback, and fixes its own work.
- `run_tests({requirements?})` and `record_generation({...})`: the same tools the
  [generation toolset](./tools.md#generation-tools) serves over MCP.

The names and shapes track the Agent Client Protocol's file-system and terminal
methods. That protocol removes those methods in its next version in favor of
client-provided tools, which is exactly this serving: the tools ride the injected
MCP server like every other jazyk tool
([MCP into sessions](../frontends/mcp.md#mcp-into-acp-sessions)).

The finish contract: `record_generation` records the manifest, then `done` ends the
turn. A turn that ends without recording fails the task; the harness checks the
ledger, not the model's word. Command execution during generation is the same trust
decision as `jazyk test` running recorded commands, made at generation time; the
sandbox is the deliverable directory, and paths that escape it are rejected.

## Incoming links

A document set describes one subject by splitting it across files. A parent lists its
parts and links to a file per part; the file details the part. The link is what says
which entity the file is about, and the parent's turn has already recorded it: the list
item yielded a requirement, and that requirement introduced the part's entity.

So the `reconcile-doc` pack names every incoming link the graph already resolved:

```
## Linked from
- docs/slides.md#/slides "[Introduction](./slide-intro.md)" introduced ent:introduction (Introduction)

primarySubject: ent:introduction (Introduction)
```

The pack always resolves the subject question, so the turn never guesses what "the
system" means:

- One introduced entity: `primarySubject: ent:introduction`. The turn reads "the
  system", "this", and "it" as that entity, and its requirements reference it
  instead of minting a second one for the same concept.
- Several introduced entities: the pack lists them as `candidateSubjects` and the
  statement's own section decides which one it constrains. "The system" in a detail
  document still means the part being detailed, never the containing application. Without it, a file linked as a part yields requirements
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
question it already asked is stuck, and the tool serving says so rather than letting
the turn spend its budget on it:

- The second identical call answers as usual, with a `repeat` field on the result saying
  the answer is unchanged and to act on it.
- The third is refused with a `repeated-call` error naming the tool and the way forward:
  act on the answer already given, or finish with `done`.

Identity is the tool name plus its arguments verbatim, counted per open task. `done` is
exempt; repairing a rejected `done` legitimately repeats it. The contract lives in the
serving, not in any agent, so every agent gets the same guard.

A refusal is not an invalid call: the call was well-formed, the model is stuck. It
never feeds the abort streak, because aborting discards staged work and a stuck model
usually holds good extractions from before it stuck. Instead, refusals are cheap
(refused calls never dispatch) and counted: past eight in one task, the serving
finishes the turn implicitly, committing the staged work under the same gates the
budget path uses. A weak model that loops keeps what it earned; the guard only stops
it paying for the loop.

## Execution

A turn runs as one [ACP worker session](../frontends/acp.md#worker-sessions):

- The session's tools are one injected MCP serving, scoped to the task
  ([MCP into sessions](../frontends/mcp.md#mcp-into-acp-sessions)).
- The prompt jazyk sends is fixed and agent-neutral: begin the named task, follow the
  returned package, finish, repair what a rejection names. The task's own contract
  (the `instructions`) and the rendered [context pack](./context.md) ride in the
  `begin_*` reply.
- The instructions state the task, the graph invariants, and the finish contract: the
  turn ends by calling `done`, the same gate everywhere it is served.
- High in every task's instructions sits the feedback contract: an instruction, a
  tool, an argument, or an error message that is ambiguous, wrong, or confusing goes
  to [`report_feedback`](./tools.md#feedback-tool), and the turn then continues with
  its best judgment. The note is one paragraph, shared by every task type, and says
  what feedback is not: a problem in the documents is a diagnostic, not feedback.
- Read tools answer immediately. Write tools stage mutations. How the agent drives
  its model between calls is the agent's business.

The message loop, the tool codecs (`native` and `text`), the first-round probe, and
the endpoint fallbacks belong to the
[embedded agent](../frontends/acp.md#the-embedded-agent): they are how a generic loop
speaks to a raw endpoint, not part of turn semantics. An external agent brings its
own loop. The [benchmark](../benchmark/benchmark.md) grades an agent profile by
running turns through it.

## Staged mutations

Write tools never touch the store directly. They stage mutations into the turn's
changeset. Each call is validated the moment it is staged, against the store plus what is
already staged, and invalid calls are rejected with a repair message. See
[validation gates](./graph.md#validation-gates).

Read tools show the snapshot the turn began with, not the staged mutations. While
mutations are staged, every read reply carries a note saying so, so a turn that reads
back a node it just staged a delete for does not conclude the delete was lost.

A turn that fails (the agent's session ends without landing its task, or is cancelled
by the idle timeout) is retried once with a fresh session, then parked with an
`incomplete-build` diagnostic. The [embedded agent](../frontends/acp.md#the-embedded-agent)
additionally aborts its own loop after three consecutive invalid rounds, so a stuck
weak model fails fast instead of spending the budget.

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

- Staged mutations per turn: default 64. Enforced by the tool serving.
- Context budget: per model profile, e.g. 24k characters for a 4B class model.
- Rounds per turn: default 24, raised for dense work items: a turn gets at least 8
  rounds per dirty section. This bounds the embedded agent's loop; an external agent
  bounds its own, and the
  [idle timeout](../frontends/acp.md#worker-sessions) bounds them all.
- Per build: a hard turn cap, so a stuck build stops instead of looping. See
  [convergence](./reconciler.md#convergence).

An agent whose session ends with mutations staged and no `done` is treated as having
called it: the serving runs the same commit gates, and a clean batch commits. Weak
models forget the finish contract more often than they stage bad work; discarding a
valid changeset over a missing `done` would punish the wrong thing. A turn with
nothing staged parks as usual.

The same reasoning bounds what one bad claim can sink. When the implicit `done` is
rejected over a dishonest `covered` claim, the harness drops the offending coverage
marks and commits the rest: the extracted requirements land, the miscovered sections
stay unprocessed, and the next build resumes them. Only the explicit `done` holds the
model to repairing its own claims.

An untouched stale anchor is different: the turn parks and stages nothing. Stale
anchors are a contract only the model can honor, and the harness never commits around
one. The next build lists the anchor again.

## Trace events

The runner translates a session's update stream into structured events: the tool call
with condensed arguments, the condensed result, and any message or thought text the
agent produced, emitted as model text. The `compile` command
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
- `llmRequest`, `llmResponse`, `llmRetry`: one model call, recorded by the
  [embedded agent's](../frontends/acp.md#the-embedded-agent) endpoint client. The
  request carries the whole outgoing message list (system prompt, context pack, and
  the conversation so far) plus the tool names offered; the response carries the raw
  assistant message, the elapsed milliseconds, and the completion tokens; a retry
  carries the attempt, the error, and how long the client waits before trying again.
  Sticky fallbacks (codec downgrade, streaming, dropped `temperature`) are notes on
  the same label, so a run's whole conversation with the endpoint is in one place. A
  session against an external agent carries none of these: that agent's model
  traffic lives in its own logs.
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
