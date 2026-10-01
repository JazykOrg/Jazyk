# Tiers, quotas, and unit economics

Every number here is a placeholder until phase 0 measures what a build costs
([README](./README.md#phases)). The method is what this file proposes; the numbers
show that the method yields sensible tiers at today's prices.

## The unit: usage in dollars

A quota is counted in usage: model spend in US dollars at the upstream list price of
the route the gateway used, cache reads at the cache-read price. The user sees
percentage bars, never raw dollars, with a translation into builds ("about 6 builds
of a project this size").

Why dollars and not requests or tokens:

- The market moved there for good reasons ([market.md#lessons](./market.md#lessons)).
  A request count is gamed by long turns; a token count treats a cache read like an
  output token, though one costs a hundred times the other.
- Expensive models drain the quota faster on their own, so one quota covers the whole
  model menu without multipliers.
- Usage spent anywhere costs jazyk exactly what it counts, so a quota sized to the
  price never loses money, whatever the client.

The gateway counts usage from each response's `usage` block (input, cached input,
output) times the route's price table. It never trusts a figure from the client or
the sandbox.

## The windows

Two rolling windows, both enforced at the gateway:

- The week: rolling 7 days. The real cap. Sized from the price (below).
- The window: rolling 5 hours, 30% of the week. It spreads use so capacity planning
  holds and a runaway build cannot burn a week in an afternoon. 30% means a week is
  about three and a half full windows.

A monthly cap is implied: 4.33 weeks. No rollover. No daily cap.

Concurrency per account caps parallel model requests: 2 on Pro, 4 on Max, 8 on Ultra.
Jazyk builds are sequential by design, so 2 covers a build plus a chat; more than
that is resale or sharing.

## Sizing rule

A tier is sized so that a subscriber who maxes every week and every compute unit costs
no more than the price after fees. Margin comes from the fact that most subscribers
use a fraction:

```
net         = price - merchant fee (about 5% + $0.50)
compute     = compute units × $0.17   (E2B, 2 vCPU 4 GB, per hour)
storage     = snapshot storage at object storage rates
weekly cap  = (net - compute - storage) / 4.33
```

At 35% average utilization, the gross margin is about 60% of net. This is stricter
than the open-model resellers (3 to 6 times the price, relying on averages) and
deliberately so: start tight, then raise quotas as the measured utilization allows.
Every change is then an increase.

## The tiers

| | Free | Pro | Max | Ultra |
|---|---|---|---|---|
| Price per month | $0 | $20 | $60 | $200 |
| Weekly usage at cost | none (your own key) | $3.25 | $10 | $32 |
| 5-hour window | none | $1.00 | $3.00 | $9.50 |
| Compute units per month | one-time trial, 3 | 25 | 80 | 300 |
| Largest sandbox | 2 vCPU, 4 GB | 2 vCPU, 4 GB | 4 vCPU, 8 GB (2 units per hour) | 8 vCPU, 16 GB (4 units per hour) |
| Cloud projects running at once | 1 (trial) | 1 | 2 | 4 |
| Project storage | 1 GB | 5 GB | 25 GB | 100 GB |
| Concurrent model requests | n/a | 2 | 4 | 8 |
| Network | registries only | registries plus an allowlist | full internet, after account age | full internet |

- Annual billing: two months free.
- Free keeps the whole binary: the local CLI and GUI with any OpenAI-compatible
  endpoint or ACP agent, and the wasm demo on jazyk.dev. The cloud trial needs a
  verified card.
- Worst-case check for Max: net $56.50, compute 80 × $0.17 = $13.60, storage about
  $0.60, weekly cap (56.50 − 13.60 − 0.60) / 4.33 = $9.77.

### What a week buys

Assumed jazyk mix [E]: 25% uncached input, 70% cache reads, 5% output. Blended price
per 1M tokens and tokens per week:

| Model and route | Blended $/1M | Pro | Max | Ultra |
|---|---|---|---|---|
| DeepSeek V4.1-Flash, official, peak | 0.14 | 23M | 72M | 230M |
| MiniMax M3 | 0.18 | 18M | 56M | 180M |
| Kimi K2.7 Code, OpenRouter | 0.46 | 7M | 22M | 69M |
| GLM-5.3, Z.AI | 0.75 | 4.3M | 13M | 43M |
| Sonnet 5.5, for comparison | 1.14 | 2.9M | 8.8M | 28M |

The TODO list records one three-sentence document billing 401k tokens. At that rate
Pro on GLM-5.3 is about ten such builds a week. Token efficiency is the product's
margin: cutting the per-round overhead (about 48k characters resent per round) moves
every number in this table, so it belongs in phase 0, before the prices are set.

## The model menu

The gateway exposes aliases, never raw upstream ids, so a route can change without a
client change:

- `jazyk/fast`: the cheapest model that passes the benchmark's extraction tier
  (candidates: DeepSeek V4.1-Flash, GLM-5.3-Flash, Qwen3.8-Flash).
- `jazyk/standard`: the strongest open model at a sane price (candidates: MiniMax M3,
  DeepSeek V4-Pro, Kimi K2.7 Code, GLM-5.3).
- `jazyk/auto`: the default. Routes per goal kind, the way `[executors]` already
  allows: alignment and extraction on `fast`, judgment and GC on `standard`. The
  routing table comes from phase 0's benchmark results, per goal kind.

Each alias pins provider and quantization on OpenRouter, or calls the provider
directly where direct is cheaper with caching (DeepSeek's own API: off-peak prices and
cache reads at $0.003 to $0.006). Prompt caching only pays when follow-up calls land
on the same provider, so the gateway pins a session's calls with OpenRouter's
`session_id` (the jazyk session id).

Frontier models stay outside the quota at first: the user's own key in `[llm]`, or an
external ACP agent through `[executors]`. A BYOK project consumes compute units only.

## Past the limit

When the window or the week runs out:

1. The build stops at the next goal boundary (below), the graph stays consistent, and
   the verdict says when the window resets.
2. The user picks: wait; buy a top-up; switch the project to their own key; or
   upgrade.

Top-ups are prepaid packs ($5, $20, $50) at list price plus 20%, valid 12 months, used
only after the window and the week are spent. Prepaid means no bad debt, and the
margin pays for the merchant fee. Compute top-ups work the same way. There is no
postpaid overage: a surprise bill is the backlash the market keeps relearning.

## How jazyk enforces and shows it

The gateway enforces; jazyk makes enforcement painless. Goals are derived from disk
and a changeset commits atomically, so stopping between sessions loses nothing.

- Before each session, jazyk asks the gateway for the remaining quota
  (`GET /v1/quota`) and compares it with the batch's estimate. If the batch cannot
  fit, the build stops before the session starts, rather than failing mid-session and
  wasting the tokens a retry and a park would cost.
- A session that started under quota finishes: the gateway allows an overrun bounded
  by one session's budget (`SESSION_ROUNDS` rounds). The overrun is charged.
- A gateway refusal is typed: HTTP 429 with
  `{"error": {"type": "quota_exhausted", "window": "5h" | "week", "resetsAt": ...}}`.
  `llm.rs` maps it to its own error, not a transient failure, so the retry loop and
  `ENDPOINT_BREAKER` do not spin on it.
- A stopped build reports a verdict of `paused` with the reason and the reset time,
  beside `converged` and the rest. `jazyk compile` after the reset resumes from disk.
- Estimates: `jazyk preview` already assembles the next prompt. A whole-build estimate
  sums the board's batches, calibrated by `jazyk status --costs` history per goal
  kind, and states it as a share of the window and the week ("this build: about 15%
  of your window"). The GUI shows it on the build button.
- Usage visibility: `jazyk usage` (window, week, compute, top-ups, reset times), a
  line in `jazyk status`, and bars in the GUI.
- The journal records `model` and `cost` on each session entry, from the gateway's
  response header (`x-jazyk-cost`), so `jazyk status --costs` reports dollars beside
  tokens. This is useful without any subscription and lands in phase 0.

Pages: [sessions.md](../../docs/compiler/sessions.md) (quota check before a session),
[compilation.md](../../docs/compiler/compilation.md) (the `paused` verdict),
[graph.md](../../docs/compiler/graph.md) (journal `model`, `cost`),
[cli.md](../../docs/frontends/cli.md) (`jazyk login`, `jazyk usage`),
[gui.md](../../docs/frontends/gui.md) (usage bars, the estimate).
