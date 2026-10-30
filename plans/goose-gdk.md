# Goose GDK integration and deployment recovery

Status: investigation and implementation plan, 2026-10-29. No runtime behavior has
changed. The boundaries below record the owner's decisions. The exact component
set remains a later decision.

## Decisions

- Use supported public GDK components. Do not depend on the `goose` application
  facade or copy its private operations into Jazyk.
- Implement application operations in Jazyk. They express Jazyk's policies for
  approval, steering, continuation, limits, compaction triggers, and completion.
- Consider upstream compaction, prompts, skills, subagents, and protocol components
  where they fit. Availability in a roadmap does not make a component usable today.
- Remove the text tool codec, toolshim, and compatibility fallback. Require native
  tool calling. Keep LocalRouter as the configured inference endpoint.
- Use the same embedded harness through the CLI, ACP, hosted service, and browser
  engine. ACP clients use this harness to execute work as well as control sessions.
- Keep MCP as the authoritative tool contract. Jazyk's graph and virtual file tools
  retain their gates, ownership checks, and atomic commit semantics.
- Preserve accepted work across API deployments. Resume the same logical job
  automatically from its latest durable execution boundary.

The [compiler](../docs/compiler/compiler.md),
[sessions](../docs/compiler/sessions.md), [MCP](../docs/frontends/mcp.md),
[ACP](../docs/frontends/acp.md), and [portable engine](../docs/frontends/wasm.md)
remain the domain and frontend contracts. Implementation starts by updating those
documents and the private service's runtime contract before changing code.

## What GDK replaces

GDK supplies maintained generic agent machinery: model and message types, provider
integration, an explicit operation pipeline, inference and tool execution, and
conversation summarization. Jazyk no longer needs to maintain all of that itself.
Upstream fixes and improvements can arrive through dependency updates.

Jazyk still owns goal derivation, scheduling, prompt payloads, graph context, gates,
workspace access, task grants, durable jobs, billing, and deployment recovery. GDK
does not supply the complete Jazyk application or a sandbox.

| Public component | Candidate use | Integration boundary |
| --- | --- | --- |
| `goose-agent` | State machine, inference, tool dispatch, skills and subagents | Implement host session loading and effect persistence. Register only permitted operations and tools. |
| `goose-provider-types` | Messages, tools, usage, provider interface and format helpers | Convert existing MCP and transport values at explicit boundaries. |
| `goose-providers` | Native provider implementations | Use the public OpenAI-compatible provider for LocalRouter where its transport fits. Preserve injected hosted transport and admission through the provider interface. |
| `goose-context-management` | Conversation compaction and its upstream templates | Persist summary, conversation replacement, usage and policy state together. Keep graph loading independent. |
| `goose-sdk-types` | Shared Goose ACP wire types | Already a transitive dependency of `goose-agent`. Map to Jazyk's existing ACP contract at the boundary. |
| `goose-sdk` | Reexports and optional foreign-language bindings | Evaluate only if a real binding consumer needs it. Rust embedding does not require the whole binding facade. |
| `goose-local-inference`, `goose-download-manager` | Local model execution and downloads | Available upstream. No current need to replace LocalRouter. |

These eight crates form the published GDK group at the inspected upstream commit.
Publication does not imply that every crate supports every target.

The generic Goose application prompt is not a prerequisite. Keep Jazyk's fixed
contract and goal payloads as the domain prompt. Adopt published component prompts,
including compaction templates, where their purpose matches. Do not import the
application facade to obtain its private prompts or policy operations.

## Existing seams

- Replace the generic loop and codec handling in `bootstrap/src/acp/agent/` and
  `bootstrap/src/llm.rs`. Keep the public ACP bridge and the external-executor path.
- Retain `session.rs` prompt assembly and goal payload files. Retain `context.rs`
  graph loading. Conversation compaction and the loaded graph are different state.
- Supply an in-process tool provider over the existing MCP list/call contract.
  Calls can remain direct within the process. Remote MCP transport should use the
  maintained Rust SDK rather than another hand-written transport.
- Keep cloud and browser workspace edits behind Jazyk's virtual filesystem. A tool
  called `edit` must mutate that workspace through its existing contract.
- Preserve per-job model admission, transport injection, usage settlement, accepted
  inputs, delivery receipts, and UI events. GDK values must flow through these
  existing host boundaries.

Application operations remain small explicit steps around the public GDK pipeline.
Jazyk decides when to stop, compact, accept steering, request approval, or continue.
If upstream later publishes an operation that fits, evaluate replacing the mechanics
while keeping Jazyk's policy and persistence contracts.

