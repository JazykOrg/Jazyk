# Parallel work without leases

Status: plan, 2026-11-17. Owner direction: no leases, no claims, no blocking. Any
number of agents (the internal loop, the built-in agent and its sub-agents, chat
conversations, separate Claude Code sessions and their own subagents, cloud
conversations) work on the board at once. Commits are the only synchronization:
the first commit wins, a later one gets a conflict and repairs its work.

## Principles

- Work is opened, never claimed. Opening a batch (or a ledger task) gives the
  agent a scoped package: the goals, the prompt, the loaded context, and write
  tools limited to those goals. It takes nothing from anyone else.
- Two agents may open the same goals. Both work. The first commit resolves them;
  the second gets a conflict report and decides: drop, merge, or fix.
- The store `.lock` stays as the commit mutex, held only for the duration of a
  commit, with a bounded wait. In cloud (in-memory workspaces) it is an in-memory
  mutex.
- A conflict is never silent and never a crash. It is a tool result the agent
  reads, like a gate refusal, within the existing attempt budget.

## What exists and what changes

| Mechanism | Today | After |
|---|---|---|
| Batch lease `local/leases/<batch>.yaml` | `begin_goals` claims, others get `claimed` | Removed. `begin_goals` opens; nothing refuses |
| `build` lease | One internal build owns the board; `begin_goals` gets `build-running`; a build refuses while task leases live | Removed. Builds and agents coexist |
| Ledger task claims (`claim`, `release_own_ledger_claim`) | One worker per generation/binding/verification target | Removed. Ledger rows commit first-wins |
| Task-runtime run leases | Fence task runs; block builds | Kept for run cancellation and ownership only; never block a build |
| `.leases.lock` | Serializes lease takeover | Removed with the leases |
| `project.lock` | Held from `begin_goals` to `done`, and for a whole build; settings writes get `project-busy` | Held only around a configuration write. Open work records the configuration hash; commit refuses `config-moved` when it changed |
| Store `.lock` | Waits forever (warns at 10 s) | Waits at most `STORE_LOCK_WAIT` (30 s), then the commit fails `store-busy`, retryable. A commit holding it past `STORE_LOCK_HOLD` (60 s) is reported. In-memory workspaces use a mutex |
| `store-moved` | Any commit since the snapshot refuses the whole changeset | Replaced by rebase-at-commit (below) |
| Board `claimedBy` | Leased goals hidden from other consumers | Replaced by `openBy`: informational, lists who has the goal open. Never filters |
| One open batch per serving | `batch-open` refusal | Several open batches per serving, each with an id; write tools take `batch` when more than one is open |
| `local/run.yaml` | One build's progress per set | One file per build run (`local/runs/<run>.yaml`); status shows all |

## Rebase at commit

Under `.lock`, a changeset that loaded generation `g` meets the store at `g'`.
When `g == g'` it commits as today. Otherwise:

1. Read the journal entries `g+1..g'`: the nodes they touched, the goals they
   resolved, the redirects they left.
2. Staged creates reconcile by natural key (exists today): a matching node turns the
   create into an update, mentions unioned.
3. A staged update, delete, or merge of a node touched since `g` is a node conflict,
   unless the change is identical (idempotent) or touches disjoint fields of an
   entity (attributes merge by name).
4. A staged mention of a node deleted or merged since `g` follows the redirect.
   A deleted node with no redirect is a conflict.
5. A goal this changeset resolves that is no longer open, or whose `change`
   differs, is a goal conflict (`goal-resolved` with the winner's generation and
   justification, or `goal-changed`).
6. No conflicts: apply the changeset on `g'`, run the normal commit (derived data,
   sweep, limits, change records, journal with `rebased_from: g`).
7. Conflicts: nothing commits. The reply lists each conflict with the winner's
   version and generation. The session stays open with its staged work, reloads
   the conflicting nodes, and either drops the staged mutation (`unstage`), redoes
   it on the new version, or resolves the goal differently. `done` again.

Natural keys, redirects, and gates already make most parallel work rebase cleanly.
Real conflicts are two agents editing the same node, which is exactly when a
review is wanted.

Ledger commits (`record_binding`, `record_generation`, `record_verdict`) follow
the same rule per row: a row recorded since the task opened is a conflict.
Deliverable file writes keep `expectedHash`; a stale hash is the conflict.

## Opening work

- `goals` stays read-only. Rows gain `openBy` (sessions with the goal open), so an
  agent can choose to avoid duplicate work, but nothing stops it.
- `begin_goals({batch?, goals?})` opens a package: prompt, loaded set, scope. It no
  longer refuses `claimed`, `build-running`, `batch-open`, or `project-busy`.
  `awaiting-release` stays: releases are approvals, not locks.
- Each open batch registers in memory (and in `local/workers/` for visibility, as
  today) with its goals, so `openBy` and the editor's agent list can show it. A
  dead worker's entry ages out by heartbeat; it never blocks.
- A serving holds any number of open batches. With one open, tools work as today.
  With several, write tools, `mark_goal_done`, `done`, and `abandon_goals` take
  `batch`. This is what lets Claude Code subagents, which share their parent's MCP
  connection, work in parallel.

## Builds

- The internal loop opens batches like any agent. It keeps up to N sessions
  running: N is a property of the executor profile (`max_sessions`, default 1 for
  local endpoints, e.g. 4 for hosted APIs), so a local Ollama build stays
  sequential by resource choice, not by lock.
