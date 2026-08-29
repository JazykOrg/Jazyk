# Jazyk

Jazyk is a natural language compiler. It treats prose documentation as the source code of a
program. Instead of constraining English, the compiler maintains a persistent semantic graph
(entities, free-form requirement statements, views, derived relationships and state
machines, sticky diagnostics) reconciled against the docs by LLM sessions resolving derived
goals, surfacing ambiguity and contradictions along the way, and renders every UML diagram
from it. Downstream usages
(code generation, test generation, project management, agent retrieval) consume the graph.
"Jazyk" means tongue/language in Slavic languages.

Status: research project; large changes in direction are acceptable and expected.

- Canonical trees: `docs/` (the design, also the dogfood input) and `bootstrap/` (the Rust
  implementation, binary `jazyk`).
- The v1 multi-step compile/link design (previously `docs/` and `bootstrap/`, then archived as
  the current trees took the names `docs2/` and `bootstrap2/`) failed in practice and was
  removed; see git history around "Failed POC". Do not resurrect it.
- `site/` hosts the jazyk.org static site and its deploy workflow.

## Architecture (goal-based reconciliation)

One persistent graph per project, edited in place, never regenerated. Ids are minted once
and immutable; natural keys make retries harmless; merges leave redirects; nothing enters
without provenance. The runtime (see `docs/compiler/compiler.md`):

- The graph: three authored node kinds (entity, requirement, view), two derived kinds
  (relationship, state machine), sections as structure, diagnostics as sticky judgment
  records. A requirement is a free-form `statement` with `entities`, plural directional
  `edges`, an optional `transition`, and `facets`; an entity carries `stereotype`,
  `parent` (one containment tree), and `attributes`; a view stores what a diagram
  includes (kind, title, ordered members, query, collapse, exclusions), never how it
  looks. Every fact has exactly one provenance: `quote` (verbatim, located
  whitespace-insensitively, never char offsets), `derived` (`from` + reasoning), or
  `decree` (a human write on the graph). Derived and decreed facts carry a ratification
  proposal toward prose; accepting it flips the provenance to `quote`.
- Goals and the board: the reconciler derives goals from disk (docs, graph, ledger, the
  change records in `status.yaml`) and never stores them. A goal is `g:<kind>:<target>`;
  its `change` is its identity across re-derivations and its `cause` names the committed
  mutation that opened it. Compile goals bring the graph in line with the docs
  (`place-anchors`, `reconcile-section`, `rejudge-pair`, `review-entity`, `retrace`,
  `conform-instance`, `bind`, `generate`, `verify`; `ratify` and `answer` block on a
  human). GC goals restructure (`declare-edges`, `dedupe-candidates`, `curate-view`,
  `split-view`, `abstract-entity`): optional at a soft limit, mandatory past the hard
  one, the limits in a registry built into the binary (`limits.rs`), never in
  `jazyk.toml`. Readiness tiers order compile goals (alignment, then ingest by document
  link level roots first, then judgment, then ledger work); a GC goal is ready when no
  compile goal is open in its target's cone. A build is bursts of compile and GC, cone
  by cone, sequential: one build under the build lease, one session at a time.
- Sessions: one session per goal batch (open goals of the highest ready tier, grouped by
  locality, filled to the context budget), run as an ACP worker session (the embedded
  agent with native or text codec, probed and sticky per run, or an external agent per
  `[executors]`). The prompt is assembled from payload files, never authored per goal:
  the fixed agent contract, the active skills, a project block, one block per goal
  (contract paragraph, change, gate, hints), the loaded set. The model loads the graph
  explicitly (`load`, `expand`, `unload`, `graph_status`; stubs and handles keep it
  under budget). Write tools stage mutations behind gates and preview the goals a
  mutation will open; `mark_goal_done` is validated against the kind's gate and takes
  a one-line justification; `mark_goal_failed` is always available; `done` runs batch
  gates; the changeset commits atomically, or the session retries once and parks.
