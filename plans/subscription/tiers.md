# Tiers, quotas, and unit economics

Prices and caps below are illustrative, pending [measurement](./models.md#measurement-gate).
Customer prices use CAD. Supplier rates use USD. Currency choice is proposed, not
settled by the owner's Canadian location.

## What is free and what is paid

- Free: native local CLI/GUI, own endpoints, own API keys, ACP agents, and the limited
  browser playground in the [workflow proposal](./workflows.md). No Jazyk account
  required and no recurring hosted model/compute allowance.
- Subscription: managed model access, usable locally and in cloud sessions.
- Cloud runtime: separately purchased compute/storage entitlement. Managed models
  in the first beta. A supported BYOK configuration is later work.
- Later promotion: perhaps C$20 off the first paid invoice, or a separately defined
  inference grant. No promotion is enabled initially. A social identity alone does
  not prevent repeat claims.

This avoids bundling 25–300 compute hours into local model plans before their cost
and demand are known.

## The usage unit

Proposed rate: 100 credits represent US$1 of usage at the published Jazyk model rate
card. Credits are a service allowance, not money withdrawable from an account.
Internally use integer micro-units; round only the display, not every tiny call.

For each request, normalize the provider's actual billable categories:

```
debit = uncached_input × input_rate
      + cached_input × cache_read_rate
      + cache_writes × cache_write_rate
      + output_and_billable_reasoning × output_rate
      + other_supported_billable_items
```

Provider schemas differ. Some include reasoning within output and cached tokens
within total input. Do not charge those twice. Exclude unsupported billable features
until metering handles them. When reserving, assume a cache miss and any applicable
cache-write/long-context premiums.

Customer debit and supplier cost are separate ledger fields. Store rate-card version,
provider route, usage source, upstream request id, request receipt, and corrections.
Show credit amounts, percentage bars, and rate details. Do not hide the unit behind
only percentages or promise a fixed number of builds.

Price updates apply to future requests under a published version. Pin the rate
version for an admitted session for a bounded time. Never retroactively reprice
completed calls. Emergency removal of an uneconomic route is explicit.

## Windows and hard limits

Proposed allowance `B` per paid monthly billing period:

- Billing period: `B`, a hard aggregate allowance.
- Rolling 7 days: at most `0.30 × B` of included usage.
- Rolling 5 hours: at most `0.10 × B` of included usage.
- Account concurrent model calls: initially 2 / 4 / 8 by tier, subject to measured
  provider capacity. This is a capacity control, not proof against sharing.
- A user can set a lower per-run spend cap. The gateway enforces it for managed use.

Each request must fit every applicable limit including outstanding reservations.
Rolling windows release allowance as individual charges age out; report the next
availability and the full replenishment time, not a fictitious single reset.
Billing-period renewal does not clear rolling history.

A rolling week is not a monthly ceiling. `4.33 × weekly cap` is a long-run average,
not a bound on one billing period. This is why the previous sizing rule is replaced.

## Illustrative local tiers

| | Free | Pro | Max | Ultra |
|---|---:|---:|---:|---:|
| Monthly price | C$0 | C$29 | C$79 | C$199 |
| Included credits | 0 | 800 | 2,400 | 6,400 |
| Rate-card equivalent, USD | 0 | $8 | $24 | $64 |
| Rolling weekly credits | 0 | 240 | 720 | 1,920 |
| Rolling 5-hour credits | 0 | 80 | 240 | 640 |
| Concurrent managed requests | 0 | 2 | 4 | 8 |
| Included cloud compute | None | None initially | None initially | None initially |

The same supported model catalog is available across paid tiers, subject to the
user's policy. Frontier use is allowed when benchmarked and accounted for at its
rate. A request too large for a tier's window needs a smaller batch, a cheaper
route, or an explicit paid balance. The same applies to a large compilation.

Launch one or two paid tiers if that makes operation simpler. The third row is a
sizing example, not a requirement to launch three plans. No annual discount initially.
An annual plan needs its own revenue-per-month calculation and monthly grants.

## Worked economics

Assumptions, not a live FX quote:

- `F = 1.45 CAD/USD`, a planning conversion rate; stress-test at 1.60 as well.
- Canadian domestic-card fees of 2.9% + C$0.30, plus 0.7% Billing and an illustrative
  0.5% Stripe Tax charge where applicable. See [Canadian billing](./platform.md#canadian-billing).
- 6% supplier funding/fee reserve, rounding above OpenRouter Standard's published
  5.5%. Adjust for funding minimums, actual fee mechanics, discounts, and settlement.
- Per-account operating/refund reserve `R` of C$4 / C$7 / C$12. These are estimates.
- Tax collected is not revenue. The simplified table excludes fees on collected tax,
  international-card/FX surcharges, and nonrecoverable taxes; launch costing adds them.

```
net_before_models = price × (1 - 0.041) - 0.30
model_cost_CAD    = B_USD × 1.06 × F
contribution     = net_before_models - model_cost_CAD - R
```

| Tier | Net before models | Models at 100% use | Reserve | Contribution before shared fixed costs |
|---|---:|---:|---:|---:|
| Pro | C$27.51 | C$12.30 | C$4 | C$11.22 |
| Max | C$75.46 | C$36.89 | C$7 | C$31.57 |
| Ultra | C$190.54 | C$98.37 | C$12 | C$80.17 |

For Pro at `F = 1.60`, model cost rises to C$13.57 and contribution falls to C$9.94.
This is contribution, not profit or a no-loss guarantee. Shared hosting, support,
provider minimums, and sparse early adoption can still make the product unprofitable.
A C$200 shared monthly service floor would need about 18 fully used Pro accounts
at the base assumptions, before owner compensation. Measure that floor.

Do not copy a vendor's 3x list-price allowance without its economics. If these caps
buy too little useful work, improve token efficiency, find better routes, change
prices, or decline to launch that offer. Lower utilization is not a substitute for
measuring the worst case.

## Cloud pricing

Start with a separately prepaid runtime balance and a published size/hour rate.
Compute is charged while the environment is running, including time waiting for an
LLM and unattended jobs. BYOK does not make the runtime free.

For an E2B-sized 2 vCPU / 4 GiB instance, US$0.1656/hour is a current supplier
compute example. At the planning FX rate that is about C$0.24/hour before the fixed
provider fee, persistence, egress, retries, and platform overhead. A customer rate
around C$0.50–0.75/hour is only a test range. Do not sell it before reconciling real
bills. Other providers need different CPU-activity assumptions.
See [sandbox costs](./sandboxes.md#cost-comparison).

Specify storage size, retained checkpoints, export bandwidth, and idle retention as
separate entitlements. A prepaid C$10 runtime purchase cannot imply perpetual free
storage. Start with a small paid cloud entitlement or a clearly disclosed retention
period, and leave its exact price to the provider trial.

## Admission and settlement

A quota balance endpoint is advisory. The atomic gateway transaction is authoritative:

1. Authenticate and resolve an allowed model/rate version. Check account status,
   concurrency, request size, context length, and output/reasoning bounds.
2. Reserve a conservative maximum cost against all limits before forwarding. Include
   existing reservations. A run-level budget has its own shared balance.
3. Forward once, with an internal request id and an upstream id where available.
4. Settle actual provider usage and release unused reservation. Settlement is
   idempotent. Record corrections as new ledger entries.
5. If a stream breaks or usage is absent, keep a bounded pending reservation and
   reconcile with the provider. Stop admitting more uncertain exposure at a low
   account/global threshold. Do not assume missing usage means zero or release a
   still-billable request merely because its client disconnected.

Reservations retain the grant, billing period, and server admission timestamp they
were assigned. Late settlement must not move an old request onto a fresh monthly
allowance or erase its rolling-window history. Test renewals with calls in flight.

An identical retry key returns the original status/receipt and does not launch a
second call. A genuinely new upstream attempt gets a new entry. Timeouts with
unknown upstream state are reconciled before automatic retries can multiply charges.
Client/sandbox token reports are never authoritative.

## Exhaustion and continuation

- Admission fails before starting a request that cannot fit its reservation.
- An already reserved call can finish within its limits. A started multi-round
  session is not guaranteed to finish. A round count alone cannot fund that promise.
- Prefer stopping before the next batch using an estimate. If a later call cannot
  be admitted, discard uncommitted staged changes, preserve committed generations,
  and record a resumable resource stop without marking goals failed or consuming
  semantic retry/parking allowances. Interrupted paid work remains visible.
- Use a typed `quota_exhausted` or `budget_exhausted` response, distinct from transient
  provider 429s. Include the limiting window, remaining balance, and next availability.
- Keep build lifecycle state separate from the graph's convergence verdict. A
  billing stop must not imply semantic convergence or create a diagnostic.
- Allow waiting, changing the selected provider/model, or deliberately spending a
  paid top-up. Never automatically change who pays.

Whole-build estimates include uncertainty and possible additional goals. Initially
show the next batch and recent comparable runs; a sum of today's board is not a
reliable upper bound on a reconciliation cascade.

## Top-ups and promotions

Add prepaid top-ups after the core ledger is proven. Display CAD purchase amount,
credits received, and the applicable model rate card. Include processing fees when
setting pack size. No automatic recharge or postpaid overage by default.

Included allowance is used first when eligible. If a pacing window blocks it, using
paid credits requires an explicit setting/action. Paid credits bypass included-use
windows, but never safety, concurrency, model, or user spend caps. Preserve purchased
balances across cancellation; allow paid-balance model use without automatically
renewing the subscription. Set expiry/refund terms before sale, not by copying Kiro.

A later signup incentive is a distinct promotional ledger grant with a global campaign
budget and one-claim controls across linked identities. Prefer a first-paid-invoice
discount for an initial experiment. A $20 inference grant incurs supplier liability;
a $20 invoice discount instead reduces revenue. Do not conflate them.
