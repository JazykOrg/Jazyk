# Faster agent answers and document edits

Status: implemented locally, 2026-10-29. Companion to [active agent steering](./steering.md).
See the [verification report](./agent-steering-acceptance.md). The implementation
uses sub-agents with shared runtime changes integrated by one owner. Merge and
publication wait for the owner's instructions.

## Desired result

A general question gets a direct answer without a mandatory project tour. A
project-specific question loads only the context needed to answer it. An explicit
follow-up such as `update the relevant section in hosting.md` edits the document
and reports the result without repeating the discovery from the previous turn.

Keep graph/prose consistency, source freshness, permissions, and compiler gates.
Measure actual completion separately from the time until useful text is visible.

## Findings from the reported conversation

The first turn calls `list_document_sets`, `task_context`, `read_document`, and
`search` before answering. Its deliberation repeatedly restates the same question
and revisits whether it can answer directly. After reading the minimal hosting
document, it still searches the empty graph. The next turn shows only `Thinking…`.

The transcript alone has no timings. The production investigation below identifies
the corresponding jobs and separates model delay from scope and recovery bugs.

## Production evidence on October 29

Read-only inspection of the service journal, job records, model attempts, and
persisted events identified both messages. The API was running release
`5bb8a3e4f3b07fade97390d6df7a963a88986091`, whose compiler lock names
`b6306da3ccd693802165bb7913fb159d4c5887fa`. The service had not restarted
since October 24. Its warning-level journal had no entries in the inspected six
hours. An empty warning journal is not evidence that the request succeeded.

- Job `360`: original CloudFront/S3 question. Started at `13:38:34 UTC`, completed
  at `13:40:01 UTC`, approximately 87 seconds. Four model rounds took about 12,
  4, 16, and 55 seconds. Calls included document-set discovery, task context,
  reading `hosting.md`, and an unsuccessful empty-graph search.
- Job `380`: `update the relevant section in hosting.md`. Its exact message is
  persisted in `compiler_events`, sequence `3`, at `13:45:43 UTC`. It was still
  running with a live writer lease when inspected after more than eight minutes.
  This was not confirmed message deletion or a confirmed dead worker.
- Both turns initially tried the OpenRouter route and received HTTP `402` in less
  than a second, then used DeepInfra. This fallback is real, but accounts for little
  of the elapsed time. A trace label naming the initial route is not proof that
  the model response came from that provider. Use actual attempt records.
- The edit's first successful model call took approximately 228 seconds, with
  22,650 input tokens and 1,934 output tokens. Its sixth took approximately 144
  seconds, with 34,654 input tokens and 2,584 output tokens. The seventh was pending
  at the inspection snapshot. Large prompts and slow generation both matter.
- The edit called `goals` and `read_section`, then `add_requirement`, which failed
  with `unknown-section` and `compile first`. It then called `release_work` for
  `compile`, `goals`, `begin_goals`, and `search`. The claimed batch included
  `main.md`. The user requested a hosting prose edit, not this compilation work.

The missing message after refresh is consistent with a split persistence path:
events are stored during the run, but the conversation book and workspace are
saved after the turn returns. A fresh session read can therefore return the old
transcript and idle state while the job remains running. New SSE connections
default to the latest event, and job detail replay returns `job.trace` events
without replaying the earlier `chat.update` message. The UI derives its chat
activity line from that stale session state. Confirm this with a refresh regression
test against the deployed behavior. The observed workflow indicator alone does
not describe which conversation or operation is active.

These are timestamped observations, not a promise that job `380` remains running.
No cancellation, retry, model change, document edit, or deployment was performed
as part of this investigation.

The workflow button has an additional presentation bug:
`../jazyk-cloud/web/src/shell/WorkflowButton.tsx` selects any active job, including
chat, labels it `Running workflow…`, and opens a `follow-<job-id>` thread. Chat
belongs to its conversation instead. The screenshot alone does not prove a compile;
the `release_work` and `begin_goals` events establish that in this incident.

## Findings from the current code