## Concurrency, isolation, and browser use

Use in-process harness instances as the first native integration. They share compiled
code and the runtime. Bound admission by active model calls, memory, tool concurrency,
and retained context. An instance still consumes memory proportional to its history
and loaded workspace. No per-agent memory or throughput claim has been measured yet.

Retain the hosted service's one writer per project, including shared files and
configuration. Retain sequential compiler sessions per document set. Different
projects can execute concurrently under their existing leases. Measure idle and
active memory, cancellation latency, event backpressure, and fairness under a
representative concurrent load before selecting capacity limits.

The existing virtual filesystem and execution scopes use thread-local state. A
default tool wrapper that moves work onto an arbitrary blocking thread can lose
that scope. Run tool calls on their owning execution thread, or pass an explicit
workspace capability through a supported boundary. Never fall back to host files
when a hosted workspace scope is missing.

The hosted harness exposes an allowlist of Jazyk MCP tools. Do not register local
developer shell/file tools, process-spawning extensions, automatic plugin loading,
or unrestricted filesystem access. Subagents inherit a permitted subset of the
parent capabilities and budgets. Do not configure credentials through global
environment mutations.

An in-process library has the host process's OS privileges. Tool restrictions limit
model-directed actions but are not protection against a library exploit. Project
commands remain in the existing isolated task runtime. Use a restricted worker
process or container if the threat model requires isolating the harness itself.
This can host multiple sessions; a process per session is not required.

Browser support is a separate acceptance gate. The `goose-agent` core has WASM
support. That does not establish support for every provider or the compaction
crate. The earlier inspection found a WASM compaction `Send`/`Sync` obstacle.
Prove the selected component set with browser inference, tools, cancellation and
compaction before removing the existing portable loop. Keep browser transport and
storage supplied by the browser host. Do not create a second tool dialect.

## Deployment recovery

### Existing protection and the missing guarantee

The private service already has durable jobs, project writer leases, revision and
epoch fencing, ordered events, atomic job finalization, and task operation keys.
Its current shutdown cancels active workers and records interruption. Retry starts
from the saved workspace. This preserves committed work but does not automatically
continue an in-flight agent conversation or staged MCP batch.

Inspection baseline: `jazyk-cloud` commit
`9adc84f0c18844d198d26270983ee576a377e0f5`, especially `docs/runtime.md`,
`server/src/engine.rs`, `server/src/projects.rs`, `server/src/task_runtime.rs`, and
`server/src/models/transport.rs`.

The optional model debug export records responses but is redacted, quota-limited,
and prunable. It cannot be the authoritative recovery log. Workspace save at the
end of a request is also too coarse to resume a staged agent session.

### Durable execution boundaries

Persist a protected execution checkpoint separately from diagnostic exports. It
contains the data required to reconstruct the same job, not a process memory dump:

- Job, conversation, accepted input, batch, model request, and tool call identities.
- The operation cursor, conversation, completed model responses, compaction state,
  pending approvals, delivered steering, and pending tool intentions and results.
- MCP session state, including staged mutations, batch claims, loaded handles and
  workspace revision. History alone cannot reconstruct an open changeset safely.
- Selected model and policy, accumulated usage, billing reservations and settlement
  identities, remaining budgets, deadlines, and execution versus suspension clocks.
- Workspace generation, writer epoch, event sequence, cancellation state, and the
  checkpoint format and runtime versions.

Record a complete accepted model response before dispatching any tool it requests.
Save each tool intention with a stable operation key before dispatch. Commit the
tool result, affected staged or committed workspace state, and the corresponding
receipt before sending the next model request. Commit each complete GDK effect
batch atomically in the authoritative execution state, with references to durable
workspace versions and usage intents. Reconcile accounting in its own store through
receipt-linked idempotent writes. Do not require a distributed transaction across
workspace storage and PostgreSQL. Network calls execute outside transactions.

Persist uncommitted MCP changes separately from canonical graph state. Checkpointing
must not publish an incomplete goal batch or bypass the existing commit gate. A
restored session resumes its staged batch against the recorded workspace revision
under a fresh valid lease.
If the revision changed, reconcile explicitly rather than apply stale writes.

Compaction is also a durable transition. Save its summary, replaced history,
provider usage, and accounting intent as one transition. Settle that intent
idempotently in the accounting store. Do not discard summary usage.
Debug-export deletion cannot remove a live job's recovery state. Retain checkpoints
for the lifetime of a resumable job, with tenant isolation and protected storage.

