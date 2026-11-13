# Sessions

A project opens all [document sets](./document-sets.md). Graph operations address one set
explicitly. Each set owns `.jazyk/<set-id>/`; project file operations share the
physical files and report every membership. The document-set contract defines
configuration, aggregate reads, management commands, and grouped compilation.

A session is one focused agent run over one goal batch. It is the only place a model
touches the compilation process. The [scheduler](./compiler.md#the-scheduler) forms the
batch from the [board](./reconciler.md#goal-derivation); the session executes as an
[ACP worker session](../frontends/acp.md#worker-sessions) against the
[executor](../frontends/acp.md#executors) the batch resolves to. Jazyk owns the batch,
the prompt, the tools, the gates, and the commit; the agent owns the model and the loop
that drives it.

The embedded agent uses the shared [GDK runtime](./agent-runtime.md). Native tool
calling is required. Its state and the open MCP batch are checkpointed together
when the host provides durable execution. Deployment suspension resumes that same
session; a normal new batch still starts with fresh context.

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
the graph. Nothing in between. A goal left open after four attempts parks
([resolving, failing, parking](#resolving-failing-parking)).

## The prompt

The prompt is assembled, never authored per goal. The same disk state renders the same
prompt, so it can be shown before it is spent. The parts, in order:

1. The agent contract, [`goals/prompts/session/agent-contract.md`](./goals/prompts/session/agent-contract.md.mustache),
   with [`feedback-note.md`](./goals/prompts/session/feedback-note.md) filled into its
   `{feedback}` slot, the second paragraph. The contract states the job (resolve the listed goals with the tools over
   the loaded graph, finish with `done`), the graph invariants, the loading rules, the
   staging rules, the review asymmetry, and the finish contract. The feedback note says
   that a confusing instruction, tool, argument, or error message goes to
   [`report_feedback`](./tools.md#feedback-tool) and the session continues on best
   judgment; a problem in the documents is a diagnostic, not feedback.
2. The active skills, each as `[skill: <name> (active)]` followed by its payload.
3. `## Project`: the generation number, the workflow mode, the open
   diagnostic counts (the judgment findings on the graph; the deterministic checks'
   findings arrive with the build's tail, and the line says so), the board counts (goals in this session, goals elsewhere, goals
   blocked on a human), and the project's lint rules with their severities when it
   declares any, numbered ([linting](./project-settings.md#linting)), so a `lint`
   diagnostic names a rule the model has read and a review's evidence names each rule
   by its number ([the gate](./goals/review-entity.md#gate)).
4. `## Goals`: one block per goal in the batch: the line `- [g:...] mandatory|optional`,
   the kind's contract paragraph (its payload file under `goals/prompts/`), the change in
   one line, the gate in one line (the kind's section of
   [`gates.md`](./goals/prompts/session/gates.md)), and the hints. The contract paragraph renders once
   per kind, under the first goal of that kind in the batch; a later goal of the same
   kind carries `Contract: as the first <kind> goal above` in its place, so a batch of
   eight pairs reads the pair contract once. The change line renders the change record
   as `field: value` pairs (`section unprocessed`, `req:orders-6 revised: statement,
   transition`), never as raw JSON. A hint repeated verbatim from an earlier goal in
   the batch (`skill judgment`, the kind's tool advice) renders once, on the first goal
   that carries it.
5. `## Loaded (x of y chars; skills z besides, uncounted)`: the [status block](./context.md#rendering) of the loaded set,
   then the skill index line, then the rendered items of the initially loaded set, the
   same text `load` returns for each (section bodies with their diff marks, nodes in
   full with their edges and stubs). What the block says is loaded is in the prompt:
   a session starts working, never with a round of reads for text the harness already
   chose.
6. The protocol line, with `{target}` set to the batch id:
   [`worker-protocol.md`](./goals/prompts/session/worker-protocol.md.mustache) when the prompt travels
   ahead of the claim (an ACP session prompt, `jazyk preview`), which tells the agent
   to call `begin_goals` first; [`serving-protocol.md`](./goals/prompts/session/serving-protocol.md.mustache)
   when `begin_goals` itself returns the prompt over a standalone MCP serving, which
   says the batch is claimed and not to call `begin_goals` again for it. The two lines
   describe one loop from either side of the claim. A session run in process with its
   batch already open (a [benchmark](../benchmark/benchmark.md#runs) case, a
   [snippet](../benchmark/benchmark.md#snippets-from-a-real-project), a
   [replay](../benchmark/replays.md)) gets
   [`open-batch-protocol.md`](./goals/prompts/session/open-batch-protocol.md.mustache) instead: its tools
   hold no `begin_goals`, and every probe of a local model once opened by noting that its
   instructions named a tool it did not have.

E.g., a tier 2 batch after a session revised `req:orders-6` (contract paragraphs
shortened here; the real ones are the payload files):

```text
[agent contract, fixed; the feedback note is its second paragraph]

[skill: judgment (active)]
[the judgment skill payload]

## Project
- generation 413, manual mode
- diagnostics: 1 error (contradiction diag:contradiction-3), 4 warnings
- board: 2 goals in this session; 19 elsewhere; 3 blocked on human answers (2 ratification proposals among them)
- lint: [1] warning: An em dash appears in prose. Use commas, periods, parentheses, or colons instead.

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
  Gate: definition claimed; one backed verdict per listed lookalike, unreferenced
  statement, open diagnostic, and composition part.
  Hints: load ent:order; skill judgment.

## Loaded (9.8k of 96k chars)
- req:orders-6    full: statement, transition placed → expired (guard 30 days), source docs/orders.md#/orders/holds
- req:payment-9   full: statement, source docs/payment.md#/payment/declines
- ent:order       full: 7 requirements, parent ent:order-service   [3 more edges: h:ent:order:related]
- ent:order-service   stub (definition only)   [5 edges loadable: h:ent:order-service]
Skills: judgment (active, 6.4k); extraction, flow-views, structural-views, abstraction, conformance (load_skill)

PROTOCOL: the `jazyk` tools carry this batch, b413-3. First call `begin_goals` with {"batch": "b413-3"} ...
```

The diagnostics line names how many of its counts are ratification proposals for
derived facts (criteria, groupings), so a session that just committed fourteen derived
criteria does not read fourteen new warnings as defects of its own making.

The shared agent contract states session rules once. Goal paragraphs, active skills,
tool schemas, and loaded packages carry the details of their own operation. The
contract does not repeat their field catalogs or examples. A worker acts on sufficient
evidence without restating the instructions or rehearsing its next tool call. Independent
calls share a reply. A dependent call waits for the earlier result.

Contract paragraphs are short and imperative: what the goal means, what evidence the
gate wants, what not to do, and that justifications and failure reasons are one or two
sentences, never essays. Each goal kind's page under [`goals/`](./goals/reconcile-section.md)
shows its block. The hints are computed and honest (what to load, which skill explains
the shape, which tool usually resolves the kind); the gate is the truth.

The `## Loaded` block is the model's working set and the harness keeps it current: it
re-renders condensed on a mutating tool reply when it changed, one line when it did not, and in full on `graph_status`
([context](./context.md#the-loaded-set)). Over ACP the whole prompt travels as the
session prompt; over plain MCP `begin_goals` carries the same assembly as
`instructions` and the loaded set as `package`, eliding the contract and the skills a
client already received in the same serving
([compilation over MCP](../frontends/mcp.md#compilation-over-mcp)). The prompt has one
source whoever consumes it.

## Payload files

Every model-facing text lives in a payload file under `goals/prompts/` or `skills/`,
embedded into the binary at compile time. Code assembles payloads and fills their
placeholders; it never carries prompt prose of its own. Tool descriptions are the one
exception: they live in the tool registry beside their schemas
([tools](./tools.md)).

- A whole-file payload is one text: the agent contract, a protocol line, a contract
  paragraph, a skill.
- A fragment file holds many short texts for one consumer. A marker line
  `<!-- name -->` opens each section, and the section runs to the next marker. Trailing
  newlines are dropped, so blank lines may separate sections. Markers never reach the
  model.
- A placeholder is `{{{name}}}` (unescaped Mustache). Rendering is one pass, so
  a quoted value that holds template syntax is never filled again. JSON braces
  remain literal. `.md.mustache` files support comments and file-relative partials.

The [prompt guide](./goals/prompts/README.md) defines the common source parser,
includes, metadata, comments, inventory, and maintenance rules. Every prompt and
skill passes through that parser before embedding.

The fragment files:

- [`gates.md`](./goals/prompts/session/gates.md): the one-line gate per goal kind.
- [`session-blocks.md`](./goals/prompts/session/session-blocks.md.mustache): the prompt's framing (the
  elided contract line, the skill and task headers, the `## Project` lines, the goal
  block labels, the `## Loaded` header and its deferred and unload lines).
- [`agent-context.md`](./goals/prompts/conversation/agent-context.md.mustache): the framing around what a
  conversation picks up and the warnings about what it skipped
  ([agent context](../frontends/agent-context.md)).
- [`serving-instructions.md`](./goals/prompts/serving/serving-instructions.md.mustache): an MCP
  serving's initialize instructions and the `next` guidance and notes in its replies,
  the benchmark case replies included ([MCP](../frontends/mcp.md)).
- [`generation-steps.md`](./goals/prompts/generation/21-generation-steps.md.mustache): the built-in
  generation worker's medium decision, its product, tests, and manifest steps, their
  corrective retries, and the criteria file for an `llm` test
  ([generation](../consumers/gen.md)).
- [`judge-input.md`](./goals/prompts/verification/judge-input.md.mustache): what an `llm` verdict reads, one
  builder for every judge.
- [`decompile-worker.md`](./goals/prompts/decompile/decompile-worker.md.mustache): the built-in drafting
  worker's role line, scope request, and retry ([decompile](../consumers/decompile.md)).
- [`answer-handling.md`](./goals/prompts/conversation/answer-handling.md.mustache): the handling contract for
  an answered question ([questions in chat](../frontends/acp.md#questions-in-chat)).

[`integration-context.md`](./goals/prompts/conversation/integration-context.md.mustache) frames the project
integration facts in initialize instructions
([project integration context](../frontends/mcp.md#project-integration-context)).

A table test names every section with its placeholders: each renders with all of them
filled and none left over, and every goal kind has its gate line.

## Skills

A skill is a prompt payload with the working knowledge for one shape of work: how flow
views order their members, what a good abstraction split looks like, how a duplicate
differs from a related concept. The contract paragraph says what to do for a goal; the
skill says how. Skills are files under `docs/compiler/skills/`, embedded into the binary
at compile time and excluded from the docs glob like the goal contracts:
[`extraction`](./skills/extraction.md) (statements, entities, edges, transitions,
attributes, coverage honesty), [`edges`](./skills/edges.md) (edge declaration and stability),
[`judgment`](./skills/judgment.md) (the review asymmetry,
duplicates, contradictions, verdict quality), [`flow-views`](./skills/flow-views.md)
(ordering, branches, participants), [`structural-views`](./skills/structural-views.md)
(class, component, and package membership, collapse, limits),
[`criteria`](./skills/criteria.md) (stated and derived acceptance criteria: one
condition, the boundary, the negative, the measure),
[`abstraction`](./skills/abstraction.md) (splitting entities and views, containment,
docs proposals), [`conformance`](./skills/conformance.md) (instances, attributes,
links). Skill text is medium-neutral: the model adapts its wording to the medium it
reads, as it does everywhere else.

`declare-edges` activates `edges`, the edge and stability rules from extraction.
Other extraction work keeps the full `extraction` skill. The focused payload does
not change the edge rules or the goal contract.

The lifecycle inside a session:

- From the first round: the skills of the batch's goal kinds (the union, from the
  [goal catalog](./reconciler.md#the-catalog)) render in the prompt as active. A goal
  kind's skill stays active while a goal of that kind is open in the batch.
- Auto-load: in a session with no fixed goals (a serving), the first node of a
  kind loaded brings the kind's skill, once. Standalone MCP calls carry their
  loaded set and skill activation state between calls in the same ledger unit.
  Expanding and unloading operate on that retained set. A new graph generation
  clears the loaded set so its status cannot describe stale facts as current.
  Already rendered skill bodies remain available without another rendering.
  Source caches are rebuilt from the current snapshot. The protected MCP checkpoint retains this state;
  older checkpoints without it start with an empty working set.
  A binding or generation claim loads document and view nodes as reference, just
  like a fixed goal batch. It does not auto-load extraction or view curation
  skills. Those skills remain available explicitly by name. A fixed goal batch never auto-loads
  skills from reference nodes, even when none of its goal kinds assigns a skill.
  A section brings `extraction`; a flow view
  (`use-case`, `activity`, `sequence`, `communication`, `timing`, `overview`) brings
  `flow-views`; a structural view (`class`, `object`, `package`, `component`,
  `composite`, `deployment`, `state`) brings `structural-views`. Entities and
  requirements bring no skill by themselves; `judgment`, `abstraction`, and
  `conformance` come from the goal kind or by name. A batch works under its goal
  kinds' skills: a section or view it loads is reference (a fan-out goal reading a
  document for the names it uses), and another skill comes by name when the work
  needs one, never as a side effect of a load that would spend half the budget.
- By name: `load_skill({name})` renders the payload in its reply and marks the skill
  active. The index line under `## Loaded` lists every skill: active ones with their
  size, the rest as loadable.
- Cap: at most eight skills render in one session (the registry constant
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
small toolset. The MCP catalog for an open batch contains that union, not every
write tool the serving can support. The embedded runner refreshes the catalog
before each model step, including after `begin_goals` or a batch transition.
Every session sees, on top of its kinds' slices:

- the [read tools](./tools.md#read-tools): `load`, `expand`, `unload`, `graph_status`,
  `search`, `read_section`, `get_entity`, `get_view`, `diagnostics`,
  `report_feedback`, and `report_spend`,
- the [goal tools](./tools.md#goal-tools): `mark_goal_done`, `mark_goal_failed`,
  `load_skill`, `done`.

The write slice per kind is on the kind's page and in the catalog: entity, requirement,
and coverage tools for `reconcile-section`; judgment tools for `rejudge-pair` and
`review-entity`; view tools for the view kinds; the [binding](./tools.md#binding-tools),
[generation](./tools.md#generation-tools), and
[verification](./tools.md#verification-tools) tools for the ledger kinds. When the
executor's profile sets `serve_files` ([generation sessions](#generation-sessions)), a
`bind` or `generate` batch adds the
[file tools](./goals/generate.md#file-and-command-tools) `read_text_file`,
`write_text_file`, and `list_files`, and a `verify` batch adds only the read-only
`read_text_file` and `list_files` ([verify tools](./goals/verify.md#tools)). A
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
  act on the answer already given, or finish with `done`. Before refusing, a read
  without side effects answers again and any other call checks whether a mutation was
  staged or committed since its last answer: a moved state is no repeat.
- A repeat whose answer differs from the previous one is no repeat: the state moved
  between them (a file rewritten, a refusal repaired), so it carries no `repeat` field
  and the count starts again. A `record_binding` that was refused for a missing test
  name, re-sent after the test file was fixed, once landed marked "unchanged". A
  `load` is the exception: it repeats by target, below.

Identity is the tool name plus its arguments verbatim, counted per open batch. An MCP
serving with no open batch counts per ledger claim (from `begin_binding` or
`begin_generation` to its record) and across its calls outside a claim. A `load`
of a target that is already loaded counts as a repeat whatever its `depth`: the way to
see more of a loaded node is `expand` on its handles. Its identity is the node the
target resolves to, so an alias and its id are one call, and a refused `load` (the target
never loaded) does not count. An `unload` clears the `load`
count for its target: re-loading a node evicted for budget is legitimate, and its
render is fresh. `done`, `mark_goal_done`, `mark_goal_failed`, `record_generation`,
and `record_binding` are exempt. Repairing files after a rejected record legitimately
repeats the same manifest or binding, even after several earlier refusals. Recording
still checks the current files, hashes, ownership, and evidence. The embedded agent's
failed-call bound still stops consecutive failures.
`settle_pairs` is exempt as a whole and counts its accepted entries instead: an entry
re-sent verbatim after it was accepted is marked on the second send and refused on the
third, while a rejected entry can be repaired and re-sent. The guard lives in the
serving, not in any agent, so every agent gets the same contract.
Task runtime calls are exempt. Their answers can change outside the graph session,
and repeated submission with the same operation key is an intentional retry.
Fixed run deadlines and the embedded agent's failed-call bound still apply.

A refusal is not an invalid mutation: the call was well-formed, the model is stuck.
The serving does not add it to its invalid-mutation count. The
[embedded agent](../frontends/acp.md#the-embedded-agent) has a separate failure
counter with a limit of twenty. Failed calls, including refusals, increment it at
most once per model transition. A successful call resets it. Refusals are cheap (a refused call never
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
  is resolved (`mark_goal_done` accepted at commit), failed with a reason (a
  `goal-failed` diagnostic), or parked.
- Read tools answer immediately. Write tools stage mutations. How the agent drives its
  model between calls is the agent's business.

The embedded message and tool execution pipeline comes from the public GDK.
Jazyk owns its application operations, MCP session state, budgets and gates. See
[the embedded runtime](./agent-runtime.md) for compaction and recovery.

## Generation sessions

The [`bind`](./goals/bind.md), [`generate`](./goals/generate.md), and
[`verify`](./goals/verify.md) goals work on the deliverable, so their sessions are
served more than the graph tools and finish through the ledger:

- The toolset adds the kind's ledger tools, `record_binding`, `record_generation`,
  `record_verdict`, and `run_tests` ([binding](./tools.md#binding-tools),
  [generation](./tools.md#generation-tools), [verification](./tools.md#verification-tools)),
  and the [file tools](./goals/generate.md#file-and-command-tools)
  `read_text_file`, `write_text_file`, and `list_files`.
- The file tools are served when the agent's profile sets
  [`serve_files`](./project-settings.md#acp): the
  [embedded agent](../frontends/acp.md#the-embedded-agent) has no editor or shell, a
  coding agent brings its own. A `verify` session gets the read-only subset, no
  `write_text_file`; a `bind` session writes test and criteria files only.
- The file tools are sandboxed to the deliverable directory: paths are relative to
  it, and a path that escapes it is rejected. The
  [task tools](./tools.md#task-runtime-tools) run saved commands in finite containers,
  with project-relative paths and fixed timeouts.
- The names and shapes follow the Agent Client Protocol's client-provided tools (its
  file-system and terminal methods), served through the injected MCP serving like every
  other jazyk tool ([MCP into sessions](../frontends/mcp.md#mcp-into-acp-sessions)).
- The finish contract is record, then `done`: the session records its row or manifest,
  marks the goal done with a one-line justification, and ends with `done`. The gate
  reads the [ledger](../consumers/gen.md#the-ledger), never the model's word: no
  current row, no resolution (up to four attempts, then parked), and a row recorded without a
  mark still resolves the goal at the next derivation.

Ledger workers judge the committed row or manifest before the session's stop. A
current binding or a fresh, valid generation manifest remains completed when a later
model call fails. An unfinished ledger goal retains the model or runtime error and
stops the action before another target is attempted. The error reaches the action
result and the endpoint breaker. It does not become a missing-row error for every
remaining target. A normal turn end, the round limit, cancellation, and recovery
suspension retain their distinct outcomes. A model transport error during review
stops further judgments without recording invented verdicts.

Runtime compaction preserves the worker's opening contract, latest successful
authoritative package exchange per unfinished target (`begin_generation`,
`begin_binding` or `begin_verification`), and latest completed tool exchange
verbatim. A successful recording closes its package. A later successful begin
replaces the older package for that target. A later file read or write does not
displace the active package's exact paths, hashes and bindings. Batched requests
retain their paired results together. Human input after the latest tool result
stays verbatim. Later assistant prose goes to the summary, rather than keeping a
long answer merely because the last tool exchange preceded it. The summarizer
receives older history and that prose, excluding retained exchanges and the opening.
Reasoning attached to retained tool requests is summarized. The project compaction
prompt asks for a concise progress handoff under 6000 characters. It does not ask
for full files or a transcript. Source, logs and exact package data remain available
through their original tools or retained exchanges.
The post-compaction size is checkpointed. When the preserved context still exceeds
the compaction threshold, another compaction requires growth of one fifth of the
context budget. Reopening the same package cannot consume the request budget in
repeated compactions.

The embedded ledger worker receives one continuation reminder when it ends a model
turn without a successful `done`. The reminder asks it to finish all assigned rows
or report the observed blocker in `done.summary`. The reminder is checkpointed
with the conversation and is not repeated after recovery. It preserves completed
writes and evidence. A second normal turn end still finishes the session. The
request budget, cancellation and transport failures retain their existing limits.
A provider response cut off by its output token limit is reported as such when
the session cannot continue, rather than as a completed normal turn.

An unfinished generation group after a normal turn end or the round limit stops
the generation action before another group starts. Later groups must not consume
partially written shared files. Completed records and writes remain available for
the next invocation. The result names the first incomplete group, the targets still
owed, and the observed session stop or gate failure in `error`. It does not convert
an ordinary unfinished gate into a transport failure. `stopped` retains the runtime
failure or the incomplete group reason. The final artifact build is skipped when
generation has failed or stopped. Binding and review retain their own target rules.

Malformed JSON tool arguments retain the parser's exact error, line and column in
the model's repair feedback. The feedback includes the complete argument length
and a bounded excerpt near the error. A shortened diagnostic excerpt does not mean
the provider cut off its output. The provider's actual finish reason is reported
separately. The rejected call does not execute. The model repairs its proposal in
its next ordinary inference. The adapter does not rewrite the proposed arguments.

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
once with a fresh session; a second failure parks the goals for this build
([parked and failed](./reconciler.md#parked-and-failed)). The
[embedded agent](../frontends/acp.md#the-embedded-agent) additionally stops at its
[failed-call limit](#repeated-calls), so a stuck weak model fails fast instead of
spending the budget.

A user stop drops the active changeset and its model context. Completed commits remain.
Late replies cannot stage mutations or commit. Cancellation does not retry the session.
An ephemeral serving receives an internal `--cancel-file` fence. Once the file exists,
it rejects calls and drops open work at end of input instead of committing implicitly.
The runner creates the fence before cancelling or closing the agent. Each attempt uses
its own fence, so rebuilding cannot reactivate a stopped session.

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
  each step, and the [board](../frontends/gui.md#editor-specification) shows it on the card. An
  accepted claim emits a `goal` trace event. A `rejudge-pair` batch also sees
  `settle_pairs`, one claim for several pairs: each entry is validated like a
  `mark_goal_done` claim and records one resolution with its own justification
  ([goal tools](./tools.md#goal-tools)).
- `mark_goal_failed({goal, reason})` is always accepted. A goal that cannot honestly be
  accomplished (documents too deeply contradictory, a target that stopped making
  sense, a repair the tools cannot express) must be failable, or the board fills with
  dishonestly resolved goals. Failure files a `goal-failed`
  [diagnostic](./model/diagnostic.md) on the goal's target with the reason, so the
  failure surfaces on the thing itself everywhere it renders, and no goal derives on that
  target while the diagnostic is open. A failed mandatory goal blocks convergence; a
  failed optional goal is recorded and stands. A change to the target resolves the
  diagnostic, and so does a person retrying it.
- Parking is neither. A goal the session neither resolved nor failed when it finished
  (out of rounds, the implicit finish, a claim the gate refused) stays open, gets one
  fresh session in the same build, and parks when that session leaves it open too.
  Parking is the build's own progress, never committed: the goal stays in memory, and in
  the build's run file while a local build runs
  ([a build's progress](./graph.md#a-builds-progress)). A parked goal counts as open in
  the verdict (`incomplete`), and the next build derives the same work and resumes it.
  Unfinished work is never silent.

A goal a staged mutation opens joins the running session when it lies in the batch's
locality, the toolset covers its kind, and the remaining budget fits it; the reply lists
it, and the session resolves it like any goal in the batch
([bubbling](./reconciler.md#bubbling)). Dismissing a limit goal is a graph write, not
goal state: the node's own limit is raised with decree provenance
([per-node bumps](./graph.md#per-node-bumps)).

## Commit

Calling `done({summary, goals?})` runs the batch gates. `goals` is a list of final
`{goal, justification, evidence?}` claims, validated by the same code as
`mark_goal_done`. A session can claim its final resolutions and request commit in
one call. Earlier claims can still use `mark_goal_done` when later work depends on
their result. A refused inline claim or batch gate rolls back this call's claims
and staged effects, leaving earlier work intact for repair.

The batch gates check:

- every goal in the batch is resolved or failed,
- every resolution's gate still holds over the final changeset,
- the kinds' batch gates: the coverage contract for `reconcile-section` (every section
  in the batch carries a mark, staged in this session or already recorded, and every
  `covered` claim is honest), stale anchors addressed, no alignment proposal left
  undecided for `place-anchors`,
- the store's gates over the changeset as a whole.

A failure names the repair, and the model gets up to two repair rounds. A clean batch
commits atomically ([changesets](./graph.md#changesets)): the mutations apply, derived
data recomputes, the resolved goals' decisions land under `graph/decisions/` where their
kind has one, the journal entry lands with `kind: session`, `batch`,
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

- Rounds: graph batches are sized so 24 rounds, 8 per section, fit ordinary work.
  This sizing estimate does not cap an agent session. The embedded ACP agent and
  the in-process worker use the same 512-model-request limit, including compaction
  requests. `JAZYK_AGENT_MAX_ROUNDS` overrides it where native environment settings
  are allowed, with a minimum of one. Hosted workspaces use the default and do not
  inherit the service environment. One request may carry several tool calls, so a
  trace's call count can exceed the limit. Reaching the limit reports
  `Agent reached the session limit after N model requests; unfinished work remains open`.
  A restored request keeps its identity and counts once against the same limit.
  An external agent bounds
  its own loop, and the idle timeout (`JAZYK_ACP_IDLE_TIMEOUT`,
  [worker sessions](../frontends/acp.md#worker-sessions)) bounds them all.
- Staged mutations per session: 1024, raised for a session holding an
  [`abstract-entity`](./goals/abstract-entity.md) goal over a requirement count to 1024
  plus that count: a split stages one move per grouped statement. A
  [`derive-criteria`](./goals/derive-criteria.md) goal raises it by three per listed
  requirement, the density the criteria skill asks for. Enforced by
  the tool serving.
- Loaded graph context: 96000 characters, counting the loaded set and the skills rendered this
  session ([policy](./context.md#policy)).
- Model context: 131072 tokens, separate from the character budget of the loaded
  graph. Reserve the requested output allowance before deciding to compact history.
  The default output allowance is 65536 tokens, including reasoning. Provider
  ceilings and explicit lower settings still apply. Compaction starts at 80 percent
  of the remaining input allowance.
- Consecutive tool errors: 20. Successful calls reset the count. Unchanged repeated
  reads still stop on the third identical call. These guards detect lack of progress.
- High-water mark: 0.9 of the context budget. Past it, `load` and `expand` are refused
  with a `context-full` error naming the unload candidates until something is unloaded;
  reads still answer ([policy](./context.md#policy)).
- Skills per session: 8, counting every skill rendered this session, active or inactive
  ([skills](#skills)). Enforced by the tool serving on `load_skill` and by the harness
  on auto-load.
- Per build: 8 × the distinct goals derived so far in the build plus 64 sessions, so a
  stuck build stops instead of looping. The count grows with the build: every goal id
  any board of this build has carried, recomputed after every commit, so goals that
  derive only after an earlier batch commits (the deeper link levels of a project)
  widen the cap instead of exhausting it ([convergence](./compilation.md#convergence)).
  When the cap is tight, compile outranks GC: the scheduler stops opening GC
  bursts before it stops compile batches. Exhaustion parks every open goal
  ([convergence](./compilation.md#convergence)). Five consecutive failed sessions
  that spent no tokens stop the build early the same way: an endpoint answering only
  errors parks the rest instead of grinding the cap, and a rate limit stops it at the
  first such failure; a session that spent nothing never counts as its goals' attempt
  ([escalation](./reconciler.md#escalation)). `jazyk compile --sessions N` replaces
  the cap with N for one run ([compile](../frontends/cli.md#jazyk-compile)).

An agent whose session ends with mutations staged and no `done` is treated as having
called it: the serving runs the same commit gates, a clean batch commits, and the goals
the session marked are resolved while the rest stay open. Weak models forget the finish
contract more often than they stage bad work; discarding a valid changeset over a
missing `done` would punish the wrong thing. A session with nothing staged and no goal
marked is a failed session and counts toward the same four-attempt goal budget.

The same reasoning bounds what one bad claim can sink. When the implicit `done` is
rejected over a dishonest `covered` claim, the harness drops the offending coverage marks
and commits the rest: the extracted requirements land, the miscovered sections stay
unprocessed, and the next build resumes them. Only the explicit `done` holds the model
to repairing its own claims.

Every session's token count lands in `build.yaml` under `costs` (`sessions`, `tokens`,
`by_kind`, `by_class`), so the choice of executor per kind is informed by what each kind
spends ([storage layout](./graph.md#storage-layout)). A session's tokens are what the
endpoint bills: the input and the output tokens of every model round, refused rounds
included, read from the usage the response carries (its total, or its prompt plus
completion tokens). A response without usage is billed by estimate, a quarter token per
character sent and received. The output tokens alone are a separate figure: the
throughput and efficiency the [benchmark](../benchmark/benchmark.md) reports, and the
`llmResponse` trace event. An agent loop resends its whole history every round, so the
input tokens are most of a session's bill; one build counted output alone and reported
13k tokens where the endpoint saw 29k of input.

Cached input tokens ride beside the bill. A provider that caches the prompt prefix
reports how much of the input it served from that cache: OpenAI chat completions in
`usage.prompt_tokens_details.cached_tokens`, an Anthropic-compatible endpoint in
`usage.cache_read_input_tokens`. Both response paths (a body, a stream) read both
names; an endpoint that reports neither counts 0. The figure is a share of the input,
not a deduction from it: the bill stays what the endpoint billed, and the cached share
says how much of it was priced at the cache rate. The embedded agent reports both per
round in its usage update's meta (`jazyk.tokens`, `jazyk.cachedTokens`), the runner sums
them over the turn, and the journal entry carries them as `tokens` and `cachedTokens`
([journal](./graph.md#journal)); `jazyk status --costs` prints the cached share beside
the spend ([status](../frontends/cli.md#jazyk-status)).

## Snapshots

With `jazyk compile --record`, every batch a session opens leaves a snapshot of the
graph as it stood: the node files under `<out>/graph/`, the status, and
`local/build.yaml`, copied to `<set-id>/snapshots/<batch id>/` in the
[log directory](./jazyk-folder.md#logs) when the batch starts (the build loop at `batchStart`, the MCP serving at `begin_goals`). A snapshot is
what makes a session reproducible after the fact: `jazyk replay extract` seeds a
[replay](../benchmark/replays.md) from it, so the harness answers the recorded calls
against the state the model actually saw, not the state the run ended in. Snapshots are
never read by the compiler and cost the size of the graph per batch, so a build without
`--record` writes none; delete them freely.

The [hosted runtime](./hosted-runtime.md#workspace-snapshots) writes no snapshots: nothing
prunes a hosted workspace, so a copy per batch would grow it without bound.

## Trace events

The runner translates a session's update stream into structured events: the tool call
with condensed arguments, the condensed result, and any message or thought text the
agent produced, emitted as model text. The `compile` command renders these live, the
[GUI](../frontends/gui.md#events) streams them to the browser, and `jazyk watch`
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
  In a [selected build](./compilation.md#selected-builds) the counts cover the
  selected goals and the event carries `selection`.
- `batchStart`: the scheduler formed a batch. Carries the batch id, the class and tier,
  the goals with their kinds and targets, and the resolved executor, so a reader sees
  what is about to run before any session starts.
- `sessionStart`, `sessionDone`, `sessionFailed`, `sessionEnded`: the session
  lifecycle. `sessionStart` opens one attempt on a batch and carries the batch id, the
  goals, the executor, the size of the initially loaded set, and the active skills;
  `sessionDone` the committed generation, the outcome per goal, rounds, and tokens;
  `sessionFailed` the reason and the attempt number, so a reader knows whether a retry
  follows; `sessionEnded` the reason nothing landed when the attempt was not the
  session's own failure: the build was cancelled, a ledger batch returned and its
  ledger judges what landed, or the runner left the attempt without a verdict (a
  panic). Every attempt ends with exactly one terminal event, `sessionDone`,
  `sessionFailed`, or `sessionEnded`. The runner emits it on every exit path
  (success, failure, cancellation, host death, the round budget), and the trace drops a
  second terminal event for an attempt it already closed. A `sessionStart` with no
  terminal event means the process died. A failure's reason names the stop in words
  (the model ended its turn with the batch still open, the round budget, the model's
  error), never a bare `nothing committed`; the refusals and the model text before it
  ride on their own `toolError` and `modelText` events under the same label, so a
  reader says why a batch did not land.
- `goal`: a goal changed state. `opened` carries the `cause` (generation, mutation,
  `via`, when the local journal holds the commit that made the finding true); `resolved` carries the `justification`; `failed` the `reason`; `parked` the
  session that left it, or the reopen cycle that parked it; `reopened` the cycle count
  of a goal re-derived with the same `change` after its resolution
  ([parked and failed](./reconciler.md#parked-and-failed)). Resolved and failed events
  fire when the claim is accepted at staging; opened events fire at commit, one per
  entry in `opened_goals`.
- `selection`: a [selected build](./compilation.md#selected-builds) widened. Carries
  the items added and the whole selection now in force. The label is `build`. The
  next board derivation applies it.
- `gcBurst`: a GC burst starts on a settled cone. Carries the kind, the target, and the
  count against the limit (`gc burst: abstract-entity ent:order (54 > 50)` on the
  terminal).
- `toolCall`, `toolResult`, `toolError`: one row per tool call, condensed.
- `modelText`: prose or reasoning the model produced. A chunk of the model's reply
  carries `reply: true`; a chunk of reasoning omits the field. One chunk never mixes
  the two. Streamed chunks retain their
  exact whitespace, including chunks containing only spaces or newlines. Consumers
  concatenate adjacent chunks of the same kind without inserting or removing
  characters, and never join reasoning to a reply. In-process workers
  coalesce text into chunks of at most 2,000 bytes or flush after 250 milliseconds
  while deltas arrive. The first delta is visible immediately. Pending text flushes
  before tool events, usage and session completion. Coalescing never drops text.
- `section`: the session moved to a section. Emitted when an accepted tool call names a
  section (`set_coverage`, `upsert_requirement`, an entity mention, `read_section`,
  `load`) that differs from the last one, so the sequence of these events is the
  session's path through the document. Carries `doc`, `section`, and the `tool` that
  named it.
- `llmRequest`, `llmResponse`, `llmRetry`: one model call, recorded by the
  [embedded agent's](../frontends/acp.md#the-embedded-agent) endpoint client. The
  request carries the outgoing messages after `prefix`, the count of leading messages
  it repeats unchanged from the previous request of the same run, plus the tool names
  offered. The first request of a run carries the whole list (the prompt and the
  conversation so far), so a reader rebuilds every request from the run's earlier
  ones. The response carries the raw assistant message, the
  elapsed milliseconds, and the completion tokens; a retry carries the attempt, the
  error, and how long the admitted host waits before trying again. Capability errors
  stop the session without switching tool formats. A session
  against an external agent carries none of these: that agent's model traffic lives in
  its own logs.
- `note`: a plain line. Verbose notes carry the full loaded set and raw payloads.
- `genEntityStart`, `genEntitySkipped`, `genEntityDone`, `genEntityFailed`;
  `verifyRowStart`, `verifyRowDone`, `verifyRowStale`, `verifyRowError`: the worker
  kinds.

Tool calls and results retain their original serialized JSON in `full` when the condensed
line omits detail. Payloads larger than 8,000 bytes stay complete and parseable. Trace
retention never cuts a tool payload into partial JSON.

A transcript keeps every prompt and reply (subject to the model message recording cap),
including the status block as each round rendered it. The live stream and the transcript
listing carry the same events with long strings elided to a preview plus a byte count. A
reader that wants the retained payload asks for that one event
([GUI jobs](../frontends/gui.md#jobs)). Terminal rendering follows the
[trace level](../frontends/cli.md#jazyk-compile): model calls print one timing line at
`--verbose` and nothing at the default level, where the tool rows already say what
happened. Host event admission and retention limits still apply. A host may omit an
oversized event with an explicit notice instead of presenting partial JSON as `full`.

## Preview

Every session prompt is assembled deterministically, so it can be shown before it is
spent. [`jazyk preview`](../frontends/cli.md#jazyk-preview) renders the next session's
prompt exactly as the model would receive it; `jazyk preview <goal|target>` renders the
batch that goal would join, or the batch of the first ready goal on that target. The
[GUI](../frontends/gui.md#editor-specification) shows the same pane before a release in `manual`
mode, and from a board card. The rendering is the same code that runs the session, it
makes no LLM call, and it writes nothing.

The transcript records the same rendering per round, so post-hoc review sees what the
model saw, verbatim. `ratify` and `answer` goals have no session; for those, preview
prints what the human owes instead of a prompt.

Accepted graph writes can publish a [live compilation preview](./live-preview.md).
The preview is provisional until the session commits.


## Live input and progress

The embedded loop accepts ordered human inputs at safe boundaries. Steering keeps cumulative round and failure budgets. It never grants permission or bypasses a gate. The host records delivery only after history insertion.

Native provider streams emit answer and thought deltas separately. Complete messages remain the source of history and tool arguments. The final assembled response is not emitted again. Partial output is retained when a model attempt fails. An attempt with visible output is not retried automatically. Round and tool timing events carry monotonic durations.

A stopped conversation retains accepted model responses and completed tool
receipts in its history. Pending calls receive interrupted results without being
dispatched. Provider call identities and native response metadata stay paired.
The next human turn receives that history and does not replay the stopped work.


Steering keeps separate input identities and delivery events. Adjacent human text is joined into one user message in model history when a template requires alternating roles. Native tool results retain their tool role and call identity. The embedded ACP agent applies the same shaping to initial MCP instruction blocks and the first human prompt.


## Conversational harness context

A conversational turn supplies its current serving contract through system-message assembly. The contract is replaced on the next turn. It is not appended to the persisted human-message history. Human messages, tool results, provider reasoning fields, and call-result pairs retain their own identities and contents.

Old conversations can contain generated contracts joined to human messages. The engine removes only a recognized exact generated prefix whose remaining text matches recorded human-message text, including consecutive merged human messages. An unrecognized prefix, missing transcript evidence, or a user quotation stays intact. Migration is idempotent and does not remove tool or assistant history.

The current conversational contract distinguishes ongoing steering from completed work. Compiler packages and batch IDs from completed or cancelled earlier turns are historical evidence. They do not claim a batch or request more compiler work. An authorized prose edit reads the current file when needed, saves the scoped change, and reports the result. Status and rationale describe that action. The agent does not repeat harness instructions as an explanation of its task. A request for detailed reasoning retains its requested scope.

## Provider reasoning summaries

A provider reasoning summary is distinct from ordinary reasoning and the answer.
The model delta field is `reasoning_summary`. A `modelText` trace event can carry
`summary: true`. An absent flag retains ordinary reasoning behavior. Completed
provider items are checkpointed with the inference receipt. A partial stream never
serves as a completed tool-call response. Recovered receipts do not repeat paid
inference or duplicate displayed summary deltas.
