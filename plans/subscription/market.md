# Market research: subscription pricing for coding agents

Researched 2026-10-01. Marks: **[V]** read today on the vendor's own page, **[S]**
from search results or third-party pages (some are SEO blogs), **[M]** from memory,
not checked, **[E]** an estimate. OpenRouter prices come from its live API on the same
day. Prices move fast in this market; recheck before setting numbers.

## Subscription coding agents

| Product | Tiers (per month) | Quota unit | Windows | Past the limit |
|---|---|---|---|---|
| Claude (Pro, Max) | Pro $20, Max 5x $100, Max 20x $200 [V] | An opaque allowance shared by chat and Claude Code [V] | Rolling 5 hours plus a weekly cap [V] | Usage credits with a monthly spend cap the user sets [V] |
| OpenAI Codex | Go $8, Plus $20, Pro $100 / $200 / $500 [V] | Messages per 5 hours by model on Plus; credits priced per 1M tokens [V] | 5 hours plus weekly on Plus, weekly only on Pro [S] | Credits or an API key [V] |
| Cursor | $20 individual, $40 per seat for teams [V]; third parties list Pro+ $60 and Ultra $200 [S] | Dollars of usage at API rates [S] | Monthly | On-demand usage, billed in arrears [V] |
| GitHub Copilot | Pro $10 (1,500 credits), Pro+ $39 (7,000), Max $100 (20,000) [V] | AI credits at $0.01, debited at each model's token rate [S] | Monthly | Pay per credit |
| Windsurf (now Devin) | Pro $20, Max $200 [V] | An allowance, size not published [V] | Daily plus weekly [V] | Extra usage at API price [V] |
| Google Antigravity, Jules | AI Pro $19.99, Ultra $99.99 and $199.99 [S] | Requests per day [S] | Refresh every 5 hours under a weekly cap [S] | n/a |
| Factory Droid | $20 / $100 / $200 [S] | Standard tokens, cached at one tenth [S] | Monthly | Droid Core: designated open models, free on separate rate limits [S] |
| Augment | $100 business plan at token cost plus 40% [S] | Credits, then dollars | Monthly | n/a |
| Zed | Pro $10 including $5 of tokens [S] | Tokens at list price plus 10% [S] | Monthly | Pay as you go |
| Kiro, Warp | $20 to $200 [S] | Credits [V/S] | Monthly | Credit packs |
| Replit, Lovable, Bolt, v0 | $20 to $100 [S] | Effort-based checkpoints, credits, or tokens [S] | Monthly | Top-ups |

