# Goose GDK implementation approval plan

Status: approved for implementation, testing, merge and deployment, 2026-10-29.
The owner also authorized live testing with Ollama or ChatGPT. This document defines
the implementation scope. The
[research and recovery plan](goose-gdk.md) records the supporting investigation.

## Outcome

Replace Jazyk's generic embedded agent machinery with public Goose GDK components.
Use one implementation for native CLI, the bundled ACP agent, hosted compilation
and chat, and browser compilation and chat. Jazyk remains the application. MCP
remains its tool and domain boundary.

A hosted API deployment suspends an eligible job at a durable boundary. The next
instance restores the same job and continues automatically. Completed model
responses and tool effects are reused. User cancellation remains final. An unknown
external outcome is reconciled or paused explicitly.

The owner approved implementation, testing, local master merge, master publication,
production deployment and post-deployment tests. Use isolated worktrees and the
existing local release build infrastructure. Upstream posting remains separate.

## Approval scope

Approval of this plan and the subsequent implementation instruction authorize:

- Update canonical documentation before each behavior change in both repositories.
- Integrate the component set below. Remove the old generic loop, text codec,
  toolshim and fallback behavior after their callers use GDK.
- Implement Jazyk's application operations and durable execution contract.
- Add async execution at session and host boundaries needed by GDK. Keep
  deterministic compiler logic synchronous.
- Implement and test the small GDK compaction portability patch locally. If no
  suitable published release exists, carry only that crate as a documented,
  temporary source dependency until an upstream release replaces it.
- Prepare upstream issue and PR material locally. Posting it is outside this
  approval. Follow Goose's Ready-issue workflow before work intended for submission.
- Run native, browser and local database tests, bounded model acceptance through
  the configured LocalRouter route, local restart tests, and concurrency profiling.
- Delegate independent implementation and review work to subagents after approval.
- Commit coherent changes on local branches. Leave main checkouts intact.

The subsequent instruction authorizes local master merges, master publication and
deployment after verification. It does not authorize new paid infrastructure,
changes to production limits or automatic model/provider reconfiguration.

## Recommended component selection

Start with the matching `=0.1.0-alpha.13` crate group and committed lockfiles. Verify
the selection in the initial build gate. Do not expand to a newer alpha merely
because it is available. Record any necessary version change with its reason.

| Component | First implementation | Host responsibility |
| --- | --- | --- |
| `goose-agent` | State machine, inference, tool dispatch, events and cancellation | Session identity, operation order, admission and persistence. |
| `goose-provider-types` | Messages, usage, public wire formatting, response decoding and retry primitives | Route selection, credentials, transport, timeouts and accounting policy. |
| `goose-providers` | Native LocalRouter OpenAI-compatible client where its public interface fits | Preserve endpoint/config precedence. Hosted and browser inference use injected host transport. |
| `goose-context-management` | Explicit conversation compaction and upstream compaction templates | Triggering, budget reservation, durable history replacement and usage settlement. |
| `goose-sdk-types` | Shared wire types already required by the agent | Preserve Jazyk's existing ACP contract. |
| Skill helpers | Reuse portable rendering where it fits existing Jazyk skills | Keep activation, embedded payloads and workspace access in Jazyk. |

Keep Jazyk's fixed agent contract, goal payloads and domain skills. Do not import
Goose's application prompt, developer extension, session database, full application
facade or private operations.

Do not add local model execution, download management or Python/Kotlin bindings to
this delivery. LocalRouter and existing browser model hosts remain the model hosts.
Do not add unused subagent scaffolding. General agent delegation is a later feature
because compiler batches remain sequential and its capability and budget policy
needs its own contract. This does not prevent implementation subagents doing the
work described in this plan.

## Baselines and repository ownership

| Repository | Inspected baseline | Scope |
| --- | --- | --- |
| Jazyk | `c3384081be76b01101a53ac17a2609e1b915b1af` | Shared harness, MCP, native and WASM engine, ACP, domain checkpoints, CLI and benchmark changes. |
| Jazyk Cloud | `c11e9ae809f2f446e9a1f33fe3f539b429d377ff` | Host persistence, billing integration, job recovery, browser transport/UI integration and deployment procedure. |
| Goose | `9fbb9a48e1d952daede4ba3f234a22bfe9265027` | Public API evidence and the isolated compaction portability change. |