| Finding | Evidence | Implication |
| --- | --- | --- |
| Project discovery is mandatory in the serving introduction | `bootstrap/src/mcp.rs`, workspace `initialize` says to start with `list_document_sets` | Even general questions are pushed toward a discovery call |
| Task instructions begin with `Read task_context` | `docs/compiler/goals/prompts/execution-contract.md`, appended to native chat by `mcp.rs` | A task-specific prerequisite reads like a global conversational prerequisite |
| Chat instructions say to read the graph; reader instructions say to look before answering | `bootstrap/src/mcp.rs`, `instructions_in` and `reader_instructions` | The prompt does not clearly distinguish general knowledge from project evidence |
| Native embedded ACP always sets `conversation: false` | `bootstrap/src/acp/agent/mod.rs` | A prose answer after tools can get the worker completion nudge and another model round |
| Portable chat sets `conversation: true` | `bootstrap/src/conversation.rs` | The native completion-mode issue does not explain every hosted or browser turn |
| Hosted restored conversations append serving instructions when context reopens | `bootstrap/src/conversation.rs`, scoped-VFS branch | Long conversations can accumulate repeated instruction blocks; fresh handle warnings still matter |
| LLM timing events already exist | `bootstrap/src/llm.rs`, `LlmRequest`, `LlmResponse`, and retry events | Extend existing tracing rather than introducing a separate timing system |
| Native SSE is assembled into a full message; loop emits thoughts after receiving it | `bootstrap/src/llm.rs`, `read_stream_message`; `bootstrap/src/agent_loop.rs` | Upstream streaming alone does not produce incremental user-visible text |
| Browser and hosted model interfaces return a completed response | `bootstrap/src/wasm/bridge.js`; `bootstrap/src/hosted.rs`, `ModelTransport` | Streaming needs changes through the adapters, not just a UI spinner |
| The editor displays returned deliberation in full | `../jazyk-cloud/web/src/chat/ChatPane.tsx`, `Thought` | Repetitive provider output dominates the transcript; presentation changes alone do not reduce inference time |

These are current source findings. The deployed engine pin and model path must be
captured when reproducing the reported behavior.

## Establish a comparable baseline

Create a small fixture with an uncompiled graph and the hosting sentence from the
reported example. Replay the original question, then the explicit edit request in
the same conversation. Keep a compiled-graph variant and an unrelated document to
detect lost edit boundaries. Use a disposable workspace, not the user's live files.

Preserve the production evidence above as the incident baseline. Also identify
the editor revision and reproduce with the deployed engine revision, backend mode,
provider/model, codec, session length, tool count, and applicable runtime settings.
Use existing debug/run exports for further detail. Do not need raw credentials or
inspect unrelated conversations. Do not resend the active production request to
create a baseline. Reproduce it in an isolated fixture.

Add correlated monotonic timing spans for:

- Browser submit, HTTP acceptance, job admission, queue/lease wait, and session open.
- Tool discovery and schema serialization, instruction/history size, and the first
  model request. Separate cold session/model load from a warm follow-up.
- Each model attempt: permit/pacing wait, transport start, first provider content
  delta when observable, full response, usage, retries, backoff, and fallbacks.
- Each tool: name, start, end, result size, error rule, and revision metadata.
- Loop-generated nudges: reason and resulting extra model round.
- First useful answer text, guarded edit commit, last answer text, turn end, event
  persistence, SSE/worker forwarding, and browser render receipt.

Use turn and request IDs across the engine, hosted service, and editor. Show
durations and counts in Debug and exports. Never include credential values.
Separate reasoning from answer content. Use `unavailable` when a provider returns
only a complete body; its first returned byte is not measured first-token latency.
Avoid summing overlapping spans or comparing absolute clocks on different hosts.

Run five comparable warm trials per selected backend/model, plus separate cold
trials. Report the median and individual samples; five samples do not establish a
reliable tail percentile. Deterministic providers verify overhead and call counts.
Matched real-provider runs establish user-visible performance. Preserve LocalRouter
and the selected model while comparing the first changes. Record provider cache
hits where available, and never equate browser model loading with steady-state chat.

## Remove irrelevant work

Update canonical docs and embedded payloads before code. Put the conversational
contract in a shared payload, not another string constant inside Rust.

- General explanation: answer directly when project state is not needed. Do not
  call discovery, task, graph, or compiler tools solely because they are present.
- Ambiguous project reference such as `here`: use selected UI context or ask a
  narrow clarification only when it changes the answer. A general answer can state
  its assumption without inspecting every project subsystem.
- Project-specific question: read the named document or relevant evidence. Use
  graph lookup when relationships or compiled facts are actually needed. An empty
  graph is not a reason to compile before answering a general question.
- Task inspection or execution: load `task_context` when a task definition,
  credential-grant configuration, or execution revision is needed. Preserve this
  prerequisite for real execution. Avoid making it mandatory for all chat.
- Document-set-local operation: resolve the selected set when needed. A general
  answer does not need a set. Never guess between ambiguous set memberships.
