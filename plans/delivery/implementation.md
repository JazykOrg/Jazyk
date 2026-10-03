# Delivery implementation plan

Status: proposal. This is an implementation sequence for the recommended
[destination checkpoint approach](./options.md#option-2-keep-a-checkpoint-per-destination).
It does not authorize implementation or deployment. The immediate deliverable is
the retained design and reasoning in this directory.

## First useful scope

Start with one destination, a frozen candidate, a confirmed checkpoint, and a
prepared transition. Support an existing external executor before building a
general provider framework or a continuous reconciliation service.

The first useful result answers:

- What is being delivered, and where?
- What was last confirmed there?
- What has changed in the product and at the destination?
- Which commitments constrain the transition?
- What concrete actions and evidence are required?
- What happens if execution stops partway through?

Do not require a release catalog, version labels, a new graph node family, or a
permanent deployment history to answer those questions.

## Phase 1 Validate the representation

Work through the three [examples](./examples.md) as fixtures. Manually specify their
destinations, candidate and baseline references, observations, preservation
requirements, plans, and expected evidence.

Include one initial delivery, one ordinary update, and one outside change per
fixture. Add an interrupted attempt and an uncertain outcome. These cases establish
whether the common records have enough information.

Exit condition: all three fixtures use the same operational envelope. Their
provider details remain in procedures and evidence. Any required additions to the
semantic graph are justified by a concrete failure of the existing model.

## Phase 2 Specify and implement preparation

Write the accepted behavior into the canonical docs first. Add the delivery
consumer contract and specify how it selects graph scope, captures candidates,
reads checkpoints, requests observations, and produces a concrete plan.

Choose the minimum durable store at this stage. Candidate content, destination
bindings, checkpoints, and attempt records must survive compiler output resets.
Establish their write and recovery protocol before adding external execution.

Reuse the existing model access and session architecture. New prompts or skills
must be payload files shared by docs and code. The harness derives work; the model
reasons within the supplied task and context budget.

Preparation can generate procedures and tests locally. It should make missing
capabilities and policy choices visible before external execution. It must also
invalidate a plan when its relevant inputs change.

Exit condition: each fixture produces a reviewable transition from retained and
observed evidence, with no external mutation during preparation. Removing compiler
output preserves or recovers the destination binding and retained baseline.

## Phase 3 Execute through a bounded contract

Define the smallest useful execution contract from the fixtures. It needs action
identity, guarded preconditions, concrete effects, completion evidence, unknown
outcomes, and recovery behavior.

Use existing libraries, APIs, command tools, and deployment services. Do not build
replacement transports or platform clients as domain logic. Generated procedures
are artifacts with identities and tests, not unrecorded commands invented during
an already approved attempt.

Start with a controllable executor that can fail before and after an external
effect. Support an external job receipt and independent result inspection. Then
add direct worker execution where useful. External writes depend on the durable
record protocol established in phase 2, including restart after recording intent
but before recording the result.

Exit condition: a completed, failed, interrupted, or uncertain attempt can be
resumed or inspected without blindly duplicating effects. Only verified outcomes
advance the confirmed checkpoint.

## Phase 4 Prove stateful delivery

Implement the stateful application fixture with generated compatibility behavior,
transition tests, concurrent-write handling, and a stated recovery boundary.

Use delivery findings to exercise the normal docs, binding, and generation loop.
The new candidate must replace the old prepared candidate explicitly. Verify that
removing a current requirement does not silently erase an active historical support
commitment.

Exit condition: fresh installation, upgrade, mixed supported versions, new writes,
interruption, and recovery satisfy the fixture's criteria. A genuinely unavailable
transition is reported as such.

## Phase 5 Prove publishing with outside edits

Implement the shared-document fixture using the same preparation and attempt
machinery. Establish exact provider capabilities for revisions, region identity,
comments, and sharing. Keep unsupported properties visible.

Exercise the distinction between preserving an external edit and adopting its intent
into future generation. Avoid making an unimplemented upstream reconciliation
mechanism an invisible prerequisite.

Exit condition: independent changes are preserved, conflicts are explicit, revision
races invalidate affected edits, and uncertain writes do not duplicate content.

## Phase 6 Add the user surface and retention

Expose destinations in the Product surface. Show the last confirmed product,
current observations, proposed changes, readiness, active attempts, and recovery
needs. Use words appropriate to the product, such as Publish or Update.

Extend the durable foundation with user-facing export and recovery, retention
roots, and garbage collection. Repeat the reset and recovery checks through the
user surface. Existing destinations must not be recreated or overwritten as if
empty.

Exit condition: a user can prepare, inspect, execute or delegate, resume, and
understand a delivery without navigating compiler internals. Required baseline
content remains available after normal cleanup and restart.

## Canonical documentation changes

The following pages would change before their corresponding code. New filenames
are suggestions, not an adopted public interface.

| Area | Docs-first work |
| --- | --- |
| Consumer contract | Add `docs/consumers/delivery.md`; link it from [main](../../docs/main.md) and [compiler](../../docs/compiler/compiler.md). |
| Candidate and artifact capture | Extend [generation](../../docs/consumers/gen.md) with immutable delivery inputs and artifact retention responsibilities. |
| Preservation and transition tests | Extend [binding](../../docs/consumers/bind.md) and [criteria](../../docs/compiler/model/criterion.md) only where existing contracts are insufficient. |
| Work derivation and gates | Specify the chosen integration in [reconciler](../../docs/compiler/reconciler.md), [sessions](../../docs/compiler/sessions.md), and [tools](../../docs/compiler/tools.md). |
| External execution | Extend [control plane](../../docs/compiler/control-plane.md) with delivery dispatch, coordination, and the distinction from compilation releases. |
| State lifetime | Specify durable operational storage and update the relevant boundaries in [graph storage](../../docs/compiler/graph.md) and [project settings](../../docs/compiler/project-settings.md). |
| User commands and views | Extend [CLI](../../docs/frontends/cli.md), [MCP](../../docs/frontends/mcp.md), and [GUI](../../docs/frontends/gui.md) after the consumer contract is settled. |
| Model payloads | Add any accepted contracts and skills under the existing embedded payload directories; document their inputs and gates. |

The design can start in a dedicated consumer module. Exact Rust types and tool
names should follow the fixture contract, not precede it. Changes to the binary
finish with the required release build under the repository's working rules.

## Acceptance matrix

| Case | Required result |
| --- | --- |
| Empty destination | Initial delivery follows stated creation and ownership policy. |
| Unreadable destination | Inspection remains unknown; no assumption of emptiness. |
| Unchanged candidate and destination | Reuse valid evidence; no unnecessary model work or external mutation. |
| Outside edit | Preserve, merge, or surface conflict according to policy. |
| Candidate changes after preparation | Affected plan and verification evidence become stale. |
| Destination changes before mutation | A relevant guard rejects or safely accounts for the change. |
| Lost response after a successful write | Inspect outcome before retry; no duplicated effect. |
| Partial application | Last confirmed checkpoint stays intact; actual progress remains visible. |
| Active old consumer | Retain its contract and required artifacts until policy permits retirement. |
| New writes before recovery | Recovery preserves them or explicitly follows the stated loss policy. |
| Compiler output reset | Recover or adopt the destination; do not treat it as a new installation. |
| External execution | Match evidence to the candidate and inspect the result before confirmation. |
| Missing retained artifact | Report unavailable evidence or recovery; do not substitute regeneration. |
| Multiple destinations | Track independent baselines and relevant cross-destination dependencies. |

These are future implementation checks, not claims that the current compiler
passes them. They should be exercised with deterministic fixtures before live
providers and paid model sessions.

## Decisions still open

- Binding: how does a user select a destination entity and connect it to an
  external identity without duplicating product facts in configuration?
- Storage: where do durable delivery records live, and how are they shared across
  machines and recovered after compiler output is reset?
- Candidate capture: start with full semantic snapshots or a dependency closure?
  Full snapshots are the simpler correctness baseline.
- Work integration: new goal kinds, a separate consumer worklist, or both? Keep
  provider operations outside the authored graph either way.
- Execution contract: which guards and receipts are common, and which belong in
  destination procedures?
- Coordination: what shared mechanism prevents competing writers addressing the
  same destination from different projects or machines?
- Completion: when can activation be confirmed while cleanup or support windows
  remain pending? The policy must state what the checkpoint certifies.
- Support: how are supported consumer cohorts and retirement conditions expressed
  without requiring a full historical version graph?
- External edits: which changes need upstream proposals, which can remain local
  to a destination, and what prevents repeated conflicts on later publication?
- Recovery: how much retained history is required by each product's obligations,
  and how is the absence of a valid recovery path presented?
- Terminology: how does product delivery coexist with the existing internal
  control-plane use of the word release?

## Deferred scope

- A general fleet manager or inventory of every installed copy.
- A mandatory product version on every successful generation.
- A permanent log of every successful deployment attempt.
- Continuous production reconciliation or automatic delivery on document saves.
- A built-in catalog of all cloud providers and publication media.
- A universal migration language or automatic inverse for every action.
- New graph kinds for deployments, versions, or observations without evidence that
  ordinary graph concepts and operational records are insufficient.

The first experiments should establish whether destination checkpoints and
transition obligations are enough. Broader version and upgrade-path machinery can
be added when a real product needs it.