Planning worktree: `~/dev/jazyk-gdk-plan`, branch `plan/goose-gdk-recovery`.
Implementation worktrees are created after approval from an updated, recorded base.
Use separate worktrees for parallel streams. Integrate local commits into one engine
integration branch and one cloud integration branch. Do not push feature branches.

The engine must build without the private repository. During development, point the
cloud worktree at the engine worktree through `JAZYK_SOURCE` and an ignored Cargo
source override. Never commit local absolute paths. The cloud release pins remain
unchanged until the coordinated merge handoff makes the engine revision available.

## Architecture

### Shared harness

Add `bootstrap/src/harness/` as the shared generic agent boundary. Its internal
modules cover provider integration, MCP tools, session/effect storage, application
operations and recovery data. Final filenames may follow the implementation, but
these responsibilities stay separate.

All model-driven entry points use this module:

- `inprocess.rs` for hosted and portable goal batches and answers.
- `conversation.rs` for native, hosted and browser chat.
- `acp/agent/` for the bundled agent exposed to IDE clients.
- `benchmark.rs` for the same native-tool path being evaluated in production.

Keep `runner.rs`, the ACP bridge and external executor selection as application
boundaries. An externally selected executor remains external. The bundled ACP
agent executes through GDK and still receives the same MCP contracts.

Use GDK's state machine directly. Do not add a second generic workflow engine or
another model/tool loop. Jazyk's operations enforce its own policy around public
inference, tool execution and compaction primitives.

### Operation contract

The pipeline implements these responsibilities in this order where applicable:

1. Validate ownership, cancellation, suspension, current policy and remaining budget.
2. Deliver accepted steering once, or yield for a pending approval or resource pause.
3. Execute pending approved MCP calls through durable intentions and receipts.
4. Complete a goal batch or turn when its existing gates permit completion.
5. Compact at a valid conversation boundary when the effective context budget
   requires it. Do not compact across unresolved tool calls.
6. Admit and perform inference. Persist its complete result before its tools run.

Stop and suspension are different signals. Stop uses cancellation and existing
discard/finish rules. Suspension checkpoints an eligible boundary without invoking
terminal cancellation behavior. No new model request starts after drain begins.

Reuse existing limits and model context settings. Do not invent a second set of
budget configuration knobs. Count compaction calls and retries against the same
job and admission limits. Keep retry policy in one layer per failure type. A cloud
provider attempt must not be retried again by an outer GDK wrapper without a new
admission decision and a durable identity.

Approval operations implement existing task grants and editor-mediated consent.
Do not introduce a separate Goose permission mode or a new general approval UI.

Preserve structured reasoning and tool-call metadata required by the provider.
Map GDK events to existing traces and UI events. Drain its event channel and apply
the existing host bounds. Do not place full transcripts in unbounded client queues.

### MCP and filesystem capabilities

Implement GDK's dynamic tool provider over existing `McpServer` list/call behavior.
Keep names, schemas, error results and permissions authoritative in Jazyk. Tool
availability is checked again at execution, including after a resume.

For native external MCP transport, replace the hand-written embedded MCP client
with the maintained Rust MCP SDK. Keep the current ACP protocol SDK. A future
public GDK protocol component can replace this transport if its supported surface
fits; do not depend on the full Goose application for it.

Hosted and browser tools call the in-process MCP server directly. They do not spawn
a server process. Register no Goose shell or native file tools in those profiles.
Native execution keeps the existing task and workspace authorization contract.

The current VFS and hosted execution contexts are thread-local. Keep one scoped
execution on its owning thread and use the dynamic provider to call tools there.
Do not use GDK's native `SyncTool` wrapper, which can move a call to a blocking
thread without its VFS scope. Missing or mismatched hosted scope fails closed.

