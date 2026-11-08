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
- The private hosted service lives in the separate
  [JazykOrg/jazyk-cloud repository](https://github.com/JazykOrg/jazyk-cloud), locally
  `../jazyk-cloud`. It owns the complete jazyk.dev editor in `web/`, including
  its branding, Board, Tabs, PWA, browser tests, and deployment. Shared compiler,
  native/WASM engine, workspace, delivery, verification, local HTTP/SSE/WebSocket,
  and ACP/LSP contracts stay here. Jazyk must build and run locally without
  depending on the private repository. `jazyk gui` starts the local API and opens
  jazyk.dev. Local and browser use require no Cloud account.

## Architecture (goal-based reconciliation)

One persistent graph per document set, edited in place, never regenerated. Ids are minted once
and immutable; natural keys make retries harmless; merges leave redirects; nothing enters
without provenance. The runtime (see `docs/compiler/compiler.md`):

- The graph: four authored node kinds (entity, requirement, criterion, view), two derived kinds
  (relationship, state machine), sections as structure, diagnostics as sticky judgment
  records. A requirement is a free-form `statement` with `entities`, plural directional
  `edges`, an optional `transition`, and `facets`; a criterion is the observable
  evidence one requirement is met (one condition, its outcome, concrete values), quoted
  from a test case or example or derived with a proposal, and what tests bind to; an entity carries `stereotype`,
  `parent` (one containment tree), and `attributes`; a view stores what a diagram
  includes (kind, title, ordered members, query, collapse, exclusions), never how it
  looks. Every fact has exactly one provenance: `quote` (verbatim, located
  whitespace-insensitively, never char offsets), `derived` (`from` + reasoning), or
  `decree` (a human write on the graph). Derived and decreed facts carry a ratification
  proposal toward prose; accepting it flips the provenance to `quote`.
- Goals and the board: the reconciler derives goals from disk (docs, graph, ledger, the
  change records under `changes/`) and never stores them. A goal is `g:<kind>:<target>`;
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
  Goose GDK agent with native tool calling, or an external agent per
  `[executors]`). The prompt is assembled from payload files, never authored per goal:
  the fixed agent contract, the active skills, a project block, one block per goal
  (contract paragraph, change, gate, hints), the loaded set. The model loads the graph
  explicitly (`load`, `expand`, `unload`, `graph_status`; stubs and handles keep it
  under budget). Write tools stage mutations behind gates and preview the goals a
  mutation will open; `mark_goal_done` is validated against the kind's gate and takes
  a one-line justification; `mark_goal_failed` is always available; `done` runs batch
  gates; the changeset commits atomically, or the session gets up to four attempts and then parks.
- The store: one YAML file per node under `.jazyk/<set>/graph/<kind>/`, named by a
  reversible file id (`ent:order` → `ent~order.yaml`; past 180 bytes a prefix and a
  hash), each file starting with its `id` and holding only the node's own facts
  (generation markers and requirement-derived entity mentions stay out), ids minted
  from natural keys with no counter (`req:<stem>-<hash>`, `crit:<stem>-<hash>`,
  `diag:<rule>-<hash>`; the slug part transliterated, collapsed to single `-`, cut to
  32 characters), natural-key upserts, changesets committed atomically
  behind `.lock`, one journal entry per generation with `opened_goals` and
  `resolved_goals` (each with its justification; a human save that dirties sections
  journals as an `edit` entry, so every ripple roots in a generation), the
  deterministic GC sweep at commit, `version: 4` in `status.yaml` (a version 3 store
  loads as it is and its next save writes the version 4 layout, ids kept; any other
  mismatch archives the out directory to `<out>.bak` and the next build reconciles
  from empty; no other migration code).
- Committed and local state: a repository commits each set's status (`status.yaml`
  plus one file per record under `changes/`, `parked/`, `failed/`, `alignment/`,
  `lookalikes/`; only what goal derivation reads), the non-derived node files
  (diagnostics included), `docs/` section trees (no line numbers, no file hash), and
  `gen/` (the ledger, one file per row), so any commit of any branch compiles
  incrementally. Everything else is local: derived default views, relationships and
  state machines (recomputed at load when missing), diagrams, docsgen, the journal,
  logs, the control plane, `markers.yaml` (generation markers), and `build.yaml` (the
  generation, the last verdict, costs, spend, document content hashes). The generation
  is a local counter raised at load to the highest generation the status names, so parallel branches never conflict on
  it. The engine generates `.jazyk/.gitignore` from this split
  (`docs/compiler/graph.md#committed-and-local-state`).
