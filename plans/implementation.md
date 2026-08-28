# Plan: implementation

Status: the execution plan for the proposal in this directory. Written for a
fresh session with no prior context: read this file first, follow its reading
list, then work the checklist. Everything here lands as one coordinated change
(the ordering below is dependency order inside one landing, not release
phases).

## Start here

- Read the repo root `CLAUDE.md` first. It carries the hard rules: docs first,
  the docs writing style (short declarative sentences, no em dashes, backticks
  for identifiers), commit identity and no co-author trailers, the GUI rebuild
  rule, LocalRouter as the LLM endpoint.
- Read the proposal, in order: [ir-stages](./ir-stages.md) (doctrine, compile
  and GC, what the content activates), [ir-graph](./ir-graph.md) (the graph,
  every diagram with its PlantUML, the limits registry),
  [agent](./agent.md) (goals, sessions, loading the graph, skills, the
  catalog), [ripple](./ripple.md) (edit paths, ambiguity, causality,
  observing a run), [orchestration](./orchestration.md) (the registry trait,
  write tools, executors, alternatives considered).
- Skim the current design the proposal builds on, because most machinery
  carries over: `docs/compiler/compiler.md`, `model.md` and `model/`,
  `graph.md`, `reconciler.md`, `turns.md`, `context.md`, `tools.md`,
  `compilation.md`, `control-plane.md`, `docs/frontends/acp.md`,
  `docs/consumers/gen.md` and `bind.md`, plus every page named in the docs
  merge table before rewriting it (`parsing.md` and `alignment.md` carry over
  unchanged).
- The implementation is `bootstrap/` (Rust, binary `jazyk`). Build:
  `cd bootstrap && cargo build --release`. Test: `cargo test`. After ANY GUI
  or binary change, finish with `cd bootstrap/gui && npm run build` then
  `cargo build --release` (the built frontend embeds into the binary).
- Fixtures: `bootstrap/example/f1` (small), `f2` (planted traps, see its
  `EXPECTED.md`). Larger corpora: `example-sort`, `example-erp`,
  `example-slides`. The dogfood is `docs/` itself, run from `docs/`.
- Local models are weak and slow, and the configured default
  (`gemma4:e4b-mlx` class) is expected to fail judgment-heavy goals. Before
  validating, set `JAZYK_MODEL` to a capable model routed by LocalRouter;
  remote runs cost money and need per-run approval from the owner.
  Benchmarking per goal kind is deferred until after this landing.

## What carries over unchanged

Do not rebuild what stands. These survive as implemented:

- The store's commit machinery: staged changesets, atomic commit behind
  `.lock`, natural-key upserts, id minting, merges with redirects, the
  journal with generation numbers, the deterministic GC sweep with
  tombstones.
- Quote provenance and its gates (whitespace-insensitive location,
  `wrong-document`, coverage honesty, entity-name doctrine).
- Parsing, section trees, alignment (moves, splits, merges, anchor
  proposals).
- The control plane: modes, releases, workers, leases, `control.yaml`.
- The ACP bridge: worker sessions, the embedded agent with its codecs and
  fallbacks, chat and follow sessions, the session store, MCP tool injection.
- The ledger: bind, generate, verify, derived statuses, traceability markers,
  the deliverable baseline.
- The LSP skeleton, the GUI shell, the terminal viewer (`viewer.rs`,
  unchanged), docsgen's page-per-entity mechanism, and the chat dual-write
  tools (`revise_requirement`, `add_requirement`, `retract_requirement`),
  which the edit paths generalize.

## Decisions to lock before coding

Each has a recommendation; decide (or ask the owner) before the store work.

- Entity natural key under containment. `name` + `scope` wrongly merges two
  same-named children of different parents. Recommend: `parent` joins the key
  when the caller supplies it; an upsert without `parent` matches a unique
  name match or errors naming candidates (consistent with the existing
  lenient-resolution gate). A wrong merge is the failure to avoid.