Audit formatting helpers as part of this boundary. In particular, disable automatic
image-path expansion for compiler text requests. Hosted attachments must resolve
through workspace capabilities. Request formatting must never read arbitrary
server paths.

### Provider and async boundaries

Keep a common GDK provider interface and explicit host transport callbacks. Native
CLI can use the public OpenAI-compatible provider for LocalRouter. Hosted transport
continues to own route policy, admission, credentials, reservations, provider
attempts and settlement. Browser transport continues to own Fetch, browser model
APIs and authentication. Reuse public GDK wire helpers rather than preserve a
second handwritten provider codec.

Use an async session boundary and propagate it through the model-driven compiler
callers that need it. Do not make parsing, graph mutation or goal derivation async.
Keep blocking native entry wrappers where needed by existing callers.

Native scoped execution is driven on its owning thread with a suitable executor.
Do not construct a multithread runtime per session or call nested blocking runtime
drivers from an already-running runtime. Host callbacks must compose as futures,
including checkpoint persistence. Establish this in the initial vertical slice.

Browser model-driven requests use an async WASM entry/driver that allows JavaScript
promises and retry timers to complete. The worker serializes project operations;
Stop and steering remain independently deliverable. Preserve existing deterministic
request routes and the UI event contract. Avoid an interim body-only bridge as the
finished architecture. It may be used only as an exploratory test.

Use the current public browser transport and its streaming events initially. Do
not require a whole `goose-providers` browser port. Browser GDK history must receive
complete native tool calls, usage and reasoning even when UI deltas are delivered
separately by the host.

### Compaction and skills

Use explicit `compact`/`summarize` operations so the summary, history replacement,
usage and recovery state are committed together. The automatic overflow wrapper
alone is insufficient because the host must retain the compacted conversation and
summary cost. Keep GDK's maintained templates. Keep Jazyk's graph loaded-set policy
independent of conversation compaction.

Keep existing skill activation and embedded domain payloads. Use portable upstream
formatting where applicable. Do not scan host directories in hosted or browser
profiles. Keep native skill roots explicit if discovery is used.

## Durable execution design

### Recovery envelope

Define a versioned, owned-data recovery envelope in the engine. It is separate
from graph `status.yaml`, debug captures, client events and process memory.

| Data | Required contents |
| --- | --- |
| Identity | Logical job, conversation, accepted input, command/targets, active batch and attempt identity. |
| Compatibility | Envelope version, engine version, GDK version and operation-set identifier. |
| Conversation | Native GDK messages and metadata, complete saved responses, pending tools, approvals and compaction state. |
| MCP session | Open batch, claimed goals and change identities, staged operations, outcomes/justifications, loaded-set state, delivered skills and repeated-call state. |
| Workspace | Recorded source revision, graph generation, durable storage version and snapshot digest. |
| Progress | Parent command continuation, completed batch receipts, current operation and checkpoint sequence. |
| Accounting | Model request identities, accumulated usage, accounting receipt identities, remaining limits and deadlines. |
| Control | Pending/delivered input identities, event cursor, cancellation and suspension reason. |

Store owned values and reconstruct runtime handles. Do not serialize mutexes,
filesystem locks, futures, credentials or thread-local scopes. Rebuild derived
indexes from the saved state where deterministic. Restore a fresh valid lease and
validate source revision before reopening the batch.

Checkpoint the active MCP session, not merely its conversation. `OpenBatch`,
`ToolSession`, `LoadedSet` and staged `Op` values are not currently a portable
recovery capsule. Add explicit snapshot/restore boundaries. Test the hidden state
that affects gates and repeated calls, as well as the visible staged mutations.

The parent compiler resumes its recorded active batch before selecting new work.
After a committed batch, derive the next board from the saved workspace and reuse
its completion receipt. Generation, binding and verification retain their existing
ledger evidence. Do not restart the entire command with fresh limits or duplicate
a completed batch because the parent stack was lost.

### Authoritative storage and receipts

In the hosted service, use the existing project SlateDB storage for execution
state, MCP capsules and tool receipts. Write the execution transition and affected
workspace state in one durable batch, with a separate internal key namespace.
Internal recovery state does not enter project files, Git sync or graph provenance.
Preserve the existing storage-head and PostgreSQL mirror/recovery discipline.