- Derived data, recomputed on every commit, no write tool: relationships from
  requirement `edges` (one node per pair, contributions grouped by direction and type,
  a typeless edge counts as `dependency`), state machines from `transition` facets (one
  per subject entity), default views (a class view per scope, a component view per
  «system», use case and sequence views per flow cluster, a state view per machine, an
  object view per instantiated type; stable ids, `default: true`, cleared by any
  mutation that names the view), and the name index.
- Views and rendering: every view emits `.puml` and `.svg` under
  `.jazyk/diagrams/<kind>/` on commit through `plantuml-little` in process (`resvg`
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
  worker-protocol.md, serving-protocol.md, open-batch-protocol.md, one contract paragraph per model-executed
  goal kind, and the
  generation payloads bind-contract.md, generate-contract.md,
  decompile-contract.md) and `docs/compiler/skills/` (extraction,
  judgment, criteria, flow-views, structural-views, abstraction, conformance). Both are embedded
  into the binary at compile time (`include_str!`), so docs and code share the same
  bytes by construction. Edit the files, never reintroduce string constants in code.
  Both directories are excluded from the docs glob (they are instructions to a model,
  not prose about jazyk). Placeholders: `{target}` (worker-protocol and serving-protocol: the batch id;
  pointers), `{GROUP}` (generate-contract).
- `docs/frontends/`: cli.md, acp.md, mcp.md, lsp.md, gui.md (local API and
  launcher), wasm.md (portable browser engine), protocol.md (editor/engine
  compatibility), viewer.md. UI presentation specifications live in the private
  `../jazyk-cloud/docs/editor/` tree.
  `docs/consumers/`: gen.md (generation + verification ledger), bind.md, decompile.md,
  docsgen.md, pm.md. `docs/benchmark/`: benchmark.md, cases.md, case.schema.yaml,
  `cases/` (the case files are embedded into the binary at compile time; they are
  fixtures, excluded from the docs glob), known-results.yaml (embedded too).
- `.jazyk/jazyk.toml`: the live project file with named document sets.
  The dogfood graph lands in `.jazyk/ds_main/` (this repository's root `.gitignore`
  keeps it out of git): `graph/<kind>/<file id>.yaml` node files (entities,
  requirements, criteria, views, diagnostics, redirects; derived default-views,
  relationships and state-machines rewritten every commit), `docs/` section trees + coverage,
  `journal/g<N>.yaml`, `diagrams/<kind>/<slug>.puml` and `.svg`, `docsgen/`, `gen/`,
  `trace/`, `sessions/`, the status (`status.yaml`, `changes/`, `parked/`, `failed/`,
  `alignment/`, `lookalikes/`), `build.yaml` (generation, verdict, costs, spend),
  `markers.yaml`.