- The store: YAML shards under `jazyk-out/graph/`, natural-key upserts, changesets
  committed atomically behind `.lock`, one journal entry per generation with
  `opened_goals` and `resolved_goals` (each with its justification; a human save that
  dirties sections journals as an `edit` entry, so every ripple roots in a generation),
  the deterministic GC sweep at commit, `version: 2` in `status.yaml` (a mismatch
  archives the out directory to `jazyk-out.bak` and the next build reconciles from
  empty; no migration code, ever).
- Derived data, recomputed on every commit, no write tool: relationships from
  requirement `edges` (one node per pair, contributions grouped by direction and type,
  a typeless edge counts as `dependency`), state machines from `transition` facets (one
  per subject entity), default views (a class view per scope, a component view per
  «system», use case and sequence views per flow cluster, a state view per machine, an
  object view per instantiated type; stable ids, `default: true`, cleared by any
  mutation that names the view), and the name index.
- Views and rendering: every view emits `.puml` and `.svg` under
  `jazyk-out/diagrams/<kind>/` on commit through `plantuml-little` in process (`resvg`
  for `.png` on demand; `JAZYK_PLANTUML` swaps in the official native binary behind the
  `render_svg` seam). A relationship touching a hidden descendant lifts to the nearest
  shown ancestor; groups collapse to one arrow per direction and type, promoted to the
  strongest type with a count; an over-limit view renders with its largest subtrees
  auto-collapsed and a visible note. Diagrams are projections: deleting a rendering
  loses nothing. A PlantUML block in a source document is input, parsed as a `diagram`
  section.
- Checks and convergence: deterministic checks close every build and run on
  `jazyk check` (coverage, reachability, stale provenance, document quality,
  justification closure, flow placement, containment, conformance, state machine
  checks, provider check, cross-class flip detection). `converged` when no open or
  failed mandatory goal of either class remains and the checks are clean; blocked and
  optional counts ride with the verdict. A no-op rebuild derives zero goals and makes
  zero LLM calls. `jazyk preview`, `explain`, and `ripple` show the next prompt, a
  goal's cause, and the causal chain of a change.

Division of labor is strict. The harness owns parsing, alignment, identity, dirtiness,
goal derivation, readiness, scheduling, gates, derived data, budgets, causality, and
rendering. The model owns extraction, same-vs-different judgment (search before create),
severity, wording, abstraction, view curation, coverage marking (covered | non-normative),
and justifications. The model never creates, routes, or prioritizes goals; it resolves,
fails, or parks them. Declarative prose states obligations: sessions restate it as a
`statement` and keep the quote verbatim (`docs/compiler/concepts/statements.md`).

## Repo layout

- `docs/main.md`: front door. `docs/compiler/`: compiler.md (division of labor,
  components, build lifecycle, outputs), parsing.md, alignment.md, model.md + `model/`
  (entity, requirement, relationship, view, state-machine, section, diagnostic),
  graph.md (storage layout, mutations, changesets, gates, derived data, change records,
  journal, GC, the limits registry, concurrency), reconciler.md (dirty set, goal
  derivation, readiness, batching, bubbling, escalation, parked and failed, flip
  detection), compilation.md (a build, compile and GC, edit paths, checks, convergence,
  coverage, incremental builds), sessions.md (anatomy, the prompt, skills, toolsets,
  execution, staged mutations, commit, budgets, trace events, preview), `goals/` (one
  page per goal kind: created when, gate, hints, what the model sees, tools),
  context.md (the loaded set, its tools, policy, axes, rendering), tools.md (the
  registry by group, toolsets), diagrams.md (rendering, the emitters, lifting and
  collapse, over-limit views, output layout, the renderer, diagrams as input),
  control-plane.md (modes, releases, workers, leases), project-settings.md, `concepts/`
  (statements, identity, scopes, judgment), schemas (draft-07 JSON Schema in YAML,
  `$id` `https://jazyk.org/schemas/*.json`). `docs/site.md` specs jazyk.org;
  `docs/TODO.md` is the live work list.
