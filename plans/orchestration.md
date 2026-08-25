# Plan: work orchestration

Status: draft for iteration. Companion plan: [IR stages](./ir-stages.md), whose stages
are the first customers of the registry defined here.

## The idea

Jazyk already contains most of an orchestration system: a durable task queue derived
from disk, leases and workers, a control plane with modes and releases, one focused
agent session per work item over ACP, trace events, a journal, and a GUI. What it does
not have is a seam. The eight task kinds, their readiness rules, their packs, their
toolsets, and their gates are hardcoded across `reconcile.rs`, `queue.rs`, `turn.rs`,
and `tools.rs`. Adding a stage means touching all of them, and nothing shows the
pipeline as a pipeline.

The plan: generalize the existing harness into an explicit orchestration layer. An
agent is a declared unit (prompt, pack template, toolset, gates) bound to a stage; a
stage declares what work it derives, what it consumes, and what it hands off; the
reconciler routes between stages generically; observability covers the whole flow.
The IR is then swappable: a new stage is one registration, not a cross-cutting edit.

This is a generalization, not a framework adoption. The reasoning is in
[alternatives](#alternatives-considered).

## What an agent is here

The word "agent" is overloaded. Jazyk splits it in two, and the split already exists:

- The task kind (jazyk's side): what work exists, what one work item is, the prompt,
  the context pack, the toolset, the validation gates, the finish contract. This is
  the part the orchestration layer makes declarative.
- The executor (the agent's side): the model loop that drives a session. Already
  pluggable per [ACP profile](../docs/frontends/acp.md#agents): the embedded agent,
  OpenCode, Codex, or any ACP agent. Jazyk never hardcodes a model loop.

So "define an agent with a prompt that handles docs-to-requirements and hands off to
the next" decomposes into: a stage definition (below) plus an ACP profile choice per
stage. Both are configuration and registration, neither is scattered code.

## The stage registry

One Rust trait, one registry, every task kind an implementation. The trait surface
stays small because the shared machinery (queue, leases, gates, packs, journal,
trace) is already generic underneath:

- `kind`: the task kind name (`reconcile-doc`, `derive-usecases`, ...).
- `unit`: what one work item is (a document, an entity, a cluster, a component), so
  the queue and the GUI can render it.
- `derive_items(store, status) -> Vec<WorkItem>`: compute this stage's pending work
  from disk state. This is the dirty-set computation, per stage. Deterministic,
  idempotent, cheap: the whole queue stays derivable from disk, which is the
  property that lets any consumer resume any build. The orchestration layer must
  not lose it.
- `ready(item, queue) -> Ready | Blocked(reason)`: the declarative ordering
  (`after: [align-doc, reconcile-doc]`, level rules, gating). The reason is a
  rendered sentence, because [visibility](#visibility) shows it.
- `pack(store, item) -> Pack`: assemble the context pack through the context engine.
- `toolset() -> &[ToolId]`: the task-scoped subset of the one tool registry.
- `gates(changeset) -> Vec<Violation>`: batch checks at `done`, on top of the
  store's own per-mutation gates.
- `prompt`: the payload file path, embedded at compile time from
  `docs/compiler/turns/prompts/` as today.

The registry is a static list of stage implementations; `[stages]` in `jazyk.toml`
selects which are active. Compiled-in registration is deliberate: the dependency
policy says hand-roll domain logic, and a dynamic plugin system is infrastructure
nobody asked for. Swappability means adding a module and a config line, not loading
code at runtime.

The existing eight task kinds port onto the trait unchanged in behavior. That refactor
is phase 1 and it is the proof the trait surface is right.

## Typed handoffs

Today a handoff is implicit: a committed changeset appends touched entities to a
`pending` block in `status.yaml`, and the review stages derive work from it. That
mechanism generalizes into the routing layer:

- A commit yields typed effects: `Dirty(kind, target)` records that a stage's unit
  needs work (`Dirty(review-entity, ent:cart)`,
  `Dirty(derive-usecases, cluster:checkout)`).
- Each stage declares which effects it emits and which it consumes. The reconciler
  computes emitted effects deterministically from the changeset (as it computes
  pair-review neighbors today), records them durably beside `pending`, and the
  consuming stage's `derive_items` reads them.
- With the [IR stages](./ir-stages.md) active, effects follow the `traces` axis:
  a changed requirement emits dirty effects for exactly the use case steps,
  allocations, and transitions tracing to it. The cone, not the pyramid.
- Every effect records its cause: the generation that emitted it, the mutation
  within it, and the edge or computation that carried the dirtiness. The ripple
  DAG of any change is then derivable from the journal, which is what
  `jazyk ripple` and the build report render. See [ripple](./ripple.md).

The handoff table (which stage feeds which) becomes data the GUI and `jazyk explain`
render, instead of knowledge spread across the reconciler. The model never routes;
routing stays deterministic. A stage that wants model judgment about what to do next
expresses it as a work item whose output is graph state that other stages derive
from, never as a direct enqueue.

## Per-stage executors

One global ACP profile becomes a default plus overrides:

```toml
[acp]
agent = "embedded"

[stages.design-architecture]
agent = "claude-code"

[stages.reconcile-doc]
agent = "embedded"
```

Extraction is cheap and local models handle it; architecture judgment wants the
strongest model available. Per-stage profiles let a build mix them, which is the
practical answer to cost on large projects. The benchmark grows a per-stage
dimension: `jazyk benchmark` grades each stage's turn kinds against the profile
assigned to it, so a weak pairing is measured before it is trusted.

## Visibility

What exists: live trace events with labels and steps, full transcripts, the journal,
the GUI activity pane, `status.yaml`. What is missing: the flow view (why is this
task queued, what will it dirty, what does the pipeline look like), cost accounting,
and a standard export.

- `jazyk explain [task|target]`: render the routing. For a queued task: which effect
  or dirty computation produced it, what its readiness predicate says, what it is
  blocked on. For a target (a doc, an entity): the cone of work a change to it would
  emit, stage by stage. Everything needed is already derivable; this is a rendering.
- Pipeline view in the GUI: stages as columns in dependency order, work items as
  cards (ready, blocked-with-reason, leased-by, parked, gated awaiting release),
  effects as the arrows between columns. The control plane files and the queue
  already hold all of it; today only the flat activity stream shows.
- Cost accounting: per-turn token counts exist in trace events and the journal.
  Aggregate them per stage, per build, per document into `status.yaml` and the GUI:
  "this build: 41 turns, 310k tokens, 78% in reconcile-doc". Per-stage cost is what
  makes the per-stage executor choice an informed one.
- OpenTelemetry export, off by default: one span per build, wave, turn, and tool
  call, with GenAI semantic-convention attributes (model, token counts) on turn
  spans, OTLP endpoint from config. This buys Jaeger or Langfuse locally, or any
  collector in CI, without inventing a trace format. One caveat: the GenAI
  conventions are pre-stable (moved to their own repository in June 2026, no
  tagged release, names still moving), so the attribute set is pinned in one
  module and treated as an export detail, easy to rename. The journal stays the
  source of truth; OTel is an export, not a store.

## Alternatives considered

- Adopt an orchestration framework (LangGraph, CrewAI, AutoGen, Mastra, agent SDKs).
  Rejected as the core. They are Python or TypeScript, so the single Rust binary
  becomes a polyglot deployment. Their center of gravity is LLM-decided control
  flow (handoffs, supervisor agents), which is the opposite of the division of
  labor jazyk is built on: deterministic scheduling is the thesis, not an
  implementation detail. And the part they would replace (queue, leases, resume,
  gates) is the part jazyk already has, tuned to its semantics. The place external
  agents do fit is the executor seam, and ACP already carries that: Claude Code or
  OpenCode as the model loop for a stage is configuration today.
- Embed a durable-execution engine (Temporal, Restate) for retries, leases, and
  resume. Rejected, with an honest reading of their state: Temporal's Rust SDK
  reached public preview in May 2026 and is not production-ready; Restate is the
  credible option (a single-binary Rust server, no dependencies, with a real Rust
  SDK). Either is still a second process journaling execution history. Jazyk's
  durability model is stronger for its shape: the queue is recomputed from disk
  state, not replayed from an event log, so there is no history to corrupt and any
  process resumes by rederiving. Replay journaling would be a downgrade dressed as
  infrastructure. Their concepts (heartbeats, lease expiry, idempotent steps) are
  already present.
- Rust agent frameworks, with Rig examined closely since it is the mature one
  (active in 2026, 20+ providers, MCP client support, OTel GenAI instrumentation,
  tracing hooks, mock models and cassette tests). Rig's `Agent` is a model-loop
  concept: preamble, tools, provider. Jazyk's agent definition lives a layer above
  (prompt payload, toolset, gates, pack, finish contract) and a layer below is
  already pluggable (any ACP agent). So Rig competes only with the embedded
  agent's endpoint client and loop, not with orchestration, and brings no gates,
  staging, or graph semantics. At that seam the trade is real but unfavorable
  today: Rig would outsource provider quirks and give model-call spans and
  mockable tests for free, but it is async (tokio) where the binary is
  deliberately sync (ureq), it is 0.x with breaking changes under an embedded
  agent that must stay a faithful ACP test double, and it has no equivalent of
  the `text` codec, the probe-and-downgrade stickiness, or the repair nudges,
  which exist for weak local models, exactly the constituency the embedded agent
  serves (strong models arrive as external ACP agents). The natural first
  adoption point is different: embeddings. Rig's embeddings and vector-store
  integrations are squarely infrastructure-from-crates, and one embedding index
  over the docs and the graph serves three deterministic consumers: the `search`
  tool's backend (already on the TODO: same interface, no schema change),
  lookalike-candidate computation for review packs (cross-document near-duplicate
  entities are a known weak spot the lexical machinery misses), and the
  reconciler's pre-partitioning (clustering requirements by similarity for the
  [IR plan's](./ir-stages.md) use-case derivation, where shingles are the
  fallback). The boundary: embeddings are a similarity signal inside
  deterministic machinery, never the context path. RAG-style assembly (retrieve
  raw doc chunks by similarity into prompts) would bypass the graph and its
  provenance, which is the thing jazyk exists to replace. Adopt at the
  embeddings seam first; revisit the client seam if the provider zoo grows past
  OpenAI-compatible.
- Do nothing (keep hardcoded kinds). Rejected by the companion plan's existence:
  five-plus new stages against four files each is exactly how the reconciler
  becomes unmaintainable.

## Migration

Behavior-preserving first, then the new surface:

1. Extract the `Stage` trait and port the existing eight task kinds onto it. Pure
   refactor: same queue, same waves, same gates, same docs. `cargo test` and the
   benchmark hold the line.
2. Typed effects: replace the `pending` block bookkeeping with recorded effects,
   emitted and consumed per stage declaration. `status.yaml` format changes;
   document it. `jazyk explain` lands here, since effects make it a rendering.
3. `[stages]` activation config plus per-stage ACP profiles, with the benchmark's
   per-stage dimension.
4. Observability: per-stage cost aggregation, the GUI pipeline view, the OTel
   export.
5. The [IR stages](./ir-stages.md) then land as registrations, one phase at a time,
   proving the seam on real new stages.

Docs first at every step: the registry, effects, and explain surfaces get pages
under `docs/compiler/`, and `control-plane.md`, `reconciler.md`, and `turns.md`
absorb the changes.

## Open questions

- Whether `derive_items` stays one method or splits (cheap dirtiness probe vs full
  item computation) for large projects; the probe is what `await_changes` and the
  watcher want.
- Effect storage: inside `status.yaml` beside `pending`, or a sibling file per
  stage. Sharding pressure says sibling files; simplicity says one file until it
  hurts.
- Whether wave granularity (all documents in a level, then reviews) survives as a
  scheduling policy per stage, or becomes uniform effect-driven readiness. Waves
  are easier to reason about; effects are finer-grained. Likely: keep waves as the
  default policy expressed through `ready`, allow stages to opt into finer grain.
- How much of the GUI pipeline view is worth building before the IR stages exist to
  populate it. Minimum: the explain command and per-stage cost, which pay off at
  eight stages already.

## References

- [Temporal Rust SDK public preview](https://temporal.io/changelog/rust-sdk-public-preview)
  and [repository](https://github.com/temporalio/sdk-rust).
- [Restate](https://restate.dev/), its
  [engine design](https://www.restate.dev/blog/building-a-modern-durable-execution-engine-from-first-principles),
  and the [Rust SDK](https://docs.rs/restate-sdk/latest/restate_sdk/).
- [OTel GenAI semantic conventions status, July 2026](https://john-hodge.com/blog/opentelemetry-genai-semantic-conventions/).
- [Agent Client Protocol](https://agentclientprotocol.com), the executor seam jazyk
  already has.
