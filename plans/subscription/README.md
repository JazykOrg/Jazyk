# Plan: subscriptions and cloud sessions for jazyk.dev

Status: proposal. Written 2026-10-01. Nothing here is scheduled. The docs-first rule
applies: every phase names the pages that change before the code does. Every step that
costs money (model runs for measurement, a sandbox provider account, a server, a
merchant of record) needs the owner's approval per action.

## The owner's ask

- A monthly subscription to jazyk.dev with several tiers. A tier grants a weekly quota
  and a session quota, the way coding agent subscriptions do.
- The models are mostly cheap open-weight models, bought through OpenRouter or a
  similar provider. Research how other harnesses price, especially the ones that
  resell open models.
- Cloud sessions: jazyk running in the cloud, so a project is actually built and a
  product produced in a session, not only compiled into a graph. Isolation matters.
  Research the existing infrastructure and how others do it.

## The files

- [market.md](./market.md): what coding agent subscriptions and open-model resellers
  charge and how their limits work, the token prices underneath, what heavy users
  consume, and the lessons from their price changes.
- [tiers.md](./tiers.md): the quota unit, the windows, the tiers with their numbers,
  the unit economics, overage, the model menu, and how jazyk itself enforces and shows
  the quota.
- [platform.md](./platform.md): the services behind jazyk.dev: accounts, billing, the
  model gateway, the project registry, hosting.
- [cloud-sessions.md](./cloud-sessions.md): jazyk in a sandbox per project: the
  architecture, isolation, egress, credentials, persistence, previews of the product,
  lifecycle, abuse, and the changes to jazyk.
- [sandboxes.md](./sandboxes.md): how Codex, Claude Code, Cursor, Copilot, Devin,
  Replit, and Lovable isolate sessions, the sandbox providers with prices, the
  self-host options, and the cost math.

## Terms

Jazyk already uses "session" for one model run over a goal batch. This plan keeps that
meaning and names the rest:

- Window: the rolling 5-hour quota, what Claude and Codex call the session limit.
- Week: the rolling 7-day quota.
- Cloud session: one project's sandbox, running jazyk, from resume to pause.
- Usage: model spend in US dollars at the upstream list price of the route used, the
  one unit every quota is counted in.
- Compute unit: one hour of a 2 vCPU, 4 GB sandbox.

## Today

- Everything is local. `jazyk gui` binds `127.0.0.1`, mints a token per start, and
  jazyk.dev (a static page on S3 and CloudFront) connects to it. The page also runs
  the compiler as wasm in the tab with an in-page or user-supplied model.
- There is no backend, no account, no billing, and no price data. Cost tracking counts
  tokens (journal `tokens`, `cachedTokens`, `status.yaml` `costs`, `report_spend`),
  never money, and the journal does not record the model per entry.
- Build and test commands run as plain `sh -c` in the deliverable. The only
  confinement is the path check on tool arguments. There is no timeout on
  `run_command`, `run_build`, or the test runners, although
  [generate.md](../../docs/compiler/goals/generate.md) says `run_command` is bounded
  by one.
- No hosted open model has been benchmarked. `qwen3.8:27b` is capable for generation
  and verification but not for compilation. The TODO list notes about 48k characters
  resent per round, and one three-sentence document billed 401k tokens.

## The shape of it

Two products on one account, shipped in that order:

1. Jazyk Pro for your own machine. A subscription buys model usage through a jazyk
   gateway. `jazyk login` points the local binary at it. No compute is hosted, so the
   first paid product carries no isolation risk and tests whether people pay.
2. Cloud sessions. The same subscription adds compute units. A project lives in a
   Firecracker microVM from a managed provider (E2B first). jazyk.dev connects to it
   through a relay as it connects to a local server today. The project is built, its
   tests run, and the product is previewed and downloaded from there.

