# Persisted open work and soft node locks

Status: implementation plan, 2026-11-17. This document records owner direction and
implementation decisions. It does not claim that the behavior is implemented.

## Scope and integration baseline

This plan branches from `feat/parallel-work` at `6d878ec8`, in the isolated
`plan/open-work-soft-locks` branch and `/Users/matus/dev/jazyk-open-work-plan`
worktree. The other effort's worktree is not edited. Implementation starts from
that branch's integrated successor, or from `master` after it merges. Before
implementation, record the actual base SHA and inspect intervening changes.

The baseline is [parallel work](parallel-work.md). Keep concurrent sessions,
multiple batches per serving, executor `max_sessions`, release gates, tier
barriers, bounded store commit locks, and informational `openBy`. Replace its
conflict refusal and ephemeral open-task records with the contracts below.

This turn delivers the plan. The specification edits, engine implementation,
behavioral tests, full Rust validation, and implementation integration are the
following work. No dogfood compilation or graph mutation is part of planning.

## Decisions

- Every accepted staged call writes through before its successful reply.
- Uncommitted graph work survives crashes, disconnects, stops, and model failures.
  Only explicit commit or discard removes the recoverable changeset.
- Soft locks are warnings derived from live open work. They never claim a node,
  hide a goal, refuse a mutation, or prevent a commit. No explicit override or
  additional review goal is required for overlapping work.
- Commit replays field changes on the current graph. The last committed writer
  wins an overlapping field. Disjoint fields survive.
- Overwrites are reported to the committer, journaled, and delivered to other
  affected open work. They are not a `commit-conflict` refusal.
- A committed field changed since the snapshot and overwritten with a different
  value creates a sticky `concurrent-overwrite` diagnostic at `info` severity.
  Identical values, disjoint fields, and staging overlap alone do not create it.
- Existing integrity, provenance, scope, approval, and completion gates remain.
  No concurrency rule grants authority or bypasses referential integrity.
- Open work is local runtime data. Native uses `local/open/<work-id>/`, hosted
  uses its record store, and browser uses its in-memory workspace.
- Heartbeat cadence is 30 seconds and inactivity TTL is 120 seconds, defined in
  `limits.rs`. Expiry releases warnings, not data. No automatic deletion.

## Deltas against parallel work

| Baseline | Decision here |
|---|---|
| First commit wins; later overlapping commit refuses and drops conflicting staged operations | Later valid commit applies, last writer wins per field; staged operations are never silently dropped |
| Node conflicts for most non-additive updates | Explicit field patches merge on the current node, including requirements, criteria, diagnostics, views, and sections |
| A previously resolved goal conflicts | Revalidate the later claim on the current candidate graph; a prior resolution alone never refuses it |
| Open task YAML is refreshed and swept on expiry | A stable work directory retains manifest and append-only events after expiry |
| Worker/task identity names work | Opaque stable `workId` names work independently of batch, worker, PID, and transport |
| Normal EOF implicitly finishes; cancellation discards | Transport EOF, Stop, timeout, and model exhaustion preserve staging and stop heartbeats |
| `unstage` is proposed but conflicts also drop operations automatically | Explicit `unstage` appends a withdrawal event; only the caller chooses what to remove |
| Live workers provide `openBy` | Live persisted work provides `openBy`; expired work remains available through discovery |
| Conflicts cost repair attempts | Informational overlap and overwrite notices cost no attempts; genuine gate failures retain existing budgets |

Keep `expectedHash` on deliverable files and old-text guards on prose edits. This
plan changes staged graph commit semantics, not arbitrary filesystem overwrite
policy. Ledger rows retain their existing safety gates; route any staged graph
operations from those lifecycles through the same open-work path. Do not silently
extend unconditional overwrites to source files or test evidence.

## Persistent record

Native layout, relative to the document-set directory:

```text
local/open/<work-id>/
  manifest.yaml
  events.jsonl
  base.yaml
```