- Payloads: `docs/compiler/goals/prompts/` (agent-contract.md, feedback-note.md,
  worker-protocol.md, one contract paragraph per model-executed goal kind, and the
  generation payloads bind-contract.md, bind-pointer.md, generate-contract.md,
  generate-pointer.md, decompile-contract.md) and `docs/compiler/skills/` (extraction,
  judgment, flow-views, structural-views, abstraction, conformance). Both are embedded
  into the binary at compile time (`include_str!`), so docs and code share the same
  bytes by construction. Edit the files, never reintroduce string constants in code.
  Both directories are excluded from the docs glob (they are instructions to a model,
  not prose about jazyk). Placeholders: `{target}` (worker-protocol: the batch id;
  pointers), `{GROUP}` (generate-contract).
- `docs/frontends/`: cli.md, acp.md, mcp.md, lsp.md, gui.md, viewer.md.
  `docs/consumers/`: gen.md (generation + verification ledger), bind.md, decompile.md,
  docsgen.md, pm.md. `docs/benchmark/`: benchmark.md, cases.md, case.schema.yaml,
  `cases/` (the case files are embedded into the binary at compile time; they are
  fixtures, excluded from the docs glob), known-results.yaml (embedded too).
- `docs/jazyk.toml`: the live project file (docs glob, roots, lint rules); the repo-root
  `jazyk.toml` redirects discovery to it. The graph lands in `docs/jazyk-out/`
  (gitignored): `graph/*.yaml` shards (entities, requirements, views, diagnostics,
  redirects; derived relationships and state-machines rewritten every commit), `docs/`
  section trees + coverage, `journal/g<N>.yaml`, `diagrams/<kind>/<slug>.puml` and
  `.svg`, `docsgen/`, `gen/`, `trace/`, `sessions/`, `status.yaml` (version,
  generation, verdict, change records, parked, failed, costs).
- `bootstrap/src/`: model.rs (node kinds, provenance, change records, journal entry,
  status), store.rs (shards, natural keys, changesets, gates at commit, journal, GC
  sweep, store version and archive), limits.rs (the limits registry and the session and
  build budgets), derive.rs (relationships, state machines, default views, flow
  clustering), goals.rs (the `GoalKind` registry trait, one implementation per kind,
  hint computers, the static registry), board.rs (the derived board, cones, readiness
  tiers, locality batching, GC gating, escalation, parked and failed, the MCP goal
  list), reconcile.rs (the build loop, checks, flip detection, verdict, whole-build
  report), session.rs (prompt assembly from payload files, skills, trace events,
  transcript), context.rs (the loaded set: `load`, `expand`, `unload`, `graph_status`,
  stubs, handles, high-water mark), tools.rs (the registry, gates at staging, bubbling
  previews, goal tools, the repeated-call guard), render.rs (emitters, lifting,
  collapse, the `render_svg` seam, `.puml`/`.svg`/`.png`), align.rs, md.rs,
  project.rs, control.rs, llm.rs (OpenAI-compatible client over ureq; sticky fallbacks
  for tools/temperature/streaming), parallel.rs, jsonrpc.rs, `acp/` (the bridge and
  the embedded agent), `gui/` (axum; the only tokio scope in the binary), mcp.rs,
  lsp.rs (read-only), cli.rs, docsgen.rs, viewer.rs, gen.rs, bind.rs, verify.rs,
  decompile.rs, feedback.rs, answer.rs, benchmark.rs. `bootstrap/editors/vscode`: LSP
  client extension. Deps: serde, serde_json, serde_norway, ureq (HTTP), notify (file
  events), regex, agent-client-protocol (ACP; futures + async-io driven by a `block_on`
  on a dedicated thread), plantuml-little + graphviz-anywhere (the in-process
  renderer; `bootstrap/.cargo/config.toml` opts into the prebuilt Graphviz archive
  download), resvg (raster), tokio + axum (gui only), include_dir (the embedded GUI
  dist). Dependency policy (owner decision, 2026-07-06): infrastructure comes from
  crates; hand-roll only domain logic. Do not reimplement transports, parsers for
  standard formats, or platform APIs.
