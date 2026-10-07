# Plan: workspaces, specs, and mappings

Status: historical proposal. Written 2026-09-25. Superseded on 2026-10-07 by
[document sets in one project](./document-sets/README.md) for configuration,
independent compilation, and frontend scope. In particular, use
`.jazyk/jazyk.toml`, stable set IDs, and simultaneous project-wide visibility from
that plan. The root config, implicit specs, and frontend switching proposed below
are not the implementation direction. Retain the remaining cases as background.

Read with [project settings](../docs/compiler/project-settings.md), the
[storage layout](../docs/compiler/graph.md#storage-layout),
[generation](../docs/consumers/gen.md), [binding](../docs/consumers/bind.md),
[decompilation](../docs/consumers/decompile.md), and the
[browser backend](../docs/frontends/wasm.md). The plan in
`plans/upstream-reconciliation.md` (propagating downstream edits back into docs) is
written separately; it depends on the mapping model here to answer "which docs own this
file", and this plan does not repeat it.

## The owner's ask

Make it easy to map documents onto source code, get docs out of source code, or a
subset of source into a document: every combination. One codebase may carry three
documents describing three features, and they cannot share one `jazyk-out`. The name
`jazyk-out` should go, maybe to a dot folder. Docs and source may sit in separate
folders. jazyk.dev may open a folder and apply a fresh document to it. Decide where the
IR lives, where the config lives, and how several configs coexist.

## Today

What exists, in one list, so the gaps below are concrete:

- A project is a directory holding `jazyk.toml`. Discovery walks up to the nearest one
  ([project discovery](../docs/frontends/cli.md#project-discovery)). A file holding
  only `redirect = "docs"` delegates to a nested directory, and a redirect above the
  working directory is a boundary, not a capture
  ([redirect](../docs/compiler/project-settings.md#redirect)).
- One project has one doc set (`[docs] glob`, `[roots] files`), one deliverable
  (`[gen] deliverable`, default `.`), one code scope (`[gen] code`, relative to the
  deliverable), and one store.
- The store is `<root>/jazyk-out/`, or `--out DIR`. It holds the graph, the section
  trees, the journal, the diagrams, docsgen, the ledger, traces, chat sessions, the
  control plane, leases, feedback, and the lock
  ([storage layout](../docs/compiler/graph.md#storage-layout)). A store version
  mismatch archives the whole directory to `<out>.bak`
  ([store version](../docs/compiler/graph.md#store-version)).
- Doc input never includes the out directory, anything named `jazyk-out*`, hidden
  directories, `target`, or `node_modules`
  ([glob](../docs/compiler/project-settings.md#glob)). The deliverable is excluded by an
  implicit leading `!<deliverable>/**` that the glob may whitelist back.
- Explicit `jazyk compile a.md b.md` outside a project runs ad hoc at the working
  directory, store at `<cwd>/jazyk-out/`.
- The ledger (`gen/ledger.yaml` in the store) records file ownership per entity and
  anchored sites per requirement; the deliverable carries no jazyk metadata
  ([traceability](../docs/consumers/gen.md#traceability)).
- The browser backend reads a picked folder once (skipping `.git`, `node_modules`,
  `target`), requires `jazyk.toml` at its top, and mirrors writes back
  ([a folder on disk](../docs/frontends/wasm.md#a-folder-on-disk)).
- `bootstrap/src/project.rs` resolves all of this with a hand-rolled TOML reader that
  flattens `[a.b]` headers into dotted keys; it has no inline tables and no arrays of
  tables. `cli.rs::resolve` hard-codes `root.join("jazyk-out")`. Fifty-one code sites
  and seventeen doc mentions name `jazyk-out`.

The examples cover two shapes only: docs and deliverable under one root
(`example-sort` with `code/`, `example-erp` with `project/`, `f2` with `product/`,
`example-slides` with `deliverable/`), and the dogfood's redirect (`jazyk.toml` at the
repo root pointing at `docs/jazyk.toml`, whose deliverable is `../project2`).

## Use cases

Each case names the user, what they do, what jazyk must know (which docs, which source
subset, where the IR lives), and what breaks today.

### The matrix

| # | Docs | Source | Direction | Case |
|---|---|---|---|---|
| 1 | one set | none | compile | docs-only project (novel, handbook, slides) |
| 2 | one set | new | docs → source | greenfield generation |
| 3 | one set | existing, whole | bind, verify | adopt a codebase with written docs |
| 4 | none | existing, whole | source → docs | decompile a whole codebase |
| 5 | none | existing, subset | source → one doc | document one module or directory |
| 6 | many sets | one | many → one | three feature specs over one codebase |
| 7 | many sets | one, overlapping | many → one | two specs constraining one module |
| 8 | one set | many | one → many | frontend and backend repos, monorepo services |
| 9 | one set | separate folder or repo | any | docs repo documenting a code repo |
| 10 | one new doc | existing folder | apply | jazyk.dev: open a folder, write a doc, apply it |
| 11 | one set | existing, partly read-only | bind, generate | vendored or third-party code |
| 12 | one set | mixed generated and hand-written | generate | generated code beside hand-written code |
| 13 | ops or security set | existing, whole | verify only | a runbook or security spec judged against code |
| 14 | one set | existing, subset | docs → source | a feature spec that may only touch its module |

### 1. Docs-only project

- User: an author of a novel, an org handbook, a slide deck.
- Does: writes prose, compiles, reads diagrams, maybe generates a manuscript.
- Jazyk must know: the doc glob, the roots. No source, or a deliverable directory that
  generation fills (`manuscript/`, `handbook/`).
- Breaks today: nothing functional. `jazyk-out/` appears in the author's folder as a
  visible, oddly named directory next to their chapters, and without a `.gitignore` it
  gets committed.

### 2. Greenfield generation

- User: a developer starting from a design document.
- Does: writes docs, compiles, runs `jazyk gen`, runs `jazyk test`.
- Jazyk must know: the docs, one deliverable directory (writable), the store.
- Breaks today: nothing. This is the shape the defaults serve (`example-sort`).

### 3. Adopt a codebase with written docs

- User: a team with a codebase and a design doc in `docs/`.
- Does: adds `jazyk.toml`, compiles, lets `bind` classify every requirement
  ([adoption](../docs/consumers/bind.md#adoption)).
- Jazyk must know: the docs glob, the deliverable (the repo root), which files are
  implementation (`gen.code`), the store.
- Breaks today: nothing essential. The store name is the complaint.

### 4. Decompile a whole codebase

- User: a maintainer inheriting an undocumented service.
- Does: `jazyk init`, `jazyk decompile`, reviews drafts.
- Jazyk must know: the source (the deliverable), where drafts land (a path the docs
  glob includes).
- Breaks today: the default glob is `docs/**/*.md`, so drafts land there; fine. A repo
  whose existing `README.md` files are all over the tree gets them compiled only if
  the glob says so.

### 5. Document one module

- User: a developer who owns `src/billing/` in a large repo and wants one document for
  it, without adopting the whole repo.
- Does: runs decompile on the module, gets `docs/billing.md`, compiles only that.
- Jazyk must know: a doc set of one file, a source subset (`src/billing/**`), a store
  that is not the repo's main one.
- Breaks today: the project is the repo. `gen.code` could scope the unclaimed report to
  the module, but that edits the one shared `jazyk.toml` and the one shared store, so
  the module's IR and the whole repo's IR cannot both exist. `jazyk decompile
  src/billing` drafts one document per scope (a directory), not one document for the
  subset.

### 6. Several feature specs over one codebase

- User: a product team writing `specs/checkout.md`, `specs/search.md`,
  `specs/accounts.md`, each owned by a different person, each compiled and verified on
  its own schedule.
- Does: edits one spec, compiles it, binds it against the code, generates only its
  feature.
- Jazyk must know: three doc sets, three source subsets (or one shared source), three
  IRs, three ledgers, and which files each may write.
- Breaks today: one config, one store. Three `jazyk.toml` files cannot sit in one
  directory. Three nested directories each with a `jazyk.toml` each get a deliverable of
  `..`, and each store lands in its own `specs/<x>/jazyk-out/`, which works by accident
  but leaves ownership unguarded: two ledgers may both claim `src/lib.rs`, and each
  generation overwrites the other's work.

### 7. Two specs constraining one module

- User: a platform team (`specs/api.md`) and a security team (`specs/security.md`) who
  both state rules about `src/auth/`.
- Does: compiles both, binds both, wants to know when one spec's generation breaks the
  other's requirement.
- Jazyk must know: both doc sets, their IRs, that both bind `src/auth/**`, and who may
  write it.
- Breaks today: impossible without two projects, and two projects never see each
  other's ledgers.

### 8. One doc set, many sources

- User: a team whose design doc covers a web frontend (repo `web/`) and a backend (repo
  `api/`), or a monorepo with `services/orders` and `services/billing`.
- Does: compiles one graph, generates or binds each component into its own tree, with
  its own toolchain.
- Jazyk must know: one doc set, one IR, several source roots, and which part of the
  graph maps onto which root (the documents name the components: «system» Frontend,
  «system» Backend).
- Breaks today: one deliverable per project, and "one toolchain per deliverable"
  ([file ownership](../docs/consumers/gen.md#file-ownership-and-conventions)) forces one
  medium for both. Splitting into two projects compiles the same docs twice into two
  diverging graphs.

### 9. Docs and source in separate folders or repos

- User: a docs team whose repo `product-docs/` documents the code in `product/`,
  checked out side by side; or a single repo with `design/` and `src/` as siblings.
- Does: compiles in the docs repo, binds and verifies against the code repo, never
  writes into it.
- Jazyk must know: docs relative to the docs folder, source at `../product`, read-only.
- Breaks today: `deliverable = "../product"` works for reading and verification, but
  nothing marks it read-only, so `jazyk gen` would write into another team's repo.

### 10. A fresh doc applied to a folder (jazyk.dev)

- User: someone on jazyk.dev with a local folder (code, no jazyk config) and an idea
  for a change.
- Does: opens the folder, writes `rate-limits.md` in the browser, compiles it, sees
  what is `verified` and what is `unimplemented`, asks for generation.
- Jazyk must know: a doc set of one new file, the folder as source, a store that does
  not disturb the folder's existing specs if it has any.
- Breaks today: the folder must hold a `jazyk.toml`. The wizard writes one, which
  turns an experiment into a project. A second fresh doc overwrites the same store.
  Natively, `jazyk compile rate-limits.md` in a folder with no config runs ad hoc with
  the store at `<cwd>/jazyk-out/`; compiling `timeouts.md` next in the same folder
  reuses that store, sees `rate-limits.md` gone from the input, and deletes its facts.

### 11. Vendored or third-party source

- User: a team with `vendor/` or `third_party/` in the tree.
- Does: binds requirements against code that includes vendored libraries (a
  requirement may be met by a vendored parser), never edits them.
- Jazyk must know: vendored paths are in scope for bind and verify, out of scope for
  generate and for the unclaimed report.
- Breaks today: `gen.code` can drop them from the unclaimed report, but then bind
  does not see them either; nothing stops a generate session from editing them.

### 12. Generated and hand-written code in one tree

- User: a team with `src/` hand-written, `src/gen/` produced by `protoc`, and new
  modules jazyk generates.
- Does: generates new features, keeps hand-written code, never touches `protoc`
  output.
- Jazyk must know: three classes of file: jazyk may write (its own and new files), may
  read but never write (hand-written code outside the feature), out of scope (other
  generators' output).
- Breaks today: ownership only guards files already in the ledger. A hand-written file
  no ledger names is fair game for a generate session's path.

### 13. Verify-only specs

- User: an SRE writing an ops runbook ("every service exposes `/healthz`"), a security
  engineer writing a security spec ("passwords are hashed with argon2").
- Does: compiles the spec, binds it against the code, reads the trace matrix, never
  generates.
- Jazyk must know: the spec's docs, the whole source read-only, its own IR and ledger.
- Breaks today: same as case 6, plus no way to say "never generate".

### 14. A feature spec confined to its module

- User: a developer generating a new feature into an existing repo, who wants the
  generated files to land only under `src/features/export/` and its tests.
- Does: `jazyk gen` for the spec.
- Jazyk must know: the writable subset (`src/features/export/**`,
  `tests/export/**`), the readable rest (to import from), support files it may touch.
- Breaks today: the deliverable is a directory; there is no writable subset inside it.

## The simplified model

Four concepts. Everything above is a combination of them.

- Workspace: a directory holding `jazyk.toml` (or, with no config, the directory a
  command runs in). Every relative path resolves against it. It holds the stores under
  `.jazyk/`.
- Spec: one doc set compiled into one IR. It has a name, a doc glob, roots, and an
  optional deliverable mapping. A workspace holds one or more specs. Today's project is
  a workspace with one spec.
- Deliverable: what a spec maps onto: one or more directories, each with globs saying
  what is in scope, what is read-only, and what is out. The word stays: in jazyk the
  docs are the source, so the code is never called source in config.
- Store: one spec's IR and everything derived from it, at `.jazyk/<spec>/` by default.
  Exactly one store per spec, never shared.

A spec is the unit of everything the runtime already does per project: one graph, one
board, one ledger, one verdict, one build lease. Nothing inside the compiler learns
about workspaces; the workspace only resolves settings and paths and hands one spec to
the existing machinery. That is the point: the change is at the edges (config,
discovery, paths, ownership across ledgers), not in the reconciler.

### A spec in config

The short form is today's file. With no `[spec.*]` table, the top-level `[docs]`,
`[roots]`, and deliverable settings form one spec named `main`:

```toml
[docs]
glob = ["docs/**/*.md"]

[roots]
files = ["docs/README.md"]

[deliverable]
dir = "code"
```

The long form names specs. Top-level tables that are not about one doc set (`[llm]`,
`[acp]`, `[executors]`, `[workflow]`, `[docs.linting]`, `[docs.handlers]`) are shared
defaults; a spec overrides any of them per field under its own name:

```toml
default = "checkout"

[llm]
model = "gpt-5.5"

[docs.linting.rules]
warnings = ["An em dash appears in prose."]

[spec.checkout]
docs = ["specs/checkout/**/*.md"]
roots = ["specs/checkout/README.md"]

[spec.checkout.deliverable]
dir = "."
include = ["src/checkout/**", "tests/checkout/**"]

[spec.security]
docs = ["specs/security.md"]
generate = false

[spec.security.deliverable]
dir = "."
include = ["src/**"]
```

- `docs` and `roots` in a spec are glob lists, the same matching rules as today's
  `docs.glob` and `roots.files`. `[spec.x.docs] glob = [...]` is accepted too, so a
  spec table can be pasted from a short-form file.
- `base` (optional) is the directory a spec's globs, doc paths, and deliverable `dir`
  resolve against, relative to the workspace. Default: the workspace. It keeps doc
  paths in the store stable when a docs tree moves under a new workspace (see
  [migration](#the-dogfood)).
- `generate = false` makes the spec verify-only: no `generate` goals derive, bind and
  verify run as usual.
- With `[spec.*]` tables present, a top-level `[docs] glob`, `[roots]`, or
  `[deliverable]` is a settings error naming the fix (move it into a spec). Two forms
  never mix in one file.
- Spec names match `[a-z0-9][a-z0-9-]*`. The name is the store directory, so it is
  stable; renaming a spec is renaming its store (see [renames](#renames-and-moves)).
- `default` names the spec commands use when none is chosen. Absent, the first
  declared spec is the default.

### The deliverable mapping

One table per spec, replacing `[gen] deliverable` and `[gen] code` (both still read as
aliases):

```toml
[spec.shop.deliverable]
dir = "."                              # default: the workspace (or the spec base)
include = ["src/**", "tests/**"]       # in scope; default: everything under dir
exclude = ["src/gen/**"]               # out of scope entirely (other generators)
readonly = ["vendor/**", "src/legacy/**"]  # in scope, never written
```

Every deliverable file falls in exactly one class, decided by the last matching
pattern, the same rule as the doc glob:

- Out of scope: not under `dir`, matched by `exclude`, not matched by `include`, or in
  the standard exclusions (the store, hidden directories, `target`, `node_modules`,
  the spec's own docs). Bind, generate, verify, the unclaimed report, and decompile
  never see it.
- Read-only: matched by `readonly`. Bind reads it and may cite it as the implementing
  file, verify runs tests that import it, decompile may draft from it. Generate never
  writes it; a write is rejected naming the path and the pattern, the same corrective
  shape as today's ownership rejection. It is excluded from the unclaimed report
  (nobody is asked to document vendored code) unless the spec says
  `report_readonly = true`.
- Writable: everything else in scope. Today's ownership rules apply unchanged: a file
  belongs to the entity whose session wrote it, support files belong to the
  deliverable.

This covers cases 11, 12, and 14 with no new concepts. Case 12's hand-written files are
`readonly` when the team wants them protected; adoption (case 3) keeps them writable,
since generating an `unimplemented` requirement often means editing existing code.

### Several deliverable roots

A spec may map onto several directories (case 8). Each is a named part:

```toml
[spec.shop.deliverable.web]
dir = "../web"
entity = "Frontend"

[spec.shop.deliverable.api]
dir = "../api"
entity = "Backend"
```

- A part's `entity` names the containment subtree it realizes, by name, resolved
  through the name index after each commit. The resolved id is recorded in
  `status.yaml`; a name that resolves to nothing, or to two entities, is a `mapping`
  diagnostic on the spec.
- An entity's generate goal writes into the part whose entity is its nearest ancestor
  (or itself). An entity under no part's subtree writes into the part with no `entity`
  key; with none such, it derives no generate goal and the unattached report says so.
- One toolchain per part, not per spec: the medium is decided per part, from the
  subtree's facts.
- Ledger paths carry the part name as a prefix (`api:src/orders.rs`). A single
  unnamed deliverable keeps unprefixed paths, so today's ledgers read unchanged.
- Bind searches every part; it does not need the entity mapping to classify.

The mapping by entity is the only place config names a graph node. It names it by the
same words the documents use, so it survives rebuilds, and it is optional: a monorepo
with one toolchain maps onto `dir = "."` and needs no parts.

### Two specs over one deliverable

Specs are independent bounded contexts, the workspace-level analogue of
[scopes](../docs/compiler/concepts/scopes.md). Each has its own IR, its own board, its
own ledger. What they share is the files, so the rules are about files:

- Reading is free. Any number of specs may bind the same file, cite the same sites, and
  run the same tests.
- Writing is exclusive. A file owned by an entity in one spec's ledger is read-only to
  every other spec in the workspace. Before a write, generate checks the other stores'
  ledgers (a workspace ownership index, rebuilt from the ledgers, never stored); a
  conflicting path is rejected naming the owning spec, with the same corrective retry
  as today.
- Support files (`Cargo.toml`, `package.json`) stay rewritable by any spec, as they
  are by any entity today. Generation across specs runs one at a time under the
  workspace lease (below), so two specs never rewrite one manifest concurrently.
- Conflicting obligations surface through the code, not through a cross-graph judge.
  If the security spec requires argon2 and the API spec's generation writes bcrypt,
  the security spec's bind row turns `failing` with the file and site as evidence. The
  workspace status lists files bound by more than one spec so a reviewer sees where two
  specs meet.
- Two specs that must agree sentence by sentence (case 7, where contradiction between
  the documents themselves matters) belong in one spec. A spec may have several roots
  and several named scopes; that is what [scopes](../docs/compiler/concepts/scopes.md)
  are for. The plan recommends this over a cross-spec contradiction checker (see
  [open questions](#open-questions)).
- A document matched by two specs' globs is compiled into both IRs, at twice the cost.
  `jazyk status` reports it as `shared-doc` (info) so the overlap is a choice, not an
  accident.

The unclaimed report exists at two levels: per spec (files in its scope no binding in
its ledger names) and per workspace (files in any spec's scope no ledger names). The
workspace number is the one decompile reads when no spec is chosen.

## Where the IR lives

### Options

| Option | Pros | Cons |
|---|---|---|
| `jazyk-out/` (today) | visible, simple | an odd name in every user folder; one per root; clashes for several specs |
| `.jazyk/` (one store) | hidden, conventional (`.git`, `.venv`, `.next`) | still one store per root |
| `.jazyk/<spec>/` | hidden, one per spec, one place to ignore, one place to find | a spec rename must move a directory |
| name from the config file (`checkout.jazyk.toml` → `.jazyk/checkout/`) | no names in config | only works with one file per spec (rejected [below](#where-config-lives)) |
| outside the tree (`$XDG_STATE_HOME/jazyk/<path hash>/`) | tree stays clean; works on read-only trees | invisible; a move or rename orphans it; not committable; no equivalent in a browser |
| inside the docs folder (`docs/.jazyk/`) | travels with the docs | several specs over one docs folder clash; the dogfood already had this and needed a redirect |
| configurable `out` | covers everything | a default is still needed |

### Recommendation

Default `.jazyk/<spec>/` at the workspace root, with two overrides:

- `store = "PATH"` per spec (or top-level for the short form), relative to the
  workspace. Absolute paths allowed. `--out DIR` on the command line wins over it, as
  today.
- `store = "@state"` puts the store at `$XDG_STATE_HOME/jazyk/<workspace hash>/<spec>/`
  (macOS: `~/Library/Application Support/jazyk/...`), for read-only trees and for
  people who want nothing in the tree. The workspace hash is of the canonical path;
  `jazyk status` prints the resolved store so it is never a mystery. When the
  workspace directory is not writable, the CLI uses `@state` and says so.

Layout:

```
.jazyk/
  .gitignore        # written by jazyk on creation: see committing below
  .lock             # the workspace lease: one build or generation at a time
  main/             # the store of spec `main`; same layout as today's jazyk-out/
    status.yaml
    graph/  docs/  journal/  diagrams/  docsgen/  gen/  trace/  sessions/
    control.yaml  workers/  leases/  feedback.jsonl  .lock
  main.bak/         # the store-version archive, a sibling as today
  checkout/
  security/
```

- The inside of a store does not change. `<out>` in every doc page keeps meaning "this
  spec's store"; only its default path moves.
- `.jazyk/` is hidden, so the existing rule already keeps it out of doc input. The
  `jazyk-out*` exclusion stays, so a leftover old store is never compiled.
- `.bak` is a sibling in `.jazyk/`, as today. Spec names cannot contain a dot, so
  `main.bak` is never a spec.
- The workspace lease in `.jazyk/.lock` serializes builds and generations across specs:
  builds are sequential by design, and this extends the rule from one store to one
  workspace. A read (status, preview, the LSP) takes no lease.
- `~/.jazyk/` stays the machine directory (global config, `projects.yaml`, benchmark
  history). A workspace rooted at `$HOME` would put its stores in the same directory,
  so a workspace at `$HOME` uses `@state`, and discovery never treats `~/.jazyk/` as a
  store.

### Committing parts of the IR

The graph is reviewable and expensive to rebuild; diagrams, traces, and sessions are
not. The self-written `.jazyk/.gitignore` decides what git sees, and one setting
chooses it:

```toml
[store]
commit = "none"   # default: .gitignore holds `*`
# commit = "state"
```

- `none` (default): `.jazyk/.gitignore` is `*`. Nothing is committed, the same outcome
  as today's `jazyk-out` in `.gitignore`, and it works in repos that never heard of
  jazyk because the ignore file ignores itself.
- `state`: the ignore file keeps `*/status.yaml`, `*/graph/`, `*/docs/`, `*/journal/`,
  and `*/gen/ledger.yaml`, and ignores the rest. That set is exactly what goal
  derivation reads (docs, graph, ledger, change records), so a fresh clone or a CI job
  derives zero goals on unchanged docs and `jazyk check` runs with no model.
- For `state` to diff cleanly, run statistics leave `status.yaml`: `costs` and `spent`
  move to `runs.yaml` beside it (ignored). This is a store layout change, so it bumps
  the store version to 3, which the archive rule already handles.
- A merge conflict in committed state is resolved by taking either side whole. The
  section trees under `docs/` carry content hashes, so whatever the kept side did not
  see is dirty on the next build and reconciles. `jazyk store take ours|theirs` does
  the checkout of the whole store directory so nobody hand-merges YAML shards.
- Rewriting `.jazyk/.gitignore` when the setting changes is the only write jazyk makes
  outside a store. A hand-edited ignore file (content not one jazyk wrote) is left
  alone with a warning.

### CI, checkouts, renames and moves

- CI: with `commit = "state"` the clone has the IR. With `none`, CI caches `.jazyk/`
  keyed on the hash of the doc files; a miss is a full build, which is the cost of not
  committing.
- Multiple checkouts (git worktrees): each checkout has its own `.jazyk/`, so two
  branches never share a store. A new worktree starts empty unless state is
  committed; `jazyk store copy --from DIR` copies another checkout's store for a spec
  (a directory copy, then the normal dirty set).
- Workspace moves: the in-tree store moves with it; nothing is keyed on the absolute
  path. `@state` stores are keyed on the path and orphan on a move; `jazyk status` in
  the moved workspace finds no store and says where it looked.

### Renames and moves

- Renaming a spec in config without moving its store starts that spec empty. `jazyk
  status` lists store directories under `.jazyk/` that match no spec as `orphaned`,
  with their generation and size. `jazyk spec rename OLD NEW` edits the config and
  renames the directory in one step.
- Moving a docs tree changes the doc paths the store keys section trees on, so every
  moved document reads as deleted and added. A spec `base` that moves with the tree
  keeps the paths stable.

### The browser

- The virtual tree already mirrors writes to the picked folder, so a build in a tab
  lands in `.jazyk/<spec>/` like a native one. The folder read already includes
  `.jazyk/`: `lib/folder.ts` skips only `.git`, `node_modules`, `target`, and
  `.DS_Store`. A test pins that, so a later skip rule for hidden directories cannot
  drop the store.
- When the user grants read but not write, or opens a folder they do not want
  touched, the store lives in IndexedDB keyed by the folder handle (the handle is
  itself storable in IndexedDB), `persistence: browser` as the descriptor already
  says. The doc they write lives there too until they save it into the folder.
- Demo fixtures pack `.jazyk/main/` instead of `jazyk-out/`; `pack-demo.mjs` and the
  fixture JSON change, and every fixture is repacked once.

## Where config lives

### Options

- One `jazyk.toml` per workspace with `[spec.<name>]` tables. One place to read, shared
  defaults written once, discovery unchanged.
- Several `*.jazyk.toml` files in one directory, one spec each, named by the file stem.
  Easy to hand one file to one team; shared settings repeat in every file or need an
  include rule; discovery must glob; two files may disagree on shared tables.
- A workspace file listing member directories, each with its own `jazyk.toml` (the
  Cargo workspace shape). Good for monorepos whose services each own their docs;
  heavier for three feature specs in one `specs/` folder.

### Recommendation

One `jazyk.toml` per workspace, specs as tables. Nested workspaces cover the monorepo
case without a members list:

- Discovery walks up to the nearest `jazyk.toml`, as today. Nearest wins.
- A directory under a workspace that holds its own `jazyk.toml` is a boundary for the
  outer workspace: the outer specs' globs never descend into it, and its files are
  never the outer specs' docs or deliverable. A service with its own docs is its own
  workspace; the repo root may still hold a workspace for cross-cutting specs.
- A directory with no `jazyk.toml` but a `.jazyk/` holding an implicit spec (below) is
  a workspace too, found by the same walk.
- The redirect is retired. A root `jazyk.toml` with a spec whose `base` is `docs`
  replaces it, and tools launched at the root and inside `docs/` resolve the same
  workspace by plain walk-up. A redirect file still loads during the transition with a
  warning naming the replacement, then becomes a settings error.

### Picking the active spec

One rule, the same for every frontend, first match wins:

1. `--spec NAME` on the command line (`JAZYK_SPEC` in the environment).
2. The spec named by an explicit input: `jazyk compile specs/search.md` picks the
   spec whose docs glob claims the file. A file claimed by several specs picks all of
   them for commands that take several (compile, check), and needs `--spec` for
   commands that take one.
3. The spec whose docs or deliverable claims the working directory (running inside
   `specs/checkout/` picks `checkout`).
4. The workspace's `default`, else the first declared spec.

Commands that act on the workspace take every spec when none is chosen: `jazyk
compile` compiles every spec with dirty input, in declaration order, one build at a
time under the workspace lease; `jazyk check` and `jazyk status` report per spec, then
the workspace lines (orphaned stores, shared docs, files bound by several specs).
Commands that read one IR (`preview`, `explain`, `ripple`, `context`, `query`,
`viewer`) use the rule above and name the spec in their first line.

Frontends:

- LSP: one server per editor workspace. Each open document is served by every spec
  that claims it. Diagnostics carry the spec name in `source` (`jazyk:security`); a
  hover over a mention shows each spec's entity in its own block. A single-spec
  workspace looks exactly as today.
- MCP: a serving is one spec, fixed at start (`jazyk mcp all --spec checkout`, or the
  rule above from the server's cwd). `graph_status` names the spec. `jazyk init` in a
  multi-spec workspace writes one entry per spec (`jazyk-checkout`, `jazyk-security`)
  so an agent sees two graphs as two servers, never a merged one.
- GUI: one server per workspace. `GET /api/project` lists the specs; every route takes
  `?spec=NAME`, defaulting to the default spec, so the single-spec API is unchanged.
  The status bar holds a spec picker; Home's recent list shows the workspace with its
  specs.
- ACP: the session's `cwd` picks the workspace; `/spec NAME` switches the spec for the
  session, the way `/model` switches the model.

### No config file at all

A command given documents in a directory with no `jazyk.toml` creates an implicit spec
instead of today's shared ad hoc store:

- `jazyk compile rate-limits.md` in `~/code/svc/` with no config above: the workspace
  is `~/code/svc/`, the spec is named after the document's stem (`rate-limits`), its
  docs are that file, its deliverable is the workspace. The store is
  `.jazyk/rate-limits/`.
- The spec's definition is written to `.jazyk/rate-limits/spec.toml` (the same keys as
  a `[spec.x]` table). The next command in that directory finds it by walk-up, so
  `jazyk gen` and `jazyk status` work with no arguments.
- A second document (`timeouts.md`) makes a second implicit spec. Neither disturbs the
  other. Several documents in one command make one spec named after the first, or
  `--spec NAME`.
- `jazyk spec save NAME` moves an implicit spec into `jazyk.toml` (creating it in the
  short form when it is the only spec, the long form otherwise) and deletes the
  `spec.toml`. Nothing else changes: the store is already where the saved spec expects
  it.
- An implicit spec inside a workspace that has a `jazyk.toml` is allowed too
  (`jazyk compile draft.md --spec draft` where no spec claims `draft.md`): the
  experiment lives beside the real specs without editing the config.
- `jazyk init` still writes a starter `jazyk.toml`; it is the way to begin a project
  on purpose, while the implicit spec is the way to try a document.

On jazyk.dev (case 10): Home's `Open folder` accepts a folder with no `jazyk.toml`.
`New document` asks for a name and creates the implicit spec over the folder. With a
writable folder the doc lands at the path the user picks (default `docs/<name>.md`)
and the store at `.jazyk/<name>/`; otherwise both stay in IndexedDB. Compile and bind
run in the tab. Generation writes files through the virtual tree; verification needs
test runs, which a tab cannot do
([what a tab does not do](../docs/frontends/wasm.md#what-a-tab-does-not-do)), so rows
stay `unverified` until the native binary runs `jazyk test` over the same folder.

## Mapping granularity

- File, directory, glob: config. `include`, `exclude`, and `readonly` are glob lists;
  a directory is `dir/**`; a file is its path.
- Entity subtree: config, for deliverable parts only (`entity = "Backend"`), resolved
  by name.
- Symbol or region: not config. Anchoring never parses the medium
  ([traceability](../docs/consumers/gen.md#traceability)), and a symbol list in config
  would be the first place jazyk knows a language. Region-level mapping already
  exists where it belongs, in the ledger: bind and generate record sites (file, line,
  `head`), which relocate by text and survive edits. A spec that must describe "the
  `parse` function" says so in its prose; bind finds the site.
- A spec never declares regions inside a file it shares with another spec. Two specs
  may bind different sites in one file; the write rule is per file, so a file with two
  specs' requirements is written by whichever spec's entity owns it and read by the
  other.

How each consumer uses the mapping:

- Bind: searches the in-scope files (writable and read-only), records files and sites.
  A binding may name a read-only file.
- Generate: writes only writable files, never a file another spec owns, into the part
  its entity maps to. Support files stay deliverable-wide within the part.
- Verify: runs tests from the part's directory; read-only files are imported like any
  other.
- Decompile: drafts from in-scope files; the inventory and the unclaimed report use
  the same classes. `jazyk decompile --into docs/billing.md` makes the whole in-scope
  set one draft scope, the one-document-for-a-subset case (5); without `--into`,
  scopes stay per directory as today. The draft lands in the chosen spec's docs; a
  path its glob does not include is rejected as today.
- Unclaimed report: writable files only, plus read-only when `report_readonly`.
- The upstream direction (`plans/upstream-reconciliation.md`): a file edit maps to
  the specs whose scope includes the file, and within each to the requirements whose
  sites sit in it. That lookup is the ledger plus the classes above; this plan owns
  it, that plan consumes it.

## Worked layouts

### Three feature specs over one repo (case 6)

```
shop/
  jazyk.toml
  specs/
    checkout.md
    search.md
    accounts.md
  src/
    checkout/  search/  accounts/  lib.rs
  tests/
  .jazyk/
    .gitignore
    checkout/  search/  accounts/
```

```toml
[spec.checkout]
docs = ["specs/checkout.md"]
[spec.checkout.deliverable]
include = ["src/checkout/**", "tests/checkout/**", "src/lib.rs"]

[spec.search]
docs = ["specs/search.md"]
[spec.search.deliverable]
include = ["src/search/**", "tests/search/**", "src/lib.rs"]

[spec.accounts]
docs = ["specs/accounts.md"]
[spec.accounts.deliverable]
include = ["src/accounts/**", "tests/accounts/**", "src/lib.rs"]
```

`src/lib.rs` is a support file (the crate root every module is declared in), so all
three may rewrite it, one at a time under the workspace lease.

### Docs repo beside a code repo (case 9)

```
work/
  product/            # another team's repo, no jazyk files
  product-docs/
    jazyk.toml
    guide/*.md
    .jazyk/main/
```

```toml
# product-docs/jazyk.toml, short form
[docs]
glob = ["guide/**/*.md"]

[deliverable]
dir = "../product"
readonly = ["**"]
```

Every file in `../product` is read-only, so the spec binds and verifies and never
writes: a generate session has no writable path to land on, and every write is
rejected. Adding `generate = false` states the intent and derives no `generate` goals
at all, which saves the sessions that would only be rejected.

### One design, two repos (case 8)

```toml
# design/jazyk.toml
[docs]
glob = ["**/*.md"]

[deliverable.web]
dir = "../web"
entity = "Frontend"

[deliverable.api]
dir = "../api"
entity = "Backend"
```

### A fresh doc in a folder (case 10)

```
svc/                  # an existing repo, no jazyk.toml
  src/  tests/
  rate-limits.md      # written just now
  .jazyk/
    .gitignore
    rate-limits/
      spec.toml       # docs = ["rate-limits.md"], deliverable dir "."
      status.yaml  graph/ ...
```

### Vendored and generated code (cases 11, 12)

```toml
[deliverable]
dir = "."
include = ["src/**", "tests/**", "vendor/**"]
exclude = ["src/proto_gen/**"]
readonly = ["vendor/**", "src/legacy/**"]
```

## Migration

The store has no migration code, as ever. The config reads both forms.

- Config: the short form is permanent (it is the one-spec case, not a legacy form).
  `[gen] deliverable` and `[gen] code` read as `[deliverable] dir` and `include`,
  forever, with no warning. `redirect` loads with a warning for one release, then is
  an error naming the replacement. `jazyk config upgrade` rewrites a file into the
  current keys and is the only writer; nobody has to run it.
- Store location: an old `<root>/jazyk-out/` is left where it is, never read, and
  never compiled (the `jazyk-out*` exclusion stays). `jazyk status` names it once as a
  leftover the user may delete. The first build under the new layout starts empty and
  reconciles, the same as a store version mismatch. Step 9 bumps the store version to
  3 anyway, so a rename into place would only save one rebuild in the window between
  steps 2 and 9 (see [open questions](#open-questions)).
- `--out` keeps its meaning (the chosen spec's store) and its name.
- `.gitignore`: the repo's `jazyk-out` and `jazyk-out.*` lines stay until no example
  or doc mentions the old name; `.jazyk/` needs no line because it ignores itself,
  but the repo adds `.jazyk/` anyway so a hand-deleted inner ignore file cannot leak a
  store into a commit.

### The examples

- `f1`, `f2`, `example-sort`, `example-erp`, `example-slides`, `example-org`,
  `example-novel`: configs unchanged (short form). Their stores move to
  `.jazyk/main/`. `EXPECTED.md` files and `bootstrap/VALIDATION.md` that name
  `jazyk-out/` paths are updated in the same commit as the code.
- A new example shows the multi-spec shape: `example-shop-specs` (or `example-sort`
  gaining a second spec, a verify-only `perf.md` over the same `code/`), small enough
  to compile on a local model.

### The dogfood

Today: `jazyk.toml` at the repo root redirects to `docs/`, `docs/jazyk.toml` holds the
project, the store is `docs/jazyk-out/`, the deliverable is `project2/`. After:

```toml
# jazyk.toml at the repo root, the only config
[spec.jazyk]
base = "docs"
docs = [
    "**/*.md",
    "!benchmark/cases/**",
    "!compiler/goals/prompts/**",
    "!compiler/skills/**",
]
roots = ["main.md"]

[spec.jazyk.deliverable]
dir = "../project2"

[docs.linting.rules]
warnings = [
    "An em dash appears in prose. This documentation uses commas, periods, parentheses, or colons instead.",
]
```

- `base = "docs"` keeps every doc path in the store as it is today (`main.md`, not
  `docs/main.md`), so the section trees stay valid; the version bump rebuilds anyway,
  but the base keeps later moves cheap.
- The store lands at `.jazyk/jazyk/` at the repo root. `docs/jazyk.toml` is deleted.
  Running from `docs/` walks up to the root file, as the redirect did by hand.
- `AGENTS.md` changes its store and layout lines.
- A second dogfood spec is possible later: `bootstrap` mapping `docs/` onto
  `bootstrap/src/**` read-only, the adoption case run on jazyk itself. Not part of
  this plan's steps.

### The site and the GUI

- `pack-demo.mjs` reads `.jazyk/main/`; fixtures repack; `app/public/demos/*.json`
  change in one commit.
- `lib/folder.ts` reads `.jazyk/` from a picked folder and accepts a folder with no
  `jazyk.toml`.
- The GUI gains the spec picker and the `?spec=` parameter; `gui/outfiles.rs`,
  `gui/deliverable.rs`, `gui/facts.rs`, and `webapi` resolve the store through the
  spec. `app/dist` is rebuilt and the release binary rebuilt after it, per the repo
  rule.
- `site/README.md` mentions `jazyk-out`; updated with the rest.

## Implementation order

Each step is shippable alone, lands docs first, and is tested without a model: unit
tests, replays of recorded sessions, and, for a behavior that needs a model, one
session run with `jazyk benchmark --project <dir> --goal <id>` against a snapshot, not
a whole-project rerun.

### Step 1: a real TOML reader

- Docs: none (no behavior change).
- Code: `project.rs` replaces the hand-rolled `Toml` with the `toml` crate (dependency
  policy: parsers for standard formats come from crates). `settings_render` keeps its
  output byte-stable for the GUI settings form.
- Tests: the existing `project.rs` tests pass unchanged; new tests parse inline tables
  and nested tables.

### Step 2: the store moves to `.jazyk/main/`

- Docs first: `graph.md#storage-layout` (default path, `.jazyk/` layout, the self-ignore
  file), `project-settings.md` (the excluded paths), `cli.md#project-discovery` and the
  `--out` option, `wasm.md#a-folder-on-disk`, `wasm.md#the-demo`. Replace `jazyk-out`
  in the seventeen doc mentions.
- Code: one function `Project::store_dir(spec)` used by `cli.rs::resolve` and every
  site that joins `jazyk-out` today (`store.rs`, `board.rs`, `reconcile.rs`, `mcp.rs`,
  `lsp.rs`, `bind.rs`, `decompile.rs`, `verify.rs`, `docsgen.rs`, `launcher.rs`,
  `gui/*`, `webapi/*`, `replay.rs`, `tools.rs`). Writes `.jazyk/.gitignore` with `*`.
  The legacy rename if the owner keeps it.
- Tests: unit tests for the default path, the ignore file, the archive to
  `.jazyk/main.bak/`, a leftover `jazyk-out/` never compiled; replays on `f1`
  unchanged apart from paths. Rebuild the GUI and repack the demos.

### Step 3: the deliverable table and its three classes

- Docs first: `project-settings.md#generation` becomes a `Deliverable` section
  (`dir`, `include`, `exclude`, `readonly`, the aliases), `gen.md#the-deliverable` and
  `#file-ownership-and-conventions` (the read-only rejection), `bind.md` (read-only
  files are bindable), `bind.md#the-unclaimed-report`, `decompile.md#inventory`.
- Code: `project.rs` (the classes), `gen.rs` and the served file tools (reject writes
  to read-only paths), `bind.rs` (scope), `decompile.rs` (inventory, `--into`).
- Tests: classification unit tests over a fixture tree; a generate replay that writes
  into a read-only path gets the rejection and the corrective retry.

### Step 4: named specs

- Docs first: `project-settings.md` (the long form, `default`, `base`, shared
  defaults, the mixing error), a new concept page `docs/compiler/concepts/specs.md`
  (workspace, spec, deliverable, store; two specs over one deliverable),
  `cli.md` (`--spec`, the selection rule, workspace commands), `graph.md` (per-spec
  store, the workspace lease), `control-plane.md` (the workspace lease).
- Code: `project.rs` (`Workspace { root, specs }`, spec resolution), `cli.rs`
  (`--spec`, `JAZYK_SPEC`, per-spec loops for compile, check, status), `control.rs`
  (the workspace lease), `store.rs` (nothing beyond the path).
- Tests: resolution unit tests for each rule of the selection order; a two-spec
  workspace fixture whose `jazyk check` reports both; a no-op rebuild of both derives
  zero goals.

### Step 5: ownership across specs

- Docs first: `gen.md#file-ownership-and-conventions` (the cross-spec rule),
  `bind.md` (shared files in status), `cli.md#jazyk-status` (workspace lines:
  orphaned stores, shared docs, files bound by several specs).
- Code: `gen.rs` (the ownership index from sibling ledgers), `reconcile.rs` and
  `cli.rs` (status lines).
- Tests: two specs, one owning `src/a.rs`; the other's generate replay is rejected on
  that path and accepted on a support file.

### Step 6: implicit specs

- Docs first: `cli.md#project-discovery` (the `.jazyk/<name>/spec.toml` marker, the
  explicit-path rule, `jazyk spec save`), `cli.md#jazyk-init` (init versus implicit),
  `project-settings.md`.
- Code: `project.rs` (discovery of implicit specs), `cli.rs` (explicit paths create
  or reuse an implicit spec, `spec save`, `spec rename`).
- Tests: compiling two documents in turn in one bare directory leaves two stores, each
  with its own graph intact (the clobbering regression); `spec save` round-trips.

### Step 7: frontends pick a spec

- Docs first: `lsp.md` (per-document specs, `source` labels), `mcp.md` (`--spec`,
  one serving per spec, init entries), `gui.md` (spec picker, `?spec=`, Home),
  `acp.md` (`/spec`).
- Code: `lsp.rs`, `mcp.rs`, `launcher.rs` and init, `gui/*` and `webapi` routes, the
  app's status bar and Home, `acp/commands.rs`.
- Tests: LSP over stdio with a document claimed by two specs; API tests with and
  without `?spec=`; the GUI built and the release binary rebuilt.

### Step 8: the browser flow

- Docs first: `wasm.md#a-folder-on-disk` (no config needed, `.jazyk/` read, IndexedDB
  store for read-only folders), `gui.md#home` (`New document`), `site.md#the-editor`.
- Code: `lib/folder.ts`, the wasm `boot` accepting an implicit spec, an IndexedDB
  sink beside the folder sink.
- Tests: the wasm backend's existing tests with a seeded folder lacking `jazyk.toml`;
  a manual run on jazyk.dev's preview build before deploy.

### Step 9: deliverable parts and the store version bump

- Docs first: `gen.md#the-deliverable` (parts, `entity`, per-part toolchain, prefixed
  ledger paths), `graph.md#storage-layout` (`runs.yaml`, version 3),
  `project-settings.md` (`[store] commit`).
- Code: `gen.rs`, `bind.rs`, `verify.rs` (parts), `model.rs` and `store.rs`
  (`runs.yaml`, version 3), the ignore file writer for `commit = "state"`, `jazyk store
  take` and `store copy`.
- Tests: part resolution by entity name, the `mapping` diagnostic for a missing name,
  a committed-state clone deriving zero goals, the version bump archiving a version 2
  store.

### Step 10: the dogfood and the examples

- Docs first: `AGENTS.md`, `project-settings.md#redirect` removed (or marked for
  removal), `docs/TODO.md`.
- Code: the repo-root `jazyk.toml` rewritten, `docs/jazyk.toml` deleted, the redirect
  error, one multi-spec example.
- Tests: `jazyk status` from the repo root and from `docs/` name the same workspace and
  store; the new example's no-op rebuild.

Steps 2, 3, and 4 carry most of the value; 5 and 6 fix the two ways work is lost
today (cross-spec overwrites, ad hoc clobbering). Parts (step 9) wait until a real
two-repo user exists.

## Open questions

- Keep the word "deliverable", or rename to "product" (the GUI lens) or "source"?
  Recommended: keep "deliverable". "Source" contradicts jazyk's premise that the docs
  are the source; "product" is the GUI's plain-language word and can stay there.
- One config file with spec tables, or `*.jazyk.toml` per spec? Recommended: one file.
  Revisit only if a team asks to own its spec's config file separately; nested
  workspaces already give a directory its own file.
- Store at `.jazyk/<spec>/` even for a single spec, or `.jazyk/` flat when there is
  one? Recommended: always `.jazyk/<spec>/` (`main` for the short form). One layout,
  and adding a second spec never moves the first store.
- Rename an old `jazyk-out/` into place, or leave it and rebuild? Recommended: leave
  it. The version bump in step 9 rebuilds anyway, and "no migration code" stays
  literal. If step 2 ships long before step 9, a one-line rename is worth it; the
  owner decides.
- Default `commit = "none"` or `"state"`? Recommended: `none`. The graph is reviewable,
  but committed shards invite hand merges and churn on every build; teams that want CI
  without a model opt in.
- A cross-spec contradiction check (two specs' requirements judged against each
  other)? Recommended: no. Specs are independent by design; the code arbitrates through
  bind, and specs that must agree belong in one spec with scopes.
- Should `jazyk compile` with no `--spec` compile every spec? Recommended: yes, every
  spec with dirty input, in order. A no-op workspace still makes zero LLM calls.
- Implicit spec name when several documents are given: the first stem, or require
  `--spec`? Recommended: the first stem, printed, so a quick try never stops on a
  usage error.
- Keep `~/.jazyk/` as the machine directory next to a `.jazyk/` store name?
  Recommended: keep it; the only clash is a workspace at `$HOME`, which uses `@state`.
  Moving machine config to `~/.config/jazyk/` is a separate decision.
- Symbol-level mapping in config (`symbols = ["parse_order"]`, resolved by text
  search)? Recommended: no. Sites in the ledger already give line-level mapping without
  knowing a language; prose names symbols when it must.
- Git URLs as deliverable directories (clone on demand)? Recommended: no. Paths only;
  checking out repos is the user's or CI's job.
