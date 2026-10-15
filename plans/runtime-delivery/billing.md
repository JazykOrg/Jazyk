# Runtime usage and admission

Status: proposed extension to existing Cloud accounting. Values below are examples,
not a published customer rate card. See [provider research](providers.md).

## One allowance, distinct meters

Keep the existing monthly, rolling five-hour, and rolling seven-day limits. LLM
cost and runtime cost both contribute customer accounting units to each applicable
window. Their raw usage stays distinct: tokens for model requests; CPU, memory,
disk, elapsed time, and transfer for runtime work.

Do not translate container seconds into fictional LLM tokens. Paid plans already
use priced supplier cost. Add `resource_kind = runtime` to the durable accounting
model, with its own versioned price snapshot and optional rate multiplier.

```text
runtime_supplier_cost = provider-priced CPU + memory + disk + billable extras
customer_units = ceil(runtime_supplier_micro_usd * runtime_multiplier_bps / 10_000)
window_used = model_units + runtime_units + other explicitly priced resource units
window_available = allowance - settled_use - unresolved_reservations
```

The formula uses integer/rational arithmetic. Carry fractional micro-dollar amounts
across metering intervals and round at settlement boundaries. Rounding each second
up independently would overcharge tiny runtimes.

Example only: a sandbox costing USD 0.05/hour costs USD 0.008333 for ten minutes.
At a 1.10 multiplier that becomes about 9,167 accounting units. A run twice as long
or a class twice as expensive drains twice as much. The proposed multiplier is an
illustration using the existing accounting convention, not a decision that 10%
margin covers runtime operations, startup, or provider fees.

## Rate card

Admin defines allowed classes and reviewed supplier rates, provider billing units,
minimum duration, image preparation cost, teardown behavior, storage/egress allowance,
and a customer multiplier. Keep a snapshot on every reservation and usage segment.
Provider discounts and free promotional credits must not make the unit economics
look permanently free.

The customer sees size, maximum duration, estimated effect on their usage meters,
and used/pending usage. Admin sees supplier exposure, customer debit, and margin.
Show `estimated` until provider pricing and actual measurements have reconciled.
Never price a sandbox using a provider's cheaper function-compute rate by mistake.

A resource request is not always a billing maximum. Providers with bursting require
hard CPU and RAM limits or admission against maximum possible usage. No unbounded
disk, image-building, egress, or log-storage expense is hidden behind a five-minute
action estimate.

## Reserve before starting

1. Resolve account entitlement, project ownership, execution policy, resource class,
   provider health, current rates, and independent runtime concurrency capacity.
2. Reserve a conservative maximum for the complete frozen task run, including bounded
   setup/image preparation, execution, collection/transfer, and shutdown lag.
3. Serialize admission with model reservations and all applicable customer/global
   caps. A model call and a sandbox must not spend the same remaining allowance.
4. Persist the run intent, fixed effective deadline, and reservation before dispatch.
5. A retry is another admitted attempt with its own operation ID. Never reserve the
   same attempt twice because a submit response was lost.

Reserve orphan exposure through the provider-enforced maximum lifetime or an
independently enforced and measured shutdown bound. Set that bound for this finite
action, not an idle development environment. A five-minute application timer does
not cap a two-hour provider allocation after an API crash. Reject providers/configs
whose worst-case exposure cannot fit admission. There are no user/model extensions
or open-ended allocations; size and timeout changes apply to a new attempt.

## Settle as time passes

Meter active billable resources in short intervals, proposed 15 to 30 seconds.
Use provider receipts or reconciled resource observations, never the task command's
claim about CPU time. Allocation-based providers bill allocated elapsed time,
including setup and collection. Containers terminate before the agent reviews the
final result, so no runtime remains allocated between repair iterations.

Persist idempotent usage segments keyed by provider resource, rate revision, and
interval sequence. Move confirmed use from pending to used and retain the unspent
reservation. Allocate rolling-window usage by consumption time, splitting segments
at boundaries as needed. Long-running actions cannot hide their cost until exit.

No double billing occurs when a build command is later referenced by tests or a
delivery: its actual runtime segment is charged once. Separate model usage remains
separate. A parent compiler job and its child sandbox share the account's allowance
but have independent usage receipts.

If budget runs out, reject the next run. Let an already reserved action finish
within its funded deadline; terminate at that bound. Cancel and read
operations remain available even without an allowance. Cancellation must interrupt
active execution, not wait for command completion. A stop request releases no
reservation until teardown is confirmed, including provider setup/cancellation
races. Reserve cleanup capacity before execution so collection and termination
do not require more admission.

Confirmed provider termination releases unused funds. A lost create/stop response
keeps unresolved exposure pending. A reaper polls by resource/operation ID, marks
orphans, attempts termination, and reconciles supplier receipts. Unknown holds do
not silently expire out of a rolling window. Operator resolution records evidence.

An actual supplier charge above its bound is recorded in full, triggers the existing
overspend review mechanism, and blocks new affected admissions. Compensation to a
customer never erases the supplier expense.

## Storage and transfer

For v1, include modest durable storage and cache quotas in the plan and cap transfer.
Include their measured supplier costs in margin monitoring. Do not make a usage
meter trickle down forever merely because a user retained a small PDF unless the
product explicitly adopts storage billing.

Alternatives are separate storage overage pricing or a unified continuously priced
storage resource. Decide after measuring real products and cache hit rates. Fixed
provider subscription fees must be allocated into margin analysis even when they
are not itemized to customers.

## Trials and local work

Trials currently combine a token limit and a supplier-cost ceiling. Recommended:
keep the model token cap, allow runtime use under the shared supplier ceiling, and
add a small explicit runtime-seconds/concurrency cap. Runtime usage does not deduct
fake tokens. A depleted model token allowance still prevents new model calls while
an already funded runtime finishes.

An alternative is a unified trial cost allowance replacing the token cap, but that
is a separate subscription change. Show runtime eligibility clearly either way.

Local Docker or host execution has no Jazyk Cloud runtime charge. A local project
that explicitly chooses a hosted runtime needs Cloud authentication and allowance.
Its model may still run elsewhere. The UI names where each resource executes.
The same rule applies to local MCP and ACP IDE clients: transport does not determine
the payer. A hosted run uses the same account reservation path from every client.

## Failure accounting

| Event | Customer and supplier treatment |
| --- | --- |
| Test assertion fails | Runtime consumed is billable; retain failure evidence. |
| User cancels | Settle consumed resources and release only confirmed unused exposure. |
| Provider fails before billable allocation | Release after provider evidence confirms no charge. |
| Provider fails after allocation | Record expense; any customer credit is a separate compensation. |
| Stop response lost | Show stopping/unknown and pending exposure until reconciled. |
| Product upload hits quota | Retain outcome and bounded recovery window; do not keep compute alive indefinitely. |

## Checks before launch

Exercise simultaneous model/runtime admission, duplicate receipts, cancellation,
provider timeouts, orphan cleanup, midnight/window boundaries, subscription renewal
while active, cumulative retry budgets,
fractional rates, rate changes, and late supplier adjustments. Verify that opening
the Logs tab, an MCP read, or an ACP reconnect cannot extend the run's deadline.
