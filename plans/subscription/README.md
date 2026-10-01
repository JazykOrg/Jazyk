# Plan: subscriptions and cloud sessions for jazyk.dev

Status: revised proposal, 2026-10-01. Owner decisions below are settled. Prices,
provider selection, and effort are proposals. This revision changes the plan only.
It does not provision services, run paid benchmarks, or change compiler behavior.

## Owner decisions

| Topic | Decision |
|---|---|
| Local subscription | Yes. One allowance for local `jazyk` and cloud sessions. Ship local access first. |
| Own provider | Keep local endpoints, API keys, and external ACP agents usable without a Jazyk subscription. Support mixing access methods by task. |
| Models | A measured selection, potentially including frontier models. Expensive models consume more allowance. Compilation, coding, and review may use different models. |
| Merchant | A Canadian sole proprietor using the existing Stripe account. Remove the Slovakia and merchant-of-record assumptions. |
| Control plane | AWS is acceptable. No EU-only constraint. |
| Sandboxes | Compare global providers. Select after testing the actual Jazyk workload. |
| Storage | Keep the first version simple. Evaluate cloud storage and GitHub as alternative project modes, with effort estimates. |
| Free | Local use only. A social-login promotion, perhaps $20, is a later option. No recurring hosted inference or compute grant at launch. |
| Documentation | Cloud and web specs belong under `docs/`, separated from the compiler for a possible future open-source split. |

Currency is not settled by the merchant's location. This plan uses CAD for proposed
customer prices and USD for upstream costs. That is a planning choice, not a claim
that the owner selected CAD or restricted sales to Canadian customers.

## Recommendation

- Sell managed model access for local Jazyk first. Keep BYOK available without an
  account. A subscriber can use their own provider for some work and the subscription
  for the rest.
- Publish a credit rate card and a billing-period allowance. Apply weekly and 5-hour
  pacing limits as well. Reserve cost before each upstream request.
- Expose model policies for compilation, judgment, coding, verification, and chat.
  Start with a small tested catalog. Do not assume cheap extraction is accurate.
- Use Stripe Checkout, Billing, and Customer Portal in the Canadian account. Use
  Stripe Tax where applicable. Jazyk remains the merchant.
- Run the trusted platform on AWS, outside the compiler package and the machines
  running user code.
- Compare E2B and Vercel first, then Daytona if either fails the workload or cost
  gates. Keep Modal and Northflank as credible alternatives. This is a conditional
  shortlist, not a provider commitment.
- Start cloud projects with versioned checkpoints in S3 and download/export. Add
  GitHub-backed projects separately. Every project has one source authority.
- Price cloud compute separately initially. A local subscriber need not buy unused
  compute. A cloud user with their own model key still pays for compute and storage.

## The files

- [market.md](./market.md): Kiro review, verified comparisons, and lessons.
- [tiers.md](./tiers.md): allowance, windows, economics, and spending stops.
- [models.md](./models.md): task routing, BYOK, frontier models, and measurement.
- [platform.md](./platform.md): Canadian Stripe billing, accounts, gateway, and AWS.
- [sandboxes.md](./sandboxes.md): global providers and workload cost math.
- [storage.md](./storage.md): storage alternatives, recovery, and effort.
- [cloud-sessions.md](./cloud-sessions.md): execution, lifecycle, and previews.
- [boundaries.md](./boundaries.md): documentation and package separation.

## Terms

- Session: one existing Jazyk model session over a goal batch.
- Usage window: the proposed rolling 5-hour allowance, not a Jazyk session.
- Billing period: the paid monthly subscription interval.
- Cloud session: a project's runtime from start or resume to stop or pause.
- Credit: a published amount of model usage. The example rate is 100 credits per
  US$1 at Jazyk's versioned rate card, not a promise of cash value.
- Compute hour: one running hour at the specified sandbox size.
- Checkpoint: a consistent, durable project version, including its persistent graph.

## Findings that change the previous plan

- Kiro illustrates shared local/cloud allowance and model-weighted credits. Arbitrary
  BYOK in its standard clients was not established from the official pages reviewed.
  OpenCode and Zed are clearer references for that feature.
- Rolling weeks do not imply a hard monthly cap of `4.33 × weekly allowance`.
  Concurrent calls, billing boundaries, annual discounts, and unpaid upgrades need
  explicit accounting.
- A quota check followed by an unreserved request can overspend. Counting rounds
  does not bound dollar cost. The gateway needs atomic reservations.
- A response header cannot report final streaming cost before completion. Use request
  receipts and a usage API.
- Rebuilding from prose loses ids, decrees, proposals, and judgment history. The
  persistent graph needs durable storage even when GitHub owns the source files.
- Pause is not a universal free-storage guarantee. Disks, snapshots, retention, and
  cross-cloud traffic belong in the estimate.
