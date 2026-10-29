# Steering an active agent

Status: implemented locally, 2026-10-29. This plan covers the Jazyk engine, its
ACP roles, and the jazyk.dev editor. See the [verification report](./agent-steering-acceptance.md).
The work stays in isolated worktrees until the owner requests a merge.
Read with [chat latency and recovery](./chat-latency.md), which records the live
incident, prompting fixes, durable admission for all messages, refresh recovery,
and removal of automatic composer suggestions. The owner approved implementation on 2026-10-29.

## Intended behavior

A person can send more information while the agent works. A message can correct
an assumption, add a constraint, or answer a question. Sending does not require
stopping the current turn. The agent retains the original task and incorporates
the new instruction at the next safe boundary.

E.g.:

```text
You: Generate the order service.
Agent: Reading the order requirements…
You: Use the existing payment adapter. Do not add a second one.
Status: Queued for the agent.
Status: Delivered to the agent.
Agent: I will use the existing adapter and continue with the order service.
```

The composer stays available while work runs. Send and Stop are separate actions.
The UI distinguishes acceptance from delivery. Delivery means the input entered
the agent's conversation, not that the model understood it or completed the work.
Already committed edits and already dispatched tools keep their actual outcomes.

## What exists

These findings come from the current checkouts, not a live production probe.

| Path | Current behavior | Work needed |
| --- | --- | --- |
| `bootstrap/src/agent_loop.rs` | Shared synchronous loop, cancellation checks, complete tool-result pairing | Input polling and delivery boundaries |
| `bootstrap/src/acp/agent/mod.rs` | Embedded ACP agent holds conversation state through a turn | Independently accessible input queue and extension handler |
| `bootstrap/src/acp/host.rs` | Session task handles permissions during a prompt; other commands queue behind it | Handle steering while reading the active prompt stream |
| `bootstrap/src/acp/proxy.rs` | IDE requests forward through typed handlers; automated sessions are read-only mirrors | Explicit extension routing and truthful capability advertisement |
| `bootstrap/src/gui/chat.rs` | A second prompt while running returns `409` | Separate input admission from prompt execution |
| `bootstrap/src/conversation.rs` | Portable chat calls the same loop and rejects overlapping prompts | Shared input lifecycle for portable execution |
| `bootstrap/src/wasm/bridge.js` | Worker blocks in `Atomics.wait`; cancellation uses shared memory | Input mailbox that works while the worker is blocked |
| `../jazyk-cloud/web/src/chat/ChatPane.tsx` | Sending returns early while running; Stop replaces Send | Working composer and input status rows |
| `../jazyk-cloud/web/src/lib/wasm-client.ts` | Page rejects a second prompt before it reaches the worker | Busy input routing and mailbox producer |
| `../jazyk-cloud/server/src/engine.rs` | Hosted prompts create jobs under worker admission and a project writer lease | Route input to the existing job without taking another writer lease |

## ACP findings

