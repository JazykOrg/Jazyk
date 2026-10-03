# Delivery workflow

Status: proposal. Read with the [representation](./model.md) and
[worked examples](./examples.md). This describes an on-demand consumer. It does not
add automatic external work to ordinary compilation.

## Prepare a delivery

### Select the candidate and destination

The request selects an independently addressed destination and a particular product
candidate. Preparation freezes the relevant specification, artifact identities,
procedures, and verification evidence.

The selected scope can be smaller than the whole product, but the plan must include
dependencies that affect its compatibility. Updating the server does not make its
old clients irrelevant because they are outside the selected files.

### Inspect and establish the baseline

Load the destination binding and last confirmed checkpoint. Inspect the destination
using available tools. Establish which assumptions are confirmed, contradicted,
or unknown.

There are three distinct starting cases:

- Confirmed initial delivery: the destination has been shown to be empty or new
  within the owned scope.
- Known destination: a retained checkpoint can be compared with fresh observations.
- Adoption: a destination already exists but its delivery history is absent or
  insufficient. Establish identity and ownership, capture evidence, and resolve
  missing intent before changing it.

An unavailable provider, missing credentials, or failed inspection is not evidence
of an empty destination.

### Compare the four inputs

The session compares the candidate, baseline, observations, and delivery obligations.
The package should make each input's identity and scope explicit.

Useful findings include:

- A requested product change with no external state implications.
- A required transformation of stored data or external structure.
- An outside edit that must be merged, preserved, or explicitly superseded.
- A compatibility obligation imposed by a still-supported consumer.
- A missing observation, unavailable capability, or unresolved policy.
- A partially completed earlier attempt that needs recovery or resumption.

An unchanged artifact is not automatically a no-op. External drift or an unfinished
transition can still require work. Conversely, a change in an irrelevant graph
fact should not manufacture a new delivery.

### Feed missing behavior back into the product

Some findings can be handled by the delivery procedure. Others reveal missing
product behavior or ambiguous requirements.

E.g.:

```text
The new server accepts only protocol 2.
The support policy still includes clients using protocol 1.
No transition can satisfy that policy with the current candidate.
```

