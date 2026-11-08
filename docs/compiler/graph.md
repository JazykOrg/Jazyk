# Graph store

A project opens all [document sets](./document-sets.md). Graph operations address one set
explicitly. Each set owns `.jazyk/<set-id>/`; project file operations share the
physical files and report every membership. The document-set contract defines
configuration, aggregate reads, management commands, and grouped compilation.

The graph store is the persistent home of the [semantic graph](./model.md). It owns
identifiers, enforces invariants, recomputes derived data, records every change, and
writes the typed dirtiness each commit causes. The store is deterministic code. No LLM
runs inside it. One graph per project, edited in place, never regenerated.

## Storage layout

The store lives in the set's out directory (`.jazyk/<set-id>/`, beside
`jazyk.toml`; `--out DIR` moves it). All files are YAML, keys in a fixed order and maps sorted by key, so
builds diff cleanly in git. The schema is [`graph.schema.yaml`](./graph.schema.yaml).
The directory is a dotfolder so it stays out of the way in a project's listing, the
same convention as `.git`. The generated `.jazyk/.gitignore` decides what a repository
commits ([committed and local state](#committed-and-local-state)). A `jazyk-out/`
directory from before the name changed is never read and never compiled
([glob](./project-settings.md#glob)): move it to `.jazyk/` to keep its graph, or leave
it and the next build reconciles from empty.

```
.jazyk/<set-id>/
  status.yaml            # version, stamped root documents, anchors marked for
                         # re-evaluation
  changes/<kind>/<subject file id>.yaml  # one open change record
  parked/<goal file id>.yaml             # one parked goal
  failed/<goal file id>.yaml             # one failed goal
  alignment/<doc file id>.yaml           # one document's pending alignment proposals
  lookalikes/<pair file id>.yaml         # one lookalike verdict
  build.yaml             # local: generation, verdict, costs, budgets spent, open
                         # diagnostic counts by severity, document content hashes
  markers.yaml           # local: the created and updated generation of every node
  graph/                 # one file per node, see node files
    entities/<file id>.yaml        # one entity
    requirements/<file id>.yaml    # one requirement
    criteria/<file id>.yaml        # one criterion
    views/<file id>.yaml           # one curated view
    diagnostics/<file id>.yaml     # one diagnostic
    redirects/<file id>.yaml       # one absorbed id: `to: <surviving id>`
    default-views/<file id>.yaml   # one default view (derived, rewritten on every commit)
    relationships/<file id>.yaml   # one relationship (derived, rewritten on every commit)
    state-machines/<file id>.yaml  # one state machine (derived, rewritten on every commit)
  docs/
    <mirrored doc path>.yaml   # per document: section tree, coverage
  journal/
    g<generation>.yaml   # one file per generation
  diagrams/
    <kind>/<slug>.puml   # one per view, rewritten on every commit
    <kind>/<slug>.svg    # its rendering (.png beside it on demand)
  docsgen/               # generated documentation, see consumers/docsgen.md
  gen/                   # the generation ledger, see consumers/gen.md
  trace/                 # session transcripts
  sessions/
    <session id>.jsonl   # one chat conversation, see frontends/acp.md#session-store
  snapshots/             # per-batch graph copies for replays, see sessions.md#snapshots
  control.yaml           # modes and releases, see control-plane.md
  workers/, leases/      # who is acting, see control-plane.md
  feedback.jsonl         # report_feedback entries
  .lock                  # single-writer lock
```

The store owns the status, `build.yaml`, `markers.yaml`, `graph/`, `docs/`, and `journal/`. `diagrams/` is build
output the renderer rewrites after every commit ([output layout](./diagrams.md#output-layout));
deleting it loses nothing. The rest belongs to the control plane, the frontends, and the
consumers.

Each document file under `docs/` holds:

- `sections`: map of internal reference → `title`, `kind`, `order`, `parent`, `raw`,
  `hash`.
- `coverage`: map of internal reference → `state`, `note`, `claimedBy`. See
  [coverage](./compilation.md#coverage).

A record holds no line numbers and no whole-file hash, so an edit to one section
changes only that section's entry. A load recomputes each section's `lines` from the
`raw` texts in document order. The content hash of the source file at last reconcile is
local, in `build.yaml`.

The status is what goal derivation reads, and nothing that changes when the graph does
not. It is `status.yaml` and one directory per kind of record, one file per record:

- `status.yaml`:
  - `version`: the store version, `4`.
  - `roots`: the documents matching the project [roots](./project-settings.md#roots),
    stamped by the build after the section trees sync. Commits outside a build (edits,
    answers, MCP sessions) read the stamp to order documents by
    [link level](./reconciler.md#link-levels) when they recompute derived data.
  - `reevaluate`: anchors placed with `reevaluate`, listed as stale until addressed.
- `changes/<kind>/<subject file id>.yaml`: one open [change record](#change-records).
  A kind and a subject hold at most one record, so the pair names the file.
- `parked/<goal file id>.yaml`: one goal left open when a budget ran out, the whole goal
  record. `failed/<goal file id>.yaml`: `{goal, reason}` for one goal a session marked
  failed, `goal` the whole record. Both persist whole, `change` payload included, so a
  parked or failed goal survives a re-derivation that would otherwise drop it. See
  [parked and failed](./reconciler.md#parked-and-failed).
- `alignment/<doc file id>.yaml`: one document's pending
  [alignment proposals](./alignment.md#what-applies-and-what-is-proposed).
- `lookalikes/<pair file id>.yaml`: one entity pair a session kept apart, `{pair,
  verdict, score, generation}`, the memory that keeps a judged pair off the board while
  its score holds ([verdicts are remembered](./goals/dedupe-candidates.md#verdicts-are-remembered)).

Record files are named like node files ([node files](#node-files)) and start with the
`id` they are named by: the subject, the goal id, the document path, or the pair. Two
branches that dirty different subjects, park different goals, or judge different pairs
write different files.

`build.yaml` holds what one checkout's runs leave behind. It is local
([committed and local state](#committed-and-local-state)):

- `generation`: bumped on every commit. See [the generation](#the-generation).
- `verdict`: `{state, open, failed, blocked, optional}`, the last build's
  [convergence](./compilation.md#convergence) with its counts.
- `costs`: `{sessions, tokens, by_kind, by_class}` for the last build, one
  `{sessions, tokens}` line per goal kind and per class (`compile`, `gc`). The cost
  of every build, by goal kind and by document, is read from the [journal](#journal)
  instead ([`jazyk status --costs`](../frontends/cli.md#jazyk-status)).
- `spent`: `{sessions, rounds, tokens}`, the budgets the last build used.
- `diagnostics`: open diagnostic counts by severity, suppressed excluded.
- `docs`: map of document path → the content hash of the source file at last
  reconcile. A document without an entry (a fresh checkout, a document another branch
  changed) takes part in [alignment](./alignment.md) as changed. Its unchanged sections
  match their records exactly, so nothing dirties.

`markers.yaml` maps every node id to `[created, updated]`, its
[generation markers](./model.md#shared-fields). It is local for the same reason as the
generation: the numbers belong to one checkout. A node without an entry (a fresh
checkout, a node another branch committed) reads with no markers.

Readers see one status: a load reads the status and `build.yaml`. Two branches that
built without changing the graph commit the same status bytes.

### Node files

Every node of the graph is one file under `graph/<kind>/`, so two branches that change
different nodes touch different files and git merges them without a conflict.

- The file name is the file id: the node id with `:` written as `~` and `/` written as
  `+`. Lowercase ASCII letters, digits, `-`, `_`, and `.` stay as they are. Every other
  byte (an uppercase letter, a byte of a non-ASCII character, a literal `~`, `+`, or `%`,
  a leading `.`) is written `%XX` in uppercase hex. E.g.:

  ```
  ent:order            → graph/entities/ent~order.yaml
  view:class/shop      → graph/views/view~class+shop.yaml
  ent:čaj              → graph/entities/ent~%C4%8Daj.yaml
  rel:order~payment    → graph/relationships/rel~order%7Epayment.yaml
  ```

- The encoding is reversible: the load reads each id back from its file name. No two
  ids share a name, on a case-insensitive file system too.
- A file id longer than 180 bytes is shortened: its first 160 bytes (cut before an
  escape it would split), then `%~`, then the 16 lowercase hex digits of the 64-bit
  FNV-1a hash of the whole id. E.g.: `ent~a-very-long-name…%~3f2a9c0d1e4b5a67.yaml`.
  The file then holds the full id under `id`, and the load reads the id from there.
  The plain form never writes `%~`, so a shortened name never meets a plain one; two
  shortened ids meet only when both their prefixes and their hashes agree.
- A file starts with the node's full `id`, then the node's fields in a fixed order, maps
  sorted by key, ending in a newline, so a file read alone, piped, or pasted says which
  node it is. A redirect file holds `id` and `to: <surviving id>`. The load checks that
  the `id` matches the file name; a mismatch makes the store read-only. Nothing in a
  file changes unless the node's own facts change: the same graph always writes the
  same bytes.
- A file holds only what the node states. Two kinds of field are derived and stay out
  of it, so a commit elsewhere in the graph never rewrites it:
  - `created` and `updated` live in the local `markers.yaml`.
  - An entity's `mentions` omit every mention that equals the `source` of a requirement
    referencing the entity ([derived data](#derived-data)). A load adds those sources
    back, after the written mentions, in requirement id order.
- A save writes the files whose bytes changed and deletes the files of nodes that left
  the graph. A file whose name does not decode, or whose content does not parse, makes
  the store read-only ([writes and unreadable files](#writes-and-unreadable-files)).
- Derived kinds (`default-views/`, `relationships/`, `state-machines/`) are local
  ([committed and local state](#committed-and-local-state)). A load that finds any of
  these directories missing recomputes all three from the facts in memory. The next
  commit writes them. A view stops being derived the moment a mutation names it
  ([default views](./model/view.md#default-views)): it moves from `default-views/` to
  `views/` and is committed from then on.

### Committed and local state

A repository commits the state a checkout needs to resume compiling without a rebuild.
Everything else is local: rebuilt on demand, or a log of what ran.

- Committed, per set:
  - The status: `status.yaml`, `changes/`, `parked/`, `failed/`, `alignment/`,
    `lookalikes/`. Goal derivation reads it.
  - `graph/` except the derived kinds: entities, requirements, criteria, curated views,
    diagnostics (open and resolved, so a judgment stays sticky across checkouts),
    redirects.
  - `docs/`: the section trees alignment compares against.
  - `gen/`: the accepted generation and verification evidence
    ([the ledger](../consumers/gen.md#the-ledger)) and the criteria files its `llm`
    rows name.
- Local, per set:
  - `build.yaml`: the generation, the last build's verdict, costs, and spend, and the
    document content hashes. These change on every run, so two branches would conflict
    on them at every merge.
  - `markers.yaml`: the generation markers, local like the generation they name.
  - `graph/default-views/`, `graph/relationships/`, and `graph/state-machines/`:
    derived, recomputed at load when missing and on every commit.
  - `diagrams/` and `docsgen/`: deterministic output, rewritten at every build's tail.
  - `journal/`: the history of this checkout. Every commit mints `g<generation>.yaml`,
    so two branches add different files under the same names, and a merge would
    conflict on every one. See [a fresh checkout](#a-fresh-checkout).
  - `trace/`, `sessions/`, `snapshots/`, `feedback.jsonl`: logs.
  - `control.yaml`, `workers/`, `leases/`, `.lock`: who is acting here and what this
    operator released. The workflow defaults come from `jazyk.toml`, which is
    committed ([modes and releases](./control-plane.md#modes-and-releases)).
  - Everything else under the out directory (decompile drafts, benchmark results,
    previews, deliverable baselines, temporary files): local by default.
- Local, per project: `sessions/`, `runs/`, `removed-sets/`, `project.lock`, `trace/`,
  `ide.json`, `ide-installer/`, and the `.bak` archives of other store versions.
  The [editor adapters](../frontends/lsp.md#editor-setup) use `ide.json` for the
  local executable and `ide-installer/` for isolated plugin installation.

The generated `.jazyk/.gitignore` states this split
([persistence and identity](./document-sets.md#persistence-and-identity)). The
[hosted runtime](./hosted-runtime.md#source-and-delivery-projections) publishes the
same committed files.

### A fresh checkout

A checkout carries the committed state and none of the local state. The first command
loads it as is. Nothing starts from empty.

- A document edited outside Jazyk (on GitHub, in another editor) holds sections that
  hash differently from its committed record under `docs/`. The next sync marks those
  sections dirty, and only they reconcile ([dirty set](./reconciler.md#dirty-set)). A
  document without a local content hash is compared section by section; one whose
  sections all match dirties nothing.
- The derived kinds and the by-reference mentions are recomputed at load. `diagrams/`
  and `docsgen/` are rewritten at the next build's tail.
- `markers.yaml` is absent: nodes read with no `created` or `updated` until a commit
  touches them.
- The journal starts empty and continues at the committed generation. History from
  before the checkout is absent: `jazyk ripple` and `jazyk explain` see only local
  generations, [flip detection](./reconciler.md#flip-detection) counts moves from the
  checkout on, and `retract_decree` on a decree made elsewhere finds no prior value. It
  deletes the decreed fact, and the next reconcile of its section extracts the quoted
  fact again.
- `control.yaml` is absent: modes read the `jazyk.toml` defaults and nothing is
  released.
- `build.yaml` is absent: the verdict and the costs read empty until the next build,
  and the generation starts at the floor ([the generation](#the-generation)).

### The generation

The generation is a local counter. Two branches that commit from one base each bump it
from the same number, so a committed counter would conflict on every parallel commit.
Nothing in the status counts commits, and its files merge cleanly when the branches
changed different things.

The journal files the store mints from the generation must not repeat the ones a
merged branch committed. A load therefore takes the generation as the larger of
`build.yaml` and the floor: the highest generation any change record or lookalike
verdict in the status names. The next commit writes the result to `build.yaml`. A
writer's `store-moved` check compares the same value. No node id and no record id
carries a counter ([identifiers](./model.md#identifiers)), so two branches never mint
one id for two different things.

Node markers (`created`, `updated`) are local and keep the generation of the checkout
that wrote them. `jazyk ripple` follows the local journal only.

### Writes and unreadable files

Every file the store writes (a node file, a doc record, a status file, a journal entry,
`build.yaml`) lands through a sibling temporary file renamed into place, so a reader or
a crash never sees a half-written file. Native replacement preserves the existing
file permissions. `save` writes `build.yaml` last: its generation
is the commit point a reader checks. A write that fails is reported with its path and the error,
never dropped.

A YAML write skips a file whose serialized bytes already equal the file on disk. A save
rewrites only the node files and doc records that changed, so a commit that touches one
node leaves the others in place, and a save with nothing changed writes nothing. The
order holds: changed node files and records first, then the deletions of removed nodes,
then the status files and the deletions of cleared records, then `markers.yaml`, then
`build.yaml`. A commit always changes `build.yaml` (its
generation moves), so the commit point is always written after the files it covers. A reader's generation check stays sound: an unchanged file
reads the same before and after the commit.

A file that exists but does not parse makes the store read-only. The load reports the
file and the parse error, reads continue with what did parse, and every writer
(`apply`, the sweep, record clearing, a persisting alignment) refuses with
`store-unreadable` naming the file until it is repaired or removed. A node file is never
read as empty because it failed to parse: that would let the next commit overwrite it,
and the sweep would collect the requirements of a document whose record did not load.

### Store version

`status.yaml` carries `version: 4`. Version 4 is the layout above: the status split into
one file per record, generation markers and by-reference mentions kept out of node
files, default views local, document records without line numbers or content hashes,
and the ledger split per row ([the ledger](../consumers/gen.md#the-ledger)).

- A version 3 store (the node files, with the status, the markers, the default views,
  and the ledger each in one place) loads as it is and is written in the version 4
  layout by its next save. Its ids stay: ids are immutable, and a version 3 id keeps
  resolving. Only ids minted from then on take the version 4 form.
- An out directory whose `status.yaml` carries any other version, or none, is archived
  whole to `<out>.bak` (a sibling directory; an existing `.bak` is replaced) at the
  start of a build, and the store starts empty. The next build reconciles from the
  empty graph, the same way a first build does. Outside a build, such a store reads as
  empty and read-only: a writer refuses with `store-version` until a build archives it.
- A `status.yaml` that exists and does not parse is not a version mismatch. The store
  is read-only with `store-unreadable` naming the file, and a build does not archive
  it: a torn or hand-broken file never costs the graph.
- When the archive itself fails (the `.bak` cannot be replaced, the rename is refused),
  the build does not proceed over the old directory: the store is read-only and the
  error names the directory.

## Mutations

A mutation is one operation on the graph. The full set:

- `upsert_entity`, `update_entity`, `delete_entity`, `merge_entities`
- `upsert_requirement`, `update_requirement`, `delete_requirement`
- `upsert_criterion`, `update_criterion`, `delete_criterion`
- `upsert_view`, `update_view`, `delete_view`
- `place_anchor`, `orphan_anchor`
- `report_diagnostic`, `update_diagnostic`, `resolve_diagnostic`
- `set_coverage`
- `edit_doc_prose`
- `edit_fact`, `retract_decree`, `bump_limit`

Sessions reach the first six groups as [write tools](./tools.md#write-tools) and
[view tools](./tools.md#view-tools). The last two groups are human paths: the
[chat tools](./tools.md#chat-tools) and the GUI inspector stage them. `mark_goal_done` and
`mark_goal_failed` are not mutations: they record goal resolutions in the journal and
clear or keep change records ([goal tools](./tools.md#goal-tools)). Relationships, state
machines, and default views have no mutation at all: they are
[derived data](#derived-data).

Their semantics:

- Upserts key on a natural key, not an id. For entities the natural key is `name` plus
  `scope`, and `parent` joins the key when the caller supplies it. An upsert without
  `parent` matches when exactly one entity with that name and scope exists, whatever its
  parent; several matches is an error naming the candidates and asking for `parent`. See
  [the natural key under containment](./concepts/identity.md#the-natural-key-under-containment).
  For requirements the natural key is the source section plus the punctuation-insensitive
  `statement`, so a punctuation or spacing edit to a sentence matches its existing
  requirement and refreshes the `statement` and `quote` in place. Punctuation-insensitive
  means case, spacing, and punctuation fall away; a comparison operator, a sign, and a
  decimal point stay, so `under 30 s` and `under 3.0 s`, or `> 30` and `< 30`, are
  different statements. A derived requirement
  keys on its punctuation-insensitive `statement` within its `from` set; a decreed one
  keys on the statement alone. For criteria the natural key is the owning requirement
  plus the punctuation-insensitive `statement`, whatever the provenance. For views the
  natural key is `kind` plus `title`. An
  upsert that matches an existing node updates it instead of creating a duplicate. This
  makes retries harmless.
- The store mints ids from natural keys: `ent:<slug>`, `req:<stem>-<hash>`,
  `crit:<stem>-<hash>`, `view:<kind>/<slug>`, `diag:<rule>-<hash>`. No id carries a
  counter, so two branches that create different nodes never mint the same id, and two
  branches that extract the same statement mint the same one. A mutation never
  supplies a new id, only references existing ones. See
  [identifiers](./model.md#identifiers).
- Every staged create carries exactly one [provenance](./model.md#provenance): a located
  quote (the `mention` of an entity, the `source` of a requirement, the `provenance` of
  an attribute), a derivation (`from` naming live nodes, with `reasoning`), or a decree
  (author, time, note). A session can stage quotes and derivations; only a human path
  stages a decree. Nothing enters the graph without provenance.
- An attribute staged without its own provenance takes the quote of the call that
  carries it.
- Deletes require a `reason`, which is recorded in the journal.
- `merge_entities` keeps one entity, absorbs the other, rewires every reference
  (requirement `entities`, `edges`, `transition.subject`, view `members`, `excluded`,
  `collapse`, the `parent` of the absorbed entity's children), unions aliases, mentions,
  and attributes (the survivor's attribute stands on a name clash, journaled), and writes
  a redirect from the absorbed id to the survivor. Downstream consumers holding the old id
  follow the redirect. A merge that would make the survivor its own ancestor is rejected.
- `upsert_view` creates or refreshes a view by `kind` and `title`. `update_view` sets
  `title`, `members` (the whole ordered list), `add_members`, `remove_members`, `query`,
  `collapse`, and `exclude` (an `{id, note}` pair). A default view carries the boolean
  field `default: true`, and any mutation that names the view clears it: `update_view`
  with any field, `edit_fact`, a decree, `bump_limit`, or an `upsert_view` that lands
  on the default's `kind` and `title`. From then on the recompute leaves the view
  alone. On a still-default view the recompute rewrites only `title` and `members`;
  its `excluded`, `collapse`, and `limits` survive it. `delete_view` requires a
  `reason`; on a default view it is refused, because the next commit would derive the
  view again. See [default views](./model/view.md#default-views).
- `place_anchor` rewrites one anchor's `doc` and `section` (and `quote` when given)
  and records the anchor under `status.yaml` `reevaluate` when asked to or when the
  quote does not locate. `orphan_anchor` records nothing; the anchor's stale state
  stands. Both are staged only by [`place-anchors`](./goals/place-anchors.md) sessions,
  which commit them like any changeset and clear their document's `alignment` block.
  Exact moves never reach a session: [alignment](./alignment.md) rewrites them in the
  store and journals one `align` entry per build.
- `edit_doc_prose` rewrites one text run in one document section. It is never staged
  alone: the [dual-write tools](../frontends/acp.md#dual-write-tools) pair it with the
  graph mutation it carries, so the prose and the graph move in one changeset. At apply
  time the old text must still be present, or the pair skips with a report: a document
  edited underneath loses the edit, never its consistency. The applied edit re-parses
  the document and absorbs the new content hash and section hashes into the same
  commit, so the edit does not dirty the document it just reconciled. Any other anchor
  in the section whose quote stops locating goes stale the normal way; the safety
  net is ordinary dirty-section work. An `old_text` of empty inserts `new_text` after
  the section's last sentence, which is how a ratification proposal lands.
- `edit_fact` sets one field on one node (`statement`, `edges`, `transition`, `facets`,
  `definition`, `stereotype`, `parent`, an attribute's `type` or `value`, a view's
  `members`). On a default view it clears `default`. Paired with `edit_doc_prose` it
  is a dual write: the fact stays quoted,
  the quote pointing at the rewritten sentence. Staged alone on a quoted fact it is a
  decree: the fact's provenance becomes `decree`, the journal records the prior value
  and its source, and a `provenance-pending` change record opens
  [`ratify`](./goals/ratify.md). Staged on a derived or decreed fact it is a decree the
  same way. See [edit paths](./compilation.md#edit-paths).
- `retract_decree` undoes one decree. A node created by decree is deleted, journaled
  with the retraction as reason; quoted requirements that referenced a retracted entity
  re-point to its parent, and an entity that holds children dissolves as
  `dissolve_entity` would, the entry carrying the `dissolve_entity` mutation with the
  children moved ([retract](./goals/ratify.md#retract)). A field decreed over a quoted
  fact returns to the prior
  value and source recorded in the decree's journal entry. The open `ratify` goal closes
  with the record it stood on.
- `bump_limit` sets `limits.<limit>` on one entity or view to a value, recorded with
  decree provenance in the journal. On a default view it clears `default`. See
  [per-node bumps](#per-node-bumps).

## Changesets

Mutations are not applied one by one. A [session](./sessions.md#staged-mutations) stages
mutations into a changeset, and `done` runs the batch gates. The changeset commits
atomically:

- every staged mutation is applied,
- [derived data](#derived-data) is recomputed,
- the [garbage collection sweep](#the-sweep) runs,
- limit counts are taken and compared to thresholds,
- [change records](#change-records) are written for the dirtiness the commit caused,
  and the records of resolved goals are cleared,
- the [journal entry](#journal) is written with its resolved and opened goals,
- the generation counter in `build.yaml` is bumped,
- the renderer rewrites `diagrams/` ([rendering](./diagrams.md#rendering)).

Docsgen is not part of a commit: the requirements documents regenerate in the build tail
([a build](./compilation.md#a-build)).

If the session is aborted, the changeset is dropped and the graph is untouched. A session
that ends with valid staged work commits it ([commit](./sessions.md#commit)).

Sessions are not the only writers. A human save that dirties sections, alignment's
mechanical moves, a dual write, a decree, a ratification, a triage, an answer, and a check run each
commit through the same path and take a generation of their own. The
[journal kinds](#journal) name them.

While a mutation is staged, the same computation that derives goals at commit previews
the goals the mutation will open, and the tool reply says so
([bubbling](./reconciler.md#bubbling)). At commit the previews become goals with causes.

Commits serialize on `.lock`. At commit time the store reconciles staged creates against
nodes committed since the session's snapshot (a dual write, a decree): a staged create
whose natural key matches an existing node becomes an update on that node, with mentions
unioned.

## Validation gates

The store validates every mutation when it is staged and rejects invalid ones. A rejection
names the violated rule and how to repair the call, because the caller is a model that will
retry. E.g.:

```
unknown id `ent:cart`; nearest existing: `ent:shopping-cart`; use it, or create the entity first
```

The gates:

- Every referenced id must exist in the graph or earlier in the same changeset.
  Resolution is lenient toward intent: an id missing its `ent:` prefix, a case or
  spacing variant of an existing id, or an entity named by its exact name or alias,
  resolves to the unique matching node. Only genuine absence or ambiguity is an error,
  and the error names the nearest candidates.
- Every requirement carries at least one `facet`
  ([facets](./model/requirement.md#facets)); a create without one is refused.
- Every `quote` must appear verbatim in its named section. Locating is
  whitespace-insensitive and forgives markdown escapes (a model often
  writes `` \` `` for a backtick inside JSON); the stored quote is the form that
  locates in the source, so provenance stays verbatim to the document. A section's
  heading is its title, not body text: a quote that is the heading is refused with a
  message saying so, since a Codex session read `### Chapter 1: Arrival` in the loaded
  section and quoted it. On an entity's mention the message adds that an entity no body
  sentence names is not minted from its heading: a local model spent six of its
  twenty-four rounds trying to mint a Warehouse system from the page heading by other
  routes, each refused, and ran out one requirement short.
- An entity created with a `mention` enters with a sentence that names it: the quote
  carries a word of its name or of an alias (case, a plural ending, words under three
  letters, and `system` aside). A quote that never names it is refused as
  `mention-unnamed`; an entity the documents never name enters with `derived`
  provenance, or not at all. A local model minted a Shop «system» twice from "the system
  shall empty the Cart" on a page headed Shop, and 1 of 810 first mentions across the
  collected runs would have been refused, itself a concept the sentence never named.
- A requirement's `quote` is one sentence: a leading list number set aside, at most one
  of its sentences runs to four words or more (so a fact line such as `Dot Feeney. Age
  58. Occupation: ferry office manager.` stands). A quote spanning two sentences is
  refused as `quote-multi-sentence`, naming them: one requirement per sentence, each
  quoting its own. A code block or diagram quotes its own line and is exempt. A local
  model merged "built using React" and "communicates over a REST API" into one statement
  quoting both sentences, and 6 of 3310 quotes across the collected runs spanned two
  sentences, each a merge or a statement quoting its neighbour's sentence.
- In a [`reconcile-section`](./goals/reconcile-section.md) session, a mention or
  requirement source must cite the section being reconciled or its document. A quote
  from a document the prose merely links to is rejected (`wrong-document`): the fact is
  anchored by this document's own sentence, e.g. a link item's text. See
  [enumerations](./concepts/statements.md#enumerations).
- A derived provenance names at least one `from` node, every one of them live, and
  carries `reasoning`. A decree is rejected from a session.
- An entity name that looks like syntax rather than a concept (a file path, a CLI flag,
  a markdown term) is rejected unless the call carries an explaining `note`.
- `upsert_entity` with a name variant of an existing entity in the same scope is
  rejected toward reuse plus an alias, unless a `note` says how they differ or
  `parent` names the lookalike itself (a child concept declared as such). A variant
  is the same tokens (a plural counts as the same only for a grouping, since an area
  named `Orders` and the concept `Order` are two things; a grouping named
  `Departments` beside a stated `Department` is a twin), a name of two
  or more tokens contained in the other (`Order Service` beside `Order Service API`),
  or a shorter name that is one specific token of an existing name (`Billing` beside
  `Billing Handler`). A compound that extends an existing single name is a different
  concept and passes (`Employee Handbook` beside `Employee`).
  See [entity](./model/entity.md#what-is-an-entity).
- `stereotype` is free-form. No gate enumerates the allowed values; a stereotype is
  judgment recorded like any fact.
- `parent` must resolve to an existing entity, and the parent tree stays acyclic: an
  entity is never its own ancestor. Any such parent is accepted (invented structure
  carries derived provenance). No gate compares `parent` with committed `composition`
  edges: composition consistency is the `containment-mismatch` check, which asks that
  the part's `parent` and the whole be comparable (one contains the other, or they are
  the same node). See [containment](./model/entity.md#containment) and
  [checks](./compilation.md#checks).
- Attributes are named, unique by name within the entity, and each carries a provenance
  (or takes the call's quote).
- A requirement must reference at least one entity.
- `edges` are directional (`a` acts on `b`), `a` and `b` are distinct, and both are among
  the requirement's own `entities`. `type`, when given, is a
  [relationship type](./model/relationship.md#types); `cardinality`, when given, is one
  of `1`, `0..1`, `1..*`, `*`.
- `transition.subject` must exist and be among the requirement's `entities`; `from` and
  `to` are non-empty state names. See [transition](./model/requirement.md#transition).
- `facets` name a facet from `behavior`, `constraint`, `failure-mode`, `quality`, each
  with `reasoning`; `measure` is accepted only on `quality`. See
  [facets](./model/requirement.md#facets).
- A view's `kind` is from the [catalog](./model/view.md#kinds), its `title` is
  non-empty, and every id in `members`, `excluded`, and `collapse` exists. Members
  follow the kind's row in the catalog: entities for structural kinds (`composite`
  exactly one), requirements for flow kinds, one entity for `state`, one entity plus
  requirements for `timing`. `query.parent` resolves to an entity. See
  [membership](./model/view.md#membership).
- `delete_entity` is rejected while any requirement references the entity or any entity
  names it as `parent`. The error lists them.
- Diagnostic `subjects` must exist. A `decision` diagnostic carries a `prompt`.
- `place_anchor` and `orphan_anchor` accept only the ids in the session's proposals, and
  a `quote` passed to `place_anchor` must locate in the target section.
- `set_coverage` with state `non-normative` requires a `note`. A placeholder note
  (`<nil>`, `none`, `n/a`) counts as absent. Restaging coverage for a section supersedes
  the earlier staged mark: one coverage mark per section per changeset.
- `edit_fact` names an editable field. Ids, `created`, `updated`, `mentions`, and
  provenance itself are never edited directly.
- `bump_limit` names a limit from the [registry](#the-registry) that applies to the
  node's kind, with a positive value.

Batch-level gates run once more when the session calls `done`, per goal kind. In a
`place-anchors` session, every proposal is decided (`undecided-proposal`). In a
`reconcile-section` session, all quotes still locate, coverage claims only touch the
batch's target sections, every stale anchor in the batch is addressed (its quote locates
again, or a staged mutation re-records it under its natural key, revises it, or deletes
it), every dirty section carries a coverage mark (staged or already recorded; enforced
for the explicit `done` only, see [budgets](./sessions.md#budgets)), and a `covered`
claim is honest. A section may be claimed `covered` only when at least one requirement
is sourced from it. A section with nothing to extract is `non-normative` with a note,
never silently `covered`. This stops a session from dropping a rejected requirement and
claiming the section anyway, and from skimming past declarative prose without extracting
([declarative prose states obligations](./concepts/statements.md#declarative-prose-states-obligations)).
Every other kind's gate is on its page under `goals/`; `mark_goal_done` is validated
against the kind's gate and a false claim is rejected with the gate named
([resolving, failing, parking](./sessions.md#resolving-failing-parking)).

## Derived data

Derived data is recomputed on every commit and never written by a tool. It cannot drift
from the facts because it is a function of them.

- Relationships are a materialized view over requirement `edges`. On commit the store
  groups edges by unordered entity pair, one node `rel:<a>~<b>` per pair, and inside it
  by direction and type: each contribution `{a, b, type, cardinality?, requirements}`
  carries the requirements behind it. An edge without `type` contributes `dependency`. A
  contribution carries a `cardinality` when the contributing edges that state one agree.
  There is no write tool for relationships, so an arrow cannot exist without a statement
  behind it. See [recompute](./model/relationship.md#recompute).
- State machines derive from requirement `transition` fields. One node `sm:<entity-slug>`
  per entity any transition names as subject: `states` is the union of named states,
  `initial` is the one state no transition reaches (absent when there are none or
  several), and each transition carries its requirement. See
  [derivation](./model/state-machine.md#derivation).
  The [checks](./model/state-machine.md#checks) run on the derived machine at the end of
  the build.
- Default views derive by rule, six kinds with stable ids: a structural level view per
  node with at least two children, the scope root included (`view:class/<slug>` or
  `view:component/<slug>` by the stereotypes present; the scope root's view is the
  per-scope view, see [level views](./diagrams.md#level-views)), a use-case view and a
  sequence view per flow cluster of a level (`view:usecase/<node-slug>-<doc-slug>`,
  `view:sequence/<node-slug>-<doc-slug>`), a state view per derived state machine
  (`view:state/<entity-slug>`), and an object view per type (`view:object/<type-slug>`,
  an entity that is `b` of an `instantiation` group). Flow clustering is deterministic:
  a `behavior` or `failure-mode` requirement is keyed by its document, each entity
  lifted to its nearest ancestor in the level, the cluster slug is the document stem,
  members are in document order, and a cluster of fewer than two members derives no
  view. A default view is stored under `default-views/`
  (local) under its stable id with the boolean field `default: true` and provenance
  `{derived: {from: [...], reasoning: "default view: <rule>"}}`. On every commit the
  store creates the views whose rule holds, rewrites `title` and `members` on the ones
  still marked `default` (their `excluded`, `collapse`, and `limits` survive), removes
  the ones whose rule stopped holding, and leaves a view without `default` alone. A
  view with a `query` recomputes its members from the query whether or not it is
  default. See [default views](./model/view.md#default-views).
- A committed requirement adds its source as a mention on every entity it references
  (deduplicated). An entity reused by reference accumulates cross-document presence
  without an explicit `upsert_entity` call. These mentions are derived: an entity file
  omits them and a load adds them back ([node files](#node-files)), so a new
  requirement never rewrites the files of the entities it names.
- Limit counts: requirements per entity, children per entity (the scope root's
  parentless entities counted under the same limit, the record written on
  `scope:<scope>`), members and rendered edges per view, participants per sequence
  view, instances per object view, states per state machine. A count that crosses its
  node's threshold writes a `threshold-crossed` record. See
  [the limits registry](#limits).
- The name index (name and alias → entity id) is rebuilt on load and after each commit.
  The [search tool](./tools.md#read-tools) queries it, and lookalike scoring across
  documents reads it.
- Derived indices are computed once per derivation and never stored. A commit, a
  [board derivation](./reconciler.md#goal-derivation), and the
  [checks](./compilation.md#checks) each build them from the facts at their start and
  pass them down: the document levels from the stamped roots, the requirement and
  entity document orders, parent to children, entity to the requirements naming it, the
  [levels](./concepts/levels.md#levels) with their members and flow views (a reference
  reaches the levels its ancestor chain passes through, so each edge and each
  requirement is visited once per level it reaches), the referrer index the
  [cone](./reconciler.md#cones) walk follows, and the name token index
  [lookalike scoring](./goals/dedupe-candidates.md) pairs by. An index is a function
  of the facts it reads, so what is derived through it is what the scan it replaces
  derives.

Renderings under `diagrams/` are derived too, one step further out: the renderer reads
the committed views and facts and writes `.puml` and `.svg`, and stores nothing back.

## Change records

A change record is the typed dirtiness one commit caused: which sections changed, which
requirements were created, revised, or deleted, which thresholds crossed. Records live in
`changes/`, one file per kind and subject. They are the input of [goal derivation](./reconciler.md#goal-derivation):
the graph alone cannot say "revised since last judged", the records can, and a goal reads
its `change` from exactly one record.

```yaml
- id: c412-7k2m            # generation and a hash of kind and subject
  generation: 412
  mutation: 3              # index in the journal entry, 0 for store-level causes
  kind: requirement-revised
  subject: req:orders-6
  via: section             # how dirtiness reached the subject
  detail: {fields: [statement, transition]}
```

- `id` is `c<generation>-<hash>`, the generation that wrote it and the four-character
  [id hash](./model.md#identifiers) of its `kind` and `subject`. Two branches never
  give one id to two records. The record lives in `changes/<kind>/<subject file
  id>.yaml`. `mutation`
  names the journal mutation behind it, `0` for a store-level cause (the sweep, a
  recompute, a check).
- `subject` is a node id, a full section reference `doc.md#/ref`, or `scope:<scope>`
  (the scope root's `threshold-crossed` record, see
  [fan-out](./reconciler.md#fan-out)). `via` is the stored
  reference or computation that carried the dirtiness (`section`, `entities`, `members`,
  `parent`, `from`, `ledger`, `sweep`, and the rest of the closed list in
  [change records](./reconciler.md#change-records)); a goal kind is never a `via`.
  `detail` is the kind-specific evidence and becomes the goal's `change`.
- A newer record of the same kind on the same subject supersedes the older one, and the
  goal's `change` follows it. Resolving a goal clears the records it stood on; a failed or
  parked goal keeps its record, so the goal derives again next build with its state.
- `requirement-created` and `requirement-revised` records feed one pair per candidate
  and clear differently: `detail.judged` lists the partner requirements already judged
  against the subject. An accepted `mark_goal_done` on `g:rejudge-pair:req:a~req:b`
  appends `req:b` to `judged` on every created or revised record of `req:a` and
  `req:a` on every such record of `req:b`, at the commit that carries the claim; the
  records stay. An `edit` option's answer on a diagnostic whose subjects are two
  requirements marks that pair judged the same way, on the records its own commit
  lands too ([answers](./model/diagnostic.md#answers)). A record's candidates are its requirement's neighbors and sticky
  partners minus `judged` ([pairs](./reconciler.md#pairs)); the sweep clears the record
  when no candidate remains, and a pair that leaves the board unresolved (waiting under
  the cap, a side deleted) marks nothing judged. A newer record on the same
  requirement supersedes the older one with an empty `judged`, so every candidate
  derives again with the new cause. E.g. `detail: {fields: [statement], judged:
  [req:payment-9]}` on `req:orders-6` derives no pair with `req:payment-9` until
  `req:orders-6` changes again.

The kinds, who writes them, and the goal each feeds:

| kind | written by | feeds |
|---|---|---|
| `section-dirty` | the `edit` entry of a build (a section added or changed) | `reconcile-section` |
| `section-removed` | the `edit` entry of a build | the sweep, in the same build; the record clears when the sweep has run |
| `anchor-stale` | the `align` entry (a quote that stops locating) or a `reevaluate` placement | `reconcile-section` |
| `alignment-pending` | the `align` entry (a proposal a document owes a decision on) | `place-anchors` |
| `requirement-created`, `requirement-revised` | a commit that creates or revises a requirement (a statement that changes only in case or punctuation is refreshed in place and writes no `requirement-revised`, so its judged pairs stand; an answer's edit lands the record with its diagnostic's pair already in `detail.judged`) | `rejudge-pair` per candidate not in `detail.judged`, at most `pairs-per-requirement` open per side ([pairs](./reconciler.md#pairs); `bind` reaches the requirement through the ledger comparison's `ledger-stale` record) |
| `criterion-created`, `criterion-revised` | a commit that creates or revises a criterion, or the sweep marking a derived criterion revised because its requirement's statement changed (`via: requirement`) | nothing on the board; the ledger comparison reads them and marks the test keyed to the criterion stale ([the ledger](../consumers/gen.md#the-ledger)) |
| `criterion-deleted` | a commit or the sweep that deletes a criterion | nothing (trail; the ledger prunes the row) |
| `requirement-deleted`, `entity-deleted` | a commit that deletes the node (a session or the sweep); the subject is the dead node | nothing (trail; the ledger prunes on `requirement-deleted`). The `node-deleted` and `view-member-gone` records the same commit writes carry the goals |
| `entity-changed` | a commit that changes an entity's fact set, except a `dual-write` commit and an accepting `ratify` commit: a human edit is the review, and the mention the landed sentence adds is the quote the person confirmed ([edit paths](./compilation.md#edit-paths)); a retract writes it as usual | `review-entity` |
| `node-deleted` | a delete or the sweep, one record per live node that still referenced the dead one (`via` names the edge) | `retrace` (a view, an instance, a derived fact); `rejudge-pair` (a surviving diagnostic subject) |
| `instance-changed` | a commit that changes an instance, its type, or the type's attributes | `conform-instance` |
| `ledger-stale` | the ledger comparison as the build derives its board (unbound, requirement changed, artifact gone); refreshed in place, cleared when the row agrees again | `bind`, `generate`, `verify` |
| `prompt-unanswered` | a commit that files a prompt on a diagnostic | `answer` |
| `provenance-pending` | a commit that lands a derived or decreed fact | `ratify` |
| `threshold-crossed` | the limit counts at commit | `split-view`, `abstract-entity` |
| `reparent-flip` | a commit that moves a child back between the same two parents it moved between before (`via: parent`, `detail.between` naming the two); the sweep's dissolve counts as a move | nothing directly: [flip detection](./reconciler.md#flip-detection) at the build's tail reads it beside the journal's replay and parks the goal behind the second move |
| `view-member-gone` | a delete that removes a curated view's member (`via` names the list: `members`, `collapse`, `excluded`) | `retrace` |
| `edges-missing` | a commit that lands a multi-entity requirement without `edges` | `declare-edges` |
| `lookalike` | the name index at commit (a cross-document score over the threshold) | `dedupe-candidates` |
| `query-match` | a recompute in which a new node matches a curated view's query | `curate-view` |
| `flow-unplaced` | the flow placement check at the end of the build | `curate-view` |

`generate` and `verify` derive from ledger state beside the records, not from a record:
`generate` when an entity's facts differ from the ledger, `verify` when a row's derived
status says action ([goal derivation](./reconciler.md#goal-derivation)).

## Journal

Every commit appends one journal file, `journal/g<generation>.yaml`. The journal is the
audit trail of the build and the ground truth of causality: it answers why the graph looks
the way it does, and [`jazyk ripple`](../frontends/cli.md#jazyk-ripple) renders the
cascade over it.

```yaml
build: g413
kind: session
batch: [g:reconcile-section:docs/orders.md#/orders/holds, g:retrace:view:usecase/holds]
mutations:
  - {op: update_requirement, id: req:orders-6, statement: "...", transition: {...}}
  - {op: update_view, id: view:usecase/holds, remove_members: [req:orders-7]}
resolved_goals:
  - {goal: g:reconcile-section:docs/orders.md#/orders/holds, justification: "Hold expiry moved to 30 days; req:orders-6 revised, section covered."}
  - {goal: g:retrace:view:usecase/holds, justification: "Dropped the deleted member; the flow reads through."}
opened_goals:
  - {goal: g:rejudge-pair:req:orders-6~req:payment-9, cause: {generation: 413, mutation: 1, via: entities}}
rounds: 6
tokens: 9800
cachedTokens: 6100
```

The entry kinds:

- `session`: a goal batch's changeset. Carries `batch`, `mutations`, `rounds`, `tokens`
  (the tokens the endpoint billed for the turn, input and output of every round),
  and the model's `reasoning` where a mutation gave one. The entry commits inside the
  serving, before the turn's cost is known, so `tokens` is stamped afterwards by the
  build that measured it, split across the `session` entries that turn wrote (a
  `checks` or `settle-diagnostics` entry between them takes no share, so the parts sum
  to the spend). `cachedTokens` is the part of the input the endpoint served from its
  prompt cache, when it reported one ([budgets](./sessions.md#budgets)): stamped and
  split the same way, absent when the endpoint reported none. A session an
  external agent drove ([compilation over MCP](../frontends/mcp.md#compilation-over-mcp))
  spends tokens the harness never sees, so its entry carries no `tokens` at all rather
  than claiming zero: reading `tokens: 0` on every session, a person would price a build
  at nothing. The agent can report its own bill instead:
  [`report_spend`](./tools.md#spend-tool) during the batch, or
  [`jazyk spend`](../frontends/cli.md#jazyk-spend) on the committed entry, sets `tokens`
  (and `cachedTokens`) to the agent's figure and marks it `tokensSource: agent`, so the
  cost report counts it apart from what the harness measured. An entry without
  `tokensSource` was measured by the harness. `rounds` counts the session's rounds, or
  an external agent's tool calls.
- `edit`: a human save that dirtied sections. Written at the start of a build when the
  parse finds changes, one mutation per dirtied or removed section, so the root of every
  ripple is itself a generation. Carries `dirtied`.
- `align`: alignment's mechanical moves and the proposals it left pending, once per build.
- `gc`: the [sweep](#the-sweep)'s deletions and dissolutions, when it did either.
- `settle-diagnostics`: diagnostics the store resolved because their subjects are gone.
- `checks`: the deterministic checks' findings and the diagnostics they settled, once
  per check run, under a generation of its own ([checks](./compilation.md#checks)).
- `decree`: a human write with no prose behind it (`edit_fact` alone, a node added by
  decree, `bump_limit`, `retract_decree`). Carries `author`, `note`, and on a mutation
  written over a quoted value, the prior value and source.
- `dual-write`: a prose replacement and its graph mutation in one changeset.
- `ratify`: a ratification proposal accepted. The proposed sentence lands in the
  document, the fact's provenance flips to `quote` on that sentence, and the new hashes
  are absorbed like a dual write. The proposal's diagnostic resolves in the same entry.
- `triage`: a human triage state set on a diagnostic
  ([lifecycle and triage](./model/diagnostic.md#lifecycle-and-triage)).
- `answer`: a human answer recorded on a prompt ([answers](./model/diagnostic.md#answers)).
  When the chosen option carries an `edit`, the entry also carries the prose replacement
  and graph mutation, like a dual write.

A mutation that moves an entity's `parent` (`update_entity`, `edit_fact`,
`group_entities`, `dissolve_entity`, the sweep's dissolve) records the prior parent in
its journal mutation, so the reparent flip replays from the journal alone
([flip detection](./reconciler.md#flip-detection)).

Every kind can carry `resolved_goals` (each with its one-line `justification`, and
`evidence` where the gate wanted some) and `opened_goals` (each with its `cause`:
generation, mutation index, and the edge or computation it traveled). The ripple DAG is
derivable, never stored: start at a generation, follow `opened_goals` to the generations
that resolved them, repeat; or walk backward from any node's `updated` marker to the
`edit` entry that started everything.

The journal is read as files, never indexed, and every reader bounds what it parses:

- A lookup that wants the latest entry reads newest first and stops at the first hit:
  the prior a decree recorded (`retract_decree` restores it), the last parent move of a
  child and the create of a parent that no longer exists (the
  [reparent flip](./reconciler.md#flip-detection) the commit records compares against
  them). An id is minted once, so the first create found is the only one.
- A build reads only the entries a session wrote (the generations between the
  session's start and its commit) to record the goals it resolved and the ones it
  opened, and the whole journal once at the build's tail for
  [flip detection](./reconciler.md#flip-detection), parsed once and shared by the
  cross-class and reparent replays.
- An entry that does not parse is skipped with one warning naming the file
  (`[jazyk] warning: journal entry ... skipped`), once per process. The journal is the
  trail, not the graph: a torn entry makes nothing read-only
  ([writes and unreadable files](#writes-and-unreadable-files)), but it is never
  silent, since a missing generation hides a cause from `jazyk ripple` and a spend from
  the cost report.

## Garbage collection

Garbage collection has two halves. The sweep is mechanical: deterministic, at every
commit, never delegated to the model. The GC goals are judgment: restructuring work the
harness derives and a session resolves. Both are named `gc` on every surface.

### The sweep

The sweep runs behind every commit, whichever path committed: a session's `done`, a
decree, a dual write, a ratification, an answer, a triage, a check run. It runs under
the commit's lock, after the commit's own entry, and a build runs it once more after
alignment, since the `edit` and `align` entries are no changeset. What it did lands as
entries of its own (`gc` for deletions and dissolutions, `settle-diagnostics` for the
settles below), each a generation; a sweep with nothing to do writes nothing. The
commit's reply lists the sweep's actions, so a serving or a frontend can say what a
commit swept. Nothing waits for the next build: the graph is honest between builds.

- A requirement whose source section disappeared and was not re-anchored during
  reconcile is deleted by the store, journaled. Its criteria go with it, and a quoted
  criterion whose own section disappeared goes the same way.
- A requirement whose `statement` changed marks its derived criteria revised
  (`criterion-revised`, via `requirement`): the evidence was written against the old
  text. An anchor named by a pending
  [alignment proposal](./alignment.md#what-applies-and-what-is-proposed) is exempt until
  the `place-anchors` session decides it.
- An entity mention is pruned when its section is gone or its quote no longer locates
  in it (a stale mention leaks statements the documents no longer make into later
  loaded sets). Quoted attributes pointing at removed sections are pruned.
- An entity with zero mentions, zero requirements, zero attributes, zero children, and no
  derived or decree provenance is deleted, with a tombstone redirect.
- A derived grouping (derived provenance, no mentions,
  [groupings](./concepts/levels.md#groupings)) with fewer than two children dissolves:
  its children reparent to its parent (parentless when the grouping was top-level), and
  the entity tombstones with a redirect to its parent, exactly as `dissolve_entity`
  would ([write tools](./tools.md#write-tools)). The dissolution is journaled as a
  sweep mutation in the `gc` entry. Below two children there is nothing to judge. An
  entity a document states holds children in role, not in provenance, and the rule
  never touches it.
- A deleted requirement takes its edges and its transition with it: relationships and
  state machines recompute; default and query views recompute. A curated view keeps its
  member list, and a `view-member-gone` record opens [`retrace`](./goals/retrace.md) on
  it. A derived node whose `from` names the dead node gets a `node-deleted` record and a
  `retrace` of its own. Apart from the dissolve rule, the sweep never deletes derived
  or decreed nodes: their fate is judgment.
- Deleting a node settles the open judged diagnostics naming it as a subject: one whose
  subjects are all gone is resolved by the store, journaled; one with surviving subjects
  writes a record on them, so a session re-judges the finding. This runs on every
  deleting commit, session and sweep alike.
- Two marker diagnostics settle at every sweep, the moment their condition clears, so a
  session never adjudicates them. An `incomplete-build` diagnostic resolves once a later
  build has resumed its goal (the goal left `parked/`). An
  `uncovered-section` diagnostic resolves once its section is marked `covered` or
  `non-normative`, or the section is gone. The [checks](./compilation.md#checks) file a
  marker whose condition holds again and update a standing one in place, so a target
  carries at most one open marker, the newest build's. The settle is journaled as
  `settle-diagnostics`.

Deletion runs the edit paths in reverse: dead prose kills quotes, which kills quoted
facts, which opens `retrace` on the views and instances that referenced them; derived
data simply recomputes. See [edit paths](./compilation.md#edit-paths).

### GC goals

The judgment half restructures: decoupling, splitting, combining. The kinds are
[`declare-edges`](./goals/declare-edges.md), [`dedupe-candidates`](./goals/dedupe-candidates.md),
[`curate-view`](./goals/curate-view.md), [`split-view`](./goals/split-view.md), and
[`abstract-entity`](./goals/abstract-entity.md). A GC goal becomes ready only when no
compile goal is open in its target's cone, so restructuring always sees settled content;
the build interleaves the classes in bursts
([compile and garbage collection](./compilation.md#compile-and-garbage-collection),
[readiness](./reconciler.md#readiness)). GC mutations are ordinary changesets: they open
compile goals like any other commit, and [flip detection](./reconciler.md#flip-detection)
bounds the alternation: a natural key flipping between the classes, or a child
reparented back and forth between the same two parents (the reparent flip, which
`group_entities`, `dissolve_entity`, and the sweep's dissolve can all cause).

## Limits

Limits make readability computed, not taste. They are built into the binary, in the
registry below, and are not project settings ([project settings](./project-settings.md)
carries none). Crossing a soft threshold opens an optional goal; crossing the hard one
makes the goal mandatory, and the build cannot report `converged` until the split, merge,
or abstraction happens ([escalation](./reconciler.md#escalation)). A limit goal is GC
work: it runs once its target's cone is quiet and sees final counts.

### The registry

| limit | soft | hard | goal |
|---|---|---|---|
| `requirements-per-entity` | 50 | 80 | `abstract-entity` |
| `children-per-entity` | 9 | 15 | `abstract-entity` (the fan-out variant) |
| `members-per-structural-view` | 20 | 30 | `split-view` |
| `edges-per-view` | 40 | 60 | `split-view` |
| `members-per-flow-view` | 12 | 20 | `split-view` |
| `participants-per-sequence-view` | 8 | 12 | `split-view` |
| `instances-per-object-view` | 15 | 25 | `split-view` |
| `states-per-state-machine` | 12 | 20 | `abstract-entity` (on the subject) |

`children-per-entity` counts one node's direct children, its
[level](./concepts/levels.md#levels). The scope root (the parentless entities of a
scope) counts under the same row, its record written on `scope:<scope>`
([the scope root](./concepts/levels.md#the-scope-root)). Crossing soft derives an
optional `abstract-entity` goal on the node or the scope with the fan-out change; hard
escalates it to mandatory; a count back under soft clears the record without a session
([fan-out](./reconciler.md#fan-out)). Minimum membership is not a limit row: a derived
grouping with fewer than two children dissolves in [the sweep](#the-sweep).
`members-per-structural-view` and `edges-per-view` bound every level view too, and an
over-limit level view renders with its largest subtrees auto-collapsed like any other
([level views](./diagrams.md#level-views)).

A new limit joins the table with both thresholds and the goal that resolves it. A state
machine over its cap opens `abstract-entity` on its subject, because the machine derives
from the subject's requirements. A violation never truncates a rendering silently: the
view renders meanwhile with collapse applied to the largest subtrees, marked as such
([over-limit views](./diagrams.md#over-limit-views)). The two counts differ on purpose:
the goal derives from the listed members, the store's truth, while the renderer judges
the members and edges it draws after collapse, so a well-collapsed view can render
cleanly while the goal stays open.

### Per-node bumps

Dismissing a limit goal is a graph write, not goal state. `bump_limit` sets
`limits: {<limit>: n}` on the node, with decree provenance recorded in the journal
(`kind: decree`). The bump is the node's soft threshold; its hard threshold is `n` plus
the registry's distance between soft and hard, so escalation keeps its shape and a
dismissal never makes the goal mandatory at once. The goal derives only when the count
crosses the bump. E.g. `requirements-per-entity` bumped to 70 escalates at 100:

```yaml
ent:order:
  name: Order
  limits: {requirements-per-entity: 70}
```

### Budgets and thresholds

Session budgets (24 rounds, 8 per section, as batch sizing; the embedded agent's flat
stop of 512 model round-trips; 1024 mutations; 96000 characters of loaded context), the
build cap (8 × the distinct goals derived so far in the build + 64 sessions), the
document quality thresholds (6000 characters per section, 40 sections per document), and
the alignment thresholds (move 0.5, split 0.6) and the alignment candidate cap
(`alignment-candidates`, three per anchor) are registry constants too, not settings.
See [budgets](./sessions.md#budgets). So is the command timeout: one shell command (a
recorded build or test, a session's `run_command`) runs 1800 seconds at most before the
harness kills it and records exit `124`; `JAZYK_COMMAND_TIMEOUT` overrides it per run
([runners](../consumers/gen.md#a-test-that-could-not-run-says-nothing)).

The criteria batch cap is one too: `CRITERIA_GOALS_PER_BATCH` is 4, the most
`derive-criteria` goals one batch packs ([batching](./reconciler.md#batching)).

The lookalike score's constants are registry constants too: `lookalike-token-weight`
0.75, `lookalike-document-weight` 0.25, `lookalike-threshold` 0.5, and
`lookalike-distinctive-holders` 3, the holder count under which a name token keeps its
full weight ([the lookalike score](./goals/dedupe-candidates.md#the-lookalike-score)).

The reopen cycle count is one too: `REOPEN_CYCLES` is 2, the times a goal may be
resolved and re-opened with the same `change` in one build before it parks
([parked and failed](./reconciler.md#parked-and-failed)).

The pair degree cap is a registry constant of the same kind: `pairs-per-requirement`
is 8, the most open `rejudge-pair` goals one requirement takes part in at once, the
rest waiting by score ([pairs](./reconciler.md#pairs)). It has no soft and hard
thresholds and no goal resolves it, so it is not a row of the table above.

## Concurrency

- Writers take `.lock`, an advisory lock the operating system releases when its holder
  exits, so a crashed writer never leaves a stale lock and a waiter never overrides a
  live one (a wait past ten seconds is reported, then continues). One changeset commits
  at a time.
- A writer commits against the generation it loaded. Under the lock it re-reads the
  generation ([the generation](#the-generation)) and refuses with `store-moved` when another writer landed
  first, so a store held in memory (a GUI, an MCP serving, a session) never overwrites
  commits it has not seen. The caller reloads and restages.
- Compilation is sequential: one build at a time (the build lease enforces it), one
  session at a time within it. See [workers and leases](./control-plane.md#workers-and-leases).
- Human paths (chat dual writes, decrees, triage, answers) commit between sessions under
  the same lock. A running session's staged creates are reconciled against them at
  commit by natural key ([changesets](#changesets)).
- Readers (e.g. the [MCP server](../frontends/mcp.md)) do not lock. They read the generation
  counter, load the node files, and retry if the counter moved mid-read.
