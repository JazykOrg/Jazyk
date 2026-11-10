# The .jazyk folder

Every file Jazyk keeps for a project, what it is for, its format, an example, where it
lives, and whether a repository commits it. The rules behind the graph files are in the
[graph store](./graph.md#storage-layout).

## Zones

A project's state lives in four places:

- **Committed**: the files a checkout needs to keep compiling without a rebuild. They sit
  at the top of `.jazyk/` and of each set directory `.jazyk/<set-id>/`. A repository
  commits them, and two branches that changed different things merge them without a
  conflict.
- **`local/`**: what one checkout's runs leave behind and the next run on the same
  checkout reads: build state, controls, the journal, locks. Never committed.
- **`derived/`**: output rebuilt from the committed files: diagrams, docsgen, derived graph kinds. Deleting it loses
  nothing. Never committed.
- **The log directory**: transcripts, chats, feedback, and replay snapshots. Outside the
  project, never committed (see [logs](#logs)).

A set directory holds committed state at its top level and nothing else, so what is
committed and what is not is visible from the listing alone.

## Layout

```
.jazyk/
  jazyk.toml                     committed   project configuration
  .gitignore                     committed   generated: what a repository commits
  .gitattributes                 committed   generated: the merge driver for set files
  README.md                      committed   generated: this layout in brief
  local/                         local       project machine state
  <set-id>/
    status.yaml                  committed   store version, root documents
    changes/<kind>/<subject>.yaml committed  one open change record
    parked/<goal>.yaml           committed   one goal a budget left open
    failed/<goal>.yaml           committed   one goal a session marked failed
    alignment/<doc>.yaml         committed   one document's pending anchor moves
    lookalikes/<pair>.yaml       committed   one entity pair kept apart
    graph/entities/<id>.yaml     committed   one entity
    graph/requirements/<id>.yaml committed   one requirement
    graph/criteria/<id>.yaml     committed   one criterion
    graph/views/<id>.yaml        committed   one curated view
    graph/diagnostics/<id>.yaml  committed   one diagnostic
    graph/redirects/<id>.yaml    committed   one absorbed id
    docs/<doc path>.yaml         committed   one document's section tree
    gen/ledger.yaml              committed   the deliverable-wide ledger keys
    gen/entities/<slug>.yaml     committed   one entity's generation state
    gen/requirements/<id>.yaml   committed   one requirement's binding and verification
    gen/criteria/<slug>.md       committed   one judged criterion's test setup
    local/                       local       this checkout's machine state
    derived/                         local       derived output
```

`<id>`, `<subject>`, `<goal>`, `<doc>`, and `<pair>` are [file ids](./graph.md#node-files):
`:` written `~`, `/` written `+`, other bytes `%XX`.

## Every file begins with what it is

Every YAML file Jazyk writes starts with two comment lines: one plain sentence saying
what the file is and that Jazyk writes it, and a schema line an editor's YAML support
follows for completion and validation. The lines are fixed per kind of file. They never
name an id, a date, or a count, so they never change and never conflict. A load skips
them like any YAML comment. E.g.:

```yaml
# Jazyk requirement: one obligation stated in your docs. Written by jazyk: edit the docs, not this file.
# yaml-language-server: $schema=https://jazyk.org/schemas/requirement.json
id: req:shop-orders-4k7q
```

The sentence for each kind is in the tables below. Each schema URL is published on
jazyk.org ([schemas](../site.md#schemas)).

## Project files

| File | What it is | Format | Where | Git |
|---|---|---|---|---|
| `jazyk.toml` | The project configuration: document sets, globs, models, workflow defaults ([project settings](./project-settings.md)). Written by a person or `jazyk setup`. | TOML | `.jazyk/` | committed |
| `.gitignore` | Generated. Ignores everything, then keeps the committed paths by name, so a new kind of output never enters a commit by accident. | gitignore | `.jazyk/` | committed |
| `.gitattributes` | Generated. Routes set files to the [merge driver](#merging-through-git). | gitattributes | `.jazyk/` | committed |
| `README.md` | Generated. This layout in brief: zones, committed or local, safe to delete. | Markdown | `.jazyk/` | committed |
| `local/project.lock` | Coordinates project operations across processes. | empty, advisory lock | `.jazyk/local/` | local |
| `local/ide.json`, `local/ide-installer/` | The editor adapters' executable path and plugin installs ([editor setup](../frontends/lsp.md#editor-setup)). | JSON, files | `.jazyk/local/` | local |
| `local/removed-sets/<id>.toml` | A removed set's definition, kept for restore ([document sets](./document-sets.md)). | TOML | `.jazyk/local/` | local |
| `local/archive/<set-id>-v<N>/` | A store of another version, archived whole by a build ([store version](./graph.md#store-version)). | a set directory | `.jazyk/local/` | local |

`.gitignore`:

```
*
!/jazyk.toml
!/.gitignore
!/.gitattributes
!/README.md
!/ds_*/
!/ds_*/status.yaml
!/ds_*/changes/
!/ds_*/changes/**
!/ds_*/parked/
!/ds_*/parked/*.yaml
!/ds_*/failed/
!/ds_*/failed/*.yaml
!/ds_*/alignment/
!/ds_*/alignment/*.yaml
!/ds_*/lookalikes/
!/ds_*/lookalikes/*.yaml
!/ds_*/docs/
!/ds_*/docs/**
!/ds_*/graph/
!/ds_*/graph/entities/
!/ds_*/graph/entities/*.yaml
!/ds_*/graph/requirements/
!/ds_*/graph/requirements/*.yaml
!/ds_*/graph/criteria/
!/ds_*/graph/criteria/*.yaml
!/ds_*/graph/views/
!/ds_*/graph/views/*.yaml
!/ds_*/graph/diagnostics/
!/ds_*/graph/diagnostics/*.yaml
!/ds_*/graph/redirects/
!/ds_*/graph/redirects/*.yaml
!/ds_*/gen/
!/ds_*/gen/**
.*.tmp
```

`.gitattributes`:

```
/ds_*/** merge=jazyk
```

## Committed set files

### `status.yaml`

The store version and what every commit outside a build reads.

```yaml
# Jazyk store status: the store version and the root documents. Written by jazyk.
# yaml-language-server: $schema=https://jazyk.org/schemas/status.json
version: 5
roots:
- docs/system.md
```

- `version`: the [store version](./graph.md#store-version).
- `roots`: the documents matching the project roots, stamped by the build.
- `reevaluate`: anchors placed with `reevaluate`, stale until addressed.

### `changes/<kind>/<subject>.yaml`

One open [change record](./graph.md#change-records): work the next build owes. A kind and
a subject hold at most one record, so the pair names the file. The file holds what the
record says, never which commit wrote it: two branches that dirty one subject the same
way write the same bytes.

```yaml
# Jazyk change record: work the next build owes. Written by jazyk.
# yaml-language-server: $schema=https://jazyk.org/schemas/change.json
kind: section-dirty
subject: docs/system.md#/platform
via: section
detail:
  doc: docs/system.md
  section: /platform
  previous: 77f71ce02b0897b1
  addedLines:
  - The Platform reports its uptime every hour.
  removedLines: []
```

### `parked/<goal>.yaml` and `failed/<goal>.yaml`

One goal a budget left open, or one a session marked failed with its reason. The whole
goal persists, `change` included, so a re-derivation keeps it
([parked and failed](./reconciler.md#parked-and-failed)). Its cause is read from the
journal, never stored.

```yaml
# Jazyk parked goal: work a build left open, resumed first next build. Written by jazyk.
# yaml-language-server: $schema=https://jazyk.org/schemas/goal.json
id: g:reconcile-section:docs/system.md#/platform
kind: reconcile-section
class: compile
mandatory: true
target: docs/system.md#/platform
change:
  dirty: {doc: docs/system.md, section: /platform}
reason: out of rounds
```

### `alignment/<doc>.yaml`

One document's pending [alignment proposals](./alignment.md#what-applies-and-what-is-proposed):
anchors whose section moved or vanished, with ranked candidates, waiting for a
`place-anchors` session.

```yaml
# Jazyk alignment proposals: anchors to re-place after a document changed. Written by jazyk.
# yaml-language-server: $schema=https://jazyk.org/schemas/alignment.json
id: docs/orders.md
doc: docs/orders.md
proposals:
- anchor: req:orders-holds-4k7q
  quote: A hold expires after 30 days.
  candidates:
  - {section: /orders/holds-and-expiry, score: 0.82}
```

### `lookalikes/<pair>.yaml`

One entity pair a session judged to be two things, with the score at judgment, so the
pair stays off the board while the score holds
([verdicts are remembered](./goals/dedupe-candidates.md#verdicts-are-remembered)).

```yaml
# Jazyk lookalike verdict: two similar entities a session kept apart. Written by jazyk.
# yaml-language-server: $schema=https://jazyk.org/schemas/lookalike.json
id: ent:audit-log~ent:audit-trail
pair: ent:audit-log~ent:audit-trail
verdict: separate
score: 0.5
```

### `graph/<kind>/<id>.yaml`

One node of the [graph](./model.md). The file holds the node's own facts only: no
generation markers, and no mention a requirement adds by reference
([node files](./graph.md#node-files)).

| Kind | Header sentence | Example fields |
|---|---|---|
| `entities/` | `Jazyk entity: one concept your docs name.` | `id`, `name`, `aliases`, `definition`, `stereotype`, `parent`, `attributes`, own `mentions` |
| `requirements/` | `Jazyk requirement: one obligation stated in your docs.` | `id`, `statement`, `entities`, `edges`, `transition`, `facets`, `source` |
| `criteria/` | `Jazyk criterion: how one would observe a requirement is met.` | `id`, `requirement`, `statement`, `given`, `when`, `then`, `source` or `provenance` |
| `views/` | `Jazyk view: what one diagram shows, curated.` | `id`, `kind`, `title`, `members`, `excluded`, `collapse` |
| `diagnostics/` | `Jazyk diagnostic: a finding or question about your docs.` | `id`, `rule`, `severity`, `subjects`, `message`, `prompt`, `answer`, `lifecycle` |
| `redirects/` | `Jazyk redirect: an id merged into another.` | `id`, `to` |

Every header sentence ends with ` Written by jazyk: edit the docs, not this file.`

```yaml
# Jazyk requirement: one obligation stated in your docs. Written by jazyk: edit the docs, not this file.
# yaml-language-server: $schema=https://jazyk.org/schemas/requirement.json
id: req:services-4h2c
statement: The Platform is built from sixteen services.
entities:
- ent:platform
facets:
- facet: constraint
  reasoning: the sentence states a fact the subject holds
source:
  doc: docs/services.md
  section: /services
  quote: The Platform is built from sixteen services.
```

```yaml
# Jazyk entity: one concept your docs name. Written by jazyk: edit the docs, not this file.
# yaml-language-server: $schema=https://jazyk.org/schemas/entity.json
id: ent:analytics-service
name: Analytics Service
stereotype: service
parent: ent:back-office
```

### `docs/<doc path>.yaml`

One source document's section tree as the last reconcile saw it: what alignment compares
the document on disk against. No line numbers and no whole-file hash, so an edit to one
section changes only that section's entry.

```yaml
# Jazyk section tree: the last compiled text of docs/system.md, used to spot edits. Written by jazyk.
# yaml-language-server: $schema=https://jazyk.org/schemas/document.json
sections:
  /platform:
    title: Platform
    kind: root
    order: 0
    raw: |-
      # Platform

      The Platform is a retail software system.
    hash: d3365c01adbf8216
coverage:
  /platform: {state: covered}
```

### `gen/`

The [generation and verification ledger](../consumers/gen.md#the-ledger), one file per
row.

- `gen/ledger.yaml`: the deliverable-wide keys (`medium`, `build`, `support`,
  `contradicted`, `tasks`).
- `gen/entities/<slug>.yaml`: one entity's generation state: the fact hash and the
  requirements it was generated against.
- `gen/requirements/<id>.yaml`: one requirement's binding: implementing files and sites,
  one test per criterion, its last verdict and evidence.
- `gen/criteria/<slug>.md`: the setup file of one judged (`llm`) criterion test.

```yaml
# Jazyk ledger row: how one requirement is implemented and verified. Written by jazyk.
# yaml-language-server: $schema=https://jazyk.org/schemas/ledger-requirement.json
id: req:main-js-2
entity: ent:sort-cli
files:
- main.js
tests:
  crit:main-js-2:
    verdict: pass
verdict: pass
```

## Local set files: `<set-id>/local/`

What this checkout's runs leave behind. A fresh checkout has none of it and works
without it ([a fresh checkout](./graph.md#a-fresh-checkout)).

| File | What it is | Format |
|---|---|---|
| `build.yaml` | The generation counter, the last build's verdict, costs, spend, open diagnostic counts, and each document's content hash at last reconcile. Its generation is the commit point readers check. | YAML |
| `control.yaml` | Workflow modes and the releases this operator approved ([control plane](./control-plane.md#modes-and-releases)). | YAML |
| `journal/g<N>.yaml` | One entry per commit: its mutations, the change records it wrote, the goals it resolved and opened, its cost ([journal](./graph.md#journal)). The only record of when and why the graph changed. | YAML |
| `workers/<id>.yaml`, `leases/<batch>.yaml` | Who is attached and which goal batch each holds ([workers and leases](./control-plane.md#workers-and-leases)). | YAML |
| `decompile/drafts.yaml` | Draft documents awaiting the owner ([decompile](../consumers/decompile.md)). | YAML |
| `deliverable-baseline/` | Deliverable files as they were before a generation run touched them ([gen](../consumers/gen.md)). | files |
| `.lock`, `.leases.lock`, `.committing` | The commit lock, the lease lock, and the marker present while a commit writes. | empty |

```yaml
# Jazyk build state: this checkout's generation, last verdict, and costs. Written by jazyk, never committed.
# yaml-language-server: $schema=https://jazyk.org/schemas/build.json
generation: 12
verdict: {state: incomplete, open: 1, blocked: 75}
spent: {sessions: 8, rounds: 124, tokens: 0}
diagnostics: {info: 48, warning: 2}
docs:
  docs/services.md: b6630e71887ddd69
  docs/system.md: 025ab4fec81a8b05
```

```yaml
# Jazyk journal entry: one commit to the graph and why it happened. Written by jazyk, never committed.
# yaml-language-server: $schema=https://jazyk.org/schemas/journal.json
build: g12
generation: 12
kind: session
batch: [g:reconcile-section:docs/system.md#/platform]
mutations:
- {op: upsert_requirement, id: req:system-platform-9k2d}
records:
- {kind: requirement-created, subject: req:system-platform-9k2d, mutation: 1}
resolved_goals:
- {goal: g:reconcile-section:docs/system.md#/platform, justification: "One new obligation extracted; section covered."}
tokens: 4100
```

## Derived set files: `<set-id>/derived/`

Rewritten from the committed files. Delete the folder at any time: a load recomputes the
derived graph kinds, and the next build's tail rewrites the rest.

| Path | What it is | Format |
|---|---|---|
| `graph/relationships/<id>.yaml` | One entity pair's typed connections, from requirement `edges` ([relationship](./model/relationship.md)). | YAML |
| `graph/state-machines/<id>.yaml` | One entity's states and transitions, from requirement `transition`s ([state machine](./model/state-machine.md)). | YAML |
| `graph/default-views/<id>.yaml` | One default view by rule; it moves to the committed `graph/views/` when curated ([default views](./model/view.md#default-views)). | YAML |
| `diagrams/<kind>/<slug>.puml`, `.svg`, `.png` | One rendering per view ([output layout](./diagrams.md#output-layout)). | PlantUML, SVG, PNG |
| `docsgen/` | Generated requirements documentation ([docsgen](../consumers/docsgen.md)). | Markdown |
| `last-build/<doc path>` | Each document as the last build stored it, for the editor's diff ([commands](../frontends/lsp.md#commands)). | the document's format |
| `preview/<hash>.json` | Live graph previews ([live preview](./live-preview.md)). | JSON |

```yaml
# Jazyk relationship: connections between two entities, derived from requirements. Written by jazyk, never committed.
# yaml-language-server: $schema=https://jazyk.org/schemas/relationship.json
id: rel:analytics-service~order-store
members: [ent:analytics-service, ent:order-store]
contributions:
- {a: ent:analytics-service, b: ent:order-store, type: association, requirements: [req:storage-access-7f3m]}
```

## Logs

Logs record what ran. Nothing reads them to compile, they grow with use, and they are
private to whoever ran it, so they live outside the project and never enter git.

- Locally, in the log directory: `JAZYK_LOG_DIR` when set, else the operating system's
  cache directory (`~/Library/Caches/jazyk` on macOS, `$XDG_CACHE_HOME/jazyk` or
  `~/.cache/jazyk` elsewhere), then `<project>-<key>/`, `<project>` the project
  directory's name and `<key>` the first eight hex digits of a hash of its absolute path.
  `jazyk status` prints the path. Files older than 30 days are removed when a build
  starts.
- In the browser, in the page's storage ([wasm](../frontends/wasm.md)).
- Hosted, through the host's record store ([host records](./hosted-runtime.md#host-records)).

| Path in the log directory | What it is | Format |
|---|---|---|
| `<set-id>/trace/<run>.jsonl` | Session transcripts, one event per line ([trace events](./sessions.md#trace-events)). | JSON Lines |
| `<set-id>/sessions/<id>.jsonl`, `sessions/<id>.jsonl` | Chat conversations ([session store](../frontends/acp.md#session-store)). | JSON Lines |
| `<set-id>/feedback.jsonl` | `report_feedback` entries ([feedback](./tools.md#feedback-tool)). | JSON Lines |
| `<set-id>/snapshots/<batch>/` | Graph copies per batch for replays, written only by `jazyk compile --record` ([snapshots](./sessions.md#snapshots)). | a set's files |
| `<set-id>/benchmark/` | Benchmark results and per-case transcripts ([benchmark](../benchmark/benchmark.md)). | YAML, JSON |
| `runs/<id>/`, `trace/` | Task runs and project-level transcripts ([task runtime](./task-runtime.md)). | files |

## Merging through git

A project can be compiled in more than one place, e.g. on jazyk.dev and on a laptop, and
the places meet only through git. The committed files are designed for that:

- No committed file carries a counter, a generation, or a local path, so two places that
  did the same thing write the same bytes.
- Node ids and record names come from natural keys
  ([identifiers](./model.md#identifiers)): different nodes never share a file, and the
  same node lands in the same file.
- Everything one place counts for itself (the generation, the journal, costs, controls)
  is in `local/`, and everything rebuilt is in `derived/`. A merge never touches them.

What still meets in one file is a real overlap: both places changed the same node,
section record, change record, or ledger row. The merge driver `jazyk merge-file`, named
in `.gitattributes` and registered in the repository's git configuration by
`jazyk setup` and by every build, merges a set file field by field:

- Fields only one side changed take that side.
- Lists that are sets (`aliases`, `entities`, `members`, `detail.judged`) take the union.
- A ledger row takes the side with the newer verdict.
- A section record takes the side whose `raw` matches the merged document on disk.
- Any other field both sides changed differently keeps the current branch's value, and
  the driver writes a `merged` change record on the node, so the next build
  re-reconciles its source instead of leaving a silent choice
  ([change records](./graph.md#change-records)).

Without the driver registered, git merges the files as text, and a conflict is resolved
by hand like any YAML conflict.

Two rules keep places from fighting:

- Compile a given document change in one place, then pull it everywhere else. Two places
  compiling the same edit reach the same ids but write different model text
  (`reasoning`, `definition`), and every such node becomes an overlap.
- Run the same store version everywhere. A binary that finds a store newer than its own
  refuses to write and asks for an upgrade; it never archives it
  ([store version](./graph.md#store-version)).

The generated `AGENTS.md` block outside `.jazyk/` follows the same rule: the same
engine version writes the same block, so it merges cleanly.