Recovery transcripts can contain private project data and must use the existing
tenant storage protections. Never persist provider credentials in them. Reserve
space for active recovery state, bound growth, and pause safely if it cannot be
saved. Diagnostic export pruning and ordinary history cleanup cannot delete the
only checkpoint or receipt of a live resumable job.

Checkpoint-only writes do not create a human source revision or project-history
entry. They still require the current writer epoch and execution sequence. Use
immutable content references or bounded deltas where useful; do not duplicate the
whole project into every checkpoint.

PostgreSQL continues to own job admission, project leases, accepted input and money.
Record stable receipt-linked accounting intents in authoritative execution state.
Apply/repair their PostgreSQL effects idempotently. Do not introduce distributed
transactions between SlateDB and PostgreSQL. A recovery test must cover a durable
storage write followed by a failed metadata/accounting mirror.

Native and browser hosts implement the same logical session/effect contract using
their existing local or browser storage. The release requirement for automatic API
restart recovery applies to hosted jobs. Preserve local session state where the
host promises persistence. Browser reload must restore a complete saved state or
report interruption; never silently replay an uncertain effect. This delivery does
not add a background browser service that keeps compiling after the tab closes.

### Side-effect protocol

1. Allocate a model request or tool operation identity in the logical job. Persist
   its request digest and intention before dispatch.
2. Revalidate ownership, cancellation, authorization and budget at the existing
   atomic admission/claim boundary. Dispatch only after the durable claim.
3. Save a complete accepted model response before invoking any requested tool.
   Save its usage/accounting identity even if the next step is suspended.
4. Commit a local tool effect, its result and MCP state as one execution transition.
   A repeated identity returns its saved result. A changed name or argument digest
   under that identity is rejected.
5. For external tasks, reuse the existing operation key and reattach to the same
   task. For other remote writes, use destination idempotency or queryable receipts.
6. Reconcile an operation dispatched before a crash whose outcome is missing.
   If it cannot be proven, retain an uncertain state and pause it explicitly.

Recheck grants and current policy on resume. Persisted approval does not override
later revocation. A stale worker cannot claim a new request after takeover. An
operation validly claimed before takeover is reconciled through its identity.

Provider acceptance before response persistence can leave uncertain output and
cost. Use provider request identities where available. Do not promise exact-token
continuation or exactly-once provider billing. A complete saved response is never
requested again.

### Drain, reclaim and deployment

Keep one writer per hosted project and sequential sessions per document set.
Different projects remain concurrent. In-process harnesses share application code;
no process per hosted agent is introduced.
Idle or suspended sessions retain durable data, not a dedicated worker thread or
an execution permit. Bound active owner threads by existing execution admission.

Add a deployment drain signal distinct from user Stop. Stop admitting new work and
new inference at the old instance. Let in-flight work reach a safe checkpoint
within the grace interval. Persist suspension and release ownership after the
checkpoint. If the process dies, lease expiry permits a new owner to reconcile
and resume eligible jobs. A failed checkpoint cannot be advertised as a successful
handoff.

The claimant atomically checks job state, current cancellation, compatibility,
writer ownership and checkpoint sequence. Recover queued work too. Resume existing
jobs before allowing conflicting writes. Do not convert resumable jobs into
terminal interrupted events merely because an instance restarted.

Add one nonterminal job state, `suspended`, with a structured reason. Normal
deployment suspension and expired ownership are eligible for automatic reclaim.
An `uncertain-outcome` reason requires reconciliation before eligibility. Preserve
existing terminal states and resource-pause semantics. Extend Stop, busy checks,
the reaper, startup claims, event handling and editor status types consistently:

```text
queued → running → suspended → running → completed | failed | cancelled
```

Keep the same job ID and input ordering. Clients reconnect from their durable event
cursor. Show transient suspension through the existing job UI. Do not create a new
conversation, reset spend, redeliver consumed steering or extend task deadlines.
Live partial model text is provisional. After interruption, replace it from the
durable message snapshot using stable message IDs. Do not append a second copy or
pretend an incomplete provider response is a completed checkpoint.

