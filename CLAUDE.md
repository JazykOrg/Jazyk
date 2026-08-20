# Jazyk

Jazyk is a natural language compiler. It treats prose documentation as the source code of a
program. Instead of constraining English, the compiler maintains a persistent semantic graph
(entities, EARS requirements, derived relationships, sticky diagnostics) reconciled against the
docs by LLM turns, surfacing ambiguity and contradictions along the way. Downstream usages
(code generation, test generation, project management, agent retrieval) consume the graph.
"Jazyk" means tongue/language in Slavic languages.

Status: research project; large changes in direction are acceptable and expected.

- Canonical trees: `docs/` (the design, also the dogfood input) and `bootstrap/` (the Rust
  implementation, binary `jazyk`).
- The v1 multi-step compile/link design (previously `docs/` and `bootstrap/`, then archived as
  the current trees took the names `docs2/` and `bootstrap2/`) failed in practice and was
  removed; see git history around "Failed POC". Do not resurrect it.
- `site/` hosts the jazyk.org static site and its deploy workflow.

## Architecture (turn-based reconciliation)

The compile/link pipeline is gone. One persistent graph per project, edited in place, never
regenerated. Three runtime components (see `docs/compiler/compiler.md`):

- Graph MCP server (`jazyk mcp graph [--write]`): the tool registry served over stdio. Read
  tools (context, expand, search, read_section, get_entity) are public; write tools mutate the
  graph behind validation gates.
- Turn harness: one focused LLM session wired to the same registry (native tool calls, or a
  JSON-action text codec for models without tool support; probed and sticky per run). Write
  tools stage mutations; `done` runs batch gates; changesets commit atomically.
- Reconciler: computes the dirty set from section-tree diffs (deterministic; moves rewrite
  references mechanically), schedules `reconcile-doc` turns in link-graph levels (root first),
  then grouped `review-entity` turns, then deterministic checks (coverage, reachability, flip
  detection). Convergence = fixed point + clean checks, under a hard budget; leftovers park
  and the next build resumes them. A no-op rebuild makes zero LLM calls.

Division of labor is strict: the harness owns identity (ids minted once, immutable; merges
leave redirects), the dirty set, validation, derived relationships (a product of requirement
`edges` only; no write tool), and context assembly (deterministic, budgeted, expansion
handles). The model owns extraction, same-vs-different judgment (search before create),
severity, and coverage marking (covered | non-normative). Provenance is verbatim quotes
located whitespace-insensitively, never char offsets. Declarative prose states obligations:
turns rephrase it into EARS, keep the quote verbatim (`docs/compiler/concepts/ears.md`).

## Repo layout

- `docs/main.md`: front door. `docs/compiler/`: compiler.md, parsing.md, model.md +
  `model/`, graph.md, context.md, turns.md + `turns/` (one page per task kind:
  goal, prompt, pack template, toolset), compilation.md (the build lifecycle),
  reconciler.md (dirty set, scheduling, task queue), control-plane.md (modes,
  releases, workers, leases), tools.md, `concepts/`, project-settings.md, schemas
  (draft-07 JSON Schema in YAML, `$id` `https://jazyk.org/schemas/*.json`).
- Prompt payloads live in `docs/compiler/turns/prompts/` and are embedded into the
  binary at compile time (`include_str!`), so docs and code share the same bytes by
  construction. Edit the files, never reintroduce string constants in code. They
  are excluded from the docs glob (they are instructions to a model, not prose
  about jazyk). Placeholders: `{target}` (worker-protocol, pointers), `{GROUP}`
  (generate-contract).
- `docs/frontends/`: cli.md, mcp.md, lsp.md, viewer.md. `docs/consumers/`: gen.md
  (generation + verification ledger), pm.md, docsgen.md. `docs/benchmark/`: benchmark.md,
  cases.md, `cases/` (the case files are embedded into the binary at compile time; they are
  fixtures, excluded from the docs glob).
- `docs/jazyk.toml`: the live project file (docs glob, roots, lint rules). The graph lands in
  `docs/jazyk-out/` (gitignored): `graph/*.yaml` shards, `docs/` section trees + coverage,
  `journal/`, `status.yaml`.
- `bootstrap/src/`: model.rs, store.rs (shards, natural-key upserts, atomic commit, journal,
  GC), context.rs, tools.rs (the registry), turn.rs (codecs, prompts), reconcile.rs, llm.rs
  (OpenAI-compatible client over ureq; sticky fallbacks for tools/temperature/streaming),
  md.rs, project.rs, cli.rs, gen.rs + verify.rs (generation ledger, verification statuses),
  docsgen.rs, viewer.rs, mcp.rs, lsp.rs (read-only), benchmark.rs, jsonrpc.rs,
  parallel.rs. `bootstrap/editors/vscode`: LSP client extension. Deps: serde, serde_json, serde_norway, ureq (HTTP), notify (file
  events). Dependency policy (owner decision, 2026-07-06): infrastructure comes from
  crates; hand-roll only domain logic. Do not reimplement transports, parsers for
  standard formats, or platform APIs.
- `bootstrap/example/f1` and `f2`: fixtures (f2 has planted traps, see its EXPECTED.md).
  `bootstrap/VALIDATION.md`: measured results, scorecard, known weaknesses.

## Build and commands

- `cd bootstrap && cargo build --release` (binary at `bootstrap/target/release/jazyk`),
  `cargo test`.
- The owner's `jazyk` command is a symlink to `bootstrap/target/release/jazyk`. After
  ANY change to the GUI frontend or the binary, always finish with
  `cd bootstrap/gui && npm run build` then `cargo build --release`: the built frontend
  (`gui/dist`) embeds into the binary at compile time, so a stale release build serves
  the old GUI. A running `jazyk gui` keeps its embedded assets; it needs a restart to
  pick up a new binary.
- `jazyk compile [path...]` (live trace; `--verbose` full packs, `--quiet` summary),
  `check`, `watch`, `status`, `context <target>`, `query <text>`, `gen [entity...]`,
  `test [--audit]`, `docsgen`, `viewer`, `mcp graph [--write]`, `lsp`, `benchmark`.
- A project is a directory with `jazyk.toml` (walk-up discovery). Run the dogfood from
  `docs/`.
- Always run `jazyk benchmark` before trusting a new model: it grades turn capability per
  codec with deterministic checks. Local 4B-class models fail it; the harness still holds
  (gates bounce bad calls, junk never lands), but judgment-quality output (reviews, lint)
  degrades.

## LLM config

Precedence per field: CLI flag → env (`JAZYK_LLM_BASE_URL`, `JAZYK_MODEL`, `JAZYK_API_KEY`) →
project `[llm]` → `~/.jazyk/config.toml` → default. The repo `.env` points
`JAZYK_LLM_BASE_URL` at LocalRouter (`http://127.0.0.1:3625`), which proxies local Ollama
models and remote providers; the global config picks the model (`gemma4:e4b-mlx`). Use
LocalRouter; do not override the endpoint unless asked. Tuning: `JAZYK_MAX_CONCURRENCY`,
`JAZYK_MAX_RETRIES`, `JAZYK_TEMPERATURE` (negative omits), `JAZYK_VERBOSE`, `JAZYK_CODEC`
(force `native`/`text`). Local models are slow: test on `bootstrap/example/f1` first.

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
