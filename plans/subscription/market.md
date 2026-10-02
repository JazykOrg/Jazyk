# Market research and the Kiro comparison

Reviewed 2026-10-01 against the official pages linked below. Vendor facts, Jazyk
recommendations, and estimates are distinguished. Prices here are USD unless stated
otherwise. Recheck before publishing an offer.

The previous comparison mixed vendor documentation, search snippets, memory, and
estimates. This revision keeps the evidence that informs the design. It removes
unverified model prices and claims about competitors' losses.

The [2026-10-02 server plan](./README.md) supersedes Jazyk architecture suggestions
in this research. Competitors' sandbox workflows do not imply a container is needed
for Jazyk's compilation-only service. This page remains dated research input.

## Kiro

### Product and subscription

Kiro covers an IDE, CLI, web sessions, and the open-source Crew workspace. The web
product runs delegated work in sandboxes. Local and web usage share the account's
allowance. This supports separating where work runs from who supplies its model.
Sources: [Kiro](https://kiro.dev/), [pricing FAQ](https://kiro.dev/pricing/),
[Crew](https://kiro.dev/crew/).

| Plan | Monthly price | Included credits |
|---|---:|---:|
| Free | $0 | 50 |
| Pro | $20 | 1,000 |
| Pro+ | $40 | 2,000 |
| Pro Max | $100 | 5,000 |
| Power | $200 | 10,000 |

Paid-plan add-ons cost $0.04 per credit. The $20 promotion is credit toward the first
paid upgrade for eligible social-login or Builder ID users, which requires a card.
It is not a blanket $20 inference grant at signup.
Source: [pricing](https://kiro.dev/pricing/).

Credits are fractional and consumption depends on the request. Allowances renew
with the billing cycle. Kiro has its own upgrade and proration rules; Jazyk should
model their cost before copying them.
Source: [billing](https://kiro.dev/docs/billing/).

### Model choice

Kiro offers `Auto` routing and direct selection. Its current table has relative
multipliers, including 0.05x for Qwen3 Coder Next, 1x for Auto, and 2.2x for Opus.
Longer contexts can change the rate. A multiplier is not an upstream token price.
Source: [models](https://kiro.dev/docs/models/).

Inference location differs from storage and customer location. Kiro documents
cross-region inference and US processing for individual accounts. Jazyk likewise
needs to disclose route geography rather than imply that Canadian billing means
Canadian inference. Source: [inference regions](https://kiro.dev/docs/models/#inference-endpoint-regions).

### Own-provider support needs qualification

I did not find official confirmation of arbitrary provider API keys or local
OpenAI-compatible endpoints throughout the standard Kiro IDE, CLI, and web product.
The enterprise governance page explicitly says model governance does not add custom
models. Crew's product page describes model calls billed to a Kiro plan. This does
not rule out extensions or newer integrations; it does not establish general BYOK.
Sources: [governance](https://kiro.dev/blog/enterprise-governance-mcp-and-models/),
[Crew billing](https://kiro.dev/crew/).

The owner's intended blend remains an explicit Jazyk requirement. OpenCode and Zed
provide clearer documented precedents for that part of the design.

### Lessons for Jazyk

- One allowance across local and cloud execution.
- A default route and an advanced explicit model choice.
- Actual model and charge visible after each task.
- Prepaid additions and a clear stop when exhausted.
- A later first-paid-upgrade promotion as one option, distinct from model or compute
  credits.

Kiro's requirements/design/task workflow is a product reference. It does not replace
Jazyk's persistent graph or justify restoring the discarded compiler architecture.

## Other useful comparisons

| Product | Verified behavior | Lesson |
|---|---|---|
| [OpenCode Go](https://opencode.ai/docs/go/) | $10 Go and $40 Go Plus. Optional provider subscription. Model-dependent monthly dollar limits, with 5-hour and weekly fractions. | The paid provider can be optional in a free tool. Publish every overlapping limit. |
| [Zed model access](https://zed.dev/docs/ai/llm-providers) | Hosted models, API access, gateways, and local models are separate paths. External agents own their configuration. | Separate agent selection from payment source. An external ACP agent need not obey the embedded model selector. |
| [Ollama](https://ollama.com/pricing) | Pro $20 includes $60 usage; Max $100 includes $300. Local models remain usable. | Generous list-rate allowances do not establish Jazyk's wholesale cost or safe margin. |

There is no universal 5-hour/weekly standard. Kiro publishes monthly allowances;
OpenCode documents all three. Jazyk's pacing windows are a product choice to test.

## Buying inference

Use commercial APIs under terms suitable for serving Jazyk users. Do not fund the
backend through individual coding subscriptions. A user's separately configured ACP
agent uses that agent's own account and terms.

OpenRouter is a reasonable first aggregator. Its current pricing lists a 5.5%
Standard platform fee and 8% Business fee. Its FAQ describes passing through model
prices and receiving provider usage. A cheap default route does not establish that
its provider, quantization, cache, or privacy behavior meets Jazyk's requirements.
Sources: [pricing](https://openrouter.ai/pricing), [FAQ](https://openrouter.ai/docs/faq).

Caching depends on provider behavior and prompt layout. OpenRouter documents
provider affinity and cache configuration. Pin and record the resolved route; do
not treat affinity as a guaranteed cache hit.
Source: [prompt caching](https://openrouter.ai/docs/guides/best-practices/prompt-caching).

The launch catalog is not fixed here. During [measurement](./models.md#measurement-gate),
record model id, provider, context tier, token/cache/reasoning rates, capability
results, and retrieval date. The previous table is not a launch rate card.

## Pricing conclusions

- Count billable work, including retries, cache writes, and billed reasoning.
- Distinguish the customer's credit debit from Jazyk's invoice cost. Fees, discounts,
  FX, and refunds make them different amounts.
- Measure cost per successful task. Repeated failures can make a cheap model costly.
- Publish balances and rates, not just percentage bars. Estimate ranges because
  reconciliation can open more goals.
- Never silently switch BYOK work to Jazyk billing or a depleted subscription to a
  user's API key.
- Avoid unlimited-use claims and guaranteed build counts. Competitor credit values
  are not evidence of sustainable Jazyk margins.

Canadian billing is in [platform.md](./platform.md#canadian-billing). The quantitative
proposal is in [tiers.md](./tiers.md).
