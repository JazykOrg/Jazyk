# The platform behind jazyk.dev

The trusted service owns accounts, entitlements, usage reservations, project metadata,
and runtime orchestration. It does not own compiler goals or mutate graph shards.
See [boundaries](./boundaries.md) and [cloud execution](./cloud-sessions.md).

## Components and trust

| Component | Responsibility | Persistent state |
|---|---|---|
| API and account UI | Identity, devices, access, subscriptions, project list | Postgres |
| Model gateway | Admission, routing, streaming, settlement, receipts | Usage ledger and reservations |
| Runtime orchestrator | Start, fence, stop, reconcile provider inventory | Project lease, runtime state, compute ledger |
| Relay | Authorized project API, events, LSP WebSocket | Short-lived connection state |
| Storage service | Checkpoint manifests, uploads, recovery, exports | S3 and Postgres pointers |
| GitHub integration | Installation access, import, explicit branch publish/PR | Installation and repository bindings |
| Preview service | Private access to an untrusted app | Expiring access grants |

One service with modules and background jobs is enough initially. Postgres supplies
transactions, uniqueness, and durable job state. Add Redis or a separate queue only
when measurements justify it. Gateway traffic and user code never share a runtime.

Use HTTP/streaming, OAuth, Stripe, SQL, AWS, and archive libraries. Start with one
OpenRouter integration and a small catalog. Jazyk-specific admission/ledger logic
is custom; provider wire protocols and credential proxies are not an excuse to
hand-roll standard infrastructure. Compare an existing proxy if multiple direct
provider protocols later justify one.

## Accounts and local access

- Google and GitHub sign-in via established OAuth/OIDC libraries or a hosted auth
  service. GitHub login and a GitHub App repository installation are separate grants.
- Use stable provider subjects as identities. Explicitly authenticate account linking;
  matching email addresses alone must not merge paid accounts or promotional grants.
- One account owner initially. Organizations, seats, and pooled team budgets come later.
- `jazyk login` uses a browser/device authorization flow. Issue revocable device
  credentials for the managed model service, with short-lived access tokens.
- Store refresh credentials through an OS credential-store library. A documented
  permission-restricted file fallback is for headless systems. Keep secrets out of
  project TOML and browser local storage.
- Login creates a named provider profile; activation is explicit. Existing CLI/env
  overrides and LocalRouter remain meaningful. Logout/revocation does not disable
  local BYOK.
- Browser sessions use secure, HTTP-only cookies, CSRF protection, and exact origin
  checks. Authorize project access on every relay connection and operation.

Local subscription use uploads model context to the gateway/upstream, even though
execution and project files stay local. State this at provider selection. Local-only
model use has a different data path.

## Canadian billing

Use the owner's Canadian Stripe account with its actual sole-proprietor details,
bank settlement settings, business address, and statement descriptor. Do not select
a merchant of record or assume an EU business. The proposed checkout currency is CAD;
USD and other currencies can be evaluated later.

Use hosted Checkout for purchase, Billing for subscription lifecycle, and Customer
Portal for payment method and cancellation. Jazyk's ledger grants service allowances;
Stripe is not the source of token usage. Keep compute and model balances distinct.

Official fee references read 2026-10-01:

| Item | Published Canadian standard rate | Planning implication |
|---|---|---|
| [Card payments](https://stripe.com/en-ca/pricing) | 2.9% + C$0.30 domestic; +0.8% international; +2% if currency conversion is required | Include the actual card mix and settlement arrangement. |
| [Billing](https://stripe.com/en-ca/billing/pricing) | 0.7% of Billing volume, pay as you go | Add to payment fees. |
| [Tax](https://stripe.com/en-ca/tax/pricing) | 0.5% per applicable transaction through no-code integrations where registered | Configuration and product choice matter; filing services are separate. |

Custom account pricing can differ. Fees may apply to amounts including collected
tax. Collected tax is a liability, not model budget. [Tier math](./tiers.md#worked-economics)
is a simplified stress case, not an accountant's settlement statement.

Jazyk remains the merchant and handles registrations and returns. Stripe Tax can
calculate supported Canadian taxes from registrations and customer information; it
does not establish which registrations this particular business needs.
Source: [Stripe's Canadian tax documentation](https://docs.stripe.com/tax/supported-countries/canada).

For most businesses, CRA's small-supplier test uses C$30,000 of worldwide taxable
supplies in a calendar quarter or across four consecutive quarters, with associated
persons included. It is not simply Canadian sales or this application's revenue.
The timing of registration depends on how the threshold is crossed. Provincial tax
rules and sales outside Canada need separate evaluation. Record province, existing
registrations, customer location evidence, product tax classification, and initial
sales territories before enabling live checkout.
Source: [CRA registration rules](https://www.canada.ca/en/revenue-agency/services/tax/businesses/topics/gst-hst-businesses/when-register-charge.html).

Charging through Canada does not require Canada-only customers or Canada-only data
processing. Do not infer either restriction from the merchant address.

## Payment and entitlement state

Stripe events can arrive more than once or out of order. Verify signatures, persist
event ids, process idempotently, and reconcile periodically against Stripe's current
subscription/invoice state. Source: [subscription webhooks](https://docs.stripe.com/billing/subscriptions/webhooks).

- Grant the billing-period allowance only for a confirmed successful payment or an
  explicitly funded promotion. A Checkout redirect does not grant access.
- Uniquely key each grant by account, paid invoice/line, and allowance period. Keep
  Stripe customer/subscription ids server-owned and validate the expected price/currency.
- Use anniversary billing periods initially. No retroactive free first-month grant.
- Upgrades apply after payment. Grant only the documented, paid incremental allowance,
  proportional to the remaining period if the price difference is prorated. Retain
  spent usage and rolling-window history. Never mint a full new month for a tiny
  proration payment.
- Downgrades and cancellation take effect at paid period end. Cancellation stops
  renewal, not access to existing project exports.
- Failed renewal blocks new paid admissions once the paid entitlement ends. Handle
  in-flight reserved calls and checkpoint an active runtime within a bounded stop grace.
- Refunds and disputes create compensating entries. Revoke unused associated grants
  and suspend further spending if needed. Do not silently create a postpaid debt.
- Model top-ups and compute purchases each have their own idempotent grant. Preserve
  purchased balances across cancellation under the published terms.

Test duplicate/delayed events, renewal failure, upgrade/downgrade races, refund after
partial use, linked identities, tax calculation, and portal cancellation before live
billing. Use Stripe test mode until these pass.

## Gateway

Expose an OpenAI-compatible model endpoint plus a separate account/usage API. The
exact paths are specified during phase 1. Implement the
[reservation protocol](./tiers.md#admission-and-settlement), not a preflight-only check.

- Device credentials are account-scoped for model access. Cloud credentials are
  short-lived and restricted to account, project, runtime lease, and spend envelope.
- Accept supported models and features only. Context/output/reasoning bounds let the
  service reserve a defensible upper cost. Do not accept a client-supplied dollar bill.
- Pass streams through and settle from provider usage. Record the upstream id and
  status even if the client closes its connection.
- Return an early request id, then expose the final receipt through a receipt/usage
  endpoint or a documented final event. No final-cost header before streaming ends.
- Track inference cost separately from funding fees. Reconcile usage and supplier
  invoices; unexplained differences alarm and can disable a route.
- Rate-card changes are versioned. Polling a supplier catalog detects changes; it does
  not silently rewrite the customer's charge history.
- Enforce per-account and global spend/concurrency limits. An account may have several
  local devices. All devices and cloud runtimes share the same atomic ledger.
- Quota exhaustion is typed and non-retryable until allowance changes. Ordinary
  provider throttling/outages keep separate retry and backoff semantics.

No arbitrary upstream URL in a subscription request. BYOK is a separate access path.
The [workflow proposal](./workflows.md) defers cloud BYOK from the first beta.
When added, hold encrypted customer credentials in a broker, initially for OpenRouter
through the already supported integration. It consumes no Jazyk inference allowance
but still consumes runtime entitlement. Other providers and generic private/local
endpoint tunneling can follow. Local BYOK remains available throughout.

## AWS deployment

Proposed beta:

- One AWS region, one deployable API/worker service on ECS/Fargate, and managed
  Postgres with backups. Use S3 for immutable checkpoint objects and artifacts,
  Secrets Manager/KMS for service secrets, and existing deployment OIDC patterns.
- Prefer `ca-central-1` for account/project data if the selected services and sandbox
  placement fit. `us-east-1` is a valid simpler alternative when co-location wins.
  Existing static-site hosting does not force the backend region.
- Compare regional service availability, latency, database/egress cost, and operational
  access before choosing. There is no Canada-only or EU-only residency promise.
- Keep private database access and least-privilege service roles. User sandboxes get
  neither a task role for this service nor broad S3 access.
- Budget for the load balancer, database floor, logs, backups, network transfer, and
  NAT/VPC networking where used. The small beta service is not cost-free at idle.
- Planning envelope: roughly US$100–300/month for a modest managed deployment before
  inference, sandboxes, significant traffic, and support. This is an estimate to
  replace with a concrete regional configuration and calculator result in phase 1.

The service must support streaming responses and long-lived SSE/WS with explicit
idle timeouts. Deploys drain connections and resume idempotent background jobs.
Pin compiler image versions independently of web and service releases.

## Data and operations

- Ledger rows hold counts, prices, ids, and timestamps, not prompt contents.
- Disable body logging in gateway/error middleware. Project traces can contain source
  and secrets; keep them private and subject to retention/export policy.
- Publish processing locations and subprocessors for billing, storage, sandboxes,
  aggregator, and resolved model providers. Do not equate storage region with inference.
- Reconcile Stripe, provider request receipts, active sandboxes, and durable checkpoints
  on independent jobs. A restart must not lose reservations or leave orphaned compute.
- Revoke tokens immediately on deletion. Delete project objects, versions, runtimes,
  snapshots, and secrets through a retriable job. Show the retention schedule for
  backups and legally required financial records; do not promise instant deletion of
  every accounting record.
- Retain portable exports when an account stops paying for the documented grace
  period. Avoid permanent free storage as an accidental cancellation policy.
