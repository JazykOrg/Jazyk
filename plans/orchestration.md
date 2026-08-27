# Plan: work orchestration

Status: proposal for iteration. Read with [ir-stages](./ir-stages.md) (doctrine,
compile and cleanup), [ir-graph](./ir-graph.md) (the graph and every diagram),
[agent](./agent.md) (goals and sessions), [ripple](./ripple.md) (propagation
and observing).

This file is the implementation seam: how goal kinds register, how executors
attach, what the visibility surfaces are, and why no external orchestration
framework sits underneath.

## The registry

One Rust trait, one registry, every [goal kind](./agent.md#the-goal-catalog) an
implementation. The trait surface stays small because the shared machinery
(store, gates, context engine, journal, trace, control plane) is generic
underneath:

- `kind`: the goal kind name, and whether it is compile or cleanup work.
- `unit`: what one target is (a document, an entity, a cluster, a view), so the
  board and the GUI can render it.
- `derive_goals(store, status) -> Vec<Goal>`: this kind's open goals from disk
  state. Deterministic, idempotent, cheap; the board stays derivable from disk,
  which is what lets any consumer resume any build.
- `ready(goal, board) -> Ready | Blocked(reason)`: the compile-tier ordering
  and gating, with the reason rendered as a sentence because the visibility
  surfaces show it.
- `pack(store, batch) -> Pack`: a batch's initially loaded graph, through the
  context engine.
- `toolset() -> &[ToolId]`: the kind's slice of the one tool registry.
- `gates(changeset) -> Vec<Violation>`: batch checks at `done`, on top of the
  store's per-mutation gates.
- `prompt`: the contract payload file, embedded at compile time.

Registration is compiled in, a static list: the dependency policy hand-rolls
domain logic, and a dynamic plugin system is infrastructure nobody needs.
Adding a capability is goal kinds plus gates plus skills in the registry, one
module.

## Write tools

The unified model keeps the tool catalog small. The existing entity and
requirement tools grow fields instead of siblings:

- `upsert_entity` and `update_entity` gain `stereotype`, `parent`, and
  `attributes` (with optional `value` per attribute).
- `upsert_requirement` and `update_requirement` gain the plural `edges` (each
  `{a, b, type?, cardinality?}`, several per statement), the optional
  `transition` facet, and `facets` (behavior, constraint, failure mode,
  quality, each with reasoning).
- `report_diagnostic` gains the `decision` rule, whose `prompt` carries the
  question and options; `answer_diagnostic` records the ruling, as today.
- New: `upsert_view`, `update_view`, `delete_view` (kind, title, ordered
  members, query, collapse, exclusions), and the goal tools
  (`mark_goal_done`, `mark_goal_failed`).

Relationships, state machines, and default views have no write tools: they are
recomputed on commit. No tool enqueues work: the model writes graph state, the
harness derives the goals.

## Executors

One global ACP profile with overrides per goal kind or per goal class:

```toml
[acp]
agent = "embedded"

[executors]
cleanup = "claude-code"          # the holistic restructuring judgment
reconcile-section = "embedded"   # extraction stays cheap
```

Per-kind cost accounting (below) is what makes the choice informed.
Benchmarking per goal kind follows the first implementation, once real use
shows what needs grading.

## Visibility

- `jazyk explain [goal|target]`: for a goal, which change produced it, what its
  readiness says, what blocks it; for a target, the cone of goals a change to
  it would open. A rendering over derivable state.
- The GUI board, the `jazyk preview` pane, and the follow sessions
  ([observing a run](./ripple.md#observing-a-run)).
- Cost accounting: per-session token counts aggregate per goal kind, per build
  stage, per build, and per document into `status.yaml` and the GUI ("this
  build: 41 sessions, 310k tokens, 78% in reconcile-section").
- OpenTelemetry export, off by default: one span per build, session, and tool
  call, GenAI semantic-convention attributes on session spans, OTLP endpoint
  from config. The GenAI conventions are pre-stable (their repository split out
  in June 2026 with no tagged release), so the attribute set is pinned in one
  module and treated as an export detail. The journal stays the source of
  truth; OTel is an export, not a store.

## Alternatives considered

- Orchestration frameworks (LangGraph, CrewAI, AutoGen, Mastra, the agent
  SDKs). Rejected as the core: they are Python or TypeScript against a single
  Rust binary, their center of gravity is LLM-decided control flow while
  deterministic scheduling is jazyk's thesis, and the part they would replace
  (queue, leases, resume, gates) is the part jazyk already has. External agents
  fit at the executor seam instead, and ACP already carries that.
- Durable-execution engines (Temporal, Restate) for retries, leases, resume.
  Rejected: Temporal's Rust SDK is a public preview, Restate is credible (a
  single-binary Rust server with a real Rust SDK) but still a second process
  journaling execution history. Jazyk's durability is stronger for its shape:
  the board recomputes from disk rather than replaying an event log, so there
  is no history to corrupt. Their concepts (heartbeats, lease expiry,
  idempotent steps) are present already.
- Rust agent frameworks, Rig foremost (active, 20+ providers, MCP support,
  OTel instrumentation, mock-model testing). Rig's agent is a model-loop
  concept: preamble, tools, provider. Jazyk's task definition lives above it
  (contracts, gates, packs) and the loop below it is already pluggable via
  ACP, so Rig competes only with the embedded agent's endpoint client, where
  it is async (tokio) against a deliberately sync binary, 0.x with breaking
  changes, and without the text codec and downgrade stickiness that exist for
  weak local models. The right entry point is embeddings: one index over docs
  and graph serving the `search` tool, lookalike candidates, and the flow
  clustering. The boundary stands regardless: embeddings are a similarity
  signal inside deterministic machinery, never the context path;
  retrieval-over-raw-prose is what jazyk exists to replace.