`manifest.yaml` contains format version, work ID, document-set identity, state,
owner epoch, owner identity, batch and goals with their original `change`, snapshot
generation, configuration hash, creation time, last activity, heartbeat,
`expiresAt`, and last complete event sequence. `base.yaml` contains the immutable
opening graph snapshot needed to restore the original view and validation context.
It uses the existing serializable store representation, excluding runtime caches,
credentials and process resources. It is local and is not a new graph format or
store-version migration. Prefer a complete snapshot initially to a new mechanism
for lazily reconstructing missing historical fields.

Owner identity includes worker ID, client name, conversation ID and display name,
session ID/name, optional parent agent, PID and host where available. Missing
values are explicit, not guessed. Identity enables out-of-band coordination;
Jazyk sends no messages to other agents. A work ID is not an authentication token.
Existing project access controls and owner receipts govern reattachment.

Events are numbered and append-only. Each event has a stable operation ID, time,
owner epoch and actor. Event types include stage-call, goal-outcome, unstage,
heartbeat, adopt, conflict-notice, notice-acknowledgment, commit-intent, committed,
and discarded. A stage-call contains all mutations from one successful tool call,
their stable mutation IDs, affected targets, and response identity mappings.
Persist goal claims, justifications, evidence, accumulated budgets and spend too.
Reconstruct staged caches by replay; do not persist competing mutable copies.

The manifest is an atomic projection of complete events. Recovery trusts complete
log records beyond a stale manifest. Ignore an incomplete trailing record with a
warning; reject and retain a corrupt interior record. Unsupported versions remain
inspectable and cannot mutate or be automatically discarded. No read silently
repairs or sweeps away recoverable work.

Native appends flush to durable storage before success, using existing filesystem
and serialization crates. Atomic manifest replacement follows durable append.
The hosted adapter must acknowledge durable append before the tool succeeds.
Browser acknowledgement means survival in that engine's workspace lifetime; a tab
close cannot promise native crash durability without host snapshot persistence.
Document this capability explicitly.

## Lifecycle and concurrency

Opening work persists the manifest and base before returning `workId`. Opening a
batch does not itself soft-lock its goals' target nodes. Only staged mutations do.
Conversational atomic graph edits use this same machinery with an empty goal list.
Build and ledger visibility records must not masquerade as staged node locks.

For each mutation call: normalize and validate against the snapshot plus own staged
work, prepare the whole call in a temporary session, compute its touched targets,
scan other live work, append the complete accepted call, then install the prepared
session state and return success with warnings. An invalid call or persistence
failure installs nothing. Multi-operation tools remain all-or-nothing. Durable
operation IDs make a retry after a lost reply idempotent, including minted IDs.

A typed target function covers every mutation variant. It includes entities,
requirements, criteria, views, diagnostics, section coverage and anchors, both
sides of merges, and collateral mutations such as reparenting or cascading delete.
References merely read are not locks. Canonicalize redirects when comparing
existing targets. Resolve natural-key creates at commit and include the surviving
target in conflict reporting. Test coverage must fail when a new mutation variant
lacks target/patch semantics. Derived projections do not acquire their own locks.

The node-to-work index is derived by scanning complete live work records. An
in-memory cache may accelerate a request but is not a separately persisted index.
Use short storage synchronization around stage publication and warning discovery
so concurrent stages cannot both miss each other. No model execution or network
wait holds the graph commit mutex. Use a consistent metadata-then-commit locking
order, or one short shared critical section, to avoid deadlock with adoption.

The later staging reply names the earlier work immediately. The earlier work gets
an overlap notice on its next tool reply, even if its earlier stage preceded the
second agent. Thus both receive warnings without requiring retroactive delivery.
Warnings name node/section, work ID, owner identity, batch/goals, staged mutations
on that target, and last activity. The warning explicitly says the change was
staged successfully and explains the commit-order consequence: if the other work
commits first, this work's commit reply will present any merge conflicts with
those committed changes. Disjoint fields merge; overlapping fields use the later
committer's value and report what it overwrote. Existing integrity gates still
apply. The warning requires no override or review acknowledgment.

