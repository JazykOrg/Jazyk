# Document sets: implementation sequence

Status: proposed. No runtime changes are included in this planning branch.

## Baseline and ownership

The current `bootstrap/src/project.rs::Project` combines root, document globs, one
output store, and generation settings. `gui/state.rs::AppState` holds one `Project`,
`out`, and `GenSettings`. `lsp.rs::Lsp` starts with one root/store. Those are the main
single-set assumptions to separate; adding a picker alone cannot implement this.

The engine repository owns project/set configuration, matching, graph routing,
jobs, portable API, native/WASM behavior, CLI, ACP, LSP, and MCP. The private
`jazyk-cloud` repository owns the existing editor, browser persistence adapters,
hosted storage adapters, tests, and publication. Accountless local and browser use
remains required. The compiler must build without the private checkout.

## 1. Land the contracts

Update the engine's canonical docs before its code:

- `docs/compiler/project-settings.md` and its schema: the new config path and shape,
  discovery, defaults, source scopes, and matching.
- `docs/compiler/compiler.md`, `graph.md`, `compilation.md`, `control-plane.md`,
  `sessions.md`, and `context.md`: one graph per set, project coordination, aggregate
  references, and bounded context loading. Keep reconciler semantics per set.
- `docs/consumers/gen.md`, `bind.md`, and `decompile.md`: source selection, shared
  file evidence, explicit targets, and write boundaries.
- `docs/frontends/cli.md`, `gui.md`, `protocol.md`, `wasm.md`, `acp.md`, `lsp.md`, and
  `mcp.md`: unconfigured projects, management, scoped actions, grouped runs, and
  protocol compatibility. Update embedded prompt/skill payloads where they assume
  one project graph or global target IDs.
- `AGENTS.md`, dogfood instructions, example configs, and ignore rules: the new
  config and store locations. Validate every relative link and heading anchor.

In the private repository, update `docs/editor/editor.md`, `browser.md`, and
`document-links.md` from the [experience plan](./editor.md) before UI changes.
Update hosted project/runtime contracts where project-scoped storage previously
assumed one graph. Do not duplicate UI specifications into engine `docs/`.

## 2. Separate project from resolved set

- Introduce a project descriptor and a resolved set context. Pass the latter to
  the current compiler machinery. No graph store merger or new linker.
- Replace subset TOML parsing/editing with crate-backed handling. Add schema
  validation, atomic revision-checked management, discovery, and memberships.
- Allocate `.jazyk/<id>` stores. Split project sessions/grouped runs from
  worker sessions. Fix reset/archive/delete boundaries before any cleanup runs.
- Convert owned fixtures and project configs. Preserve legacy stores until a
  deliberate reset or compatible relocation. No graph migration code.
- Make CLI list, add, preview, named compile, dirty compile, and aggregate status
  work end to end before adding the UI.

Acceptance: two disjoint sets compile independently; renaming one keeps its graph;
resetting one cannot move/delete config or another graph. Membership preview and
actual compiler input agree for exact files, disjoint folders, and exclusions.

## 3. Route operations and schedule grouped work

- Change `gui/state.rs`, `gui/jobs.rs`, watchers, and control-plane entry points
  to route through project/set contexts. Do not implement mutable global selection.
- Carry set identity through `webapi/`, `gui/`, WASM bridges, events, deep links,
  trace retrieval, and output reads. One physical-file save updates all memberships.
- Derive summary states and grouped runs on the backend. Extend job coalescing to
  use set IDs. Serialize native, browser, CLI, and agent work at project scope.
- Apply source scope and content-hash checks to generated writes. Invalidate
  bindings in all affected sets after shared source changes.
- Introduce the new editor protocol and compatibility tests before publishing a
  client that depends on it.

Acceptance: all-dirty compiles only eligible sets, a blocked/failed child does not
hide others, cancellation preserves committed work, and colliding local IDs remain
distinct in every read/write/cache/event path.

## 4. Expose the same project to agents and IDEs

