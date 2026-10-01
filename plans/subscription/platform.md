# The platform behind jazyk.dev

Today jazyk.dev is a static page on S3 and CloudFront, and jazyk has no backend. A
subscription needs a small trusted service: accounts, billing, the model gateway, and,
for cloud sessions, the project registry, the orchestrator, and the relay. This file
covers all but the sandbox side, which [cloud-sessions.md](./cloud-sessions.md)
covers.

## Components

| Component | Does | Holds |
|---|---|---|
| `api.jazyk.dev` | accounts, sign-in, subscription state, project registry, API for the page | Postgres |
| The gateway | OpenAI-compatible endpoint, quota windows, routing, metering | provider keys, usage ledger |
| The orchestrator | creates, resumes, pauses, snapshots sandboxes | sandbox provider key |
| The relay | proxies the page's API, SSE, and LSP WebSocket to a sandbox | per-session tokens |
| The proxies | egress filter, git credential proxy | git tokens |
| Object storage | project snapshots, artifacts | user data |

One Rust service (axum, as the GUI already uses) can hold all of these as modules at
first, with the egress proxy as a separate process. It never shares a host with user
code.

## Accounts

- Sign-in with GitHub (most users have it, and phase 4 needs a GitHub link anyway)
  and an email link as the fallback. OAuth through a crate, sessions in Postgres.
  A hosted auth service (WorkOS, Clerk, Supabase Auth) is the alternative if time
  matters more than one more vendor.
- An account owns projects, a subscription, top-ups, and device tokens.
- `jazyk login` runs the OAuth device flow and stores a device token in
  `~/.jazyk/config.toml`. The token scopes to the gateway only, can be revoked from
  the page, and lists its devices.
- Teams wait until phase 5: one owner per account until then.

## Billing

The tiers are flat fees with quotas jazyk enforces itself, plus prepaid top-ups. The
billing provider needs subscriptions and one-time purchases, nothing metered. That
widens the choice to every merchant of record.

- Recommended: a merchant of record (Paddle, Polar, or Stripe Managed Payments). From
  Slovakia, selling to consumers worldwide, it removes VAT OSS filing and every
  foreign registration for about 3.5 to 5% of revenue.
- The alternative is Stripe Billing with Stripe Tax, jazyk as the merchant: cheaper
  per transaction, a tax filing burden on the owner.
- Webhooks set the subscription state in Postgres; the gateway reads tier and limits
  from there (cached), never from the provider live.
- Card verification for the cloud trial goes through the same provider.
- Downgrades take effect at the period end; upgrades at once, with the new quotas
  immediately.

## The gateway

An OpenAI-compatible endpoint at `https://api.jazyk.dev/v1`. Jazyk already resolves
`base_url` and `api_key` by precedence, so a logged-in local binary and a sandbox
both reach it with no new client code: `JAZYK_LLM_BASE_URL` plus a token.

- Auth: a device token (local) or a short-lived session token minted per cloud
  session, scoped to one project.
- Before forwarding: check the tier, the window, the week, and concurrency; resolve
  the alias to a route ([tiers.md#the-model-menu](./tiers.md#the-model-menu)).
- After: compute the cost from the `usage` block and the route's price table, write
  it to the usage ledger (account, project, model, route, tokens, cost, time), return
  `x-jazyk-cost`.
- Streaming passes through; usage is read from the final chunk (`include_usage`,
  which jazyk already requests).
- Price tables are configuration, refreshed from OpenRouter's model API daily, with
  alerts when a price moves.
- Kill switches: per account, per model, global spend per hour.
- `GET /v1/quota` returns the windows, the week, top-ups, and reset times.

Build it rather than run LiteLLM or a similar proxy: the windows, the session
overrun, the typed refusal, and the estimate endpoint are domain logic, and the
pass-through is a few hundred lines on crates jazyk already uses.

Upstream accounts: OpenRouter (pinned providers, one key, 5.5% top-up fee) for most
models; DeepSeek's own API when off-peak pricing and its cache reads beat it. Never a
coding plan: their terms forbid backends and automation
([market.md](./market.md#lessons)).

## Hosting

- Recommended: the existing AWS account in eu-central-1, beside jazyk.dev's bucket.
  One small instance or container for the service, RDS or a managed Postgres, S3 for
  snapshots. EU data residency for GDPR.
- Hetzner or Fly are cheaper for the service itself; the sandbox provider dominates
  the bill either way ([sandboxes.md#cost-math](./sandboxes.md#cost-math)).
- `api.jazyk.dev` for the API and the relay. Previews of user products go on a
  separate registrable domain (for example `jazyk.app`), never a subdomain of
  jazyk.dev, so a generated app cannot read the page's storage or cookies.
- Deploys follow the existing pattern: GitHub Actions with OIDC, monitored to the end.

## Privacy and terms

- Subprocessors: the merchant of record, the sandbox provider, OpenRouter and the
  model providers it routes to (some outside the EU), AWS. Published in a list.
- Prompts and project content pass through the gateway; the ledger stores counts and
  costs, never content. Traces stay inside the project's `.jazyk/`.
- Terms forbid resale and account sharing (the concurrency cap enforces it), and
  forbid using compute for mining, scanning, or mail.
- Account deletion deletes snapshots, sandboxes, and tokens; the ledger keeps
  anonymized rows for accounting.