E.g.:

```text
Staged successfully. WARNING: ent:order also has staged changes in work w-other
(worker agent-2, client Codex, session orders, batch b412-1; last active 10s ago).
If that work commits first, your commit reply will report any merge conflicts
with its committed changes. Disjoint fields merge; your later commit wins
conflicting fields and reports what it overwrites, subject to integrity gates.
Use read_open_work(w-other) to inspect its staged mutations.
```

Reads carry one short note plus work IDs; full mutation details come from
`read_open_work`. Overlap does not create mandatory review goals or mark adjacent
nodes dirty merely to require acknowledgment. Normal commit dirtiness and goal
derivation still follow actual graph changes.

A heartbeat refreshes only work still attached to an active execution. A connected
server must not refresh abandoned or stopped conversations indefinitely. Stop
fences late callbacks, ends heartbeats and preserves work. Expired work has no
soft locks and no active `openBy`; its bytes remain until explicit adoption or
discard. Clock-skew or malformed times surface as record warnings. Use an injected
clock in tests. Heartbeats alone never create graph generations.

Resume reattaches with `workId` and the current owner receipt. It restores the
original snapshot, events, scope, outcomes, budgets and pending notices, not a
silently updated base. A caller that cannot prove ownership must explicitly adopt
expired work. Adoption atomically increments the owner epoch, records previous and
new identities, refreshes expiry and journals the handoff. A resumed old process
cannot stage, heartbeat, discard or commit with its obsolete receipt. Expiry alone
also requires reattachment or adoption before further mutations.

Abandon/discard removes staged payloads and releases warnings. Preserve only a
small lifecycle audit receipt with work ID, actor, reason and time. Adoption and
discard append to `local/journal/open-work.jsonl`, separate from graph-generation
entries; hosted uses the corresponding record stream. This records lifecycle
changes without fabricating a graph commit or keeping discarded mutation content.

## Commit algorithm

Run the common algorithm in the store layer for MCP, internal sessions,
conversations, hosted and WASM. The existing MCP-only conflict precheck is not the
authoritative implementation.

1. Verify owner epoch, configuration, scope and approval. Acquire the bounded
   commit mutex and load the current store inside it.
2. Fold staged mutations into explicit patches against their original base.
   Omitted fields remain omitted. A supplied identical value is still write intent
   and wins if the current committed value differs. Preserve per-call ordering.
3. Reconcile creates by natural key, follow redirects, and remap all affected IDs
   and evidence. Never resurrect a deleted node implicitly. A missing target with
   no redirect returns an integrity repair message and retains staging.
4. Apply only intended fields to the current graph. Scalar fields replace.
   Attributes merge by attribute name. Explicit collection additions/removals apply
   by element identity; a whole-list replacement replaces that field. Coupled
   statement/provenance/quote edits validate together. Do not replay an entire stale
   node over the current one.
5. Compare each intended field with its base and current value. Record an overwrite
   when another commit wrote that field since the snapshot and this commit changes
   its current value. A delete records the node fields it removes, including
   intervening edits. For repeated staged edits, report the committed value replaced
   by the final effective patch, not intermediate values from this work.
6. Revalidate every goal claim and batch gate on the current candidate graph. Two
   claims for the same goal may both pass. A closed goal is not an error by itself;
   a changed target/change requires current evidence and cannot clear newer work
   merely by matching the goal ID. Retain the original claim in the audit and report
   an actual gate failure with a repair message. Never invent a successful claim.
7. Compute derived data, sweep, dirtiness and informational overwrite diagnostics.
   Publish graph changes and the graph journal through the existing commit path.
   Include work ID, owner identity, owner epoch, commit operation ID,
   `rebased_from`, and structured overwrites.
8. Finalize work as committed, release its soft locks, and return success with
   `overwrites` and `conflictNotices`. Other open work is never awaited.