- `[executors]` selects agents, not embedded model profiles. Per-task model selection
  and typed budget stops require documented compiler changes.
- Hiding a key from a command's environment does not protect it from a compromised
  sandbox. Account-wide credentials stay outside it.

## Current implementation

These observations come from the repository:

- [GUI](../../docs/frontends/gui.md) and [browser backend](../../docs/frontends/wasm.md)
  already distinguish local-server and browser capabilities. Cloud execution is new.
- [LLM settings](../../docs/compiler/project-settings.md) accept an endpoint and
  credentials. Explicit CLI and environment settings retain their precedence.
  Login must not silently replace LocalRouter or a project's endpoint.
- [Executors](../../docs/compiler/project-settings.md#executors) separate batches by
  agent. Model-policy compatibility becomes another batching constraint.
- [Journal entries](../../bootstrap/src/model.rs) carry token counts, including
  external-agent reports. These are diagnostics, not a trusted billing ledger.
- [The process seam](../../bootstrap/src/sys.rs) runs `sh -c` with `.output()`.
  Generation and verification call it. Timeout, cancellation, and bounded output
  need work before cloud release.
- [TODO](../../docs/TODO.md) records repeated prompt overhead. Existing
  [validation](../../bootstrap/VALIDATION.md) has mixed model results. Neither
  establishes a hosted catalog or a reliable cost per successful project.

## Delivery sequence

Estimates are engineering days for one experienced maintainer, including specs and
focused tests. They are planning ranges, not measured delivery times. Vendor access,
benchmark runs, and product iteration add elapsed time. Write the named specs before
implementing behavior. Commit and push each coherent change.

| Phase | Work and documentation first | Exit condition | Effort |
|---|---|---|---|
| 0. Measure and specify | Generic usage records, model policies, stop semantics. Update `docs/compiler/`, `docs/benchmark/`, and relevant schemas. See [measurement](./models.md#measurement-gate). | Quality and cost table for representative tasks, warm/cold cache, failures, and retries. | 4–8 days |
| 1. Local subscription | `docs/cloud/` account, billing, gateway specs; `docs/web/` usage UI; CLI login and provider selection. AWS service, Stripe test mode, reservations, receipts. | Logged-out BYOK works. Streaming works. Concurrent requests and duplicate webhooks cannot overspend or double-grant. | 15–25 days |
| 2. Execution and provider trial | Command limits in consumer/tool specs. Cloud session and operations specs. Run [sandbox acceptance checks](./sandboxes.md#selection-gate). | Restore, cancellation, egress, SSE/WS, and usage reconciliation pass on the chosen provider. | 6–10 days |
| 3. Cloud-only private beta | Storage/session specs and web project flow. Orchestrator, relay, compute wallet, supported-provider BYOK, checkpoints, private preview, export. | VM loss preserves graph ids. One writer survives service failover. Exhaustion stops safely. | 10–18 days orchestration plus 6–10 days storage |
| 4. GitHub-backed projects | Cloud GitHub/storage specs and web publish/PR flow. | Import, explicit publish, moved branches, revoked installation, and restore preserve work without force-pushing. | 8–15 days, plus 2–4 if both storage modes ship together |
| 5. Public launch | Reconciliation, deletion/retention, restore drills, support, monitored rollout, pricing publication. | Measured margin and capacity pass. Cancellation and export work. No unresolved billing or data-loss defects. | 5–10 days |

The first local paid release is roughly 19–33 engineering days before contingency.
A cloud-only private beta adds roughly 22–38 days. Storage alternatives and overlap
are detailed in [storage.md](./storage.md#effort-and-release-options).

Top-ups can follow the first local beta. Teams, annual discounts, automatic product
deployment, two-way local/cloud sync, and signup incentives are later work.

Measurement uses the configured LocalRouter endpoint unless the owner requests a
change. Select candidate routes through it where possible. This plan does not pick
another default model or authorize a paid benchmark campaign.

## Remaining decisions

- Public CAD prices versus USD pricing from the Canadian account. The
  [draft economics](./tiers.md#illustrative-local-tiers) use CAD explicitly.
- Initial sales territories, province, and tax registrations. Resolve during billing
  setup, not from the repository's timezone.
- Which measured models meet each task's quality bar, and the frontier-use budget.
- The sandbox winner after the same workload runs on the finalists.
- Whether GitHub is needed for the first cloud beta. Default: cloud-only first.
  [GitHub-first](./storage.md#effort-and-release-options) suits a developer-only beta.
- The later promotion's currency and meaning: invoice discount or inference credits.
  Google/GitHub login alone does not establish a unique human.

Keep unresolved commercial choices here. Accepted behavior moves into the separated
specifications described in [boundaries.md](./boundaries.md).