Sources: [Claude Max](https://support.claude.com/en/articles/11049741-what-is-the-max-plan),
[Claude Code costs](https://code.claude.com/docs/en/costs),
[Codex pricing](https://learn.chatgpt.com/docs/pricing),
[Cursor](https://cursor.com/pricing),
[Copilot plans](https://docs.github.com/en/copilot/get-started/plans),
[Devin pricing](https://devin.ai/pricing),
[Kiro billing](https://kiro.dev/docs/billing/),
[Zed](https://zed.dev/pricing).

## Subscriptions that resell open-weight models

These are the closest analogs to jazyk.dev.

| Product | Price | Quota | How usage is counted | Usage granted vs price |
|---|---|---|---|---|
| Z.ai GLM Coding Plan | Lite $18, Pro $72, Max $160 [S] | Lite 2,000 credits per 5 hours and 10,000 per week; Pro 12,000 / 60,000; Max 28,000 / 140,000 [V] | Token-weighted credits: `(in × m_in + cached × m_cached + out × m_out) / 10,000` [V] | Only supported tools, no sharing, ban after 3 violations [V] |
| MiniMax Token Plan | $22 / $55 / $132 [V] | Rolling 5 hours plus weekly, counts not published [V] | Not defined | Overflow at $1 per 1,000 credits [V] |
| Kimi | ¥49 to ¥699 [V] | One token-based credit pool plus Kimi Code capped at 5 runtime hours a week [V] | Tokens | n/a |
| Alibaba Qwen Coding Plan | Pro $50 [V] | 6,000 per 5 hours, 45,000 per week, 90,000 per month [V] | One request is one model call; a task is 5 to 30 calls [V] | Interactive use only: no scripts or backends [V] |
| Synthetic.new | $30 per pack, up to 5 [V] | 500 requests per 5 hours, refilling 5% every 15 minutes, plus $24 a week of API credit [V] | Requests weighted by model price [V] | About 3.4x; restructured in Feb 2026 after losing money on new subscribers [S] |
| OpenCode Go | Go $10, Go Plus $40 [V] | Dollars per model: the 5-hour window is 20% of the monthly cap, the week 50% [V] | Dollars at list token rate | Up to 6x for cheap models [V] |
| Ollama Cloud | Pro $20 → $60 of usage, Max $100 → $300 [V] | Dollars at each model's rate, monthly, no rollover [V] | Tokens × rate, off-peak discounts [V] | 3x; concurrency 1 / 3 / 10 [V] |
| Cerebras Code | Pro $50 (24M tokens a day), Max $200 (120M) [V] | Tokens per day | Tokens | Very generous; both sold out [V] |
| Chutes | Plus $10, Pro $20 [V] | Daily, capped at a multiple of pay-as-you-go value since Feb 2026 [S] | Requests, then dollars | n/a |
| NanoGPT | $8 [S] | 60M input tokens a week, 10 concurrent [V] | Tokens | Tightened in Feb 2026: the top 1 to 5% drove over half the tokens [V] |
| ClinePass | $9.99 [V] | 2 to 5 times standard API rate limits [V] | n/a | 14 open models [V] |
| Kilo Pass | $19 / $49 / $199 [S] | $26.60 / $68.60 / $278.60 of credits [S] | Dollars at provider rates | 1.4x |
| Featherless | $10 / $25 / $75 [S] | Unlimited tokens, limited by concurrency [S] | Concurrency | n/a |

Sources: [Z.ai devpack](https://docs.z.ai/devpack/overview),
[Z.ai usage policy](https://docs.z.ai/devpack/usage-policy),
[MiniMax](https://platform.minimax.io/docs/guides/pricing-token-plan),
[Kimi](https://www.kimi.com/en/help/membership/membership-pricing),
[Qwen Coding Plan](https://www.alibabacloud.com/help/en/model-studio/coding-plan),
[Synthetic pricing](https://synthetic.new/pricing),
[Synthetic rate limits](https://synthetic.new/rate-limits),
[OpenCode Go](https://opencode.ai/docs/go/),
[Ollama](https://ollama.com/pricing),
[Cerebras Code](https://www.cerebras.ai/code),
[Chutes](https://chutes.ai/pricing),
[NanoGPT update](https://nano-gpt.com/blog/subscription-update-february-2026),
[ClinePass](https://cline.bot/cline-pass).

## Token prices underneath

USD per 1M tokens: input / output / cache read, from the OpenRouter API on 2026-10-01
[V].

| Model | OpenRouter default | Official provider | Note |
|---|---|---|---|
| GLM-5.3 | 0.22 / 3.39 / 0.18 | Z.AI 1.40 / 4.40 / 0.26 | Cheapest endpoint Baidu FP8 0.155 / 0.49 / 0.029 |
| GLM-5.3-Flash | 0.15 / 0.50 / 0.03 | n/a | |
| Kimi K3 | 0.66 / 10.00 / 0.66 | Moonshot 3.00 / 15.00 / 0.30 | |
| Kimi K2.7 Code | 0.67 / 3.35 / 0.18 | n/a | |
| DeepSeek V4.1-Flash | 0.03 / 0.50 / 0.01 | 0.15 to 0.30 / 0.60 to 1.20 / 0.003 to 0.006 (off-peak to peak) | Peak 01 to 04 and 06 to 10 UTC on weekdays |
| DeepSeek V4-Pro | 0.66 / 1.98 / 0.022 | 0.66 to 1.32 / 1.98 to 3.96 / 0.022 to 0.044 | |
| MiniMax M3 | 0.30 / 1.20 / 0.06 | same | |
| Qwen3.8-Flash, 3.7-Plus, 3.8-Max | 0.15 / 0.47 / 0.016; 0.32 / 1.28 / 0.064; 2 / 6 / 0.25 | n/a | |
| gpt-oss-120b | 0.03 / 0.17 (cheapest) | Groq 0.15 / 0.60, Cerebras 0.35 / 0.75 | |
| Frontier for comparison | Sonnet 5.5 2 / 10 / 0.20; Opus 5.5 4 / 20 / 0.20; GPT-6.1 Sol 2 / 10 / 0.10; Gemini 3.8 Flash 0.75 / 3.75 / 0.075 | | |

OpenRouter mechanics [V]:

- 5.5% fee on card top-ups, no markup on inference. BYOK is free for the first $25k a
  month, then 5%. No public volume discount; a business tier with EU in-region routing
  at an 8% fee is reported [S].
- Caching is automatic for DeepSeek, Z.AI, Moonshot, Groq, and OpenAI. OpenRouter
  keeps follow-up requests on the same provider for cache hits (10 minutes idle, or
  pinned with `session_id`).
- The default endpoint is often a small provider with FP4, FP8, or unstated
  quantization. Pin the provider and the quantization for stable quality and cache
  hits.

Sources: [OpenRouter FAQ](https://openrouter.ai/docs/faq),
[prompt caching](https://openrouter.ai/docs/guides/best-practices/prompt-caching),
[DeepSeek pricing](https://api-docs.deepseek.com/quick_start/pricing).

## What heavy users consume

- Anthropic: about $13 per developer per active day and $150 to $250 per month across
  enterprise deployments; 90% stay under $30 per active day [V].
- Its example agent session is 1.2k input, 5.3k output, 940k cache read, and 50k cache
  write tokens: about 94% cache reads, so the cache-read price drives the cost [V].
- NanoGPT: the top 1 to 5% of users consume over half the tokens [V]. Anthropic said
  its weekly caps would touch fewer than 5% of subscribers [S].
- Heavy individuals: 10B tokens over 8 months on a $200 plan, about 19 times the price
  at API rates [S].

Jazyk sessions differ from a chat agent's: many short sessions (24 rounds at most, a
24k-character context budget) with a fixed contract and skills up front. The cache
ratio is likely lower than 94%; phase 0 measures it.

## Billing providers

| Option | Cost | Fit |
|---|---|---|
| Stripe Billing | 0.7% of billing volume plus card fees [V] | Best tooling, but jazyk is then the merchant: EU VAT OSS filed from Slovakia, UK and US taxes, Stripe Tax extra [M] |
| Stripe Managed Payments | about 3.5% on top of Stripe fees [S] | Merchant of record on Stripe, public preview since Feb 2026, absorbing Lemon Squeezy [S] |
| Paddle | 5% plus 50¢ [S] | Mature merchant of record; weak usage billing, which jazyk does not need |
| Polar | 5% plus 50¢ on Starter, lower on paid plans [V] | Developer-focused merchant of record |
| Lemon Squeezy | 5% plus 50¢ [S] | Being folded into Stripe Managed Payments |
| Metronome, Orb, Lago, OpenMeter | enterprise or self-hosted | Usage billing engines; not needed while tiers are flat fees |

Sources: [Stripe billing pricing](https://stripe.com/billing/pricing),
[Polar pricing](https://polar.sh/resources/pricing).

## Lessons

- The windows are settled: a 5-hour window plus a week (Claude, Codex Plus,
  Antigravity, GLM, MiniMax, OpenCode Go, Synthetic). OpenCode Go states its windows
  as fractions of the monthly cap; Synthetic refills gradually instead of locking out.
- The unit moved from requests to dollars of usage at list token rates: Cursor in
  2025, Copilot in June 2026, Ollama in September 2026, and OpenCode Go, Synthetic,
  Augment, Zed. Request counts get gamed by long agent turns; Alibaba now counts each
  model call and Synthetic weighs by price.
- Open-model resellers grant about 3 to 6 times the price in list-rate usage;
  frontier-model products about 1 to 2 times. The generous ones rely on most users
  using a fraction, and several had to tighten (Synthetic, NanoGPT, Ollama pausing Max
  sign-ups, Cerebras selling out).
- The coding plans (Z.ai, Alibaba) forbid backends, automation, and resale. Jazyk runs
  automated sessions, so it buys API capacity, never coding plans.
- Changes cause the backlash, not prices: Cursor's "unlimited" that meant Auto only
  (refunds in July 2025), Claude's weekly caps in 2025, GLM's price doubling with a
  regional premium, Windsurf's switch with no grandfathering, Augment's unpredictable
  credits. Avoid "unlimited", publish the formula, grandfather existing subscribers,
  and start tight so that every change is an increase.
- Concentration is real: plan for concurrency caps, a weekly ceiling per account, and
  an overflow path (top-up credits, or a cheap model on its own rate limit, as Factory
  does with Droid Core).