Each overwrite contains canonical target, field path, base value, prior committed
value, replacement value, prior generation, prior author/work, and committing
work/author. Use an explicit missing-value marker distinct from null. Existing
journal entries may lack author; return `unknown`, never infer it from batch.
Journal attribution must be added for human and mechanical graph writers too.

A sticky informational diagnostic uses a deterministic ID per commit and affected
node, with structured field conflicts. It survives unrelated commits and normal
checks. Human triage can resolve it. Do not emit Telegram or block readiness or
convergence for these information findings. Preserve the journal when the affected
node is deleted; use the diagnostic model's existing deleted-subject handling.

## Notice delivery and commit recovery

Notify every other open changeset touching any node changed by a commit, including
unexpired work editing a disjoint field and retained expired work. Distinguish a
node-change notice from a field overwrite. Notices include commit identity,
changed fields and relevant overwrite records. Staging remains intact.

A durable delivery cursor per work makes notices appear on the next tool reply,
including errors and reads. `done` repeats its pending and previously delivered
unresolved conflict notices, so a read cannot consume the only warning before
commit. Deduplicate by commit ID and target. Notices are data, not instructions
from another agent.

The commit journal is authoritative for notice recovery. A crash between graph
commit and recipient-event append must not lose notification: derive missing
notices by scanning graph entries since the recipient's cursor, using its retained
staging history. Mark cursors only after delivery is durable. Terminal open-work
records and commit operation IDs make a lost `done` reply retry return the original
commit receipt rather than applying mutations twice.

A commit intent plus journal receipt allows recovery after a crash before work
cleanup. Recovery must also exercise the existing `.committing` partial-file
window. Do not remove staging merely because a commit intent exists. Reconcile
against a completed graph receipt; recover/retry an interrupted commit under the
store mutex before accepting another write. If the existing multi-file commit
path cannot recover this deterministically, implement that recovery as a required
prerequisite rather than claiming atomicity from individual file renames.

## Tool and frontend contract

All operations remain document-set scoped and respect project authorization.

| Surface | Contract |
|---|---|
| `open_work({node?, goal?, state?, cursor?, limit?})` | Paginated summaries, active by default; explicit expired/all discovery; filters compose |
| `read_open_work({id, cursor?, limit?})` | Manifest, ownership history, ordered staged mutations and outcomes, notices, stable mutation IDs; no heartbeat side effect |
| `begin_goals({workId?, ...})` | With work ID, resume owned work; otherwise open new work and return stable `workId` alongside batch |
| `adopt_open_work({id, expectedEpoch, reason})` | Explicit expired-work takeover, atomically journaled; rebuild context and scope without granting approvals |
| `discard_open_work({id, expectedEpoch, reason})` | Own or expired work only; atomic receipt and payload removal; active foreign work refuses |
| `unstage({workId?, mutationIds})` | Withdraw an atomic staged call or a dependency-safe group; replay and revalidate remaining work, retaining it unchanged on refusal |
| `done` and staged writes | Select by `workId` when ambiguous; retain baseline `batch` selection within a serving; return warnings, notices and commit overwrite fields |
| Graph reads | One-line staging note naming work IDs; committed snapshot semantics remain explicit |
| Board/web API | Structured `openBy` entries from live work, identity, work ID, activity/expiry; never filter ready goals |

One batch can have many work IDs. Work selectors never assume a derived batch ID
uniquely identifies persisted work across workers. Invalid or ambiguous selection
returns the available IDs. `unstage` cannot silently remove dependent creates or
claims; its repair response names dependencies. `abandon_goals` delegates to discard
for the selected own work. Read-only servings expose discovery only.

Reuse current HTTP/SSE change surfaces to announce open-work changes. Hosted record
changes must wake board consumers without depending on filesystem events. Engine
API fields are in this repository; editor cards and browsing UI belong to
`jazyk-cloud` and require a separate implementation. No UI completion is claimed by
adding fields here.

## Documentation and code order

Each implementation slice updates its requirements before code. Read the
[prompt guide](../docs/compiler/goals/prompts/README.md) before model-facing edits.