- Scheduling prefers ready batches whose cones do not overlap open work, as an
  optimization against wasted spend. Overlap is allowed when nothing else is ready.
- Readiness keeps its meaning with a barrier: the loop does not open a higher tier
  while a lower-tier batch it opened is in flight. A GC goal is ready when no compile
  goal is open or in flight in its cone.
- Two builds may run at once (a terminal `jazyk compile` and the GUI). They share
  the board and race on commits. Each keeps its own run file. The deterministic
  tail (checks, render, docsgen, verdict) runs in whichever build empties the
  board; verdict writes are commits and rebase like any other.
- The GUI job queue may still run its own jobs one at a time; that is a queue
  policy, not a lock.

## Sub-agents

- Claude Code, Codex, and other external agents use their own subagents. Each
  subagent calls the same Jazyk tools; with several open batches per serving they
  work in parallel. Nothing in Jazyk spawns them.
- The built-in agent and cloud conversations get `spawn_agent`, `wait_agents`,
  `agent_status`, `message_agent`, `cancel_agent`, run in process through the GDK's
  `SubagentOperation`: a child is a harness run with its own conversation and its
  own open batches; its final reply is delivered to the parent as a message;
  stopping the parent stops the children; a concurrency cap bounds them. Children
  are visible as conversations with a parent (the agent tree, `GET /api/agent-tree`,
  the `agents` event).
- The browser engine runs children as interleaved async tasks on its one thread:
  sessions mostly await the model, and commits serialize naturally. Hosted runs
  them as tasks.
- The `subagents` engine branch's HTTP callback and per-conversation capability
  layer is superseded and dropped. Its agent tree, agent list, and the editor's
  `feat/subagents` branch carry over.

## Docs to change (docs first)

- `compiler/control-plane.md`: rewrite workers and leases (no leases, open work
  registry), replace sequential builds with concurrent work, dispatch.
- `compiler/graph.md`: concurrency (lock wait and hold bounds, in-memory mutex),
  changesets (rebase at commit, conflicts), journal (`rebased_from`).
- `compiler/reconciler.md`: scheduling with N sessions, tier barrier, GC readiness
  with in-flight work, parked and failed under races.
- `compiler/compilation.md`: a build, convergence with concurrent builds.
- `compiler/sessions.md`: commit conflicts, `unstage`, attempts.
- `compiler/jazyk-folder.md`: remove `leases/` and `.leases.lock`, add
  `local/runs/`.
- `compiler/tools.md`, `frontends/mcp.md`: `begin_goals` semantics, `openBy`,
  several open batches, conflict replies, removed refusals.
- `compiler/document-sets.md`, `compiler/project-settings.md`: `project.lock` scope,
  `config-moved`.
- `frontends/acp.md`, `frontends/gui.md`, `frontends/subagents.md`, `frontends/wasm.md`,
  `compiler/agent-runtime.md`, `compiler/hosted-runtime.md`: sub-agents in process.
- Prompt payloads: the conflict reply, `unstage`, `batch` argument guidance, the
  sub-agent task framing. Remove the `claimed`, `build-running`, `batch-open`,
  `project-busy` guidance.
- Editor (`jazyk-cloud/docs/editor/`): `openBy` on board cards, several build
  sessions at once in Running agents.

## Implementation order

Each step lands docs first, then code, tests, merge.

1. Store `.lock` bounds and the in-memory mutex. Small, independent.
2. Rebase at commit with conflict reports, `unstage`, journal `rebased_from`.
   This is the foundation; with leases still present it changes nothing visible.
   Tests: two stores from one generation committing disjoint, same-node, and
   same-goal changesets; natural-key creates; redirects.
3. Remove leases, the build lease, ledger claims, `.leases.lock`; `begin_goals`
   opens; `openBy`; `project.lock` narrowed with `config-moved`. Builds and MCP
   workers now overlap.
4. Several open batches per serving (`batch` argument).
5. Internal loop with N sessions per executor, tier barrier, per-run files.
6. In-process sub-agents for the built-in agent (native) via `SubagentOperation`.
7. The same in hosted and browser conversations (async interleaving).
8. Editor: `openBy`, several build sessions, and the `feat/subagents` branch
   rebased on the final contract.

## Tests that prove it

- Two MCP servings on one set open the same batch; both finish; the first commits,
  the second gets `goal-resolved` and abandons cleanly.
- Two servings open disjoint batches and both commit, in either order.
- A build and an MCP agent run at once on `bootstrap/example/f1` and converge.
- A replay with two sessions touching one entity produces a node conflict, the
  loser repairs, and the graph matches a sequential run up to ids.
- A commit waiting past `STORE_LOCK_WAIT` fails `store-busy` and retries.
- Browser engine: two sub-agents interleave and both commit.

## Risks

- Duplicate spend when agents choose the same work. `openBy` and cone-aware
  scheduling reduce it; the owner accepts the rest.
- Order-dependent graphs: who commits first decides names and merges. Natural keys
  and `dedupe-candidates` keep it correct, not identical across runs.
- Readiness assumptions: a tier barrier keeps alignment before ingest; tests in
  `reconcile/e2e.rs` must cover concurrent tiers.
- Budgets: the build budget counts in-flight sessions; spend reporting sums
  concurrent runs.
