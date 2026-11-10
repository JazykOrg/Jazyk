# The .jazyk folder

Every file Jazyk keeps for a project, what it is for, its format, an example, where it
lives, and whether a repository commits it. The rules behind the graph files are in the
[graph store](./graph.md#storage-layout).

## What is committed

A repository commits the current state of the graph, never how it got there:

- The graph: the nodes the documents state (entities, requirements, criteria, views,
  diagnostics, redirects).
- The decisions: judgments made on purpose so a later build does not ask again (two
  similar entities kept apart, a pair of requirements judged consistent, an entity
  reviewed). Each decision names what it judged, so it lapses by itself when that
  changes.
- The sources: each document's sections as the graph last reflected them. They are what
  tells a build which sections changed since.
- The ledger: which deliverable files implement which requirement, and their last
  verdict.

History is what git is for. Work to do is never stored: every build computes it again
from the committed state and the documents on disk
([what needs building](./graph.md#what-needs-building)). A build's own progress (the batch
it is on, what it parked, its spend) lives in memory, in the build's run file on a local
machine, or in the host's database when hosted, and is gone when the build ends.

## Zones

- **Committed**: `.jazyk/` and the set directory `.jazyk/<set-id>/`, except the two
  folders below.
- **`local/`**: what one machine's runs leave behind: the commit counter, the last
  verdict, costs, controls, the journal, locks, an interrupted build's run file. Never
  committed.
- **`derived/`**: output rebuilt from the committed files: derived graph kinds, diagrams,
  docsgen. Deleting it loses nothing. Never committed.
- **The log directory**: transcripts, chats, feedback, replay snapshots. Outside the
  project, never committed ([logs](#logs)).

## Layout

```
.jazyk/
  jazyk.toml                         committed   project configuration
  .gitignore                         committed   generated: what a repository commits
  .gitattributes                     committed   generated: the merge driver for set files
  README.md                          committed   generated: this layout in brief
  local/                             local       project machine state
  <set-id>/                          one document set
    store.yaml                       committed   store version
    graph/entities/<id>.yaml         committed   one entity
    graph/requirements/<id>.yaml     committed   one requirement
    graph/criteria/<id>.yaml         committed   one criterion
    graph/views/<id>.yaml            committed   one curated view
    graph/diagnostics/<id>.yaml      committed   one finding or question
    graph/redirects/<id>.yaml        committed   one merged id
    graph/decisions/lookalikes/<pair>.yaml  committed   two similar entities kept apart
    graph/decisions/pairs/<pair>.yaml       committed   two requirements judged together
    graph/decisions/reviews/<id>.yaml       committed   one entity reviewed
    graph/decisions/conformance/<id>.yaml   committed   one instance checked against its type
    graph/decisions/structure/<id>.yaml     committed   one requirement judged not structural
    sources/<doc path>.yaml          committed   one document as the graph last reflected it
    gen/ledger.yaml                  committed   the deliverable-wide ledger keys
    gen/entities/<slug>.yaml         committed   one entity's generation state
    gen/requirements/<id>.yaml       committed   one requirement's binding and verdict
    gen/criteria/<slug>.md           committed   one judged criterion's test setup
    local/                           local       this machine's state
    derived/                         local       rebuilt output
```

`<set-id>` is the set's id from `jazyk.toml`: generated as `ds_` and 32 hex digits when
a set is created (e.g. `ds_7b91362a262947c0b363a30768ad419b`), or written by hand
(e.g. `ds_main`). It never changes; the set's friendly `name` lives beside it in
`jazyk.toml` and can be renamed freely ([document sets](./document-sets.md)).

`<id>`, `<pair>`, and `<doc path>` are [file ids](./graph.md#node-files): `:` written `~`,
`/` written `+`, other bytes `%XX`. A pair is the two ids joined by `~`, smaller first.

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

Each schema URL is published on jazyk.org ([schemas](../site.md#schemas)).

## Project files: `.jazyk/`

### `jazyk.toml`

The project configuration: document sets, globs, models, workflow defaults
([project settings](./project-settings.md)). Written by a person or `jazyk setup`.
Committed.

```toml
version = 1
[[sets]]
id = "ds_main"
name = "Main"
docs = ["docs/**/*.md"]
roots = ["docs/main.md"]
```

### `.gitignore`

Generated. Ignores everything, then keeps the committed paths by name, so a new kind of
output never enters a commit by accident. Committed.

```
*
!/jazyk.toml
!/.gitignore
!/.gitattributes
!/README.md
!/ds_*/
!/ds_*/store.yaml
!/ds_*/graph/
!/ds_*/graph/**
!/ds_*/sources/
!/ds_*/sources/**
!/ds_*/gen/
!/ds_*/gen/**
.*.tmp
```

### `.gitattributes`

Generated. Committed.

```
/ds_*/** merge=jazyk
```

When a merge finds a file under a set directory changed on both sides, git runs the
merge driver named `jazyk` instead of its own line-by-line merge
([merging through git](#merging-through-git)). Git takes the driver's command from the
repository's own configuration, which a repository cannot commit, so `jazyk setup` and
every build register it there. Where it is not registered (a fresh clone before the
first Jazyk command, a merge on GitHub's website), git merges the file as plain text.

### `README.md`

Generated. This layout in brief: what is committed, what is local, what is safe to
delete. Committed.

### `local/`

Project machine state. Never committed.

| File | What it is | Format |
|---|---|---|
| `project.lock` | Coordinates project operations across processes. | empty, advisory lock |
| `ide.json`, `ide-installer/` | The editor adapters' executable path and plugin installs ([editor setup](../frontends/lsp.md#editor-setup)). | JSON, files |
| `removed-sets/<id>.toml` | A removed set's definition, kept for restore ([document sets](./document-sets.md)). | TOML |
| `archive/<set-id>-v<N>/` | A store of another version, archived whole by a build ([store version](./graph.md#store-version)). | a set directory |

## Committed set files: `.jazyk/<set-id>/`

### `store.yaml`

The store version, the only thing a load checks before reading the rest
([store version](./graph.md#store-version)).

```yaml
# Jazyk store: the version of this set's files. Written by jazyk.
# yaml-language-server: $schema=https://jazyk.org/schemas/store.json
version: 5
```

### `graph/<kind>/<id>.yaml`

One node of the [graph](./model.md). The file holds the node's own facts only: no
generation markers, and no mention a requirement adds by reference
([node files](./graph.md#node-files)).

| Kind | Header sentence |
|---|---|
| `entities/` | `Jazyk entity: one concept your docs name.` |
| `requirements/` | `Jazyk requirement: one obligation stated in your docs.` |
| `criteria/` | `Jazyk criterion: how one would observe a requirement is met.` |
| `views/` | `Jazyk view: what one diagram shows, curated.` |
| `diagnostics/` | `Jazyk diagnostic: a finding or question about your docs.` |
| `redirects/` | `Jazyk redirect: an id merged into another.` |

Every header sentence ends with ` Written by jazyk: edit the docs, not this file.`

An entity:

```yaml
# Jazyk entity: one concept your docs name. Written by jazyk: edit the docs, not this file.
# yaml-language-server: $schema=https://jazyk.org/schemas/entity.json
id: ent:analytics-service
name: Analytics Service
stereotype: service
parent: ent:back-office
```

A requirement, with the sentence it came from:

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

A criterion, what a test checks:

```yaml
# Jazyk criterion: how one would observe a requirement is met. Written by jazyk: edit the docs, not this file.
# yaml-language-server: $schema=https://jazyk.org/schemas/criterion.json
id: crit:services-4h2c-m2
requirement: req:services-4h2c
statement: Listing the Platform's services returns sixteen services.
given: The Platform is deployed.
when: Its services are listed.
then: Exactly sixteen services are listed.
provenance:
  derived:
    from: [req:services-4h2c]
    reasoning: the count the requirement states, made observable
```

A curated view:

```yaml
# Jazyk view: what one diagram shows, curated. Written by jazyk: edit the docs, not this file.
# yaml-language-server: $schema=https://jazyk.org/schemas/view.json
id: view:class/data-layer-neighbours
kind: class
title: 'Data Layer: neighbours'
members:
- ent:billing-service
- ent:reporting-service
```

A diagnostic: a finding, a question for a person, or a goal a build could not finish:

```yaml
# Jazyk diagnostic: a finding or question about your docs. Written by jazyk: edit the docs, not this file.
# yaml-language-server: $schema=https://jazyk.org/schemas/diagnostic.json
id: diag:goal-failed-3kq8
rule: goal-failed
severity: warning
subjects:
- docs/system.md#/platform
message: 'reconcile-section on docs/system.md#/platform failed: the section states two contradictory uptimes'
lifecycle: open
```

A redirect:

```yaml
# Jazyk redirect: an id merged into another. Written by jazyk: edit the docs, not this file.
# yaml-language-server: $schema=https://jazyk.org/schemas/redirect.json
id: ent:purchase
to: ent:order
```

### `graph/decisions/<kind>/<id>.yaml`

A judgment a session or a person made on purpose, kept so no later build asks again.
Each decision records a hash of what it judged; once that changes, the decision no
longer applies and the question comes back by itself. A decision with no subject left in
the graph is deleted by the sweep.

| Kind | Decides | Lapses when |
|---|---|---|
| `lookalikes/<pair>.yaml` | Two entities with similar names are different things ([dedupe](./goals/dedupe-candidates.md#verdicts-are-remembered)). | Their lookalike score rises above the score judged. |
| `pairs/<pair>.yaml` | Two requirements were judged against each other: consistent, or a diagnostic was filed ([rejudge-pair](./goals/rejudge-pair.md)). | Either statement changes. |
| `reviews/<id>.yaml` | An entity's definition, parent, and attributes were reviewed against its requirements ([review-entity](./goals/review-entity.md)). | The entity's facts change. |
| `conformance/<id>.yaml` | An instance was checked against its type ([conform-instance](./goals/conform-instance.md)). | The instance or its type changes. |
| `structure/<id>.yaml` | A requirement naming several entities states no structural link between them ([declare-edges](./goals/declare-edges.md)). | The statement changes. |

```yaml
# Jazyk decision: two similar entities judged to be different things. Written by jazyk.
# yaml-language-server: $schema=https://jazyk.org/schemas/lookalike.json
id: ent:audit-log~ent:audit-trail
verdict: separate
score: 0.5
```

```yaml
# Jazyk decision: two requirements judged against each other. Written by jazyk.
# yaml-language-server: $schema=https://jazyk.org/schemas/pair.json
id: req:orders-holds-4k7q~req:payment-refund-2m9d
verdict: consistent
statements:
  req:orders-holds-4k7q: 9f2c41aa07b3d118
  req:payment-refund-2m9d: 41d0e7c2b9aa6f30
```

```yaml
# Jazyk decision: an entity reviewed against its requirements. Written by jazyk.
# yaml-language-server: $schema=https://jazyk.org/schemas/review.json
id: ent:order
facts: 554cea9f27e74ea7
```

### `sources/<doc path>.yaml`

One source document's sections as the graph last reflected them. A build compares the
document on disk with this file to know which sections changed since, and to move a
fact whose section was renamed or split ([alignment](./alignment.md)). No line numbers
and no whole-file hash, so an edit to one section changes only that section's entry.

```yaml
# Jazyk source: docs/system.md as the graph last reflected it, used to spot edits. Written by jazyk.
# yaml-language-server: $schema=https://jazyk.org/schemas/source.json
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
row: which deliverable files implement which requirement, and what the last test run
said.

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

## Local set files: `.jazyk/<set-id>/local/`

What this machine's runs leave behind. A fresh clone has none of it and works without it
([a fresh checkout](./graph.md#a-fresh-checkout)).

| File | What it is | Format |
|---|---|---|
| `build.yaml` | The commit counter, the last build's verdict, costs, spend, open diagnostic counts, and each document's content hash at last reconcile (a fast check before comparing sections). Its counter is the commit point readers check. | YAML |
| `run.yaml` | An interrupted build: its batch, the goals it parked, its spend so far. A build that ends deletes it; the next one resumes from it or starts over. Hosted builds keep the same in the host's database. | YAML |
| `control.yaml` | Workflow modes and the releases this operator approved ([control plane](./control-plane.md#modes-and-releases)). | YAML |
| `journal/g<N>.yaml` | One entry per commit on this machine: its mutations, the goals it resolved, its cost ([journal](./graph.md#journal)). What `jazyk explain`, `ripple`, and the cost report read. | YAML |
| `workers/<id>.yaml`, `leases/<batch>.yaml` | Who is attached and which goal batch each holds ([workers and leases](./control-plane.md#workers-and-leases)). | YAML |
| `decompile/drafts.yaml` | Draft documents awaiting the owner ([decompile](../consumers/decompile.md)). | YAML |
| `deliverable-baseline/` | Deliverable files as they were before a generation run touched them ([gen](../consumers/gen.md)). | files |
| `.lock`, `.leases.lock`, `.committing` | The commit lock, the lease lock, and the marker present while a commit writes. | empty |

```yaml
# Jazyk build state: this machine's commit counter, last verdict, and costs. Written by jazyk, never committed.
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
# Jazyk control: workflow modes and approved releases on this machine. Written by jazyk, never committed.
# yaml-language-server: $schema=https://jazyk.org/schemas/control.json
compile: manual
generate: manual
worker: any
released:
  compile:
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
resolved_goals:
- {goal: g:reconcile-section:docs/system.md#/platform, justification: "One new obligation extracted; section covered."}
tokens: 4100
```

## Derived set files: `.jazyk/<set-id>/derived/`

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

```json
{"at":"2026-11-10T09:12:00Z","kind":"confusing","subject":"upsert_criterion","message":"The given/when/then fields read as required for a derived criterion."}
```

## Merging through git

A project can be compiled in more than one place, e.g. on jazyk.dev and on a laptop, and
the places meet only through git. The committed files are designed for that:

- They hold the current state only: no counter, no generation, no work queue, no history,
  no local path. Two places that did the same thing write the same bytes.
- Node ids and decision names come from natural keys
  ([identifiers](./model.md#identifiers)): different nodes never share a file, and the
  same node lands in the same file.
- After a merge, the next build computes what needs building from the merged state, the
  same as after any edit. Nothing is carried over that could disagree with it.

What still meets in one file is a real overlap: both places changed the same node,
decision, source, or ledger row. The merge driver `jazyk merge-file` merges such a file
field by field:

- Fields only one side changed take that side.
- Lists that are sets (`aliases`, `entities`, `members`) take the union.
- A decision takes the side whose hashes match the merged graph; a ledger row takes the
  side with the newer verdict.
- A source takes the side whose `raw` matches the merged document on disk.
- Any other field both sides changed differently keeps the current branch's value, and
  the driver drops the `hash` of the node's source section in `sources/`, so the next
  build sees that section as changed and reconciles it again. Nothing is chosen
  silently.

Two rules keep places from fighting:

- Compile a given document change in one place, then pull it everywhere else. Two places
  compiling the same edit reach the same ids but write different model text
  (`reasoning`, `definition`), and every such node becomes an overlap.
- Run the same store version everywhere. A binary that finds a store newer than its own
  refuses to write and asks for an upgrade; it never archives it
  ([store version](./graph.md#store-version)).