- `bootstrap/src/`: model.rs (node kinds, provenance, change records, journal entry,
  status), store.rs (node files and file ids, the status split, natural keys, changesets, gates at commit, journal, GC
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
  project.rs, control.rs, llm.rs (configured model transport with public GDK wire
  helpers), `harness/` (the shared public GDK runtime), session_recovery.rs (owned
  execution capsules and parent progress), parallel.rs, jsonrpc.rs, `acp/` (the
  bridge and the embedded agent), `gui/` (axum), mcp.rs,
  lsp.rs (read-only), cli.rs, docsgen.rs, viewer.rs, gen.rs, bind.rs, verify.rs,
  decompile.rs, feedback.rs, answer.rs, benchmark.rs. `bootstrap/editors/vscode`: LSP
  client extension. Deps: serde, serde_json, serde_norway, ureq (catalog HTTP),
  reqwest + eventsource-stream (native model HTTP/SSE), notify (file
  events), regex, agent-client-protocol (ACP; futures + async-io driven by a `block_on`
  on a dedicated thread), plantuml-little + graphviz-anywhere (the in-process
  renderer; `bootstrap/.cargo/config.toml` opts into the prebuilt Graphviz archive
  download), resvg (raster), tokio (native model/MCP I/O and GUI), axum + tower-http
  (the GUI server and its CORS layer), goose-agent + goose-provider-types + the
  documented portable goose-context-management source patch. Dependency policy
  (owner decision, 2026-07-06): infrastructure comes from
  crates; hand-roll only domain logic. Do not reimplement transports, parsers for
  standard formats, or platform APIs.
- Examples, each a project with its own `.jazyk/jazyk.toml`: `bootstrap/example/f1` (small)
  and `f2` (planted traps, see its EXPECTED.md); at the repo root `example-sort` (a
  sort utility stated near pseudo-code, with its code), `example-erp` (a multi-document
  software system), `example-slides` (a slide deck, non-software), `example-org` (an
  organization) and `example-novel` (a narrative), the last two created by the
  landing's fixture workstream (`plans/implementation.md`). `bootstrap/VALIDATION.md`:
  measured results, scorecard, known weaknesses.

## Build and commands

- `cd bootstrap && cargo build --locked --release` (binary at
  `bootstrap/target/release/jazyk`), `cargo test --locked`.
- The owner's `jazyk` command is a symlink to `bootstrap/target/release/jazyk`. After
  ANY change to the binary, finish with `cargo build --release`; a running `jazyk gui`
  needs a restart to pick up a new binary. The binary carries no page: `jazyk gui`
  serves the API and opens jazyk.dev (or `--gui-url`, e.g. a `vite` dev server), which
  connects to it (`docs/frontends/gui.md#connecting-to-a-local-jazyk`). The page
  lives in the private sibling repo: `cd ../jazyk-cloud/web && npm run build`
  (`dist/`, never committed). Its compiler input is pinned by
  `compiler.lock.json`; `JAZYK_SOURCE` permits a local override outside CI.
  Editor tests and jazyk.dev publication belong to that repository. Engine builds
  never require the editor checkout. The independent `site/` workflow owns jazyk.org.
- `jazyk compile [path...]` (live trace; `--verbose` the cascade and full prompts,
  `--quiet` summary), `check`, `watch`, `status`, `preview [goal|target]`,
  `explain [goal|target]`, `ripple <target|generation|doc> [--back]`,
  `context <target>`, `query <text>`, `gen [entity...]`, `test [--audit]`, `docsgen`,
  `viewer`, `mcp graph [--write]`, `lsp`, `benchmark`.
- A project is a directory with `.jazyk/jazyk.toml` (walk-up discovery). Run the
  dogfood from the repository root. Each set stores state directly in `.jazyk/<id>/`.
- Always run `jazyk benchmark` before trusting a new model: it grades session capability
  through the native GDK runtime with deterministic checks (cases per goal kind are deferred, see
  `docs/benchmark/benchmark.md`). Local 4B-class models fail it; the harness still holds
  (gates bounce bad calls, junk never lands), but judgment-quality output (reviews, lint)
  degrades.

## Validation and hosted CI

- Follow [repository validation](docs/main.md#repository-validation). Run the
  relevant local checks before pushing: `cd bootstrap && cargo test --locked` for
  engine changes, `cd runtime-worker && go test ./...` for helper changes, and
  `cd bootstrap && cargo check --locked --target wasm32-unknown-unknown --lib` for
  portable engine changes. Workflow/docs-only changes need workflow syntax,
  trigger/input checks, and documentation link checks rather than engine rebuilds.
- Preserve the release build requirement above after every binary change.
  Default hosted CI uses the Rust test profile. Explicit manual
  `release_validation=true` runs `cargo build --locked --release` and
  `cargo test --locked --release` instead. Both modes check WASM and the Go helper.
- CI runs automatically for relevant pull requests and supports manual dispatch.
  No engine CI run is expected after a push to `master`. Do not wait for one or
  dispatch replacement CI after routine tasks. Run hosted CI only when the owner
  requests it or as part of explicitly authorized release validation.
- When reporting hosted validation, verify the run's exact commit SHA and report
  its URL, conclusion, and selected inputs. Report local checks as local evidence.
- `site/` publication remains separate and automatic for relevant `master` changes.
  A CI dispatch never deploys the static site, editor, or hosted service.

## LLM config

Precedence per field: CLI flag → env (`JAZYK_LLM_BASE_URL`, `JAZYK_MODEL`, `JAZYK_API_KEY`) →
project `[llm]` → `~/.jazyk/config.toml` → default. The repo `.env` points
`JAZYK_LLM_BASE_URL` at LocalRouter (`http://127.0.0.1:3625`), which proxies local Ollama
models and remote providers; the global config picks the model (`gemma4:e4b-mlx`). Use
LocalRouter; do not override the endpoint unless asked. Tuning:
`JAZYK_TEMPERATURE` (negative omits), `JAZYK_VERBOSE`, `JAZYK_PLANTUML` (an official PlantUML native binary behind the render
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

- Create an isolated local worktree with a local Git branch for each change.
- Commit coherent changes after relevant verification. Keep unrelated user edits
  intact and leave the worktree clean at the end of each step.
- Keep work branches local. Do not push feature branches or create branches or
  pull requests on GitHub.
- Wait for the owner's explicit approval before merging into local `master`.
  After approval, merge locally and push only `master`. Never merge through GitHub.
- Keep secrets out of tracked files. `.env`, `*.env`, `*/target/`, and generated state inside `.jazyk`
  are gitignored. Commit `.jazyk/jazyk.toml` and `.jazyk/.gitignore`. In this
  repository the root `.gitignore` also keeps every set's compiled state out, so the
  dogfood and the examples commit only their configuration.
- Git remote: `git@github.com:JazykOrg/Jazyk.git`. A push to `master` that changes
  `site/` or its deployment workflow publishes jazyk.org via GitHub Actions.
  Engine-only pushes do not deploy or start engine CI.

<!-- JAZYK:BEGIN -->
<!-- Automatically updated by Jazyk. To opt out, set [agents] block = false in .jazyk/jazyk.toml. -->
## Jazyk project

You are a coding agent working in a Jazyk project. Answer the current request and
carry authorized work through to a concrete result. Incorporate steering of
unfinished work in arrival order. Resume completed or cancelled work only when
asked. Answer a self-contained general question directly, without project tools.
For project-specific questions, inspect the relevant current files and evidence.
Do not ask for repository, branch, or other context already supplied.

### Requirements and implementation

Prose documents define intended behavior. The semantic graph records their
requirements and observable criteria. Code and other deliverables must satisfy
those requirements. Tests provide evidence, not permission to change behavior.

For a code change, inspect the relevant docs, requirements, existing code, and
ownership first. A repair within existing requirements can change code and tests
without rewriting unchanged docs. For an authorized behavior change, update the
docs first, reconcile the affected requirements when authorized, then implement
and verify them. Never invent requirements to justify arbitrary code. If the
implementation should become the documented behavior, inspect it through reverse
compilation and surface differences from existing intent before ratifying them.
Ambiguity or contradiction needs a diagnostic or a concrete user decision.

### Scope and evidence

A question, review, or status request is read-only. A docs edit does not authorize
compilation, implementation, task execution, or publication. Explicit requests
authorize their named work once. Honor project approval and cost rules before
additional model sessions or paid actions. Tool hints and your own offers do not
expand authorization. If a necessary step lacks authority, explain the specific
step and request it. Do not silently bypass the step or claim synchronization.

Preserve unrelated files, human instructions, and existing test assertions.
Read current text before editing and use returned hashes and IDs. Never edit
Jazyk graph nodes, ledgers, or generated state by hand. Use the supplied tools.
Do not put credentials in docs or task definitions. An available tool or readable
skill does not grant permission to use it. Active tool packages and enforced
gates supply the current scope and evidence requirements; follow them.

### Workflow skills

Before a workflow, call `read_jazyk_skill` with its name below. Use
`list_jazyk_skills` for the current catalog and required capabilities. These
built-in skills carry the detailed procedures. Read only the relevant skills.

- `inspect-project`: Inspect project questions, code, docs, reviews, and status without changing them.
- `edit-documentation`: Edit prose, requirements, criteria, or intended behavior without implicit compilation.
- `change-code`: Repair or change code within requirements; update docs first for authorized behavior changes.
- `compile`: Compile documentation into the semantic graph and resolve reconciliation goals.
- `generate`: Implement documented requirements and record deliverables and tests.
- `bind`: Connect existing implementation and test evidence to requirements.
- `verify`: Run tests or judge observable criteria and record current evidence.
- `reverse-compile`: Draft documentation from observed implementation and surface intent conflicts.
- `execute-task`: Inspect and execute the requested task under its approval and cost rules.
- `recover-workflow`: Diagnose failed tools, conflicts, interrupted work, or missing prerequisites.
- `publish`: Push, publish, or deploy only with explicit authority and verified integration state.

For a mixed request, read the relevant skills and order authorized work as
needed: docs → graph → implementation → evidence. Do not require unrelated
lifecycle steps for a simple question or narrow repair. If tools are missing,
state the concrete missing capability once. With no Jazyk MCP connection, use
`jazyk setup` to configure one when authorized, or inspect with `jazyk status`,
`jazyk explain`, and `jazyk preview`. Do not imitate successful Jazyk tool calls.

### Project configuration

Document sets:

- `ds_main` (Main): docs `docs/**/*.md` (3 exclusions), roots `docs/main.md`, source `project2/**`

`.jazyk/jazyk.toml` is authoritative. Use `list_document_sets` when a set-local
operation needs an ID. Do not guess an ambiguous set or reuse IDs across sets.
Refresh `project_context` when repository, branch, integration, or sync state
matters. Serving capabilities and integration metadata are additional context,
not new authorization.

### Results

Report actual saved edits, checks and failures, unresolved docs/graph/code sync,
and push or deployment state separately. Workspace edits and passing tests do
not prove a push, publication, deployment, or complete requirements coverage.
A failed call is not a saved edit. When the work is complete or blocked, give a
concise result. Do not substitute a promise to act for the result.
<!-- JAZYK:END -->