Every quota is counted in usage, at the gateway, outside anything the user controls.
The windows follow the industry shape: a 5-hour window and a week. Each tier is sized
so that a user who maxes every window costs no more than the price, and a typical user
leaves a healthy margin. Raising a quota later is welcome. Cutting one causes the
backlash every vendor in [market.md](./market.md#lessons) went through.

## Phases

Each phase stands on its own.

0. Measure (days, with approved model spend). Record the model and the cost per
   journal entry. Benchmark the candidate open models through OpenRouter with pinned
   providers. Compile and generate `bootstrap/example/f1`, `f2`, `example-sort`, and
   `example-erp` with each, and table the tokens, cache ratio, and dollars per build.
   Cut the per-round overhead the TODO names. The tier numbers in
   [tiers.md](./tiers.md) are placeholders until this table exists. Pages:
   [graph.md](../../docs/compiler/graph.md) (journal fields),
   [benchmark.md](../../docs/benchmark/benchmark.md), `known-results.yaml`.
1. Accounts, the gateway, and the subscription for local use. Sign-in on jazyk.dev,
   the merchant of record, the gateway with its windows, `jazyk login`, usage bars in
   the CLI and the GUI, a build that parks at a goal boundary when the quota runs
   out, and an estimate before a build. Pages: a new `docs/frontends/cloud.md`,
   [cli.md](../../docs/frontends/cli.md), [gui.md](../../docs/frontends/gui.md),
   [sessions.md](../../docs/compiler/sessions.md),
   [compilation.md](../../docs/compiler/compilation.md),
   [project-settings.md](../../docs/compiler/project-settings.md),
   [site.md](../../docs/site.md) (a pricing page).
2. Hardening jazyk for untrusted commands. Timeouts, output caps, and process-group
   kill on every command, an environment scrubbed of secrets, and a `[cloud]` setup
   command. This is worth doing locally too. Pages:
   [gen.md](../../docs/consumers/gen.md), [generate.md](../../docs/compiler/goals/generate.md),
   [project-settings.md](../../docs/compiler/project-settings.md).
3. Cloud sessions, invite only. The orchestrator on E2B, a microVM per project, the
   relay, the GUI's cloud backend, default-deny egress, credentials only at the
   proxies, persistence to object storage, idle pause. Pages: `docs/frontends/cloud.md`,
   [gui.md#connecting-to-a-local-jazyk](../../docs/frontends/gui.md#connecting-to-a-local-jazyk)
   (a cloud backend beside it), [control-plane.md](../../docs/compiler/control-plane.md).
4. The product in the cloud. Preview URLs on a separate domain, artifact downloads,
   GitHub through the git proxy, import from a repository or an example. Pages:
   `docs/frontends/cloud.md`, [gui.md#product](../../docs/frontends/gui.md#product).
5. Open to everyone. Abuse heuristics, top-up credits, teams. Revisit self-hosting on
   Hetzner bare metal when managed sandbox spend passes about $2k a month
   ([sandboxes.md](./sandboxes.md#cost-math)).

Phase 0 decides whether the economics work at all: a subscription is a margin on the
cost of a build, and that cost is not measured yet.

## Open questions for the owner

- Can the subscription be used from a local `jazyk`, not only in the cloud?
  Recommended: yes, it is phase 1 and the cheapest way to learn. Since the quota is
  counted at cost, usage spent outside jazyk loses no money; the concurrency cap stops
  resale.
- Frontier models: resell them inside the quota (they drain it about 8 times faster
  than DeepSeek Flash), or only through the user's own key? Recommended: the user's
  own key or an external ACP agent (Claude Code, Codex) at first; add a strong tier to
  the menu when phase 0 shows which goal kinds need it.
- The merchant of record: Paddle, Polar, or Stripe Managed Payments; or Stripe Billing
  with VAT filed from Slovakia. Recommended: a merchant of record
  ([platform.md#billing](./platform.md#billing)).
- Price in USD or EUR. Recommended: USD on the page (the market prices in USD), the
  merchant of record localizes.
- The sandbox provider: E2B (best fit, US company, EU region to confirm) or Northflank
  (EU West, about 2.5 times cheaper, more to build). Recommended: E2B for the beta
  behind a `Sandbox` interface, Northflank as the fallback.
- Where the control plane runs: the existing AWS account (eu-central-1) or Hetzner or
  Fly. Recommended: AWS, beside the existing hosting, unless cost says otherwise.
- Project storage in the cloud: a jazyk-hosted git repository per project, the user's
  GitHub, or both. Recommended: object storage snapshots always, GitHub optional.
- A free tier: local only (the binary stays fully usable with any endpoint) plus a
  one-time cloud trial after card verification. Recommended: yes, and never free
  compute without a card.
- Do the cloud pages live in `docs/` (behavior of the binary and the page) with prices
  only on the site? Recommended: yes. `docs/` states what jazyk does with a quota and
  a sandbox; the numbers live on jazyk.org's pricing page.