1. Update [graph](../docs/compiler/graph.md),
   [sessions](../docs/compiler/sessions.md),
   [control plane](../docs/compiler/control-plane.md),
   [folder layout](../docs/compiler/jazyk-folder.md),
   [tools](../docs/compiler/tools.md) and
   [tool schema](../docs/compiler/tools.schema.yaml). Specify persistence, field
   semantics, warning versus failure shapes, lifecycle events and recovery.
2. Update [MCP](../docs/frontends/mcp.md),
   [reconciler](../docs/compiler/reconciler.md),
   [hosted runtime](../docs/compiler/hosted-runtime.md) and
   [WASM](../docs/frontends/wasm.md). Remove contradictory implicit-finish,
   stop-discard, worker-expiry sweep, conflict-refusal and one-batch wording.
   Review compilation, ACP, runtime, CLI and generated agent guidance for the same
   assumptions. Do not hand-edit graph/ledger state or run dogfood LLM commands.
3. Implement the shared open-work record module, layout, typed target/field patch
   extraction, durable native event storage and registry limits. Replace
   `control.rs` open-task YAML lifecycle without restoring exclusive leases.
4. Integrate `tools/session.rs` staging and outcomes at complete-call boundaries.
   Existing `stage()` only pushes into `Vec<Op>`; it is insufficient for tools
   staging several operations. Preserve whole-call rollback and replay caches.
5. Implement last-writer field replay in `store.rs` and `store/rebase.rs`, structured
   commit results, attribution and `store/journal.rs`. Replace the refusal/drop path
   in `mcp/lifecycle.rs`. Move current-store gate checking under the same mutex.
6. Add MCP discovery/resume/adopt/discard/unstage, shared reply notices, session and
   runner heartbeats, ownership fencing, EOF/Stop persistence and commit recovery.
   Reuse `session_recovery.rs` checkpoints while making the open-work log the source
   of truth for staged mutations. Capsules reference work ID/epoch/sequence and
   never revive discarded or committed staging.
7. Wire `board.rs`, web API board/events, hosted `records.rs` and projection seams,
   browser VFS and WASM runner. The existing `RecordStore` has ordered append but no
   conditional ownership update: add a capability for atomic epoch comparison and
   append, or serialize it with the authoritative host project owner. Do not emulate
   adoption with a racy `meta`/`put_meta` pair. Hosted writes fail clearly without a
   durable record store. No fallback into Git-exported workspace files.
8. Update session, serving and inline prompt payloads and relevant workflow skills.
   Explain that warnings follow accepted writes and state what conflicts the later
   committer will see. Inspect, proceed, wait, or coordinate outside Jazyk without
   requiring an override or review acknowledgment. Explain overwrite replies, recovery IDs, expiry,
   explicit adoption/discard, and integrity-gate repair. Share guidance through
   partials and preserve parser conventions. No corrective Rust prompt strings.
9. Run focused tests, then full validation, commit coherent slices, integrate with
   `git merge --no-ff` after the parallel-work base is integrated, and push engine
   changes to `master`. Do not merge the plan branch into master prematurely if it
   would import the other effort's unverified implementation. No deployment.

## Tests and acceptance evidence

Use temporary stores, deterministic clocks, barriers, process fixtures and replayed
sessions. No paid model sessions and no `jazyk compile/check/gen/test` on dogfood.