The first rollout is special. Existing legacy agents have no execution checkpoints.
Let them finish before replacing their worker, or delay the cutover. Do not claim
they can be reconstructed midway. The deployment preflight refuses an unsafe
takeover. Completed conversation history stays readable; a new GDK turn starts
with validated history at a turn boundary. Do not build a text-tool execution
compatibility adapter.

Later upgrades and rollbacks must read active checkpoint formats. If incompatible,
delay until affected runs finish. Do not build a multi-version worker router in
this delivery. Keep the last compatible binary and checkpoint backup procedure.
Do not change graph store version or archive a graph to force execution recovery.

## Work packages and delegation

The root agent owns architecture, canonical contract changes, integration commits,
dependency manifests and release pin coordination. Subagents receive bounded file
ownership. They do not merge, push, deploy or modify another stream's files.

### Gate 0: documentation and feasibility

Root work, after approval:

- Update `docs/compiler/sessions.md`, `hosted-runtime.md`, `control-plane.md`,
  `context.md` where needed, `docs/frontends/acp.md`, `mcp.md`, `wasm.md`,
  `protocol.md`, CLI/configuration docs and benchmark documentation.
- Update cloud runtime, model/billing recovery, editor/browser and operations docs
  in the private worktree. Specify serialized data, key ownership, suspend/Stop
  behavior, event changes and model capability refusals before code changes.
- Establish the common session, host callback and recovery types. Demonstrate one
  real MCP goal batch with an injected native provider and one Promise-backed
  WASM round trip. Prove owner-thread tool execution and non-deadlocking callbacks.
- Reproduce the compaction WASM failure. Validate the narrow portability patch with
  native and WASM tests. Retain an exact patch and upstream issue/PR draft locally.
- Record toolchain and wasm-bindgen compatibility. Preserve matching package/CLI
  versions across the engine and editor build.

The inspected GDK manifest requires Rust `1.94.1` or newer. The local compiler is
`1.99.1`, the WASM target is installed, and the editor currently pins wasm-bindgen
`0.2.128`. Verify the same requirements in clean native/server/WASM build
environments. Use matching cargo/rustc binaries; do not change global defaults.

If a published compaction fix is unavailable, place only the normalized public
crate under `bootstrap/vendor/goose-context-management/`. Preserve its license,
original version/source digest and the minimal patch. Use a direct relative path
dependency so dependent cloud builds do not require a root-only Cargo patch.
Document removal when the first suitable upstream release is selected. Do not
vendor the agent, provider suite or full Goose application.

Gate: native and WASM vertical slices work with the agreed host types. Resolve
material architecture changes before splitting parallel work. Do not hand agents
unstable interfaces and ask them to invent competing solutions.

### Parallel implementation wave

| Stream | Ownership | Deliverable and dependency |
| --- | --- | --- |
| A. Shared harness | Engine `harness/`, `inprocess.rs`, `conversation.rs`, `runner.rs`, MCP/session snapshot changes and related tests | One runtime, policies, tool receipts, explicit compaction and resumable domain state. Uses Gate 0 host types. |
| B. Hosted recovery | Cloud `server/src/engine*`, `projects.rs`, `storage/`, model/billing integration, required SQL migration and server tests | Durable transition storage, claim fencing, accounting reconciliation, drain and automatic reclaim. Develop against the frozen engine recovery interface. |
| C. Browser integration | Engine `wasm/` bridge, cloud `web/src/wasm-backend/`, model adapters, capability UI and related tests | Async worker integration, same native-tool harness, durable effects, Stop/steering and browser compaction tests. Uses Gate 0 runtime entry and callback types. |
| Root integration | Engine dependency/build files, `llm.rs`, ACP agent/MCP transport, benchmark/config cleanup and cross-repository coordination | Native LocalRouter and ACP integration, retire old generic transport/codec code, keep manifests and source pins coherent. |