- Explicit edit: act within the named scope. Do not ask again for permission
  already given by `update the relevant section`.

The prompting fix must cover the whole chain, not just the first system message:

- The chat contract explicitly separates explanation, document edit, compiler
  work, generation, and execution. An edit request alone does not release compile.
- Scope `Read task_context`, goal-loop instructions, and release guidance to their
  applicable workflows. A repair hint is information, not new user authorization.
- Revise `compile first` errors for conversational writes to describe the guarded
  prose-edit alternative when it exists. Do not suggest unrelated lifecycle work
  as a universal recovery action.
- Keep compiler instructions in goal packages and task execution instructions on
  the task path. Reduce redundant blanket instructions in conversational context.
- Make document editing available before compiling. If the required action is
  unsupported, give one clear blocker rather than repeatedly probing lifecycles.
- Preserve existing human releases, grant checks, worker claims, and gate checks.
  Review whether `release_work` can enforce the declared request scope at its
  admission boundary as well as instructing the model. Do not rely solely on a
  model deciding that a compile is authorized.

Do not add a second model request just to classify every user message. Use a clear
shared contract and supplied context first. Tools remain available for warranted
investigation. Avoid keyword-only routing that misclassifies a general question
as execution merely because it mentions a deployment script or credentials.

Supply a compact initial context block with backend capabilities, selected file
and document set when known, and graph availability. Include revision identifiers
and mark unavailable facts. Avoid sending all document contents or scanning the
whole graph to save one discovery call. Reuse authoritative workspace summaries
already loaded during session setup where possible.

## Reuse context without reusing stale writes

Retain the preceding answer, file identity, and document version across follow-ups.
Use the prior content as the basis for `update hosting.md` when it is still current.
Check the current version through existing guarded read/write paths. A changed hash
requires a fresh read and a narrow reapplication; an expired handle requires reload.
Do not repeat task and graph discovery if those facts do not affect the edit.

Deduplicate unchanged serving instructions across hosted context reopening by
instruction fingerprint and capability/version identity. Reissue a compact context
reset notice when handles become invalid. Changed contracts enter in full. Keep
the model's valid tool-call/result history and distinct user messages intact.
Do not blindly strip provider-required history fields to shrink prompts.

Use existing dual-write tools for edits to compiled facts. There is no equivalent
guarded MCP prose-edit tool for the uncompiled fixture: `add_requirement` requires
a stored section, while generic `write_text_file` excludes documents and the
generic tool path can reject calls before a graph exists. This is a concrete tool
gap, not something prompting alone can fix.

Document and expose `edit_document` or `write_document` alongside `read_document`,
before the no-graph gate. Reuse `bootstrap/src/webapi/docs.rs` `doc_write` and
`doc_content`, which already provide document validation, hashes, atomic writes,
and binary refusal. Preserve alignment dirtiness, membership, edit journaling, and
ACP edit delegation. Use the existing document-save semantics rather than a raw
filesystem escape. Keep binary source documents on their format-specific paths.

Report the committed diff and actual result. `I will update it` is not completion.

## End conversational turns correctly

Carry an explicit session purpose from the host into the embedded loop. Chat uses
conversational completion. Goal workers retain their completion nudges and gates.
Do not globally change `conversation` to true in the generic ACP agent, since it
also executes compiler workers. External ACP agents retain their own turn semantics.

For a chat answer, do not request another model round solely because tools were
used earlier in the turn. Keep bounded repairs for malformed tool calls, empty
responses, and genuine unperformed actions. Test the action-announcement heuristic
against optional offers such as `If you want, I can add this to the docs`, so a
completed explanation does not accidentally start work or get nudged to do it.

At each legitimate completion boundary, integrate the steering queue before closing
the turn. A late input must not be dropped by the faster completion path. Keep
budgets cumulative across steering and repair rounds.

## Deliver progress and answers incrementally

Add a provider delta callback/event path alongside the complete response used for
history and tool validation. Keep answer, thought, usage, and tool argument events
distinct. Deduplicate the final assembled message against already emitted deltas.
Never execute a tool from incomplete streamed arguments.

Wire the native SSE parser, ACP notifications, hosted model adapter, hosted event
stream, browser model bridge, and editor reducer where each supports streaming.
Some providers return a body only; retain that supported fallback honestly. Preserve
timeouts, retry bounds, cancellation, redaction, and partial-output error semantics.
A retry must visibly distinguish an abandoned partial response from its replacement.

