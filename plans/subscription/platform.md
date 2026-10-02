# The platform behind jazyk.dev

Revised 2026-10-02. The trusted platform hosts the shared Jazyk service and owns
accounts, entitlements, reservations, project bindings, storage transactions, and
optional execution orchestration. Compiler goals, graph gates, and mutations remain
inside the portable compiler. See [server](./server.md), [boundaries](./boundaries.md),
and [optional execution](./cloud-sessions.md).

## Components and trust

| Component | Responsibility | Persistent state |
|---|---|---|
| API and account UI | Identity, devices, access, subscriptions, project list | Postgres |
| Model gateway | Admission, routing, streaming, settlement, receipts | Usage ledger and reservations |
| Shared compiler service | Concurrent project contexts, admission, per-project scheduling and commit | Jobs, writer epochs, graph-state manifests |
| Protocol edge and local connector | Authorized project API, ACP/LSP, revision exchange and events | Device/project bindings and resume cursors |
| Storage service | Virtual workspace revisions, graph commits, recovery, exports | S3 objects and transactional Postgres heads |
| GitHub adapter | Tree/blob reads, draft overlay, explicit publish/PR | Installation, branch, base commit and publish receipts |
| Optional execution broker | Materialize, run, import outputs, destroy, reconcile provider inventory | Execution state, evidence and compute receipts |
| Preview service | Private access to an untrusted app | Expiring access grants |

One service with modules and background jobs is enough initially. Postgres supplies
transactions, uniqueness, and durable job state. Add Redis or a separate queue only
when measurements justify it. Hosted compilation runs in the shared service. Customer
code execution remains outside it. No per-project sandbox is required for compilation.

Use HTTP/streaming, OAuth, Stripe, SQL, AWS, and archive libraries. Production supplier
selection remains open between OpenRouter, OpenAI, and Anthropic. Start with one
measured integration and a small catalog. Jazyk-specific admission/ledger logic
is custom; provider wire protocols and credential proxies are not an excuse to
hand-roll standard infrastructure. Compare an existing proxy if multiple direct
provider protocols later justify one.

## Accounts and local access

- Google and GitHub sign-in via established OAuth/OIDC libraries or a hosted auth
  service. GitHub login and a GitHub App repository installation are separate grants.
- Use stable provider subjects as identities. Explicitly authenticate account linking;
  matching email addresses alone must not merge paid accounts or promotional grants.
- One account owner initially. Organizations, seats, and pooled team budgets come later.
- The proposed `jazyk login` uses browser/device authorization. Issue revocable
  credentials for the project service with short-lived, project-scoped access.
- Store refresh credentials through an OS credential-store library. A documented
  permission-restricted file fallback is for headless systems. Keep secrets out of
  project TOML and browser local storage.
- Accountless local access uses a local owner principal and the local server.
  Login adds remote project/credit access, not an automatic upload or route change.
  Explicitly select remote compilation and authorize a local folder's connector.
  Existing local CLI/env overrides and LocalRouter remain meaningful. Do not forward
  executable local configuration or local credentials as trusted hosted settings.
- Browser sessions use secure, HTTP-only cookies, CSRF protection, and exact origin
  checks. Authorize project access on every connection and operation.

Local-remote use transfers source/graph inputs to the remote compiler and its model
provider. The computer remains storage authority. Cloud-backed projects have durable
remote storage. Explain the different retention paths at selection; a local folder
does not imply local processing. See [local-remote storage](./storage.md#local-remote).

Use the [account/project record groups](./server.md#database-and-durable-state), scoped
database policies, and project capabilities. Authenticated membership is independent
of payment: exhausted credits prevent new paid jobs, not an owner's authorized reads
or exports within retention. Anonymous requests cannot start hosted model work.

## Canadian billing

Use the owner's Canadian Stripe account with its actual sole-proprietor details,
bank settlement settings, business address, and statement descriptor. Do not select
a merchant of record or assume an EU business. The proposed checkout currency is CAD;
USD and other currencies can be evaluated later.

Use hosted Checkout for purchase, Billing for subscription lifecycle, and Customer
Portal for payment method and cancellation. Jazyk's ledger grants service allowances;
Stripe is not the source of token usage. Keep model, managed-service, storage, and
optional execution usage categories distinct. One prepaid wallet is possible if its
rate card and reservations explain each debit. Never double-debit a model call inside
a reserved job budget.

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
- Failed renewal blocks new paid admissions once entitlement ends. Handle in-flight
  reservations, commit valid pending state through its storage authority, and stop
  optional execution within a bounded grace. Keep read/export policy explicit.
- Refunds and disputes create compensating entries. Revoke unused associated grants
  and suspend further spending if needed. Do not silently create a postpaid debt.
- Model top-ups and compute purchases each have their own idempotent grant. Preserve
  purchased balances across cancellation under the published terms.

Test duplicate/delayed events, renewal failure, upgrade/downgrade races, refund after
partial use, linked identities, tax calculation, and portal cancellation before live
billing. Use Stripe test mode until these pass.

## Gateway

Start with a project/job API and an internal provider adapter used by the embedded
ACP worker. A public compatible model endpoint is optional later work, not the core
subscription product. Specify exact paths before implementation. Implement the
[reservation protocol](./tiers.md#admission-and-settlement), not a preflight-only check.

- Device credentials authenticate account/project connections. Job/model capabilities
  bind tenant, project, run, active authority, and spend envelope. Optional execution
  credentials bind the environment too. No account-wide token enters customer code.
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
  local devices. All remote project jobs and execution environments share one ledger.
- Quota exhaustion is typed and non-retryable until allowance changes. Ordinary
  provider throttling/outages keep separate retry and backoff semantics.

No arbitrary upstream URL in a hosted subscription request. Local BYOK is a separate path.
The [workflow proposal](./workflows.md) defers cloud BYOK from the first beta.
When added, hold encrypted customer credentials in a broker, initially for OpenRouter
through the already supported integration. It consumes no Jazyk inference allowance
but still consumes managed-service/storage entitlement and any explicit execution
budget. Other providers and generic private/local endpoint tunneling can follow.
Local BYOK remains available throughout.

## AWS deployment

Proposed beta:

- One AWS region, one deployable shared API/compiler service on ECS/Fargate, and
  managed Postgres with backups. This deployment container hosts many projects; it
  is not a project execution environment. Use S3 for immutable workspace/state objects,
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
  re-estimate for compiler CPU/memory and measured concurrency before hosted release.

The service must support streaming responses and long-lived SSE/WS with explicit
idle timeouts. Deploys drain connections and resume idempotent background jobs.
Pin the compiler library/binary version per compatible state contract. Drain writers
on deploy and preserve checkpoints before a version change.

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