- ACP project tools and MCP serving list and address every set in one connection.
  Worker sessions stay constrained to their assigned set.
- LSP handles workspace-folder changes, multi-set diagnostics, and typed explicit
  management/compile commands. Update the VS Code extension for menu actions and
  membership decorations without making it a required client.
- Replace per-project target assumptions in session persistence, prompt context,
  slash commands, and graph handles. A bare ambiguous local ID cannot mutate a graph.

Acceptance: one conversation lists both features, edits settings for each, compiles
one, then queries the other. One LSP server reports findings from both, clears only
the resolved set's findings, and compiles an explicitly chosen set.

## 5. Build the editor flow

Use a separate worktree in `jazyk-cloud` for implementation. Preserve the current
Tabs/Board, editor buffers, project menu, chat, and local/browser persistence.

- Open unconfigured folders without initialization. Implement the add-set form,
  match preview, new Markdown creation, and conflict recovery.
- Add Document sets and Files membership decoration from engine responses.
  Implement mark/exclude menus, multi-select, and management pages.
- Add the compile split button, named progress, cancellation, retry, and all-dirty
  default. Keep save flushing and backend/project identity in asynchronous callbacks.
- Scope graph caches/routes/items by project and set. Keep physical file buffers
  keyed by project and path so overlapping membership cannot fork unsaved edits.
- Persist the nested stores and project metadata in browser exports, folder mirrors,
  saved projects, and hosted storage. Test reopening without dropping any set.
- Update `compiler.lock.json` and the hosted compiler pin only to the accepted
  engine revision. Local overrides remain development-only. Show the integrated
  application locally before any separate publication.

Acceptance: perform the user's complete flow with no TOML editing, first with a
browser folder backend and then a local native API. Repeat with No AI, a failed
save, a shared file, and a reload during a queued run.

## Acceptance matrix

| Scenario | Required evidence |
|---|---|
| Arbitrary folder, cancel Add | No `.jazyk` created; all files remain browsable. |
| First set: two files plus two folders | Preview and compile use exactly the same matched documents. |
| Second feature in the same docs folder | Both remain visible; each has an independent store and verdict. |
| Shared source | Both sets can read it; changes invalidate relevant evidence in both. |
| Shared document | One buffer, two memberships, both dirty after a save. |
| Broad source includes another set's docs | Docs are excluded from all generated write scopes. |
| Compile checkout with search dirty | Only checkout runs; search remains visibly dirty. |
| Compile all dirty, one fails | Remaining sets run; per-set outcomes and group failure are explicit. |
| No-op compile | No new goals or model calls. |
| Duplicate IDs inside separate graphs | Routes, events, caches, diagnostics, and answers stay correctly scoped. |
| Rename/remove/reset one set | Other sets and config survive; docs/source are never deleted. |
| Config changed by editor and agent | Stale edit conflicts; unrelated fields/comments are retained. |
| Dirty save fails before Compile | No job compiles stale text. |
| Source changes outside Jazyk during generation | Hash conflict prevents an unchecked overwrite. |
| Folder permission fails or backend switches | Drafts survive; a late response cannot write/navigate the next project. |
| Restart, export/import, native/WASM parity | All definitions, graphs, and references reopen with their identities. |
| Existing root config or mixed old/new configs | Visible conversion/conflict state, no automatic graph reset. |
| IDE with two workspace folders | Project identity as well as set identity routes every action. |

Use focused resolver/service tests, integration tests with deterministic model
fixtures, and private-repo browser tests for these behaviors. Run the existing
Rust suite and a release build after binary changes. Run the WASM build and protocol
parity tests for the portable surface. No model-powered dogfood run is required to
validate this planning change.

## Later import work

After the complete Markdown flow works, specify the import manifest and adapter
contract, choose maintained Word/PDF converters, and test conversion quality on
tables, headings, images, scans, and repeated imports. This is a separate milestone,
not a dependency of multi-set configuration or compilation.