Coalesce small deltas before database persistence and rendering while preserving
order and a short bounded flush interval. Do not write one database row or render
the full transcript for every token. Measure forwarding and render delay separately
from provider time. Treat streaming as a responsiveness improvement unless full-turn
measurements also improve.

Keep brief visible activity messages such as `Reading hosting.md` and `Saving the
deployment section`. Preserve access to returned thoughts and diagnostics. A compact
thought presentation can be evaluated after real latency improvements, but do not
claim that collapsing text makes inference faster or generate extra model commentary
solely to animate the UI.

## Keep accepted messages and recover active turns

Reliability work precedes performance tuning. Extend the shared input lifecycle
from the steering plan to initial prompts and ordinary follow-ups as well.

- Persist the submitted message and its ID atomically with job admission before
  returning acceptance. Retain the original text independently of a successful
  workspace snapshot or model response. Record rejection explicitly.
- Restore chat state by joining conversation identity to its authorized active or
  latest job. Show `Working on your message` immediately after refresh, including
  the pending user message, latest known action, elapsed time, and Stop.
- Provide one replayable conversation timeline that merges saved transcript and
  durable in-flight chat events by stable IDs and ordered cursors. Do not replay
  just compiler trace events. Load a snapshot and its cursor, then fill the gap
  before subscribing to future events. Cover full refresh and SSE reconnect.
- Make the job's running, paused, cancelled, failed, or interrupted state
  authoritative. Keep workflow, session row, transcript activity, and composer
  consistent. Label chat activity as chat work, not an unexplained build workflow.
  Activity clicks for a chat job select its conversation, not a `follow-` build
  mirror. Restore the selected conversation on reload.
- Do not clear a draft before explicit admission. Preserve a recoverable submitted
  copy and display admission errors. The current empty catch in `PromptBox` must
  not swallow a pre-admission HTTP rejection after discarding the user's text.
- A model timeout, worker panic, expired lease, process restart, or event-writer
  failure produces a durable terminal or recovery state. Separate a healthy job
  heartbeat from actual model/tool progress. A long provider call is not evidence
  that the worker died, and a live lease is not evidence of useful progress.
- Use explicit provider-call deadlines and bounded retries. Show the current wait
  and timeout state; do not add automatic duplicate prompt submissions or replay
  an edit with an unknown commit outcome. Reconcile message and operation IDs first.
- Retry only known-safe failed model attempts within the existing budget. For an
  interrupted turn with possible writes, inspect committed revisions and task/run
  operation keys before resuming. Offer an explicit resume/retry action when the
  outcome is uncertain. Preserve original messages and partial visible responses.
- Native and hosted conversations retain their documented durability. Browser
  accountless conversations currently live only in the tab; add project-scoped
  storage for submitted text, timeline, IDs, and interrupted status if refresh
  recovery is promised there. Never persist model credentials with that history.

Audit provider fallback health separately. The observed `402` should be visible
in administration and subject to bounded route cooldown rather than being retried
on every new turn indefinitely. Preserve configured routing and billing policy;
do not disable or fund a provider as part of this fix without an explicit decision.

## Remove composer suggestions

Remove the automatic `ask about hosting.md` chip and equivalent selection-based
suggestion chips from `PromptBox`. The user requested a plain composer. Remove the
corresponding presentation rule and unused styling/imports where appropriate.
Keep intentional user-supplied context and normal text entry. Do not replace the
chip with another unsolicited suggestion, and do not silently inject the removed
suggestion's contents into model prompts.

## Model and transport tuning after measurement

Do not silently change the owner's model or LocalRouter endpoint. Evaluate a
supported chat reasoning budget or faster model only if measured provider time
remains dominant after removing unnecessary rounds and context. Verify supported
parameters against the actual provider before adding them. Keep worker settings
independent, and run `jazyk benchmark` before trusting a newly selected model.

Measure prompt size and tool-schema overhead before introducing lazy tool catalogs
or history compaction. Keep these as evidence-driven follow-ups. Do not hide tools
needed by a later explicit edit or add repeated tool-discovery model calls.
Preserve batching of independent native tool calls when useful. No change to the
compiler's sequential build/session scheduling is part of this work.

## Acceptance and performance targets

These are proposed targets, not measured results. Establish the baseline first.

