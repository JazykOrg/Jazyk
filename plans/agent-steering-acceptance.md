# Agent steering and chat recovery verification

Status: implemented locally, 2026-10-29. The owner approved implementation and
requested worktree-only work until a merge instruction. Read with
[steering](./steering.md) and [chat latency](./chat-latency.md).

## Review locations

- Engine: `/Users/matus/dev/jazyk-agent-work`.
- Editor and hosted service: `/Users/matus/dev/jazyk-cloud-agent-work`.
- Local editor preview: `http://127.0.0.1:4320/`.
- Verified engine code: `e6f003f0d8c4239bb2659b3b5240b8baf96686fa`.

Both worktrees use the local branch `feature/agent-steering-recovery`. Final
implementation commits remain local. No merge or deployment was performed.
One earlier engine snapshot was pushed before the owner clarified worktree-only
work. There were no further pushes after that instruction.

The private compiler pins name the local engine commit. Local verification uses
the explicit browser source override and a temporary server dependency override.
No machine-specific dependency path is committed. Publish or merge the engine
before promoting the private pin to a remotely built release.

## Implemented behavior

- General questions no longer receive mandatory task discovery, graph discovery,
  or compiler workflow instructions. Detailed lifecycle instructions apply to
  requested lifecycle work.
- `write_document` provides a hash-guarded prose edit before compilation. It
  preserves document membership, binary restrictions, safe paths, and editor
  delegation. Unreadable existing documents are never treated as missing.
- Explicit chat purpose suppresses worker completion nudges. Workers retain
  their gates and completion behavior. Optional offers do not become new work.
- Conversation instructions are deduplicated across hosted context reopening.
  Invalid graph handles still require a reload before use.
- Native ACP, browser chat, and hosted chat accept ordered steering inputs.
  Already dispatched tools keep their results. Unstarted stale calls receive
  paired skipped results before the new input enters model history.
- Input IDs, delivery states, turn identity, atomic completion admission,
  cancellation, and safe continuation prevent lost or duplicated input. Multiple
  user inputs merge only in model history where a chat template needs alternating
  roles. Their transcript records and delivery events remain separate.
- Hosted initial prompts are durable before acceptance. Native admission is
  flushed to its transcript. Browser conversations and pending messages persist
  under the project identity. Rejected sends preserve the draft.
- Refresh merges saved conversation history with active job events and input
  records. Event watermarks prevent sequence reuse after interrupted snapshots.
  Bounded replay prioritizes the newest events and explicitly reports truncation.
- Diagnostic answers are validated through the existing answer engine. A selected
  owning conversation can handle several answers without a second handler acting
  on the same answer. Unrelated conversations do not receive the input.
- Native SSE and hosted OpenAI/Anthropic SSE deliver incremental answer and thought
  content. Browser model adapters show incremental content without exposing raw
  JSON actions or executing incomplete tool arguments. Body-only responses remain
  supported. Complete responses still drive history and validation.
- Model attempts expose provider, total, and first-content timing where available.
  Billing rejection has a bounded provider cooldown. No configured model,
  LocalRouter endpoint, credential grant, or production setting was changed.
- Send and Stop are separate. Running chat activity opens its conversation.
  Automatic `ask about` suggestion chips are removed.

## Verification

- Engine: `cargo test`, 741 passed, 3 intentionally ignored tests. Native and text
  codecs, skipped-call pairing, input-close races, local model role compatibility,
  safe document writes, context reuse, recovery, and persistence failures passed.
- Engine release: `cargo build --release` completed. The worktree binary's help
  command runs. The primary checkout's executable was not replaced.
- Hosted service: 143 passed, 3 intentionally ignored tests, against the final local engine.
  Tests use a private socket-only cluster and deterministic providers. They cover
  admission deduplication, active refresh, fenced delivery, cancellation, diagnostic
  failures, FIFO continuations, sequence recovery, and incremental SSE.
- Editor: production WASM/Vite build, TypeScript, 465 unit tests, and all 17
  Chrome chat tests passed, including the bounded-replay notice. The preview
  serves the final rebuilt assets.
- Actual blocked browser worker tests passed for steering, refresh without
  resubmission, Stop without restart, and failed admission retaining the draft.
- An actual embedded ACP subprocess accepted a follow-up during a delayed model
  request. External adapter version/capability negotiation has deterministic
  coverage. Live external Claude/Codex sessions were not exercised.
- Changed documentation passed relative-link, anchor, and em-dash checks.
  `cargo fmt --check` still reports repository formatting differences that also
  occur in the unchanged primary checkout. No repository-wide formatting rewrite
  was applied.

Logs and test artifacts are local under the cloud worktree's `.local/` directory
and `/tmp/jazyk-agent-*.log`. The socket-only PostgreSQL cluster uses port `55431`.
The editor preview uses port `4320`. Generated WASM, build output, dependencies,
test reports, and database files are ignored.

## Performance interpretation

The deterministic incident fixture now needs one model round for a general answer
and two for an authorized document edit and its confirmation. Repeating the same
message ID does not execute another turn. This verifies control flow, not a
matched live-provider speedup over the production incident.

The production baseline was approximately 87 seconds for the first answer and
more than 19 model rounds for the follow-up that drifted into compilation. No
new live-provider latency percentage or tail percentile is claimed. Use the added
attempt timings for a matched evaluation after the owner chooses to run it.

## Scope and handoff

Compile/build mirrors remain read-only as specified by the approved first-release
plan. Steering does not change goal scheduling, grants, ratification, or gates.
An interrupted turn with an uncertain write outcome needs explicit recovery;
refresh and transport reconnect never blindly rerun it.

The local implementation is ready for review. Merge, remote publication, service
restart, and production verification wait for the owner's instructions.
