# Document sets

A project contains independent named document sets. Each set has its own graph,
ledger, diagnostics, diagrams, and compile state. The project opens every set
together. Documents and source files keep their chosen locations.

## Project configuration

`.jazyk/jazyk.toml` describes the whole project. Its parent `.jazyk` directory is
metadata, never the project root. All document, source, root, handler, and tool
paths resolve from the project folder. Machine settings stay in
`~/.jazyk/config.toml`; that file never marks a project.

- `version = 1` versions the new configuration schema, independently of graph and
  editor protocol versions.
- `[[sets]]` is the only document-set form, including projects with one set.
  An empty set list is a configured project with no documents assigned.
- `id` is immutable and unique. `name` is required and unique within the project
  after trimming and case folding. Names can contain spaces and Unicode.
- `docs` is an ordered pattern list. It can be empty (a set with no documents yet,
  the first one added later) or temporarily match zero files, e.g. a newly selected
  empty folder. A list of exclusions alone is rejected. A warning names every
  positive pattern that currently matches nothing. It does not silently expand the selection.
- `roots` is an optional ordered list of entry-point globs over that set's effective
  documents. Report unmatched positive patterns. Omission retains the current
  no-roots behavior, including document scheduling in path order.
- `source` is an optional ordered pattern list, empty by default. Empty means no
  source mapping, not the whole project. A directory selected in the UI becomes
  explicit globs; it does not impose a folder layout.
- Project-level `[llm]`, `[acp]`, `[executors]`, `[workflow]`, and `[gen]` supply shared
  defaults, with corresponding `[sets.llm]`, `[sets.acp]`, etc. overrides. Linting
  uses `[linting.rules]` and `[sets.linting.rules]`; format handlers use
  `[handlers.<name>]` and `[sets.handlers.<name>]`. This keeps the `docs` path array
  separate from settings tables. Omitted values inherit; arrays replace as a whole
  and an empty array clears an inherited array. Named handler/agent entries merge
  by key, with an explicit disabled flag to remove an inherited entry.
- Model resolution keeps CLI → environment → set → project → global → default.
  Secrets remain outside committed configuration.

The example uses the minimum keys. The schema must also retain existing generation,
lint, handler, executor, and workflow capabilities under a resolved set. For this
release the deliverable root is the project folder; `source` selects its allowed
paths. An empty source selection disables bind/generate/verify work. Choosing
source does not release generation. Existing manual workflow gates still apply.
The old `[gen] deliverable` and `code` path settings are replaced by `source`.
`[gen] worker` remains. Required manifests or support files outside selected paths
need an explicit source selection before generation may write them.

Use maintained TOML parsing and editing crates. Typed edits preserve unrelated
settings, comments, and ordering. Do not extend the current handwritten TOML subset.
Every management edit supplies the config hash it read; a stale hash returns a
conflict with the current revision. Parsing, validation, preview, and commit use
one shared service across transports. Saves use atomic replacement.

## Discovery and opening

- CLI discovery walks up to the nearest `.jazyk/jazyk.toml`.
- GUI folder opening uses the selected folder as its boundary. If it belongs to an
  ancestor project, offer that project explicitly instead of silently showing files
  outside the selected folder. An explicitly created nested project is allowed.
- LSP supports each declared workspace folder and its discovered project. It also
  reacts when a new config is saved in an initially unconfigured workspace.
- A project does not automatically import nested projects. Its matchers skip nested
  `.jazyk/jazyk.toml` project boundaries, including under broad recursive globs.
  Opening the nested folder makes it a separate project.
- Reading an arbitrary folder creates no config, graph, or starter documents.
  `Add document set` saves the first config. `jazyk init` may explicitly create an
  empty project. Canceling setup leaves the folder untouched.