- Examples, each a project with its own `jazyk.toml`: `bootstrap/example/f1` (small)
  and `f2` (planted traps, see its EXPECTED.md); at the repo root `example-sort` (a
  sort utility stated near pseudo-code, with its code), `example-erp` (a multi-document
  software system), `example-slides` (a slide deck, non-software), `example-org` (an
  organization) and `example-novel` (a narrative), the last two created by the
  landing's fixture workstream (`plans/implementation.md`). `bootstrap/VALIDATION.md`:
  measured results, scorecard, known weaknesses.

## Build and commands

- `cd bootstrap && cargo build --release` (binary at `bootstrap/target/release/jazyk`),
  `cargo test`.
- The owner's `jazyk` command is a symlink to `bootstrap/target/release/jazyk`. After
  ANY change to the GUI frontend or the binary, always finish with
  `cd bootstrap/gui && npm run build` then `cargo build --release`: the built frontend
  (`gui/dist`) embeds into the binary at compile time, so a stale release build serves
  the old GUI. A running `jazyk gui` keeps its embedded assets; it needs a restart to
  pick up a new binary.
- `jazyk compile [path...]` (live trace; `--verbose` the cascade and full prompts,
  `--quiet` summary), `check`, `watch`, `status`, `preview [goal|target]`,
  `explain [goal|target]`, `ripple <target|generation|doc> [--back]`,
  `context <target>`, `query <text>`, `gen [entity...]`, `test [--audit]`, `docsgen`,
  `viewer`, `mcp graph [--write]`, `lsp`, `benchmark`.
- A project is a directory with `jazyk.toml` (walk-up discovery). Run the dogfood from
  `docs/`.
- Always run `jazyk benchmark` before trusting a new model: it grades session capability
  per codec with deterministic checks (cases per goal kind are deferred, see
  `docs/benchmark/benchmark.md`). Local 4B-class models fail it; the harness still holds
  (gates bounce bad calls, junk never lands), but judgment-quality output (reviews, lint)
  degrades.

## LLM config

Precedence per field: CLI flag → env (`JAZYK_LLM_BASE_URL`, `JAZYK_MODEL`, `JAZYK_API_KEY`) →
project `[llm]` → `~/.jazyk/config.toml` → default. The repo `.env` points
`JAZYK_LLM_BASE_URL` at LocalRouter (`http://127.0.0.1:3625`), which proxies local Ollama
models and remote providers; the global config picks the model (`gemma4:e4b-mlx`). Use
LocalRouter; do not override the endpoint unless asked. Tuning: `JAZYK_MAX_RETRIES`,
`JAZYK_TEMPERATURE` (negative omits), `JAZYK_VERBOSE`, `JAZYK_CODEC` (force
`native`/`text`), `JAZYK_PLANTUML` (an official PlantUML native binary behind the render
seam instead of the in-process renderer). Builds are sequential by design (one session at
a time). Local models are slow: test on `bootstrap/example/f1` first.

## Docs-first workflow (hard rule)

`bootstrap` is an example implementation of `docs`. Any behavior change lands in `docs/`
first, then the code. No undocumented features. Mid-implementation discoveries flow
docs → code, never code alone.

## Docs writing style (match this exactly)

The owner is strict about voice. When writing or editing anything under `docs/`:
- Plain, to-the-point, short declarative sentences.
- Never use em dashes. Use commas, periods, parentheses, or colons.
- Bullet lists with `- `, nested where useful.
- Backticks for identifiers, filenames, field names, rule names.
- `e.g.` and `E.g.:` with fenced code blocks for examples. `→` for sequences.
- Sparing bold. One H1 per file. Headings `#`, `##`, `###`.
- No marketing language. State what it does.
- Cross-link with relative markdown links and anchors (GitHub slug of the heading).

After editing docs, check that relative links and anchors resolve and that there are no em
dashes. docs is also the compiler's own input, so keep statements extractable.

## Working norms

- Always commit and push all changes when the work is done. Do not leave the tree dirty.
- Keep secrets out of tracked files. `.env`, `*.env`, `*/target/`, and `jazyk-out` anywhere
  are gitignored.
- Git remote: `git@github.com:JazykOrg/Jazyk.git`. Pushing to master deploys
  `site/` to jazyk.org via GitHub Actions.
