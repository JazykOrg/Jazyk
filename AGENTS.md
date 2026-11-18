# Jazyk

Jazyk is a natural language compiler. Prose is the source; LLM sessions reconcile a
persistent semantic graph against it. Diagrams, code/test generation, project
management, and agent retrieval consume the graph. Research project; large changes
in direction are acceptable. The failed v1 multi-step compile/link design was
removed (git history: "Failed POC"). Do not resurrect it.

## Ownership and navigation

- `docs/` defines the design and is the dogfood input. `bootstrap/` implements it
  in Rust (`jazyk`). Start at [docs/main.md](docs/main.md); live work is in
  `docs/TODO.md`, measured results in `bootstrap/VALIDATION.md`.
- This repository owns the compiler, native/WASM engine, workspace, delivery,
  verification, local HTTP/SSE/WebSocket, and ACP/LSP contracts. It must build and
  run locally without the private Cloud repository or a Cloud account.
- `../jazyk-cloud` ([repository](https://github.com/JazykOrg/jazyk-cloud)) owns the
  hosted service and complete jazyk.dev editor: `web/`, branding, Board, Tabs, PWA,
  browser tests, deployment, and `docs/editor/` specifications.
- `jazyk gui` serves the local API and opens jazyk.dev, or `--gui-url` for a dev
  server. It embeds no page. See [local connection](docs/frontends/gui.md#connecting-to-a-local-jazyk).
- `site/` and its workflow own jazyk.org independently. `runtime-worker/` is the
  bounded Go task helper; `npm/jazyk/` and `scripts/release/` own packaging,
  smoke checks, and publication. `plans/` holds working plans and reviews.
- Engine code is under `bootstrap/src/`. Core modules build natively and for WASM;
  native-only modules are gated off wasm32. Infrastructure comes from crates;
  hand-roll only domain logic. Do not reimplement transports, standard-format
  parsers, or platform APIs.

Read the relevant specifications before changing behavior:

- [Compiler](docs/compiler/compiler.md), [model](docs/compiler/model.md),
  [graph/store](docs/compiler/graph.md), [file layout](docs/compiler/jazyk-folder.md).
- [Reconciler](docs/compiler/reconciler.md), [compilation](docs/compiler/compilation.md),
  [goal contracts](docs/compiler/goals/), [sessions](docs/compiler/sessions.md),
  [context](docs/compiler/context.md), [tools](docs/compiler/tools.md),
  [diagrams](docs/compiler/diagrams.md).
- [Frontends](docs/frontends/), [consumers](docs/consumers/),
  [benchmark](docs/benchmark/benchmark.md), [graph schema](docs/compiler/graph.schema.yaml).
- Small fixture: `bootstrap/example/f1`; traps: `bootstrap/example/f2/EXPECTED.md`;
  GC: `bootstrap/example/e2e-gc`. Root `example-*` projects cover software,
  non-software, ledger, and level-shape cases. Each has `.jazyk/jazyk.toml`.

## Architecture invariants

- One graph per document set, edited in place, never regenerated. IDs are immutable;
  natural-key upserts make retries harmless; merges leave redirects. Every fact has
  exactly one provenance: verbatim whitespace-insensitive `quote` (never character
  offsets), `derived` (sources + reasoning), or explicit human `decree`. Derived and
  decreed facts carry ratification proposals; acceptance changes provenance to `quote`.
- Authored nodes: entity, requirement, criterion, view. Requirements are free-form
  `statement`s with entities, directional `edges`, optional `transition`.
  Criteria state observable conditions, outcomes, and concrete values; tests bind to
  them. Entities have one containment parent. Views store inclusion, never appearance.
  Sections supply structure; diagnostics retain judgments.
- Relationships, state machines, default views, and the name index are derived on
  every commit, without write tools. Relationships come from requirement edges
  (typeless means `dependency`); state machines come from transitions.
- Goals are derived from docs, graph, decisions, sources, and ledger, never stored.
  `g:<kind>:<target>` identifies a goal, `change` persists its identity across
  derivations, and `cause` names the committed mutation that opened it.
  Compile readiness: alignment → ingest (linked roots first) → judgment → ledger.
  `ratify` and `answer` need humans. GC is optional at soft limits, mandatory past
  hard limits, and ready only without open compile goals in the target cone.
  Limits belong in `limits.rs`, never `jazyk.toml`.
- Nothing claims work: builds, sessions, and agents open batches freely and commits
  are the only synchronization (first commit wins; a later one rebases or gets a
  commit conflict). A build runs up to the executor's `max_sessions` at once, one
  batch each from the highest ready tier, grouped by locality and context budget. Workers use embedded Goose GDK or external
  ACP executors. Prompts assemble payloads and skills, never per-goal authored text.
  Models explicitly load graph context. Writes stage behind gates, preview new goals,
  and commit atomically. Completion needs a gate-checked justification; failure is
  always available. Batch gates run at `done`; four attempts precede parking.
- The harness owns parsing, alignment, identity, dirtiness, goal derivation,
  readiness, scheduling, gates, derived data, budgets, causality, and rendering.
  Models own extraction, same/different judgment (search before create), severity,
  wording, abstraction, view curation, coverage (`covered` / `non-normative`), and
  justifications. Models resolve, fail, or park goals; never create or prioritize them.
  Restate obligations as statements while preserving quotes verbatim.
- `.jazyk/<set>/` commits current state only: `store.yaml`, `graph/` (including
  diagnostics and hash-invalidated decisions), `sources/`, and `gen/` ledger rows.
  Node YAML uses reversible file IDs and holds only its own facts, no generations
  or causes. Changesets commit behind `.lock` with deterministic GC. `local/` holds
  journals, progress, workers, leases, and locks; `derived/` holds rebuilt graph
  kinds, diagrams, and docsgen. Generations are local counters. Logs live outside
  the project or in the host record store (`jazyk status` prints the log directory).
- Store version is 5. Version 4 loads and rewrites at next build opening, preserving
  IDs. Newer stores stay read-only (`store-newer`), never archived; older stores
  archive under `.jazyk/local/archive/` and rebuild from empty. No other migrations.
- Diagrams are disposable projections, emitted as `.puml`/`.svg` through
  `plantuml-little`; `resvg` supplies PNG on demand. `JAZYK_PLANTUML` selects the
  official native renderer. Hidden endpoints lift to shown ancestors; arrows group
  by direction/type with strongest type and count. Over-limit views auto-collapse
  with a note. Source PlantUML blocks are `diagram` input sections.
- Deterministic checks close builds and run with `jazyk check`. Convergence needs
  clean checks and no open/failed mandatory compile or GC goals; verdicts include
  blocked/optional counts. A no-op rebuild has zero goals and zero LLM calls.
  `preview`, `explain`, and `ripple` expose prompts and causality.

## Docs and prompts

`bootstrap` is an example implementation of `docs`. Behavior changes land in docs
first, then code, including discoveries during implementation. No undocumented
features. Repairs within existing requirements need not rewrite unchanged docs.

For `docs/`:

- Write short declarative sentences, extractable as requirements. No marketing.
- No em dashes; use commas, periods, parentheses, or colons.
- Use `- ` bullets, backticks for identifiers/files/fields/rules, sparing bold,
  one H1, and `#`/`##`/`###` headings.
- Use `e.g.` or `E.g.:` with fenced examples and `→` for sequences.
- Use relative Markdown links and GitHub heading anchors. Check links, anchors,
  and absence of em dashes after editing.

Before changing model-facing text, read the [prompt guide](docs/compiler/goals/prompts/README.md).
Keep all instructions, including corrective guidance, in docs payloads. Use the
common parser (`bootstrap/prompt_parser.rs`), Markdown, reusable partials, and
source comments with known rationale. `.md.mustache` supports variables, comments,
and file-relative includes; `build.rs` embeds payloads and `payload.rs` renders
values once. Keep purpose, consumers, inputs, and ordering in each prompt, not a
separate catalog. Numeric prefixes require an actual workflow sequence. Update
parser tests when conventions change. Domain and agent workflow skills live under
`docs/compiler/skills/`; frontmatter is catalog metadata, only the body reaches
models. Prompt/skill directories are excluded from the docs glob.

## Local workflow and Git

- Use an isolated local worktree and local branch for each change. Preserve
  unrelated edits. Commit coherent changes after verification; leave the worktree
  clean at each step.
- After verification, commit your changes, merge the work branch into local
  `master` with `git merge --no-ff`, and push `master` without asking for further
  approval. Keep work branches local; do not push feature branches or create
  GitHub branches or PRs. Never merge through GitHub.
- Deploy only when explicitly instructed. This includes pushing changes that
  trigger automatic deployment; get explicit deployment instructions before
  pushing those changes.
- Remote: `git@github.com:JazykOrg/Jazyk.git`. Relevant `site/` or site-workflow
  changes on `master` publish jazyk.org. Engine-only pushes do not deploy or start CI.
- Keep secrets out of tracked files. `.env`, `*.env`, `/bootstrap/target/`, and
  `**/.jazyk/` are ignored. Only project configuration (`.jazyk/jazyk.toml`,
  `.jazyk/.gitignore`) is tracked. Stage existing `.jazyk/` files with `git add -u`
  (plain `git add` stages but exits 1); new project configuration needs `git add -f`.
- Project discovery walks up to `.jazyk/jazyk.toml`. Run dogfood at repository root;
  its graph is ignored under `.jazyk/ds_main/`. Set IDs are minted once (slug + five
  random characters); older `ds_` IDs remain valid.

## Build and validation

Follow [repository validation](docs/main.md#repository-validation). From `bootstrap/`:

```sh
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo check --locked --target wasm32-unknown-unknown --lib
cargo build --locked --release
```

- Engine changes require tests and formatting; portable changes also require WASM
  checking. Helper changes require `go test ./...` in `runtime-worker/`.
  Workflow/docs-only changes need relevant syntax, trigger/input, and link checks,
  not engine rebuilds. Rust is pinned in `bootstrap/rust-toolchain.toml` (1.99.1,
  with `wasm32-unknown-unknown`).
- After ANY binary change, finish with a release build: the owner's `jazyk` symlinks
  to `bootstrap/target/release/jazyk`. Restart running `jazyk gui` to use it.
- Prevent stale cross-worktree test binaries: use a private `CARGO_TARGET_DIR` or
  distinct `--config 'profile.dev.package.jazyk.codegen-units=<n>'` per worktree.
- Editor build: `cd ../jazyk-cloud/web && npm run build`. Never commit `dist/`.
  `compiler.lock.json` pins compiler input; `JAZYK_SOURCE` is a non-CI local override.
  Editor tests/publication belong to Cloud; engine builds need no editor checkout.
- Relevant PRs run CI automatically; `master` pushes do not run engine CI. Do not
  wait for or dispatch replacement CI for routine tasks. Hosted CI needs an owner
  request or explicit release-validation authority.
- Default CI uses Rust's test profile. Manual `release_validation=true` builds and
  tests with `--locked --release`; both modes check formatting, WASM, and Go.
  Report exact run SHA, URL, conclusion, and inputs. Label local evidence as local.
  CI dispatch deploys neither site, editor, nor service.

## Commands and LLM configuration

See [CLI reference](docs/frontends/cli.md) for all commands. Bare `jazyk` opens the
launcher; short names such as `claude` and `vscode` are commands too. Key tools:
`compile`, `check`, `status`, `preview`, `explain`, `ripple`, `gen`, `test`, `gui`,
`mcp`, `acp`, `lsp`, `benchmark`, and task/run commands.

LLM precedence per field: CLI → env (`JAZYK_LLM_BASE_URL`, `JAZYK_MODEL`,
`JAZYK_API_KEY`) → project `[llm]` → `~/.jazyk/config.toml` → default. Repo `.env`
uses LocalRouter at `http://127.0.0.1:3625`; global config picks `gemma4:e4b-mlx`.
Use LocalRouter; do not override the endpoint unless asked. `JAZYK_TEMPERATURE`
(negative omits), `JAZYK_VERBOSE`, and `JAZYK_PLANTUML` tune execution.

Always run `jazyk benchmark` before trusting a new model. Local 4B-class models
fail capability checks; gates protect graph integrity but not judgment quality.
Local models are slow; start with `bootstrap/example/f1`. A local endpoint runs one
session at a time by default (`max_sessions`).