- A request that needs a document set, asked of a project that has none (a new
  folder, or a project still carrying only a root `jazyk.toml` from before document
  sets), answers `409` with `no-document-set: this project has no document set yet`
  and says to add one under Document sets. It is a state of the project, never a
  server failure: every backend (native, WASM, hosted) answers it the same way, and
  file browsing keeps working.
- A project with no `.jazyk/jazyk.toml` whose root `jazyk.toml` still carries the
  paths from before document sets (`[docs] glob`, and `[roots] files`) gets them back
  as a suggestion: the `/api/sets` summary carries `suggested: {name, docs, roots}`,
  and Add document set opens prefilled with it. Nothing is written until the person
  saves the set; the root file is never read as configuration.
- Invalid config keeps file browsing and config editing available. Show the parse
  error and suspend operations that require its mappings. Do not use stale mappings
  for writes while silently falling back to the last valid config.

## Matching and file roles

One engine resolver computes membership for the explorer, compiler, source tools,
watchers, LSP, and ACP. Clients do not approximate globs or infer membership from
folder names.

- Exact file paths and globs can be mixed. `!` excludes. Later matches override
  earlier matches, as in current docs matching. Matches are deduplicated.
- The folder picker expands a documents folder to supported Markdown globs, and a
  source folder to `path/**`. An advanced editor exposes the actual patterns and
  exclusions. A literal path picker escapes glob metacharacters.
- Normalize paths relative to the project. Reject absolute paths, traversal outside
  it, and symlinks escaping it. External folders require a later mount contract;
  choosing a common parent project is the initial way to include sibling folders.
- Compiler state, archives, `.git`, build caches, and existing mandatory exclusions
  remain ineligible. An explicit unsupported doc file gets a format error, never a
  successful compile with zero content. Config is a project file, not doc input.
- A document can belong to several sets. Preview lists these overlaps and explains
  that compilation and cost occur separately. Saving deliberately retains them.
  Editing that file dirties every claiming set.
- Source overlaps are normal. A source badge states scope, not proof that a
  requirement implements or owns the file. The ledger remains that evidence.
- Files claimed as documents by any set are excluded from source write scopes in
  every set. Broad source patterns cannot authorize generation over another set's
  prose. Show these exclusions in the preview rather than hiding them.
- Membership returns all `{setId, role, matchedPattern}` entries. A folder has an
  explicit selection marker when its rule selects descendants. A mixed parent has
  a descendant summary; it is never marked as if all its files shared one role.

Config changes preview added/removed documents and source files for each affected
set. Removing membership never deletes the file. Removing a document triggers that
set's normal deleted-document reconciliation. Source membership changes invalidate
affected bindings and checks. They never silently preserve a clean verdict.

## Persistence and identity

```text
.jazyk/
  jazyk.toml
  .gitignore
  project.lock                 project operation coordination
  sessions/                    project-wide user conversations
  runs/                        grouped job records
  removed-sets/                 retained definitions for local recovery
  <set-id>/
    graph/ docs/ journal/ diagrams/ docsgen/ gen/ trace/ sessions/
    status.yaml control.yaml workers/ leases/ .lock
```

- Each set lives directly at `.jazyk/<set-id>/` and owns the existing store shape
  and its worker session history. Set IDs require the `ds_` prefix, keeping them
  distinct from project directories such as `sessions`, `runs`, and `removed-sets`.
  The project owns conversations that discuss several sets and records grouping jobs.
- A native project operation explicitly unlocks when its guard ends. A duplicated
  descriptor inherited by a concurrently starting child cannot prolong that lock.
- A set rename changes only `name`. It preserves IDs, paths, graph, history, and
  bindings. Copying a set definition mints an ID and starts an empty graph.
- Project-level references carry `setId` and `localId`. JSON graph identifiers use
  `<kind>:<setId>::<local-tail>` on the wire. The kind prefix stays unchanged.
  The API decodes these identifiers before accessing a store. Local identifiers
  on disk stay unchanged. Explicit `setId` and qualified identifiers must agree.
  Node, goal, generation, trace,
  diagnostic, and diagram IDs are only unique inside their store. Search results,
  caches, deep links, and events must carry the set ID. Existing graph IDs stay as-is.