Use at most three implementation subagents alongside the root. Each stream has a
separate worktree. Shared definitions are root-owned; changes are requested through
the root. Stream A owns `mcp.rs`, `tools.rs` and `context.rs` changes. Stream C owns
the WASM bridge. Root owns `Cargo.toml`, lockfiles and protocol-version integration.
Any newly discovered overlap is assigned before editing it.

Order integration by dependency: shared engine commits, then hosted and browser
consumers. Local cloud testing uses the combined engine worktree. Do not cherry-pick
an incomplete consumer as a releasable independent change.

### Verification and review wave

Once implementation streams finish, use available subagents for independent
reviews of recovery, capability isolation and browser/ACP behavior. Reviewers must
inspect another stream and its failure tests. The root resolves findings, runs the
integrated acceptance matrix and prepares the local preview.

Retire `agent_loop.rs` only after all its production callers use GDK. Remove text
codec selection from configuration, benchmark execution, docs and editor controls.
Reject explicit old text-codec settings with an actionable message. Retain old
benchmark results and transcript display as historical data without executing
their old codec. Do not keep an old-loop switch or hidden fallback.

## Acceptance matrix

| Area | Required evidence |
| --- | --- |
| Goal batches | Real MCP begin/load/stage/mark/done calls preserve gates, provenance, identity and atomic commit. Invalid model writes cannot land. |
| Native parity | CLI compile, chat and ledger paths use GDK. A no-op rebuild makes zero model calls. LocalRouter configuration precedence is unchanged. |
| ACP | Initialize/new/prompt/cancel/config/close work through the bundled GDK agent. Tool status, usage, steering and editor-mediated edits remain correct. External executor selection still works. |
| Browser | Actual browser runs an async model/tool round trip, timer retry, compaction and persistence. Stop works during inference; queued steering is delivered once. Native-tool messages survive the host bridge. |
| Model capabilities | A native-tool rejection gives an explicit unsupported capability result. No text downgrade. Verify endpoint sources first. Enable WebLLM/Prompt API sources for agent work only after native-tool support is proven. |
| Compaction | Summary and replaced history remain paired, usage is charged once, pending tool calls stay valid, and restoration preserves required provider metadata. |
| Staged recovery | Restore an open batch with staged mutations, loaded handles, hidden gate state and a current lease. Finish it once without redoing saved model calls. |
| Model response boundary | Kill after response save and before tool dispatch. Resume using the saved response and its accounting identity. |
| Tool boundary | Kill before dispatch, after effect/before receipt, and after receipt/before inference. Prove receipt reuse, idempotent reconciliation or an explicit uncertain pause. |
| Parent command | Restart between batches and after batch commit/before parent acknowledgement. Do not lose the command, repeat a batch or reset limits. Cover compile and ledger work. |
| Hosted ownership | Race takeover with old-owner model dispatch, task submission and save. Stale claims fail. Stop racing with reclaim remains final. |
| Storage/accounting | Crash after authoritative storage write and before PostgreSQL mirror/settlement. Repair from receipts without duplicate usage or lost liabilities. |
| Tasks and grants | Restart with an existing task, reattach by operation key and import once. A revoked grant is rejected. Task deadlines do not restart. |
| Deploy/rollback | Run a local process restart during live work. Same job resumes and SSE reconnects without a false finished event. Incompatible binaries refuse takeover. |
| Isolation | Concurrent projects cannot share tools, model routes, VFS state, receipts or inputs. Host filesystem canaries remain unread/unmodified. Shell and process tools remain unavailable in hosted/browser profiles. |
| Backpressure | A slow UI does not stop execution or grow queues without bound. Cancellation releases admitted work without permitting late mutations. |

Use deterministic mocked providers with real compiler and storage execution for
failure cases. Persist across an actual process boundary for restart evidence.
Use the existing local PostgreSQL and storage test facilities. Do not require AWS
provisioning or production access for these checks.

The current in-memory object store alone cannot prove process restart recovery.
Use the existing injectable object-store boundary with a persistent local test
store supplied by the maintained object-store library. Keep this in the test
harness. Do not add a production storage backend or hand-roll an S3 service for it.