The model reports the missing compatibility behavior. A proposed product decision
follows the existing [ratification](../../docs/compiler/model/diagnostic.md#ratification-proposals)
or [answer](../../docs/compiler/model/diagnostic.md#answers) path. The harness derives
the resulting binding and generation work. The model does not enqueue a goal.

After the product changes, preparation selects the new candidate and revises the
plan. Temporary compatibility behavior needs an explicit lifetime or retirement
condition so it can be removed when its obligation ends.

### Produce a concrete transition

The plan identifies the external objects it changes, the procedures it executes,
their dependencies, and the evidence needed at each boundary. It explains data
preservation, coexistence, expected interruption, and recovery where applicable.

The model can draft this plan and its procedures. The harness validates its input
references and execution contract. Destination tools establish the provider facts.
Semantic judgment alone cannot certify an external result.

Preparation should leave a reviewable artifact even when execution is delegated.
The plan says what remains unknown and what product or policy changes it requires.

## Validate and execute

### Bind execution to the prepared plan

Execution acts on the particular prepared plan and its candidate. A release or
authorization for one artifact set does not silently extend to later generated
bytes or newly introduced effects.

The first integration should execute only on an explicit delivery request. It can
reuse existing authorization and executor policy. Automatic application would need
a separately stated policy. Ordinary permission to generate local files does not
implicitly authorize production changes.

Execution policy can include standing authorization for a defined scope. Any
required human review applies to the concrete plan and its effects. The exact
control-plane representation is a specification decision.

### Recheck the conditions that matter

The world can change between planning and execution. Recheck relevant identities,
revisions, support assumptions, and preconditions before the affected action.

Use atomic provider guards where available. A separate check followed by a write
does not eliminate a concurrent-write race. Where no such guard exists, use an
appropriate exclusion mechanism, a procedure tolerant of concurrent change, or
report the limitation.

A destination lease can serialize Jazyk writers. It does not prevent a human editor,
another pipeline, or an application from changing external state. Multiple local
projects addressing the same destination need a shared coordination mechanism.

The observation guards should permit expected live activity. New orders do not
invalidate a migration merely because the row count changed, provided the migration
procedure handles concurrent writes as specified.

### Record progress around external effects

Before an action with external effects, durably record its identity, intended
effect, inputs, and how to determine whether it completed. Record its outcome and
evidence afterward.

This does not create a transaction spanning Jazyk and the provider. It makes the
gap explicit and recoverable.

- A completed action has evidence for its expected effect.
- A failed action records the observed failure and any known partial effect.
- A timed-out or interrupted action can have an unknown outcome.
- An unknown outcome triggers inspection before retry or compensation.

Use provider idempotency keys where available. Otherwise the procedure needs a
destination-specific way to recognize an existing effect. Blindly replaying an
append-to-document operation or a non-repeatable data transformation is invalid.

Retries are a property of the action and its observations. A generic instruction
to rerun every failed command is insufficient.

### Verify and confirm

Verification observes the external result against the relevant criteria. A script
exit code or an agent's completion statement is supporting evidence, not the
complete delivery verdict.

Checks may cover artifact identity, public access, data invariants, client behavior,
preserved editorial content, and recovery conditions. The destination and candidate
versions under test must be recorded.

When the required checks succeed, write the immutable checkpoint and atomically
advance the destination's confirmed pointer. A failure leaves the previous
checkpoint intact and the actual attempt state visible.

An outside deployment can be confirmed through the same path. Import its receipts,
establish the candidate identity, and verify the destination. If artifact identity
cannot be established, record the uncertainty or use adoption. Do not claim that
Jazyk delivered or verified facts the evidence does not establish.

## Recovery

Recovery is a transition from the current observed state toward a stated acceptable
state. Its procedure depends on what actually happened.

Possible responses include resuming the intended update, reverting a compatible
artifact, applying a compensating change, preserving a partial result for repair,
or restoring data under an explicit recovery policy.

Restoring old code is distinct from restoring old data. New code may have written
values the old code cannot read. Restoring a backup may discard later accepted
writes. A recovery plan must explain those consequences and use the stated policy.

An attempt should identify boundaries beyond which an earlier recovery procedure
is invalid. Cleanup that removes an old data representation may cross such a
boundary even if the main activation succeeded days earlier.

Recovery procedures need verification too. A plan that claims rollback support
must establish the conditions under which it holds. Lack of a valid rollback can
be an acceptable stated policy for a particular product, but it must not be
presented as a reversible operation.

## Waiting and deferred cleanup

Some transitions wait for clients to reconnect, a migration to finish, a review,
or a support window to end. Persist the attempt or a deferred action and release
the model session. Resume from new evidence when requested or when an explicitly
configured executor reports an event.

A candidate can be active while compatibility cleanup remains pending. Report both
facts. Whether activation is sufficient to confirm delivery depends on the
requirements and the plan's completion conditions. A required wait cannot be erased
by labelling the attempt successful.

Cleanup that occurs later should use fresh observations and current policy. It
must not blindly execute a destructive command prepared against an old state.

## Work derivation and convergence

The delivery consumer can reuse sessions, loaded context, criteria, diagnostics,
workers, and leases. The registry and harness still own goal derivation, gates,
readiness, and scheduling.

The implementation must decide whether delivery work uses new goal kinds, a
separate consumer worklist, or a combination. The smallest initial scope is a
bounded preparation request plus an operational attempt. Do not introduce one
universal goal kind for every provider action before the experiments justify it.

Compiler convergence and external delivery progress have different lifetimes.
Compile and generation work can settle while production deliberately remains on
the old candidate or a client support window remains open.

Present separate facts:

- Product generation and verification state.
- Transition readiness, including missing decisions or capabilities.
- Last confirmed delivery and current observed differences.
- Active attempt, waiting conditions, or recovery needs.

A current observation can require a provider read even for a no-op delivery. That
does not change the existing rule that a no-op compiler rebuild makes zero model
calls. A delivery whose relevant inputs and evidence remain valid should likewise
reuse its result instead of spending a model session unnecessarily.

## Verification strategy

Bind evidence to preservation and transition criteria as well as final-state
criteria. A final product test and a transition test can exercise the same
requirement under different starting conditions.

Useful scenarios include:

- Initial delivery to a confirmed empty destination.
- Upgrade with existing records or editorial changes.
- Old and new clients or processes coexisting where support requires it.
- Candidate changes after preparation.
- External revision changes immediately before execution.
- Interrupted execution before and after each external effect.
- A successful external effect followed by a lost response.
- Recovery after the new product has accepted additional writes.
- A missing local compiler store with the destination still present.

Use deterministic fixtures and controllable executors for failure cases first.
Exercise real providers later to establish which adapter assumptions hold.