| Scenario | Required assertions |
|---|---|
| Crash after stage success | Kill the process, reopen by work ID; mutation IDs, staged writes, scope, outcomes and original base survive; no graph commit occurred |
| Disk failure / torn append | Refused call changes neither effective staging nor touched-node index; complete earlier records survive; corrupt interior logs remain inspectable |
| Two agents stage one node | Both calls succeed without override; later caller gets a warning immediately explaining second-committer conflict reporting, earlier gets it next reply; full foreign staging and identity can be read; no overlap review gate is created |
| Both commit the same field | Both commits succeed in either order; later value wins; reply/journal include exact prior value, generation and author; info diagnostic exists |
| Disjoint fields | Entity name and definition, distinct attributes, requirement/criterion/view/diagnostic fields survive both commits; no false overwrite diagnostic |
| Explicit collections | Add/remove by member identity commutes where disjoint; overlapping add/remove follows commit order; full replacement is reported accurately |
| Expiry and adoption | Expired work remains on disk, unlocked; new actor adopts and commits; lifecycle journal has old/new identity; old owner epoch cannot mutate |
| Adoption/discard race | Exactly one ownership transition succeeds; active foreign discard refuses; discard removes payload, retaining only audit receipt |
| Conflict notice | Still-open work gets node/field details next tool reply and again at done; notices survive disconnect and lost-recipient-append window |
| Same-goal completion | Later claim passes when current gate holds; stale evidence or newly opened change fails with repair and retains staging |
| Redirect/deletion | Redirects remap staged refs and notice targets; missing deleted targets refuse with repair; no dangling IDs or implicit resurrection |
| Whole-call atomicity | Multi-operation grouping and inline done claims persist all or none; unstage preserves dependency integrity and updates warnings |
| Commit crash/retry | Crashes during graph publication, after journal and before cleanup/response produce one commit and a replayable receipt, never double application |
| Transport and Stop | stdio EOF, HTTP close, model failure, timeout and Stop preserve work; late callbacks are fenced; explicit abandon discards |
| Natural-key race | Concurrent creates map to one surviving ID, patch explicit fields, and report overwrites without dropping unrelated fields |
| Hosted/browser | Shared engine contract tests run with native, record-store and memory adapters; separate hosted processes share records and atomic adoption |
| Board and journal | Active openBy is visible without hiding readiness; expired work is discoverable; every overwrite/adoption has complete audit evidence |
| Prompt payloads | Warning and successful-conflict guidance render in native/WASM; no remaining first-wins/drop/implicit-EOF instructions |

Add tests alongside `store/tests/journal.rs`, `store/rebase.rs`, `tools/tests`,
`mcp/tests/lifecycle.rs`, `mcp/tests/workers.rs`, hosted recovery/workspace tests,
and WASM runner tests. Exercise central behavior once per storage adapter and only
necessary consumer wiring elsewhere. Replays never substitute for process-crash
and persistence-failure coverage.

Implementation validation, from `bootstrap/`, with a private target directory:

```sh
RUSTC_WRAPPER= CARGO_TARGET_DIR=/Users/matus/dev/jazyk-open-work-target cargo test --locked
RUSTC_WRAPPER= cargo fmt --check
RUSTC_WRAPPER= CARGO_TARGET_DIR=/Users/matus/dev/jazyk-open-work-target cargo clippy --locked --all-targets -- -D warnings
RUSTC_WRAPPER= CARGO_TARGET_DIR=/Users/matus/dev/jazyk-open-work-target cargo check --locked --target wasm32-unknown-unknown --lib
RUSTC_WRAPPER= CARGO_TARGET_DIR=/Users/matus/dev/jazyk-open-work-target cargo build --locked --release
```

After integration, rebuild release in the owner's checkout to refresh its linked
binary. Restart running `jazyk gui` only for the authorized binary change. Check
edited documentation links, heading anchors, schema syntax and absence of em dashes.
Planning alone requires plan link/style checks and `git diff --check`, not Rust
builds or graph reconciliation. Configure the owner's signed commit identity before
committing. Keep feature branches local and preserve unrelated work.

## Remaining work and boundaries

The plan settles the requested behavior and test contract. Normative docs, schema,
code, fixtures, validation and implementation merge remain. Graph reconciliation
is pending and must not start a dogfood model session under this request.

Host-side durable atomic adoption and commit-crash recovery are required engineering
work, not optional follow-ups. The host adapter implementation and editor UI in
`jazyk-cloud` remain separately owned. Browser process-exit durability requires host
persistence and is not supplied by an in-memory workspace. Expired work may grow
without bound until explicitly discarded; paginated discovery makes it manageable
without silently losing recovery data. Retention automation needs separate design.
