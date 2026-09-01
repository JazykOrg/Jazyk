# Sessions

A session is one focused agent run over one goal batch. It is the only place a model
touches the compilation process. The [scheduler](./compiler.md#the-scheduler) forms the
batch from the [board](./reconciler.md#goal-derivation); the session executes as an
[ACP worker session](../frontends/acp.md#worker-sessions) against the
[executor](../frontends/acp.md#executors) the batch resolves to. Jazyk owns the batch,
the prompt, the tools, the gates, and the commit; the agent owns the model and the loop
that drives it.

One agent, many goal kinds. Variety lives in the goal kinds, which are data: a contract
paragraph, a gate, hints, a skill. The session prompt is one fixed contract, and
everything kind-specific arrives inside it as data. The model never creates, routes, or
prioritizes goals. It resolves them, fails them, or leaves them for the next session.

## Anatomy

A session is given:

- a batch: one to a handful of goals of one class and one readiness tier, grouped by
  locality ([batching](./reconciler.md#batching)). The batch id `b<generation>-<n>`
  (the generation the board derives from and the batch's index in it) names the
  session, its lease, its trace file, and the `{target}` of the worker protocol line.
- the assembled [prompt](#the-prompt): the agent contract, the active
  [skills](#skills), the project block, one block per goal, and the initially
  [loaded set](./context.md#the-loaded-set) for the batch's locality.
- a [toolset](#toolsets): the union of what the batch's goal kinds need.
- [budgets](#budgets): rounds, staged mutations, context size.

A session starts with fresh context. Nothing carries over from an earlier session except
the graph itself, so a retry is a clean run, and what the model saw is exactly what the
prompt and the tool replies rendered ([preview](#preview)).

A session produces either a committed [changeset](./graph.md#changesets) with each goal
of the batch resolved, failed, or left open, or a failed session that leaves no trace in
the graph. Nothing in between. A goal left open after its retry parks
([resolving, failing, parking](#resolving-failing-parking)).

## The prompt

The prompt is assembled, never authored per goal. The same disk state renders the same
prompt, so it can be shown before it is spent. The parts, in order:

1. The agent contract, [`goals/prompts/agent-contract.md`](./goals/prompts/agent-contract.md),
   with [`feedback-note.md`](./goals/prompts/feedback-note.md) spliced in as its second
   paragraph. The contract states the job (resolve the listed goals with the tools over
   the loaded graph, finish with `done`), the graph invariants, the loading rules, the
   staging rules, the review asymmetry, and the finish contract. The feedback note says
   that a confusing instruction, tool, argument, or error message goes to
   [`report_feedback`](./tools.md#feedback-tool) and the session continues on best
   judgment; a problem in the documents is a diagnostic, not feedback.
2. The active skills, each as `[skill: <name> (active)]` followed by its payload.
3. `## Project`: the generation number, the workflow mode, the open
   diagnostic counts, and the board counts: goals in this session, goals elsewhere,
   goals blocked on a human.
4. `## Goals`: one block per goal in the batch: the line `- [g:...] mandatory|optional`,
   the kind's contract paragraph (its payload file under `goals/prompts/`), the change in
   one line, the gate in one line, and the hints.
5. `## Loaded (x/y chars)`: the [status block](./context.md#rendering) of the loaded set,
   then the skill index line.
6. The [`worker-protocol.md`](./goals/prompts/worker-protocol.md) line, with `{target}`
   set to the batch id.

E.g., a tier 2 batch after a session revised `req:orders-6` (contract paragraphs
shortened here; the real ones are the payload files):

```text
[agent contract, fixed; the feedback note is its second paragraph]

[skill: judgment (active)]
[the judgment skill payload]

## Project
- generation 413, manual mode
- diagnostics: 1 error (contradiction diag:contradiction-3), 4 warnings
- board: 2 goals in this session; 19 elsewhere; 3 blocked on human answers

## Goals
- [g:rejudge-pair:req:orders-6~req:payment-9] mandatory
  This goal judges one pair: a revised requirement against one neighbor that overlaps
  it. Decide exactly one verdict: duplicate, contradiction, or consistent. Ground it in
  the quotes. A wrong delete destroys information; when in doubt, keep both and file.
  Change: req:orders-6 revised (g413: statement, transition guard).
  Gate: a verdict for the pair in evidence; for an acting verdict, the mutation or
  diagnostic that carried it.
  Hints: load req:orders-6; load req:payment-9; skill judgment.
- [g:review-entity:ent:order] mandatory
  This goal judges one entity whose fact set changed. Read its requirements as a whole,
  refresh the definition when it drifted, judge every listed lookalike, file or resolve
  diagnostics. When everything is coherent, mark the goal done with no mutation.
  Change: entity-changed (g413 via requirements: req:orders-6).
  Gate: definition current; lookalikes judged; diagnostics filed or resolved.
  Hints: load ent:order; skill judgment.

## Loaded (9.8k/24k chars)
- req:orders-6    full: statement, transition placed → expired (guard 30 days), source docs/orders.md#/orders/holds
- req:payment-9   full: statement, source docs/payment.md#/payment/declines
- ent:order       full: 7 requirements, parent ent:order-service   [3 more edges: h:ent:order:related]
- ent:order-service   stub (definition only)   [5 edges loadable: h:ent:order-service]
Skills: judgment (active, 6.4k); extraction, flow-views, structural-views, abstraction, conformance (load_skill)

PROTOCOL: the `jazyk` tools carry this batch, b413-3. First call `begin_goals` with {"batch": "b413-3"} ...
```

Contract paragraphs are short and imperative: what the goal means, what evidence the
gate wants, what not to do, and that justifications and failure reasons are one or two
sentences, never essays. Each goal kind's page under [`goals/`](./goals/reconcile-section.md)
shows its block. The hints are computed and honest (what to load, which skill explains
the shape, which tool usually resolves the kind); the gate is the truth.

The `## Loaded` block is the model's working set and the harness keeps it current: it
re-renders condensed on every mutating tool reply and in full on `graph_status`
([context](./context.md#the-loaded-set)). Over ACP the whole prompt travels as the
session prompt; over plain MCP `begin_goals` carries the same assembly as
`instructions` and the loaded set as `package`, eliding the contract and the skills a
client already received in the same serving
([compilation over MCP](../frontends/mcp.md#compilation-over-mcp)). The prompt has one
source whoever consumes it.

## Skills

A skill is a prompt payload with the working knowledge for one shape of work: how flow
views order their members, what a good abstraction split looks like, how a duplicate
differs from a related concept. The contract paragraph says what to do for a goal; the
skill says how. Skills are files under `docs/compiler/skills/`, embedded into the binary
at compile time and excluded from the docs glob like the goal contracts:
[`extraction`](./skills/extraction.md) (statements, entities, edges, transitions,
attributes, coverage honesty), [`judgment`](./skills/judgment.md) (the review asymmetry,
duplicates, contradictions, verdict quality), [`flow-views`](./skills/flow-views.md)
(ordering, branches, participants), [`structural-views`](./skills/structural-views.md)
(class, component, and package membership, collapse, limits),
[`abstraction`](./skills/abstraction.md) (splitting entities and views, containment,
docs proposals), [`conformance`](./skills/conformance.md) (instances, attributes,
links). Skill text is medium-neutral: the model adapts its wording to the medium it
reads, as it does everywhere else.

The lifecycle inside a session:

- From the first round: the skills of the batch's goal kinds (the union, from the
  [goal catalog](./reconciler.md#the-catalog)) render in the prompt as active. A goal
  kind's skill stays active while a goal of that kind is open in the batch.
- Auto-load: the first node of a kind loaded in the session brings the kind's skill,
  once. A section brings `extraction`; a flow view (`use-case`, `activity`, `sequence`,
  `communication`, `timing`, `overview`) brings `flow-views`; a structural view
  (`class`, `object`, `package`, `component`, `composite`, `deployment`, `state`)
  brings `structural-views`. Entities and requirements bring no skill by themselves;
  `judgment`, `abstraction`, and `conformance` come from the goal kind or by name.
- By name: `load_skill({name})` renders the payload in its reply and marks the skill
  active. The index line under `## Loaded` lists every skill: active ones with their
  size, the rest as loadable.
- Cap: at most four skills render in one session (the registry constant
  `skills-per-session`, [budgets](#budgets)). The cap counts the skills rendered this
  session, active or inactive, because rendered text stays in the conversation. An
  auto-load past the cap does not fire and the index line says `(cap reached)`; a
  `load_skill` past the cap is refused naming the cap and the skills already rendered.
- Budget: a skill counts against the [context budget](#budgets) from the moment it
  renders until the session ends. It renders once; a later reference to it re-renders
  nothing.
- Inactive: unloading the last loaded node of a kind marks the kind's skill inactive.
  The text already in context stands, the index line shows `(inactive)`, and the skill
  keeps its chars and its cap slot. Loading a node of that kind again re-activates the
  skill without re-rendering it. A goal kind's skill never goes inactive while its goal
  is open.

## Toolsets

The session's toolset is the union of what the batch's goal kinds need, computed by the
harness ([toolsets](./tools.md#toolsets)), so a batch of extraction goals still sees a
small toolset. Every session sees, on top of its kinds' slices:

- the [read tools](./tools.md#read-tools): `load`, `expand`, `unload`, `graph_status`,
  `search`, `read_section`, `get_entity`, `get_view`, `diagnostics`, and
  `report_feedback`,
- the [goal tools](./tools.md#goal-tools): `mark_goal_done`, `mark_goal_failed`,
  `load_skill`, `done`.

The write slice per kind is on the kind's page and in the catalog: entity, requirement,
and coverage tools for `reconcile-section`; judgment tools for `rejudge-pair` and
`review-entity`; view tools for the view kinds; the [binding](./tools.md#binding-tools),
[generation](./tools.md#generation-tools), and
[verification](./tools.md#verification-tools) tools for the ledger kinds. A `bind`,
`generate`, or `verify` batch adds the
[file and command tools](./goals/generate.md#file-and-command-tools) when the
executor's profile sets `serve_files` ([generation sessions](#generation-sessions)). A
tool outside the batch's toolset is not served, so the model cannot call it.

A batch resolves to exactly one executor: the scheduler never groups goals whose kinds
resolve to different profiles ([executors](../frontends/acp.md#executors)). The same
serving, prompt, and toolset serve an ACP worker session and an external agent that
claims the batch over [MCP](../frontends/mcp.md#compilation-over-mcp) on its own.

## Repeated calls

The same call with the same arguments has the same answer. A model that re-asks a
question it already asked is stuck, and the tool serving says so rather than letting the
session spend its budget on it:

- The second identical call answers as usual, with a `repeat` field on the result saying
  the answer is unchanged and to act on it.
- The third is refused with a `repeated-call` error naming the tool and the way forward:
  act on the answer already given, or finish with `done`.

Identity is the tool name plus its arguments verbatim, counted per open batch. A `load`
of a target that is already loaded counts as a repeat whatever its `depth`: the way to
see more of a loaded node is `expand` on its handles. An `unload` clears the `load`
count for its target: re-loading a node evicted for budget is legitimate, and its
render is fresh. `done`, `mark_goal_done`, and
`mark_goal_failed` are exempt; repairing a rejected claim legitimately repeats it. The
guard lives in the serving, not in any agent, so every agent gets the same contract.

A refusal is not an invalid call: the call was well-formed, the model is stuck. It never
feeds the abort streak, because aborting discards staged work and a stuck model usually
holds good work from before it stuck. Refusals are cheap (a refused call never
dispatches) and counted: past eight in one batch, the serving finishes the session
implicitly, committing the staged work under the same gates the [budget path](#budgets)
uses. A weak model that loops keeps what it earned; the guard only stops it paying for
the loop.

## Execution

A session runs as one [ACP worker session](../frontends/acp.md#worker-sessions):

- The runner resolves the batch's executor and creates a session on that agent whose
  tools are one injected MCP serving, scoped to the batch's toolset
  ([MCP into sessions](../frontends/mcp.md#mcp-into-acp-sessions)).
- The assembled prompt travels as the session prompt. It is agent-neutral: the contract,
  the goals, the loaded graph, and the protocol line, the same text for every agent.
  `begin_goals` for the batch answers with a short acknowledgement; the serving already
  holds the batch.
- The runner reminds once when the prompt ends in prose without the batch landing, then
  translates the agent's update stream into [trace events](#trace-events), closes the
  session, and waits for the teardown so a session that ended with staged work still
  lands it through the implicit finish.
- Success is read from the store, never from the agent's word: every goal in the batch
  is resolved (`mark_goal_done` accepted at commit), failed with a reason, or parked.
- Read tools answer immediately. Write tools stage mutations. How the agent drives its
  model between calls is the agent's business.

The message loop, the tool codecs (`native` and `text`), the first-round probe, and the
endpoint fallbacks belong to the
[embedded agent](../frontends/acp.md#the-embedded-agent): they are how a generic loop
speaks to a raw endpoint, not part of session semantics. An external agent brings its
own loop. Chat, answer, and follow sessions are the other session kinds
([sessions](../frontends/acp.md#sessions)); they run on the `[acp]` agent, not on the
executor overrides. The [benchmark](../benchmark/benchmark.md) grades an agent profile
by running sessions through it; cases per goal kind are
[deferred](../benchmark/benchmark.md#deferred-cases).

## Generation sessions

The [`bind`](./goals/bind.md), [`generate`](./goals/generate.md), and
[`verify`](./goals/verify.md) goals work on the deliverable, so their sessions are
served more than the graph tools and finish through the ledger:

- The toolset adds the kind's ledger tools, `record_binding`, `record_generation`,
  `record_verdict`, and `run_tests` ([binding](./tools.md#binding-tools),
  [generation](./tools.md#generation-tools), [verification](./tools.md#verification-tools)),
  and the [file and command tools](./goals/generate.md#file-and-command-tools)
  `read_text_file`, `write_text_file`, `list_files`, `run_command`.
- The file and command tools are served only when the agent's profile sets
  [`serve_files`](./project-settings.md#acp): the
  [embedded agent](../frontends/acp.md#the-embedded-agent) has no editor or shell, a
  coding agent brings its own. A `verify` session gets the read-only subset, no
  `write_text_file`; a `bind` session writes test and criteria files only.
- The tools are sandboxed to the deliverable directory: paths are relative to it, a
  path that escapes it is rejected, and `run_command` runs under it with a timeout.
- The names and shapes follow the Agent Client Protocol's client-provided tools (its
  file-system and terminal methods), served through the injected MCP serving like every
  other jazyk tool ([MCP into sessions](../frontends/mcp.md#mcp-into-acp-sessions)).
- The finish contract is record, then `done`: the session records its row or manifest,
  marks the goal done with a one-line justification, and ends with `done`. The gate
  reads the [ledger](../consumers/gen.md#the-ledger), never the model's word: no
  current row, no resolution (one retry, then parked), and a row recorded without a
  mark still resolves the goal at the next derivation.

## Staged mutations

Write tools never touch the store directly. They stage mutations into the session's
changeset. Each call is validated the moment it is staged, against the store plus what
is already staged, and an invalid call is rejected with a repair message naming the
violated rule ([validation gates](./graph.md#validation-gates)). The reply to an
accepted mutation carries two more things: the goals the mutation will open at commit
([bubbling](./reconciler.md#bubbling)), and the condensed status block of the loaded set.

Read tools show the committed snapshot the session began with, not the staged mutations.
While mutations are staged, every read reply and every status line touching a staged
node carries a note saying so, so a session that reads back a node it just staged a
delete for does not conclude the delete was lost.

A session that fails (the agent's session ends without landing its batch, is cancelled by
the idle timeout, or exhausts its repair rounds) leaves no trace: the changeset is
dropped and the graph is untouched. Its goals return to `open`, and the batch is retried
once with a fresh session; a second failure parks the goals with an `incomplete-build`
diagnostic ([parked and failed](./reconciler.md#parked-and-failed)). The
[embedded agent](../frontends/acp.md#the-embedded-agent) additionally aborts its own
loop after three consecutive invalid rounds, so a stuck weak model fails fast instead of
spending the budget.

## Resolving, failing, parking

Every goal in the batch ends in one of three states, and the session says which.

- `mark_goal_done({goal, justification, evidence?})` resolves a goal. The serving
  validates the claim against the kind's gate when the call is staged and again at
  commit over the final changeset, and rejects a false one with the gate named; the
  model supplies what the gate asks for or fails the goal. The justification is
  mandatory and concise, one or two sentences of why the gate holds. `evidence` carries
  what a kind's gate reads (a `rejudge-pair` verdict, the mutation or diagnostic that
  carried it). The journal records the resolution with its justification
  (`resolved_goals`), [`jazyk ripple`](../frontends/cli.md#jazyk-ripple) shows it beside
  each step, and the [board](../frontends/gui.md#board) shows it on the card. An
  accepted claim emits a `goal` trace event.
- `mark_goal_failed({goal, reason})` is always accepted. A goal that cannot honestly be
  accomplished (documents too deeply contradictory, a target that stopped making
  sense, a repair the tools cannot express) must be failable, or the board fills with
  dishonestly resolved goals. A failed goal keeps its target, so the failure surfaces on
  the thing itself everywhere it renders. A failed mandatory goal blocks convergence; a
  failed optional goal is recorded and stands. It reopens when its subject changes again
  or a human retries it.
- Parking is neither. A goal the session neither resolved nor failed when it finished
  (out of rounds, the implicit finish, a claim the gate refused) stays open, gets one
  fresh session in the same build, and parks when that session leaves it open too.
  Parked goals persist by id in `status.yaml` with their change payloads, count as open
  in the verdict (`incomplete`, with an `incomplete-build` diagnostic), and resume first
  in the next build. Unfinished work is never silent.

A goal a staged mutation opens joins the running session when it lies in the batch's
locality, the toolset covers its kind, and the remaining budget fits it; the reply lists
it, and the session resolves it like any goal in the batch
([bubbling](./reconciler.md#bubbling)). Dismissing a limit goal is a graph write, not
goal state: the node's own limit is raised with decree provenance
([per-node bumps](./graph.md#per-node-bumps)).

## Commit

Calling `done({summary})` runs the batch gates:

- every goal in the batch is resolved or failed,
- every resolution's gate still holds over the final changeset,
- the kinds' batch gates: the coverage contract for `reconcile-section` (every section
  in the batch carries a mark, staged in this session or already recorded, and every
  `covered` claim is honest), stale anchors addressed, no alignment proposal left
  undecided for `place-anchors`,
- the store's gates over the changeset as a whole.

A failure names the repair, and the model gets up to two repair rounds. A clean batch
commits atomically ([changesets](./graph.md#changesets)): the mutations apply, derived
data recomputes, the change records the mutations caused are written and the resolved
goals' records cleared, the journal entry lands with `kind: session`, `batch`,
`mutations`, `resolved_goals`, `opened_goals`, `rounds`, and `tokens`
([journal](./graph.md#journal)), the generation bumps, the renderer redraws the views
the commit touched, and the board re-derives.

An explicit `done` holds the model to its claims: a session that extracts requirements
from a section and never marks it is sent back to set the mark, not committed around,
and a `done` that leaves a goal open is rejected naming the goal. A batch that cannot be
repaired within its rounds finishes implicitly (see [budgets](#budgets)): valid staged
work commits, the open goals stay open for their retry.

One thing the harness never commits around: an untouched stale anchor. Stale anchors are
a contract only the model can honor. A `reconcile-section` goal whose section carries a
stale anchor the session neither re-anchored, revised, nor deleted stays open, the
mutations staged against that section are dropped, and the rest of the batch commits.
The next build lists the anchor again.

## Budgets

Budgets are registry constants built into the binary, not settings
([budgets and thresholds](./graph.md#budgets-and-thresholds)):

- Rounds: batches are sized so 24 rounds, 8 per section, fit ordinary work. The
  embedded agent's loop stops at 48 model round-trips (`JAZYK_AGENT_MAX_ROUNDS` to
  override), a flat runaway stop, not a per-batch budget: one round-trip may carry
  several tool calls, so a trace's call count can exceed it. An external agent bounds
  its own loop, and the idle timeout (`JAZYK_ACP_IDLE_TIMEOUT`,
  [worker sessions](../frontends/acp.md#worker-sessions)) bounds them all.
- Staged mutations per session: 64. Enforced by the tool serving.
- Context: 24000 characters, counting the loaded set and the skills rendered this
  session ([policy](./context.md#policy)).
- High-water mark: 0.9 of the context budget. Past it, `load` and `expand` are refused
  with a `context-full` error naming the unload candidates until something is unloaded;
  reads still answer ([policy](./context.md#policy)).
- Skills per session: 4, counting every skill rendered this session, active or inactive
  ([skills](#skills)). Enforced by the tool serving on `load_skill` and by the harness
  on auto-load.
- Per build: 3 × the derived goals plus 8 sessions, so a stuck build stops instead of
  looping. When the cap is tight, compile outranks GC: the scheduler stops opening GC
  bursts before it stops compile batches. Exhaustion parks every open goal
  ([convergence](./compilation.md#convergence)).

An agent whose session ends with mutations staged and no `done` is treated as having
called it: the serving runs the same commit gates, a clean batch commits, and the goals
the session marked are resolved while the rest stay open. Weak models forget the finish
contract more often than they stage bad work; discarding a valid changeset over a
missing `done` would punish the wrong thing. A session with nothing staged and no goal
marked is a failed session, retried once.

The same reasoning bounds what one bad claim can sink. When the implicit `done` is
rejected over a dishonest `covered` claim, the harness drops the offending coverage marks
and commits the rest: the extracted requirements land, the miscovered sections stay
unprocessed, and the next build resumes them. Only the explicit `done` holds the model
to repairing its own claims.

Every session's token count lands in `status.yaml` under `costs` (`sessions`, `tokens`,
`by_kind`, `by_class`), so the choice of executor per kind is informed by what each kind
spends ([storage layout](./graph.md#storage-layout)).

## Trace events

The runner translates a session's update stream into structured events: the tool call
with condensed arguments, the condensed result, and any message or thought text the
agent produced, emitted as model text. The `compile` command renders these live, the
[GUI](../frontends/gui.md#activity) streams them to the browser, and `jazyk watch`
prints one line per goal opened or resolved. The generation and verification workers
emit their own kinds per entity and per ledger row. The committed changeset with the
same information persists in the [journal](./graph.md#journal).

Every event carries a `label`: the batch id (`b413-3`) for everything a session produces,
`gen ent:cart` or `verify req:...` for the worker kinds, and `build` for what the
scheduler emits between sessions. The label is the grouping key, so a reader reassembles
one session from the transcript. A `step` names the position inside the label (`r3` for
a round, `product 1/2` for a generation part).

The event kinds:

- `board`: the board summary, emitted once after the build derives the board, before
  any batch forms. Carries the open goal count, the count per kind, and the blocked
  count; the label is `build`. It renders as the summary line
  [`jazyk compile`](../frontends/cli.md#jazyk-compile) prints first
  (`compile: 21 goals (9 reconcile-section, ...), 3 blocked`).
- `batchStart`: the scheduler formed a batch. Carries the batch id, the class and tier,
  the goals with their kinds and targets, and the resolved executor, so a reader sees
  what is about to run before any session starts.
- `sessionStart`, `sessionDone`, `sessionFailed`: the session lifecycle. `sessionStart`
  carries the batch id, the goals, the executor, the size of the initially loaded set,
  and the active skills; `sessionDone` the committed generation, the outcome per goal,
  rounds, and tokens; `sessionFailed` the reason and whether a retry follows.
- `goal`: a goal changed state. `opened` carries the `cause` (generation, mutation,
  `via`); `resolved` carries the `justification`; `failed` the `reason`; `parked` the
  session that left it. Resolved and failed events fire when the claim is accepted at
  staging; opened events fire at commit, one per entry in `opened_goals`.
- `gcBurst`: a GC burst starts on a settled cone. Carries the kind, the target, and the
  count against the limit (`gc burst: abstract-entity ent:order (54 > 50)` on the
  terminal).
- `toolCall`, `toolResult`, `toolError`: one row per tool call, condensed.
- `modelText`: prose or reasoning the model produced.
- `section`: the session moved to a section. Emitted when an accepted tool call names a
  section (`set_coverage`, `upsert_requirement`, an entity mention, `read_section`,
  `load`) that differs from the last one, so the sequence of these events is the
  session's path through the document. Carries `doc`, `section`, and the `tool` that
  named it.
- `llmRequest`, `llmResponse`, `llmRetry`: one model call, recorded by the
  [embedded agent's](../frontends/acp.md#the-embedded-agent) endpoint client. The
  request carries the whole outgoing message list (the prompt and the conversation so
  far) plus the tool names offered; the response carries the raw assistant message, the
  elapsed milliseconds, and the completion tokens; a retry carries the attempt, the
  error, and how long the client waits before trying again. Sticky fallbacks (codec
  downgrade, streaming, dropped `temperature`) are notes on the same label. A session
  against an external agent carries none of these: that agent's model traffic lives in
  its own logs.
- `note`: a plain line. Verbose notes carry the full loaded set and raw payloads.
- `genEntityStart`, `genEntitySkipped`, `genEntityDone`, `genEntityFailed`;
  `verifyRowStart`, `verifyRowDone`, `verifyRowStale`, `verifyRowError`: the worker
  kinds.

Payloads are recorded in full and shipped condensed. A transcript keeps every prompt and
reply as it was sent (capped per message, so one runaway payload cannot fill the disk),
including the status block as each round rendered it; the live stream and the transcript
listing carry the same events with long strings elided to a preview plus a byte count. A
reader that wants the whole payload asks for that one event
([GUI jobs](../frontends/gui.md#jobs)). Terminal rendering follows the
[trace level](../frontends/cli.md#jazyk-compile): model calls print one timing line at
`--verbose` and nothing at the default level, where the tool rows already say what
happened.

## Preview

Every session prompt is assembled deterministically, so it can be shown before it is
spent. [`jazyk preview`](../frontends/cli.md#jazyk-preview) renders the next session's
prompt exactly as the model would receive it; `jazyk preview <goal|target>` renders the
batch that goal would join, or the batch of the first ready goal on that target. The
[GUI](../frontends/gui.md#preview) shows the same pane before a release in `manual`
mode, and from a board card. The rendering is the same code that runs the session, it
makes no LLM call, and it writes nothing.

The transcript records the same rendering per round, so post-hoc review sees what the
model saw, verbatim. `ratify` and `answer` goals have no session; for those, preview
prints what the human owes instead of a prompt.