- Remove set saves its definition in `removed-sets/<id>.toml`, detaches it from
  config, and retains its store. Restore validates the saved paths/name and attaches
  the same ID; files changed while detached are dirty on restore. A separate purge
  action explicitly deletes retained state and the saved definition. Neither
  action deletes docs or source. Removed IDs cannot be silently reused for an
  unrelated set. An externally deleted definition leaves an orphaned store visible
  for recovery; do not infer new memberships from it.
- Archive/reset operations target a set store, never the enclosing `.jazyk` folder.
  A graph-version mismatch must not archive the config or other sets.
- All graph changes retain existing provenance and changeset gates. A project
  operation spanning sets is several identified commits, not a cross-store atomic
  transaction. Its result reports partial completion honestly.

Track `.jazyk/jazyk.toml` and `.jazyk/.gitignore`. Generated state stays ignored.
The generated ignore file can contain `*`, `!jazyk.toml`, and `!.gitignore`.
Existing repository rules that ignore `.jazyk` itself must be removed or narrowed;
an ignore file inside an ignored parent cannot make config trackable. Setup reports
that conflict. The repository conversion fixes its own rules and verifies with
`git check-ignore`. Worktrees keep separate local state.

## Dirty state and execution

The reconciler continues to operate on one resolved set. Project orchestration
opens stores as needed, assembles summaries, routes operations, and serializes jobs.
Opening a project loads every definition and summary, not every graph into an agent
prompt. Graph access stays explicit and subject to existing context budgets.

- Each summary has separate execution state, compile-needed status, convergence
  verdict, source-evidence freshness, and blocked/error counts. “Up to date” does
  not imply “no problems”. “Source changed” does not imply “docs changed”.
- The all-dirty compile action includes new sets with documents and sets with
  pending runnable compile work, including invalidated ledger work allowed by their
  workflow gates. Sets awaiting only a human response are listed as blocked.
  Empty sets appear as `No documents`, not as successfully compiled.
- The engine refreshes disk and goal state before selecting dirty sets. Browser
  backends refresh their authorized filesystem mirror using the same UI action.
  Clients drain pending saves first. A failed save prevents submission against old
  text.
- A grouped run records selected set IDs and config revision. It runs one set at a
  time, in configuration order, with one model session at a time. A project-wide
  build lease also applies to CLI, ACP, and external workers.
- Recheck a queued set before it starts. Skip a set that became clean. New dirty
  sets are left for the next explicit run; watch mode may enqueue a follow-up.
  Edits during a build retain the existing incremental reconciliation behavior.
- Starting the same set twice coalesces pending work by set ID. Different sets
  must not coalesce as one single-project job. Cancellation stops the current set
  through existing cancellation and removes the remaining children. Already
  committed work survives. A failed or human-blocked set does not prevent later
  independent sets from running. A project-wide failure, e.g. lost folder access,
  stops the group.
- Running work pins its mapping revision. Membership/removal edits affecting a
  running or queued set return a busy result. The user can cancel it, then save.
  External config edits cause safe cancellation before any new write uses changed
  mappings, followed by revalidation. Name-only changes can apply immediately.

Source writes must check current content hashes and allowed paths at write time.
After a shared source file changes, every set with relevant bindings loses its
previously fresh evidence. Project serialization prevents simultaneous Jazyk writes;
it does not prove two sets agree semantically. Rebinding and verification establish
their respective results. Detect stale writes from external editors too. There is
no “last compiled set wins and every other set stays green” behavior.

## API and agents

The portable API is project-scoped for file IO and set management, and explicitly
set-scoped for graph operations. The surface:

| Operation | Scope |
|---|---|
| Project descriptor and sets list | All definitions, capabilities, summaries, config revision |
| Preview configuration edit | Resolved paths, overlaps, exclusions, affected sets; no writes |
| Create/update/remove set | Typed config patch with expected revision |
| Project file tree/content | One physical file entry, every membership, shared save hash |
| Search/status/problems/history | All sets by default; results carry set identity |
| Graph read/write, answer, preview, generate, verify | Required `setId` and local target |
| Compile job | Exactly one of `setIds: [...]` or `scope: "dirty"` |

Use `/api/sets` for management and `/api/sets/{setId}/...` for graph routes.
Project aggregates use project routes with optional set filters. Project-level
events carry no set; set events and child jobs always carry one. Event cursors are
project-wide while graph generations remain per set. On reconnect, refresh all
affected summaries rather than comparing generations across stores.

The editor protocol is version `2`. It carries explicit document-set scope. Native and WASM expose the same contract. A newer editor can retain an
explicit legacy single-set adapter, but must not send multi-set writes to an older
engine. Check protocol/capabilities before replacing the active project or drafts.

ACP chat and MCP serving start with all set summaries and project tools. Provide
`list_document_sets`, `preview_document_set`, `create_document_set`,
`update_document_set`, `remove_document_set`, and `compile_document_sets` through
the same service. The agent can search across sets and perform successive actions
in different sets within one conversation. Every graph tool includes its set ID.
There is no hidden `/switch-set` prerequisite. Ambiguous write targets return the
candidate sets rather than guessing from the focused editor tab. Compiler worker
sessions remain restricted to the set whose batch they resolve.

## CLI and LSP

E.g.:

```sh
jazyk init
jazyk sets list
jazyk sets add feature-xyz --docs docs/xyz.md --docs 'docs-xyz/**/*.md' --source 'src/**'
jazyk sets preview feature-xyz
jazyk compile --set feature-xyz
jazyk compile --dirty
jazyk status
```

Names resolve to immutable IDs. Flags also accept IDs. No-selector aggregate reads
show all sets. Bare compile means all dirty sets. Mutations such as generate, bind,
decompile, and answering a diagnostic require an explicit set unless exactly one
exists. Path-limited compile resolves all sets claiming those paths and prints the
targets; it does not silently create sets. `--out`, if retained, requires one
explicit set and cannot redirect into another set's store or the project config.

LSP indexes all sets for every opened project. Diagnostics carry a readable set
name in `source` and stable project/set/diagnostic identity in `data`. Hovers and
references group results by set. Duplicate local IDs cannot overwrite each other.
Document changes refresh all claiming sets, and source changes refresh relevant
bindings. Clearing one set's findings preserves the other sets' diagnostics.

Add explicit `workspace/executeCommand` actions for list/manage/compile with typed
project/set arguments. File/folder context menus in an IDE extension call these
same commands. Standard code actions expose applicable file operations; a generic
LSP client need not implement Jazyk's tree decoration UI. Mutations use the same
preview and revision checks as the GUI. They are explicit actions, never side
effects of hover or diagnostic requests. This expands today's mostly read-only LSP
contract and must be documented as such.

## Existing projects

The runtime reads only `.jazyk/jazyk.toml`. It does not backfill old projects,
convert root configs, or migrate graphs. Repository examples and bundled demos
are converted manually. Opening an old folder leaves its files unchanged and
shows setup until the user defines document sets.

A graph reset archives only the selected set's store. Configuration and other
sets remain in place.

## Native request contexts

Each request resolves its document set from the current configuration. Graph
responses qualify identifiers, including references and map keys. Project file
paths remain physical paths. Jobs record their set and configuration revision
when queued. A changed configuration cancels a queued job before it writes.
A group compile visits the selected sets in configuration order. Cancellation
stops the current build at its next boundary and skips the remaining sets.

## Generated file navigation

Project output listings prefix each relative path with its set ID. Output reads
resolve that prefix before entering the set's store. Rendered diagram links carry
the same identity. Shared source-file reads combine ownership across claiming sets.