The published [prompt-turn contract](https://agentclientprotocol.com/protocol/prompt-turn)
defines `session/prompt` and cancellation. Cancellation finishes the original
prompt with `cancelled`; it is a distinct operation from adding information.

The current [Codex adapter steering contract](https://github.com/agentclientprotocol/codex-acp/blob/main/docs/steering.md)
documents `_session/steering` as a pre-standard extension. It advertises support
through `InitializeResponse._meta.steering.supported`. Its request carries
`sessionId`, `prompt` content blocks, and optional steering metadata.
The [Claude adapter implementation](https://github.com/agentclientprotocol/claude-agent-acp/blob/main/src/acp-agent.ts)
also implements this extension and its host-owned idle fallback.
These are moving upstream contracts. Pin supported adapter versions and verify
their behavior during implementation. Do not infer support from an agent name.

Use this request when the negotiated adapter supports the idle fallback:

```json
{
  "sessionId": "session-id",
  "prompt": [{ "type": "text", "text": "Use the existing payment adapter." }],
  "_meta": { "steering": { "idleBehavior": "promptRequired" } }
}
```

Map the outcomes as follows:

- `injected`: the input joined the active turn. Its original prompt still owns
  completion, cancellation, errors, and usage. Do not create another prompt owner.
- `promptRequired`: the adapter consumed nothing. Keep the input queued. Wait for
  the outstanding prompt to settle, then start a normal prompt with that input.
- `startedNewTurn`: the adapter started a turn itself. Never resend the input.
  Require an adapter-specific lifecycle integration before enabling this path.
- `failed` or a definite rejection: retain a visible failed input with retry.
- Transport timeout: delivery is unknown. Do not blindly resend an extension
  request that has no negotiated idempotency support.

The support flag alone does not guarantee the optional idle behavior. Enable
live steering for tested versions that support it. Otherwise queue a normal
follow-up until the current prompt settles. The UI states that fallback clearly.
Do not implement steering by submitting concurrent `session/prompt` requests.

## Shared input lifecycle

The engine owns the semantics. Editor adapters transport and present them.

- Address input by project, conversation, client-generated `messageId`, and
  expected `turnId`. Hosted routing also binds it to the authorized job and owner.
- Start with text input. Keep ACP content blocks intact in the ACP proxy. Reject
  unsupported content explicitly in the embedded agent.
- Serialize admissions per conversation. Assign a monotonic input sequence.
  Reusing an ID with the same payload returns the existing record. Reusing it
  with different content fails. Preserve distinct repeated messages with new IDs.
- Track `queued`, `delivered`, `failed`, `cancelled`, and `delivery_unknown`.
  Record the actual owning turn when delivered. Keep a reason for failed inputs.
- Accepting an input and closing a turn use the same admission state. An input
  either joins the active turn or belongs to a subsequent prompt. It cannot fall
  between them or start two continuations.
- Drain inputs in order. Recheck before reporting ordinary turn completion.
  A message arriving after the close boundary starts a follow-up prompt.
- Stop closes admission to that turn and cancels undelivered inputs associated
  with it. Show them with a resend action. It must not trigger an automatic
  continuation after the user stopped. Session deletion closes the queue too.
- Explicitly rejected requests leave the draft available. An ambiguous network
  failure reconciles by message ID before offering a retry.
- Persist admission and delivery records with native transcripts and hosted jobs.
  On restart, recover queued inputs explicitly and reconcile uncertain delivery.
  Do not promise exactly-once delivery across an external adapter crash.
  Browser inputs initially share the existing tab-lifetime conversation scope.

Proposed HTTP surface:

```text
POST /api/chat/sessions/{id}/inputs
  {messageId, expectedTurnId, text}
  -> 202 {messageId, sequence, state, turnId, deliveryMode}

GET /api/chat/sessions/{id}
  -> existing transcript plus current turn and input records
```

`deliveryMode` is `steer` or `next_turn`. Publish numbered `chat.input` events
through the existing stream with the same fields. Snapshot and stream replay use
message IDs to update one row. Preserve `/prompt` for existing clients. Advertise
the new contract through backend capabilities and protocol compatibility checks.
Slash commands retain their current command semantics; they do not silently
become steering text or dispatch a concurrent build.

## Delivery inside the embedded loop

Add a generic input source to `LoopArgs`. Keep it independent of graph concepts.
The queue must be accessible without acquiring the history lock held by the loop.

- Poll before a model request, after its complete response, between tool calls,
  and before an ordinary final return.
- If input arrives during a model request, finish that request, then reconsider
  its unstarted actions with the new input. V1 does not promise token-level
  interruption. Keep partial streamed prose visible as earlier output.
- If input is pending before dispatch, give every unstarted native tool call a
  paired skipped result. Preserve actual results for calls already dispatched.
  Append the human messages after all paired results, then call the model again.
  Apply equivalent ordering to the text codec. Never execute the abandoned calls
  after the revised response.
- Emit delivery only after inserting the input into history. Keep separate input
  records even if a model template requires adjacent user messages to be merged.
- Keep round, context, cost, and failure budgets. Steering does not reset them.
  A budget stop can leave queued messages visible for an explicit continuation.
- Pending permissions stay attached to their original requests. Freeform input
  never grants permission. Permission responses remain on their existing path.

The embedded ACP agent advertises the extension once it implements these rules.
The ACP host forwards steering from its active session event loop. The IDE proxy
passes it only to a writable, owned conversation and advertises only support it
can actually route. Read-only automated mirrors remain read-only in this phase.

## Browser and hosted delivery

### Browser worker

An ordinary worker `postMessage` cannot interrupt the synchronous Rust loop.
Extend the existing shared-memory bridge with a dedicated bounded input mailbox,
separate from the model reply buffer and cancel flag. The responsive page is the
producer. A synchronous portable input hook is the consumer at loop boundaries.
Publish payload bytes before atomically publishing their sequence. Acknowledge
consumption from the worker with input IDs. Reject full or oversized submissions
without claiming acceptance. Bind the mailbox to a project and conversation epoch
so switching projects cannot inject old input into a new run.

The page retains a FIFO for inputs waiting for mailbox capacity or a subsequent
turn. Clearly distinguish local waiting from engine acceptance. Closing and input
admission must handshake with the page so an arrival during the final return
becomes a follow-up. Preserve existing model cancellation behavior. This needs
real worker tests, since a mocked request handler misses the blocked-worker case.

### Hosted jobs

Add a durable input inbox to the private service. Authorize every enqueue against
the account, project, conversation, and active job. The existing job owner drains
it through an engine input hook. A wake notification can reduce latency; durable
records remain authoritative across service instances and lease transfers.

Enqueue must not acquire the project writer lease already held by the run or
consume another compiler worker slot. Only the current fenced owner may deliver
input. Use the normal admission path for a genuine subsequent job. Preserve model
selection, provider limits, usage attribution, and pause/recovery behavior. A pause
retains pending input and does not automatically resume paid model execution.

## Answers to project questions

Freeform steering and an answer to a diagnostic are related but distinct inputs.
Use the existing diagnostic-answer API and its validation for question cards.
An arbitrary chat message does not silently resolve a diagnostic.

- Keep question cards answerable while an agent works. Allow several independent
  answers without waiting for the first handling session to finish.
- Record a targeted answer once with its diagnostic ID and answer identity. Route
  a notification to the active conversation only when it owns the relevant work.
- Deliver the durable answer reference and latest committed generation. Reload
  affected graph context before acting. Revalidate stale staged changes through
  the existing commit gates. Steering never bypasses provenance or ratification.
- Claim answer handling once. The active agent and a background answer session
  must not both act on the same answer. For an unrelated active session, use the
  existing answer workflow under normal scheduling and leases.
- In the blocked browser worker, queue the answer request until a tool boundary,
  execute it through the existing answer engine, then deliver its result. Do not
  announce that the answer was recorded while it is only waiting on the page.

## Editor presentation

Keep the composer enabled during a turn. Enter sends, Shift+Enter adds a newline.
Show Send beside a separate Stop button. Retain the existing agent activity line.
Add each accepted message once to the transcript and update its status in place.
Use `Queued for the agent`, `Delivered to the agent`, and a visible failure reason.
For a fallback adapter, use `Queued for the next turn`. Avoid promising immediate
effect while a model request or tool is still running.

Keep permission controls and diagnostic answers inline and usable. Errors preserve
the submitted text. Reconnecting fetches input records before replaying new events.
Read-only build threads remain visibly read-only; the composer must not redirect
a purported steer into an unrelated chat conversation.

## Implementation order

1. Document the contract in engine `docs/frontends/acp.md`, `gui.md`, `wasm.md`,
   and `protocol.md`, and the diagnostic answer contract where it changes. Update
   private `docs/editor/editor.md`, `browser.md`, and `docs/runtime.md` before
   changing their implementations. Add the new scenario to the live work list.
2. Implement the input state machine, IDs, snapshots, events, and loop boundaries.
   Prove ordered delivery and completion races with deterministic fake providers.
3. Implement embedded ACP steering, host dispatch, and IDE proxy routing. Test
   capability negotiation, idle fallback, permissions, and external adapter errors.
4. Wire native HTTP input admission and the editor composer. Keep the UI feature
   capability-gated until a backend supports the full contract.
5. Implement the browser mailbox and targeted answer queue. Test with the actual
   worker blocked on a delayed model reply and on tool execution.
6. Implement the hosted durable inbox, fenced delivery, and continuation admission.
   Test with PostgreSQL and concurrent authorized requests across service instances.
7. Add question-answer ownership and context refresh. Verify that multiple answers
   do not race a build or produce duplicate answer handlers.
8. Run the full acceptance matrix, release-build the engine, update the private
   compiler pins, and show the integrated local preview. Publication is a separate
   explicitly authorized step.

## Acceptance criteria

- A follow-up sent during a delayed model request changes the next decision.
- A follow-up during a tool preserves its actual result and suppresses stale
  unstarted calls. Native and text histories remain valid and resumable.
- Several follow-ups arrive once and in order, with delivery status per message.
- Completion and enqueue racing repeatedly produce neither lost input nor two
  continuation prompts. Test the race at both engine and page/job boundaries.
- Stop, delete, provider failure, exhausted budgets, permission waits, and project
  switching never leak pending input into another turn or conversation.
- Duplicate HTTP requests and SSE replay do not duplicate messages. Ambiguous
  adapter delivery is visible and never triggers an automatic duplicate retry.
- External agents without steering receive a normal queued next turn. Supported
  agents retain a single terminal prompt owner through injection and idle fallback.
- Native, endpoint-backed browser, in-page browser model, and hosted chat pass the
  same scenarios. Browser tests exercise shared memory and an actually busy worker.
- Two question answers can be submitted during work, each commits once, each gets
  one handler, and subsequent model context sees the committed answers.
- Existing permission, graph gate, account isolation, paused-job, lease, and usage
  behavior still passes its relevant tests. No compiler scheduling authority moves
  into the model.

## Scope after the first release

Steering a compile/build thread requires a separate job-level contract. A build
spans several goal sessions, and changing its scheduler or gates through a chat
message would violate the harness boundary. A later phase can address a job and
explicit batch, validate the instruction scope, and carry it across worker session
changes. It must define what happens when no matching batch is running. The first
release supports writable agent conversations and answering project questions
during work. It does not claim that typing into a build mirror steers the compiler.