- Statement rename. `ears` becomes free-form `statement`; the EARS shape gate
  is deleted. No migration code: introduce a store version (none exists
  today), recorded in `status.yaml`. An out directory without one counts as a
  mismatch, and a mismatch archives the whole out directory to
  `jazyk-out.bak` and reconciles from the empty graph (the first build is not
  special by design).
- Renderer spike first. Before depending on it, prove `plantuml-little`
  builds and renders the proposal's fixture diagrams offline (it needs the
  `graphviz-anywhere` prebuilt native library per platform; the crate is
  young and its repository link is dead, so pin the version and vendor the
  source if needed). If the spike fails, invoke the official PlantUML GraalVM
  native binary behind the same render seam instead, and note it in the docs.
- Flow view default order: document order of the member requirements; an
  explicit member list overrides. Reordering judgment stays with
  `curate-view`.
- Facet storage: `facets: [{facet, reasoning}]` with `facet` one of
  `behavior`, `constraint`, `failure-mode`, `quality` (quality carries its
  `measure` when stated).
- Goal ids: `g:<kind>:<target>`, derivation-stable, with the `change` payload
  as identity across re-derivations.

## The docs merge

`docs/` is both the spec and the dogfood input. Rewrite it to the proposal
first, then make `bootstrap` match. The dogfood cannot compile between the
docs rewrite and the code landing; that is expected, both land together. Keep
every page extractable (the docs are compiler input) and in the house style.