| Scenario | Required behavior |
| --- | --- |
| Original general question, no project context needed | One model round, zero project/task lookups, direct answer, no edit |
| Same question explicitly about a saved deployment task | Read necessary task context; explain from evidence; no task execution |
| Question about the hosting document | Read its current contents; no unrelated graph tour |
| `update the relevant section in hosting.md` | Guarded scoped edit and concise confirmation; no repeated discovery or permission request |
| Empty graph | General answer and authorized prose edit remain possible; no automatic compile |
| Compiled requirement edit | Prose and graph stay consistent through the existing edit contract |
| File changed between turns | Hash conflict causes a fresh read and safe retry; unrelated text remains intact |
| Provider streaming available | Answer text appears before the complete response, once, in order |
| Provider streaming unavailable | Accurate activity state; no fabricated token progress |
| Steering during answer or edit | Ordered input delivery; no early completion that loses a message |
| Worker goal session | Existing nudges, gates, evidence, and convergence remain valid |
| Refresh during delayed model call | Accepted second message and running status survive; no duplicate submission |
| Refresh after tool write but before turn end | Preserve the message and show actual commit/recovery state; retry does not repeat the write |
| SSE disconnect and snapshot race | Replay the complete chat timeline without gaps, duplicates, or stale idle state |
| Admission rejected or response lost | Text stays recoverable; reconcile by message ID before retrying |
| Worker crash or expired lease | Durable interrupted state and explicit recovery; original input remains visible |
| Plain composer | No automatic ask-about suggestion chips |

The empty-graph edit regression must assert that `release_work`, `begin_goals`,
compilation, task execution, and unrelated source edits did not occur. Use the
exact two-turn incident fixture in a model evaluation as well as deterministic
tool-path tests. Test scoped authorization on callbacks/tool admission, not just
the visibility of a UI button.

Target at least a 40% reduction in median full-turn time for the reported fixture
on the same model/backend, conditional on a baseline with avoidable rounds. For a
clean one-round baseline, report the actual result rather than forcing that target.
For streaming, target delivery/render within 250 ms of a received text delta under
local controlled conditions, excluding provider generation time. Set hosted targets
from its measured persistence and network baseline. No fixed two-second answer
promise across cloud models and local models.

Correctness remains a release condition. Evaluate the example answer's factual
quality separately; faster must not preserve unsupported absolute claims or record
them as project requirements without checking. Use current authoritative references
when validating the deployment advice during implementation. Do not run deployment
commands, provision resources, or request actual AWS secrets for this fixture.

## Implementation and sub-agent assignments after approval

1. One integration owner creates isolated worktrees, preserves incident evidence,
   and adds refresh/persistence and scope-control regressions before optimization.
   It captures the baseline and
   writes the shared docs and interface decisions for both plans. Agree on turn
   IDs, session purpose, input events, delta events, and completion ordering first.
2. Agent A implements engine loop behavior, conversational payloads, context reuse,
   purpose propagation, and embedded/host ACP steering. It owns `agent_loop.rs` and
   `acp/`. Keep steering and latency commits coherent and separately reviewable.
3. Agent B implements browser and editor changes, including the mailbox, composer,
   incremental display, delivery states, and browser acceptance scenarios. It owns
   the private editor tree and coordinates engine bridge changes with Agent A.
4. Agent C implements hosted instrumentation, durable input routing, model delta
   transport, and PostgreSQL tests. It owns private server changes and coordinates
   shared `hosted.rs`/conversation interfaces with the integration owner.
5. The integration owner reviews cross-repository pins, question-answer handling,
   worker regressions, cancellation, and matched before/after timings. Agents must
   not edit shared files concurrently or independently publish from their branches.

Durable admission, recovery, and prompt-scope fixes land first. Instrumentation
lands before optimization. Prompt and completion fixes land before
the wider streaming transport changes, so their effect can be measured separately.
Use deterministic delayed providers for loop/race tests, actual workers for browser
transport tests, real PostgreSQL with mocked providers for hosted tests, and a small
matched live sample for model latency. Run the relevant engine/editor/server checks,
then finish with the engine release build. Update compiler pins and show an integrated
local preview. Deployment requires a separate deliberate release handoff.

## Documentation targets

Engine docs: `docs/frontends/acp.md`, `gui.md`, `wasm.md`, `protocol.md`,
`docs/compiler/sessions.md`, `tools.md`, and `hosted-runtime.md` where contracts
change. Edit embedded payload files under `docs/compiler/goals/prompts/`; move
revised conversational policy there rather than adding prose constants in code.
Update `docs/TODO.md` when implementation starts.

Private docs: `../jazyk-cloud/docs/editor/editor.md`, `browser.md`, and
`../jazyk-cloud/docs/runtime.md`. Keep UI policy in the private repository and shared
agent/runtime semantics in the engine. Check relative links and anchors and keep
the docs free of em dashes before each coherent commit.
