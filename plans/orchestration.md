# Plan: work orchestration

Status: proposal for iteration. The proposal set:
[ir-stages](./ir-stages.md) (doctrine and the stage ladder),
[ir-graph](./ir-graph.md) (the graph, diagrams, profiles),
[agent](./agent.md) (the agent and the goal system), [ripple](./ripple.md)
(convergence and observing it), this file.

This file is the implementation seam: how goal kinds register, how executors
attach, what the visibility surfaces are, and why no external orchestration
framework sits underneath.

## The registry

One Rust trait, one registry, every [goal kind](./agent.md#the-goal-catalog) an
implementation (a stage is a family of goal kinds). The trait surface stays
small because the shared machinery (store, gates, context engine, journal,
trace, control plane) is generic underneath:

- `kind`: the goal kind name.
- `unit`: what one target is (a document, an entity, a cluster, a view), so the
  board and the GUI can render it.
- `derive_goals(store, status) -> Vec<Goal>`: this kind's open goals from disk
  state. Deterministic, idempotent, cheap; the board stays derivable from disk,
  which is what lets any consumer resume any build.
- `ready(goal, board) -> Ready | Blocked(reason)`: the tier ordering and
  gating, with the reason rendered as a sentence because the visibility
  surfaces show it.
- `pack(store, batch) -> Pack`: a batch's initially loaded graph, through the
  context engine.
- `toolset() -> &[ToolId]`: the kind's slice of the one tool registry.
- `gates(changeset) -> Vec<Violation>`: batch checks at `done`, on top of the
  store's per-mutation gates.
- `prompt`: the contract payload file, embedded at compile time.

Registration is compiled in, a static list: the dependency policy hand-rolls
domain logic, and a dynamic plugin system is infrastructure nobody needs.
Adding a stage is goal kinds plus gates plus skills in the registry, a module
and a config line.

## Write tools

Joining the existing catalog, all staging into changesets behind the same
gates: `upsert_usecase`, `update_usecase`, `delete_usecase`, `upsert_instance`,
`update_instance`, `delete_instance`, `upsert_component`, `update_component`,
`delete_component`, `upsert_interface`, `update_interface`, `delete_interface`,
`upsert_statemachine`, `delete_statemachine`, `upsert_interaction`,
`delete_interaction`, `report_adr` (upsert by natural key; supersede, never
rewrite), `set_trace_coverage({stage, target, state, note?})` (the per-stage
coverage mark; `not-applicable` requires the note), and the goal tools
(`mark_goal_done`, `mark_goal_failed`). Chat gains `answer_adr` on the
prompt-and-answer machinery. Relationships keep having no write tool, and no
tool enqueues work: the model writes graph state, the harness derives the
goals.

## Executors

One global ACP profile with per-family overrides:

```toml
[acp]
agent = "embedded"

[stages.composition]
agent = "claude-code"
```

Per-family cost accounting (below) is what makes the choice informed.
Benchmarking per goal kind follows the first implementation, once real use
shows what needs grading.

## Visibility

- `jazyk explain [goal|target]`: for a goal, which change produced it, what its
  readiness says, what blocks it; for a target, the cone of goals a change to
  it would open, stage by stage. A rendering over derivable state.
- The GUI board, the `jazyk preview` pane, and the follow sessions
  ([observing a run](./ripple.md#observing-a-run)).
- Cost accounting: per-session token counts aggregate per goal kind, per
  family, per build, and per document into `status.yaml` and the GUI ("this
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
  and graph serving the `search` tool, lookalike candidates, and the
  reconciler's clustering. The boundary stands regardless: embeddings are a
  similarity signal inside deterministic machinery, never the context path;
  retrieval-over-raw-prose is what jazyk exists to replace.
