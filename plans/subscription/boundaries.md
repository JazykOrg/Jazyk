# Compiler, web, and cloud boundaries

The compiler may become a separate open-source project. Cloud consumes its
interfaces. Local compilation must not need an account, Stripe, AWS, or the cloud
database. This is the target layout for implementation, not a move of existing specs
or a claim that proposed behavior already exists.

## Documentation

| Tree | Owns | Excludes |
|---|---|---|
| `docs/compiler/` | Graph, goals, scheduling, sessions, generic model profiles, usage observations, resource stops | Tiers, Stripe, accounts, sandbox vendors |
| `docs/consumers/` | Generation, binding, verification, portable command limits | Hosted packaging |
| `docs/frontends/` | CLI, ACP, MCP, LSP, viewer, local server API, browser compiler adapter | Hosted account workflows |
| `docs/web/` | Web application navigation, backend selection, projects, account/usage screens | Goal derivation and monetary enforcement |
| `docs/cloud/` | Accounts, billing, gateway, catalog, orchestration, storage, GitHub, operations | Compiler graph ownership |
| `docs/site.md` and `site/` | Marketing site and commercial offer | Compiler constants |
| `plans/subscription/` | Alternatives, research, estimates, unresolved choices | Normative implemented behavior |

Create these pages when their implementation phase starts:

- `docs/cloud/cloud.md`: overview and service boundaries.
- `docs/cloud/accounts.md`: identity, devices, sessions, and access.
- `docs/cloud/billing.md`: entitlements, renewals, balances, refunds, receipts.
- `docs/cloud/gateway.md`: reservations, routing, and usage API.
- `docs/cloud/models.md`: managed catalog and rate-card behavior.
- `docs/cloud/sessions.md`: runtime lifecycle and orchestration.
- `docs/cloud/storage.md`: modes, checkpoints, recovery, retention.
- `docs/cloud/github.md`: installations, import, publish, conflicts.
- `docs/cloud/operations.md`: deployment, capacity, reconciliation, restore.
- `docs/web/web.md`: application overview and backend choices.
- `docs/web/accounts.md`: account and usage screens.
- `docs/web/projects.md`: storage choice, save, publish, export.

Start web work from the [workflow proposal](./workflows.md) and
[prototype](./ux/index.html). `docs/web/` owns the unified picker, run/save/AI labels,
and upgrade journeys. The frontend specs retain native ACP/LSP and browser capability
contracts. Narrowing browser onboarding must include a transition for existing projects.

The [GUI spec](../../docs/frontends/gui.md) mixes server and page behavior. When web
work starts, move page-only sections into `docs/web/` and leave links from the
existing page. Keep the local API/capability contract in `docs/frontends/`. Update
relative links in one coherent change; do not duplicate requirements.

Link the new web/cloud roots from [main.md](../../docs/main.md) when they exist.
Keep the aggregate dogfood project. Add a compiler-only project config when testing
extraction of the open-source tree, scoped to compiler, consumers, portable frontends,
and benchmark specs. Keep prompt/skill exclusions. Do not implicitly split the
persistent dogfood graph.

## Implementation packages

- `bootstrap/` remains the compiler and portable frontends. Add generic interfaces
  here before cloud integrations.
- New top-level `cloud/` owns authentication, billing, gateway policy, Postgres, S3,
  and sandbox adapters. One Rust/axum service with modules is sufficient initially.
  It does not import compiler internals to derive or prioritize goals.
- Keep `bootstrap/app/` in place initially. Put cloud page behavior behind the
  existing backend interface. A later move to `web/` need not delay local billing.
- Keep Stripe, OAuth, cloud SDKs, SQL clients, and deployment definitions out of the
  compiler dependency tree. Use libraries for infrastructure as `AGENTS.md` requires.

Cloud starts a released binary and uses versioned API, events, and checkpoint
contracts. One sandbox holds one project's graph and build lease. Platform leases
fence competing sandboxes; they do not replace compiler leases. Cross-project work
can run concurrently. Each project's builds remain sequential.

## Portable additions

- Named model/provider profiles and per-task selection.
- Usage observations and optional receipts. Unknown cost is not zero.
- Typed provider resource stops and resumable run status. Financial exhaustion is
  not a semantic diagnostic or a failed goal.
- Timeouts, cancellation, output limits, and a consistent export/checkpoint seam.

Hosted setup, network rules, storage mode, and secrets belong to the cloud project's
configuration. A portable environment recipe can be specified later if useful.
Do not add a provider-specific `[cloud]` table to compiler settings for the beta.

Optional CLI login/usage integration resolves a standard credential/profile. Core
code sees a provider, model, limits, and errors, not an account tier.

The compiler's no-migration/version-mismatch policy remains. Optional telemetry must
not casually force graph resets. Restore pins the compiler version and retains the
pre-upgrade checkpoint. The separate cloud database uses ordinary SQL migrations;
the compiler graph policy does not prohibit those.

## Acceptance

- Free local commands and the browser backend build without cloud credentials.
- Routing and resource stops can be tested without Stripe or a sandbox.
- Cloud outages do not disable local BYOK or graph inspection.
- An extracted compiler tree builds with its payloads and tests without `cloud/`
  or web billing code.
- Links and anchors resolve after moves. New specs use the existing short declarative
  style and contain no em dashes.