### Effects and uncertain outcomes

For a graph or virtual file mutation, commit the effect and its receipt together.
A repeated operation key returns the recorded result. Task submission reuses the
existing operation key to reattach to the same task run, including a Fargate task
that continued while the API restarted. Do not restart its external deadline.

For remote effects, use destination idempotency or queryable operation receipts.
If a process dies after an external effect but before its receipt, reconcile the
outcome. If neither mechanism exists, pause that operation as uncertain. Never
blindly replay a potentially completed publication or other external write.

A provider request can also be uncertain if the provider accepted it before a
complete response was saved. Reconcile by provider request identity where supported.
Otherwise expose the uncertainty and preserve unsettled accounting. Do not promise
exactly-once model billing or resume from an arbitrary generated token. A saved
complete response is reused without issuing that request again.

### Planned API deployment

1. Close admission for new execution and stop claiming jobs on the old instance.
   Continue the status and Stop paths, and persist accepted input or reject it
   clearly during the drain.
2. Let in-flight model and tool operations reach a safe durable boundary within a
   bounded drain interval. A checkpoint write failure prevents a graceful handoff.
3. Mark the logical job as resumable. Deployment suspension is distinct from user
   cancellation, completion, and failure. Do not emit a terminal finished event.
4. Release ownership only after checkpoint commit. Fence the old owner by epoch;
   lease expiry is the fallback after an abrupt death.
5. The replacement worker validates compatibility, claims a fresh lease, restores
   the checkpoint, reconciles pending effects, and continues automatically.
6. Clients reconnect using the same job and event cursor. Preserve input ordering,
   conversation identity, usage, budget caps, and cancellation decisions.

User Stop remains final and never auto-resumes. A Stop received during the handoff
must win over a later resume claim. Deployment does not reset spend limits, create
a second logical billed turn, or redeliver already consumed steering.

Validate current ownership and checkpoint sequence before model dispatch, external
task submission, executing an approval, and checkpoint commit. A stale worker must
not begin a new paid request or external effect after takeover. Use the existing
atomic admission and operation claim boundaries to fence dispatch as well as saves.

Start with the existing API and worker deployment topology. Bounded drain,
checkpoint, restart, and automatic reclaim can work without introducing a workflow
platform or a separate worker service. If workers later become separate services,
the same contract applies. Zero HTTP downtime is a separate availability objective.

After an abrupt kill, resume the latest committed boundary. Reconcile uncertain
in-flight work before continuing. Safe recovery takes precedence over replaying an
unknown external effect just to keep the job running.

### Upgrade and rollback compatibility

Version the recovery envelope independently of graph `status.yaml`. Record the
compiler, selected GDK release, operation set, and serialized state formats. The
deployment preflight checks whether the candidate can restore every active format.

If it cannot, retain a compatible worker to finish those jobs or delay the upgrade.
Apply the same check to rollback. Do not archive a graph, discard a checkpoint, or
silently start a fresh session to make an incompatible binary accept a live job.
The graph's no-migration rule remains unchanged.

### Recovery acceptance checks

- Deploy with a live model call, an open MCP batch, a pending approval, and queued
  steering. Resume the same jobs with their identities, budgets and staged state.
- Kill before tool dispatch, after an effect but before its result is saved, and
  after a saved result but before the next inference. Confirm receipt reuse or an
  explicit uncertain outcome, with no duplicated effect.
- Restart while a task runs. Reattach to the same task and import its result once.
- Kill during compaction and during effect persistence. Restore a complete prior
  state or a complete new state, never a partial transition.
- Reject late writes from the old worker. Race Stop against resume. Confirm a
  cancelled job never resumes and queued input is delivered once.
- Race takeover against an old worker's model request and external task submission.
  Confirm a stale owner cannot claim new dispatch. Reconcile an operation that was
  validly claimed before takeover through its existing operation receipt.
- Resume event streams without a false terminal event. Preserve cumulative costs
  and limits. Test both compatible upgrade and rollback with active checkpoints.

These checks are release gates for automatic recovery. A conversation restore demo
alone is insufficient.

## GDK maturity and upstream maintenance

Snapshot: 2026-10-29. Upstream inspected at
`9fbb9a48e1d952daede4ba3f234a22bfe9265027` in `~/dev/goose`.