| current page | action |
|---|---|
| `main.md` | Update the How-it-works overview: goals, views, compile and GC. |
| `compiler/compiler.md` | Rewrite: components are the store, the goal board, the scheduler and the serving (agent.md's terms), the renderer. |
| `compiler/model.md`, `model/entity.md`, `model/requirement.md`, `model/relationship.md` | Rewrite to the proposal's kinds and fields (stereotype, parent, attributes; statement, plural directional edges, transition, facets; per-direction contributions). |
| `model/section.md`, `model/diagnostic.md` | Light: sections unchanged; diagnostics gain the `decision` rule. |
| new `model/view.md`, `model/state-machine.md` | The view kind; the derived machine and its checks. |
| `compiler/concepts/ears.md` | Becomes `concepts/statements.md`: free-form doctrine. Keep the subject doctrine, declarative-prose-states-obligations, enumerations, granularity; drop the shape check. |
| `compiler/concepts/` others | `identity.md` gains the natural-key decision; `scopes.md`, `judgment.md` light. |
| `compiler/reconciler.md` | Rewrite: goal derivation, change records, readiness tiers, cones, locality batching, GC gating, bubbling, escalation. |
| `compiler/compilation.md` | Rewrite: compile and GC bursts, convergence verdict with counts, the four edit paths from [ripple](./ripple.md#edit-paths) (prose; dual write with human-accepted rewrites; decree with ratification proposal; deliverable), and the checks list (justification closure, flow placement, conformance, machine checks, provider check, cross-class flip detection, coverage as today). |
| `compiler/turns.md` + `turns/` | Becomes `sessions.md` + `goals/` (one page per goal kind with its contract, gate, hints) + payload dirs `docs/compiler/goals/prompts/` and `docs/compiler/skills/` (embedded via `include_str!`, excluded from the docs glob like today's prompts). |
| `compiler/context.md` | Rewrite: the loaded set, `load`/`unload`/`graph_status`/`expand`, stubs with counts, budget high-water, the `related` axis. |
| `compiler/graph.md` | Update: `views.yaml` shard, derived `state-machines.yaml`, per-direction relationship contributions, change records in `status.yaml`, journal entries with `opened_goals`/`resolved_goals` and justifications plus the new `edit` entry kind (a human save that dirties sections journals as its own generation, the root of every ripple), per-node limit bumps, the garbage-collection section widened to name both halves (the deterministic sweep it already describes and the GC goal class), the built-in limits registry. |
| `compiler/tools.md` + `tools.schema.yaml` | Update the registry: view tools, goal tools, extended entity and requirement tools, `decision` diagnostics. |
| `compiler/control-plane.md` | Light: sequential builds stated; `[executors]` overrides. |
| `compiler/project-settings.md` + schema | Remove `[limits]`; add `[executors]`; keep docs, roots, llm, acp, gen, workflow. |
| new `compiler/diagrams.md` | Rendering: view to `.puml` to `.svg`/`.png`, the emitters, lifting at the frontier, the out layout, `plantuml-little` and `resvg`. |
| `consumers/docsgen.md` | Entity pages embed rendered images and cross-link; ratification proposals render as diagnostic prompts with suggested edits; the rest as today. |
| `frontends/lsp.md` | Hover images and page links. |
| `frontends/cli.md` | `compile` output (board summary, `gc burst:` lines, verdict counts), `preview`, `explain`, `ripple` (`--back`; `ripple <generation>` doubles as the whole-build report: causality DAG, cost totals, parked and failed with reasons), `watch` goal lines. |
| `frontends/gui.md` | The board (compile and GC columns, goal cards with states and justifications), the loaded-set panel in session views, the preview pane shown before a release in manual mode, the inspector (justification click walks, dual-write and decree edits), interactive projections rendered from the graph directly. |
| `frontends/acp.md`, `mcp.md` | Executor overrides per goal kind or class; compilation over MCP claims goal batches through the same `begin`/`done` shape. |
| `consumers/gen.md`, `bind.md` | `statement` field; grouping by component where containment structure exists; generation records each invented choice as a diagnostic graded by the scope of the invention (error, warning, suppressible info); the unattached-remainder measure. |
| `consumers/decompile.md`, `pm.md` | Light touch-ups. |
| `benchmark/` | Mark deferred; keep the harness compiling. |
| `TODO.md` | Refresh against this landing. |
| repo root `CLAUDE.md` | Rewrite the Architecture and Repo layout sections to the landed design (they describe the outgoing turn design, and the layout listing is already stale); keep the norms sections untouched. |

## The code

Workstreams in dependency order, all in `bootstrap/src/`.

1. Model and store (`model.rs`, `store.rs`): the extended fields and new
   kinds; three provenance kinds (`quote`, `derived {from, reasoning}`,
   `decree {author, at, note}`); the view node with ordered members, query,
   collapse, exclusions; per-direction relationship contributions recomputed
   on commit; derived state machines recomputed on commit; default views
   derived on commit; change records written per commit; journal entries with
   opened and resolved goals; the limits registry as constants with per-node
   decree bumps; gates updated (EARS shape gone; transition subjects exist;
   view members exist; parent acyclic and consistent with stated
   composition); dual-write changesets (prose replacement and graph mutation
   in one commit, absorbing the new section hashes so the edit does not
   re-dirty its document), decree retraction, and the provenance flip to
   `quote` when a ratified sentence reconciles; the `edit` journal entry for
   human saves; the new store version field with the archive-and-recompile
   behavior.
2. Goal derivation (`reconcile.rs`, `queue.rs`): the registry trait from
   [orchestration](./orchestration.md#the-registry); `derive_goals` per kind
   over disk plus change records; readiness tiers and cones; locality
   batching under the context budget; GC gating on quiet cones; bubbling
   previews at staging; escalation at hard thresholds; parked and failed
   goals persisted in `status.yaml`; the deterministic checks (justification
   closure over provenance and `from` chains, flow placement feeding
   `curate-view`, conformance, machine checks, provider check); cross-class
   flip detection parking an oscillating pair as one `unstable-derivation`
   diagnostic carrying both justifications (two flips park).
3. Sessions and loading (`turn.rs`, `context.rs`, `tools.rs`): the one agent
   contract as a payload file; per-kind contract paragraphs and skills as
   payload files; toolset union per batch; the loaded set in the serving with
   `load`, `unload`, `graph_status`, `expand` and the status block on every
   mutating reply; `mark_goal_done` (gate validation, mandatory one-line
   justification), `mark_goal_failed`; the repeated-call guard extended to
   `load`; `load_skill` with the skill lifecycle (auto-load on the first node
   of a kind per session, the index line in the status, the per-session cap,
   inactive marking when the last node of a kind unloads); budgets, retry
   once, park.
4. Rendering (new `render.rs`): after the spike, the emitters, one per
   catalog kind, with the proposal's showcase in
   [ir-graph](./ir-graph.md#every-diagram-from-one-example-graph) as the
   reference output; lifting and collapse at the view frontier (one arrow
   per direction-and-type group, collapsed and lifted arrows promoting to
   the strongest type with a count); over-limit views rendering with
   auto-collapse of the largest subtrees, visibly marked; `.puml` plus
   `.svg` per view under `<out>/diagrams/`, `.png` through `resvg` where a
   surface needs raster.
5. Surfaces (`docsgen.rs`, `lsp.rs`, `cli.rs`, `gui/`, `mcp.rs`, `acp/`):
   docsgen entity pages embedding the images with relative links, and
   ratification proposals rendered as prompts; LSP hover with image and page
   link; the CLI commands, compile output, and the whole-build report; the
   GUI board, loaded-set panel, preview pane, and inspector with
   justification walks plus dual-write and decree edits (rebuild rule!);
   goals-over-MCP; `[executors]` resolution.
6. Consumers (`gen.rs`, `bind.rs`, `verify.rs`, `decompile.rs`,
   `feedback.rs`, `answer.rs`): the `statement` rename everywhere; component
   grouping where containment structure exists; invented choices recorded as
   diagnostics graded by invention scope; everything else stands.
7. Fixtures and dogfood: update `f2`'s `EXPECTED.md` for free-form
   statements and the new kinds (`f1` has no `EXPECTED.md` and gains none);
   create the two fixtures the landing promises, a small organization corpus
   and a small narrative corpus, beside `example-slides`; compile `f1` with a
   capable model, then `f2`, then the examples, then the dogfood.

## Definition of done

- `cargo test` green, including new unit tests for: goal derivation and
  identity across re-derivations, cone readiness, escalation, per-direction
  relationship recompute, state machine derivation and its checks, view
  defaults, lifting (including a multi-type pair collapsing under it), the
  render emitters against the showcase, justification closure, flow
  placement, cross-class flip parking, change records, gate rejections.
- `jazyk compile` on `f1` and `f2` converges; `f2`'s traps still trip.
- The dogfood: `cd docs && jazyk compile` reaches `converged` (blocked and
  advised counts are acceptable, silence is not); `<out>/diagrams/` holds
  `.puml` and `.svg` per view; docsgen pages embed the images; `jazyk
  preview`, `jazyk explain`, `jazyk ripple` answer; LSP hover shows a
  diagram; the GUI board renders goals live.
- An immediately repeated `jazyk compile` derives zero goals and makes zero
  LLM calls.
- A dual write edits the sentence and the graph in one changeset without
  re-dirtying the document; a decree queues a ratification proposal, and
  accepting it flips the fact's provenance to `quote`.
- Docs tree and binary agree: no page describes machinery the binary lacks,
  and no behavior lacks a page. Prompts and skills are bytes shared between
  docs and binary via `include_str!`. The root `CLAUDE.md` describes the
  landed design.
- Committed and pushed per the root `CLAUDE.md` norms, tree clean.

## Deferred, explicitly

Not part of this landing: parallel sessions (sequential by design for now),
benchmark cases per goal kind (after real use shows what needs grading),
embeddings-backed search and clustering (the Rig note in
[orchestration](./orchestration.md#alternatives-considered)), the OTel
export, cost-accounting views beyond per-build totals in `status.yaml`.

## Risks

- `plantuml-little` supply chain: young crate, dead repository link. Pin,
  vendor if needed, keep the official native binary swap authorized behind
  the render seam, and diff fixture renders against it in CI when
  benchmarking starts.
- Model capability: goal sessions assume a capable executor; the default
  local model will fail judgment-heavy goals. The harness holds (gates bounce
  junk), but validate on `f1` with a capable model before the dogfood.
- Token cost: the dogfood under the full design is a large run; get `f1` and
  `f2` clean first, and remember remote runs need per-run approval.
- Oscillation tuning: GC-vs-compile flip detection has no measured
  thresholds yet; start conservative (two flips park).
- The natural-key decision is load-bearing: settle it first, it shapes
  upserts, gates, and merges everywhere.