## Verification commands and measured results

Run commands against the isolated worktrees, not the owner's running installation.
The exact focused test filters are recorded with implementation results.

- Engine: `cargo fmt --check`, focused harness/MCP/hosted tests, then `cargo test`
  in `bootstrap/`. Build the native release at the end, as required by AGENTS.md.
- WASM: `cargo check --locked --target wasm32-unknown-unknown --lib`, then the
  release WASM build used by the editor and Promise-backed/browser acceptance.
- ACP and lifecycle: adapt and run `bootstrap/tests/acp_runtime_acceptance.py`,
  `runtime_acceptance.py`, `runtime_lifecycle_acceptance.py` and the relevant
  generation/verification tests. Remove obsolete codec axes.
- Cloud: run focused storage, model/accounting and engine tests, then
  `python3 scripts/dev.py test` against isolated local test state.
- Editor: install dependencies in its worktree, then `npm run build`,
  `npm run test:unit` and relevant Playwright browser/local-backed/hosted tests.
- Goose patch: native tests, WASM checks and Promise-backed compaction tests, with
  formatting and clippy for the proposed upstream change.
- Model acceptance: run the native-tool benchmark against the configured model
  through LocalRouter before trusting it. Run a bounded `f1` compile and relevant
  chat/generation smoke tests only for a model that passes the required capability.
  Do not silently change the endpoint or switch to a paid remote model.

Measure the existing path before replacement and the GDK path on the same fixtures.
Use delayed mock inference to separate harness overhead from provider latency.
Record 1, 8 and 32 active projects, plus 100 idle sessions, subject to local machine
capacity. Record RSS, thread/task counts, time to first model request, scheduling
fairness, cancellation latency, checkpoint latency/bytes and event queue behavior.
Also record optimized compressed WASM size and startup time.

These profiles are measurement points, not new production quotas. A superlinear
memory increase, starvation, growing idle work, leaked permits or unexplained
material regression must be resolved or brought back for review. Do not claim
capacity or performance improvements without these results.

## Risk controls and completion gates

| Risk | Handling |
| --- | --- |
| Alpha API changes | Exact pins, public surface only, upgrade through the same native/browser/recovery matrix. |
| Compaction release timing | Small tested temporary source dependency, explicit patch provenance and removal criterion. No whole-harness fork. |
| Async scope loss | Owner-thread dispatch and actual Promise-backed tests before parallel implementation. |
| Hidden MCP state | Explicit recovery capsules and gate/repeat tests, not serialization of conversation alone. |
| Duplicate remote effects | Stable identities, current admission fencing, destination receipts and uncertain-outcome reconciliation. |
| Browser model limitations | Capability refusals, no text codec fallback, no false claims that every local model can drive tools. |
| Cross-store partial save | Authoritative durable transitions with idempotent metadata and accounting repair. |
| Legacy first deployment | Finish old jobs or delay. No fabricated checkpoint continuity. |
| Scope expansion | Return for approval if a whole provider-suite port, full async domain rewrite, graph migration, new worker platform or persistent fork becomes necessary. |

Finish only when the integrated native, ACP, hosted and browser paths pass the
required acceptance checks, documentation matches, obsolete loop code is removed,
and the relevant worktrees are clean and committed. Report model/environment
limitations separately from implementation failures. An untested recovery path is
not declared complete.

## Review and later merge handoff

Provide the owner with:

- Engine and cloud worktree paths, local branch names and commit IDs.
- A runnable local editor/API preview showing the real MCP workflow.
- A demonstrated suspend/restart/resume of the same hosted job, with event and
  accounting evidence and an external-effect crash case.
- Test results, concurrency/bundle measurements, known limitations and any upstream
  patch still carried locally.
- The exact paired source-pin updates and merge order.

The owner has approved this handoff and the release. After verification, merge the
engine locally into `master` and push only `master`. Pin both cloud compiler inputs
to that published engine revision and validate a clean pinned build. Then merge
and push cloud `master`, deploy the API and editor, and run the post-deployment
checks. Preserve concurrent source changes and the existing model route policies.