| Signal | Verified public state | Consequence for Jazyk |
| --- | --- | --- |
| Public SDK direction | Announced June 16, 2026 | The embedding effort is recent. |
| First provider/SDK publication | July 2, 2026 | About four months of published history. |
| First agent/context publication | August 13, 2026 | About eleven weeks of public runtime history. |
| Latest published group | `0.1.0-alpha.13`, October 24, 2026 | Still an alpha dependency. |
| Agent releases | Eight publications, August 13 through October 24 | Active delivery, limited longevity. |
| Observed release intervals | Approximately 4 to 15 days, average 10.3 days | Observation, not a promised release schedule. |
| Compatibility | Upstream explicitly permits API changes between alpha releases | Budget for integration updates. |

The earlier ACP client called `goose-sdk` is not evidence of an older embedded
agent runtime. Alpha numbering also skips versions: the provider/SDK packages have
ten published versions, while agent/context have eight.

The October 20 Q4 roadmap targets a supported GDK release in Q4 2026. It includes
public tools and approval, compaction, skills and subagents, steering and
cancellation, MCP, and ACP. Goose should consume these public components itself.
The target also includes maturity guidance, examples, and integration coverage.
This is a quarterly direction, not a fixed 1.0 date or a support contract.

GDK has a change-triggered release PR workflow and tag-triggered publication. The
full Goose application's weekly release process is not a GDK schedule. No public
GDK LTS period, fixed release day, or guaranteed compatibility window was found.
Alpha.12 changed cancellation finalization, emitter methods, and a yield enum.
Upstream improvements are usable, but upgrades are not assured to be drop-in.

Two open issues are directly relevant to recovery:

- [#11808](https://github.com/aaif-goose/goose/issues/11808) reproduces a repeated
  tool dispatch when the process dies between its side effect and result persistence.
  An embedding report also confirms that stable tool call keys can deduplicate the
  actual effect. Jazyk must own this contract regardless of an upstream fix.
- [#13080](https://github.com/aaif-goose/goose/issues/13080) reports that the full
  application's session manager does not atomically persist a whole effect batch.
  It describes a source review and a proposed fault-injection scenario, not an
  executed reproduction. Jazyk's own effect handler must provide atomic batches.

Recommendation: keep GDK as the preferred integration candidate. Adopt public
pieces behind Jazyk's host boundaries, pin exact versions, and make recovery and
browser checks prerequisites for replacing the current production path. Do not
infer runtime reliability from the older full application's age or popularity.

## Implementation sequence

1. Update canonical engine and private runtime documentation. Define the checkpoint
   and receipt contract, cancellation semantics, compatibility policy, and exact
   public component set.
2. Build one native vertical slice with a real Jazyk MCP batch and LocalRouter.
   Exercise the same harness through ACP. Remove the text codec and compatibility
   code as part of the replacement.
3. Add hosted effect persistence, staged MCP checkpoints, tool receipts, usage
   recovery and lease takeover. Pass deployment and crash checks before cutover.
4. Prove the selected browser component set. Resolve public crate portability gaps
   upstream where practical. Do not depend on private application internals.
5. Evaluate optional public prompts, skills, subagents and additional providers.
   Measure concurrent server sessions before choosing production capacity limits.

Pin all selected GDK crates to the same exact published alpha and retain the lockfile.
Perform deliberate upstream updates with native, hosted, ACP, browser, compaction,
security and recovery integration checks. These verify host contracts rather than
mirror upstream implementation. Prefer upstream fixes over a long-lived fork.

## Primary sources

- [SDK direction, June 16](https://github.com/aaif-goose/goose/discussions/9811).
- [Q4 roadmap, October 20](https://github.com/aaif-goose/goose/discussions/13028).
- [SDK publication history](https://index.crates.io/go/os/goose-sdk) and
  [agent publication history](https://index.crates.io/go/os/goose-agent).
- [Public reference and alpha warning](https://github.com/aaif-goose/goose/blob/9fbb9a48e1d952daede4ba3f234a22bfe9265027/documentation/docs/gdk/reference/index.md).
- [Agent changelog](https://github.com/aaif-goose/goose/blob/9fbb9a48e1d952daede4ba3f234a22bfe9265027/crates/goose-agent/CHANGELOG.md).
- [Published crate group](https://github.com/aaif-goose/goose/blob/9fbb9a48e1d952daede4ba3f234a22bfe9265027/release-plz.toml).
- [Release PR workflow](https://github.com/aaif-goose/goose/blob/9fbb9a48e1d952daede4ba3f234a22bfe9265027/.github/workflows/gdk-release-pr.yml)
  and [publication workflow](https://github.com/aaif-goose/goose/blob/9fbb9a48e1d952daede4ba3f234a22bfe9265027/.github/workflows/gdk-release.yml).
