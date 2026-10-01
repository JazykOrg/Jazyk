# Models, access methods, and task policies

The subscription supports a tested catalog, potentially including frontier models.
All supported models draw from the same allowance at published rates. Model family
is not a billing tier. A more expensive model consumes the allowance faster.

## Independent choices

| Choice | Options | Owns the bill |
|---|---|---|
| Execution | Local machine or cloud sandbox | Local hardware owner or Jazyk compute wallet |
| Embedded model access | Local endpoint, user API key, Jazyk subscription | User or Jazyk model wallet |
| Agent | Embedded worker or external ACP profile | The configured agent's provider, unless explicitly integrated with the Jazyk gateway |
| Project source | Local folder, cloud-only, GitHub-backed | Separate from model access |

Free local use needs no Jazyk account. Login adds a managed access profile. It must
not overwrite explicit CLI flags, environment settings, a project's provider, or
the global LocalRouter configuration.

Choose the paying profile explicitly. Show the effective endpoint, model, and payment
source before the first paid run. Bind stored credentials to their issuer/origin;
never combine a Jazyk token with an arbitrary endpoint inherited from project config.

## User controls

The first settings screen has a default policy and optional task overrides:

| Purpose | Existing work covered | Policy question |
|---|---|---|
| Compilation | Anchor placement, section reconciliation, retracing | Which model extracts facts accurately and follows tools? |
| Judgment and structure | Pair/entity review, conformance, GC | Which model handles ambiguity and abstraction without losing obligations? |
| Coding | Binding and generation | Which model/agent produces working deliverables? |
| Verification | Model work for `verify`, audits | Which model independently checks evidence? Deterministic tests need no model selection. |
| Interaction | Chat and answer/follow assistance | Which model gives responsive help? Human `ratify` and `answer` goals remain human gates. |

- Default: `Auto`, using a versioned, benchmarked routing policy.
- Advanced: override a purpose or goal kind with an exact supported model/profile.
- Controls: maximum spend per run, maximum request cost, permitted frontier routes,
  and reasoning effort where the selected model supports it.
- Show the current route, cost range, and payment source. A switch applies to the
  next session, not halfway through a tool conversation.
- Presets such as Economy and Quality may follow measurements. Do not publish a
  quality promise before the relevant goal kinds pass.

For example, a user can compile through their local endpoint and generate code on
the subscription, or compile on the subscription and delegate coding to their own
ACP agent. There is no automatic transfer of costs between those access methods.

## Relationship to executors

[Executors](../../docs/compiler/project-settings.md#executors) currently select
agent profiles per goal kind/class. They do not select an embedded model directly.
Extend the portable specification before introducing settings:

- Resolve the agent with the existing executor rules first.
- For embedded work, resolve the model profile from an explicit invocation override,
  then applicable environment overrides, then per-kind/per-purpose mappings, then
  the existing project/global default. Preserve existing `[llm]` field precedence
  for legacy configurations. Specify the exact new syntax and collision rules in
  phase 0, with examples showing environment overrides and mixed BYOK/subscription.
- A batch must be compatible in agent, model profile, credential source, and policy.
  Split incompatible batches while retaining readiness tiers and locality ordering.
- External agents own their model configuration unless their adapter explicitly
  supports selecting it. Display that fact. `report_spend` is informational, never
  authoritative for charging an account.
- Record policy version, requested alias, resolved model/provider, reasoning effort,
  and usage on the run. The model does not route or prioritize goals.

The gateway resolves managed aliases within the user's allowed model/cost bounds.
A client-supplied purpose is a routing hint, not proof of a cheaper entitlement.
Task policy never changes the price of a token from the same route.

## Catalog and fallback

Candidate families can include Qwen, DeepSeek, GLM, MiniMax, Kimi, and commercial
frontier providers. Neither this list nor a public coding leaderboard establishes
compiler capability. Keep the initial catalog small, perhaps one economical route
and one stronger route with separate coding defaults if measurements justify them.

Every route records:

- Provider and exact model/version, context limit, codec, tool support, and supported
  generation parameters.
- Input, cache-read, cache-write, output, reasoning, long-context, and other billable
  rates. Record which categories overlap to prevent double-counting.
- Rate-card version and effective date, upstream fees, and supported usage receipts.
- Processing geography, retention/training policy, availability, and measured latency.
- Benchmark results by purpose, including failures and retries.

Pin a route for the session to preserve behavior and cache opportunity. Fallback
requires compatible capabilities, privacy conditions, and the user's maximum rate.
An unavailable route with no eligible fallback stops with a clear reason. It never
silently escalates to a much more expensive model or bypasses a provider restriction.

## Measurement gate

Use `jazyk benchmark` before trusting a candidate, as `AGENTS.md` requires. Then
measure end-to-end quality and cost; the existing benchmark alone does not grade
all goal kinds.

1. Start with `bootstrap/example/f1`, then `f2` and its planted traps.
2. Compile and generate `example-sort`; build and run its tests.
3. Test an incremental edit, contradiction, and rename without recreating identities.
4. Use `example-erp` for multi-document work and generated-service/database behavior.
5. Include a non-software fixture to avoid selecting purely for coding performance.
6. Repeat cold and warm runs, including no-op rebuilds, a provider failure, a quota
   refusal, and a retry. Separate compilation, coding, verification, and chat costs.

Record at least:

| Field | Why |
|---|---|
| Fixture, compiler/payload version, model/provider, codec, policy | Reproducibility |
| Actual input, cached input, cache writes, output, billed reasoning | Correct accounting |
| Calls, tool/gate failures, retries, unresolved goals, wall time | Harness compatibility and latency |
| Expected diagnostics, coverage, graph stability, build/test result | Quality, beyond apparent convergence |
| Supplier cost and customer debit, including unsuccessful attempts | Sustainable pricing |
| Cold/warm distributions and cost per successful task | Realistic estimates |

Use the configured LocalRouter endpoint and record its resolved provider where
available. Unknown upstream cost remains unknown. Do not infer price from an alias
or count a local `report_spend` value as a provider receipt.

Cut repeated prompt overhead without changing the agent contract's meaning. Compare
before/after cost and quality. Record successful and failed sessions in run telemetry;
a journal-only approach misses paid attempts that never commit a generation.

Exit: a reproducible table justifies each offered purpose/model combination and the
[allowance](./tiers.md). Record commercial serving eligibility before selling a route.
