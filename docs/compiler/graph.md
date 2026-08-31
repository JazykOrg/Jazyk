# Graph store

The graph store is the persistent home of the [semantic graph](./model.md). It owns
identifiers, enforces invariants, recomputes derived data, records every change, and
writes the typed dirtiness each commit causes. The store is deterministic code. No LLM
runs inside it. One graph per project, edited in place, never regenerated.

## Storage layout

The store lives in the project's out directory (default `jazyk-out/`). All files are YAML,
sorted by key, so builds diff cleanly in git. The schema is
[`graph.schema.yaml`](./graph.schema.yaml).

```
jazyk-out/
  status.yaml            # version, generation, verdict, change records, parked and
                         # failed goals, costs, budgets spent, open diagnostic counts
                         # by severity, pending alignment proposals, anchors marked
                         # for re-evaluation, stamped root documents
  graph/
    entities.yaml        # map: id -> entity
    requirements.yaml    # map: id -> requirement
    views.yaml           # map: id -> view (curated views and the defaults the store derives)
    diagnostics.yaml     # map: id -> diagnostic
    redirects.yaml       # map: absorbed id -> surviving id
    relationships.yaml   # map: id -> relationship (derived, rewritten on every commit)
    state-machines.yaml  # map: id -> state machine (derived, rewritten on every commit)
  docs/
    <mirrored doc path>.yaml   # per document: content hash, section tree, coverage
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
  control.yaml           # modes and releases, see control-plane.md
  workers/, leases/      # who is acting, see control-plane.md
  feedback.jsonl         # report_feedback entries
  .lock                  # single-writer lock
```

The store owns `status.yaml`, `graph/`, `docs/`, and `journal/`. `diagrams/` is build
output the renderer rewrites after every commit ([output layout](./diagrams.md#output-layout));
deleting it loses nothing. The rest belongs to the control plane, the frontends, and the
consumers.

Each document file under `docs/` holds:

- `contentHash`: the hash of the source file at last reconcile.
- `sections`: map of internal reference → `title`, `kind`, `order`, `parent`, `raw`,
  `hash`, `lines`.
- `coverage`: map of internal reference → `state`, `note`, `claimedBy`. See
  [coverage](./compilation.md#coverage).

`status.yaml` holds:

- `version`: the store version, `2`.
- `generation`: bumped on every commit.
- `verdict`: `{state, open, failed, blocked, optional}`, the last build's
  [convergence](./compilation.md#convergence) with its counts.
- `changes`: the open [change records](#change-records).
- `parked`: the goals left open when a budget ran out, each entry the whole goal
  record. `failed`: `{goal, reason}` per goal a session marked failed, `goal` the whole
  record. Both persist whole, `change` payload included, so a parked or failed goal
  survives a re-derivation that would otherwise drop it. See
  [parked and failed](./reconciler.md#parked-and-failed).
- `costs`: `{sessions, tokens, by_kind, by_class}` for the last build, one
  `{sessions, tokens}` line per goal kind and per class (`compile`, `gc`).
- `spent`: `{sessions, rounds, tokens}`, the budgets the last build used.
- `diagnostics`: open diagnostic counts by severity, suppressed excluded.
- `alignment`: pending [alignment proposals](./alignment.md#what-applies-and-what-is-proposed),
  one block per document. `reevaluate`: anchors placed with `reevaluate`, listed as
  stale until addressed.
- `roots`: the documents matching the project [roots](./project-settings.md#roots),
  stamped by the build after the section trees sync. Commits outside a build (edits,
  answers, MCP sessions) read the stamp to order documents by
  [link level](./reconciler.md#link-levels) when they recompute derived data.

### Store version

`status.yaml` carries `version: 2`. On load, an out directory whose `status.yaml` lacks
`version` or carries a different one is archived whole to `<out>.bak` (a sibling
directory; an existing `.bak` is replaced) and the store starts empty. The next build
reconciles from the empty graph, the same way a first build does. There is no migration
code: a version mismatch is a fresh start with the old store kept beside it.

## Mutations

A mutation is one operation on the graph. The full set:

- `upsert_entity`, `update_entity`, `delete_entity`, `merge_entities`
- `upsert_requirement`, `update_requirement`, `delete_requirement`
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
  requirement and refreshes the `statement` and `quote` in place. A derived requirement
  keys on its punctuation-insensitive `statement` within its `from` set; a decreed one
  keys on the statement alone. For views the natural key is `kind` plus `title`. An
  upsert that matches an existing node updates it instead of creating a duplicate. This
  makes retries harmless.
- The store mints ids: `ent:<slug>`, `req:<doc-stem>-<n>` (`req:x-<n>` for derived and
  decreed requirements), `view:<kind>/<slug>`, `diag:<rule>-<n>`. A mutation never
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
  with the retraction as reason. A field decreed over a quoted fact returns to the prior
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
- the generation counter in `status.yaml` is bumped,
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
- Every `quote` must appear verbatim in its named section. Locating is
  whitespace-insensitive and forgives markdown escapes (a text-codec model often
  writes `` \` `` for a backtick inside JSON); the stored quote is the form that
  locates in the source, so provenance stays verbatim to the document.
- In a [`reconcile-section`](./goals/reconcile-section.md) session, a mention or
  requirement source must cite the section being reconciled or its document. A quote
  from a document the prose merely links to is rejected (`wrong-document`): the fact is
  anchored by this document's own sentence, e.g. a link item's text. See
  [enumerations](./concepts/statements.md#enumerations).
- A derived provenance names at least one `from` node, every one of them live, and
  carries `reasoning`. A decree is rejected from a session.
- An entity name that looks like syntax rather than a concept (a file path, a CLI flag,
  a markdown term) is rejected unless the call carries an explaining `note`.
- `upsert_entity` with a name variant of an existing entity (token containment, same
  scope) is rejected toward reuse plus an alias, unless a `note` says how they differ.
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
- Default views derive by rule, six kinds with stable ids: a class view per scope
  (`view:class/<scope>`), a component view per system (`view:component/<system-slug>`,
  a containment root with at least one child), a use-case view and a sequence view per
  flow cluster (`view:usecase/<cluster-slug>`, `view:sequence/<cluster-slug>`), a
  state view per derived state machine (`view:state/<entity-slug>`), and an object
  view per type (`view:object/<type-slug>`, an entity that is `b` of an
  `instantiation` group). Flow clustering is deterministic: a `behavior` or
  `failure-mode` requirement is keyed by its actor (the entity labeled `actor` among
  its `entities`, or its first entity when none is) and by its document, the cluster
  slug is `<actor-slug>-<doc-stem>`, members are in document order, and a cluster of
  fewer than two members derives no view. A default view is stored in `views.yaml`
  under its stable id with the boolean field `default: true` and provenance
  `{derived: {from: [...], reasoning: "default view: <rule>"}}`. On every commit the
  store creates the views whose rule holds, rewrites `title` and `members` on the ones
  still marked `default` (their `excluded`, `collapse`, and `limits` survive), removes
  the ones whose rule stopped holding, and leaves a view without `default` alone. A
  view with a `query` recomputes its members from the query whether or not it is
  default. See [default views](./model/view.md#default-views).
- A committed requirement adds its source as a mention on every entity it references
  (deduplicated). An entity reused by reference accumulates cross-document presence
  without an explicit `upsert_entity` call.
- Limit counts: requirements per entity, children per entity, members and rendered edges
  per view, participants per sequence view, instances per object view, states per
  state machine. A count that crosses its node's threshold writes a `threshold-crossed`
  record. See [the limits registry](#limits).
- The name index (name and alias → entity id) is rebuilt on load and after each commit.
  The [search tool](./tools.md#read-tools) queries it, and lookalike scoring across
  documents reads it.

Renderings under `diagrams/` are derived too, one step further out: the renderer reads
the committed views and facts and writes `.puml` and `.svg`, and stores nothing back.

## Change records

A change record is the typed dirtiness one commit caused: which sections changed, which
requirements were created, revised, or deleted, which thresholds crossed. Records live in
`status.yaml` under `changes`. They are the input of [goal derivation](./reconciler.md#goal-derivation):
the graph alone cannot say "revised since last judged", the records can, and a goal reads
its `change` from exactly one record.

```yaml
- id: c412-3               # generation and index
  generation: 412
  mutation: 3              # index in the journal entry, 0 for store-level causes
  kind: requirement-revised
  subject: req:orders-6
  via: section             # how dirtiness reached the subject
  detail: {fields: [statement, transition]}
```

- `id` is `c<generation>-<index>`, the record's index within its generation. `mutation`
  names the journal mutation behind it, `0` for a store-level cause (the sweep, a
  recompute, a check).
- `subject` is a node id or a full section reference `doc.md#/ref`. `via` is the stored
  reference or computation that carried the dirtiness (`section`, `entities`, `members`,
  `parent`, `from`, `ledger`, `sweep`, and the rest of the closed list in
  [change records](./reconciler.md#change-records)); a goal kind is never a `via`.
  `detail` is the kind-specific evidence and becomes the goal's `change`.
- A newer record of the same kind on the same subject supersedes the older one, and the
  goal's `change` follows it. Resolving a goal clears the records it stood on; a failed or
  parked goal keeps its record, so the goal derives again next build with its state.

The kinds, who writes them, and the goal each feeds:

| kind | written by | feeds |
|---|---|---|
| `section-dirty` | the `edit` entry of a build (a section added or changed) | `reconcile-section` |
| `section-removed` | the `edit` entry of a build | the sweep, in the same build; the record clears when the sweep has run |
| `anchor-stale` | the `align` entry (a quote that stops locating) or a `reevaluate` placement | `reconcile-section` |
| `alignment-pending` | the `align` entry (a proposal a document owes a decision on) | `place-anchors` |
| `requirement-created`, `requirement-revised` | a commit that creates or revises a requirement | `rejudge-pair` per neighbor (`bind` reaches the requirement through the ledger comparison's `ledger-stale` record) |
| `requirement-deleted`, `entity-deleted` | a commit that deletes the node (a session or the sweep); the subject is the dead node | nothing (trail; the ledger prunes on `requirement-deleted`). The `node-deleted` and `view-member-gone` records the same commit writes carry the goals |
| `entity-changed` | a commit that changes an entity's fact set | `review-entity` |
| `node-deleted` | a delete or the sweep, one record per live node that still referenced the dead one (`via` names the edge) | `retrace` (a view, an instance, a derived fact); `rejudge-pair` (a surviving diagnostic subject) |
| `instance-changed` | a commit that changes an instance, its type, or the type's attributes | `conform-instance` |
| `ledger-stale` | the ledger comparison as the build derives its board (unbound, requirement changed, artifact gone); refreshed in place, cleared when the row agrees again | `bind`, `generate`, `verify` |
| `prompt-unanswered` | a commit that files a prompt on a diagnostic | `answer` |
| `provenance-pending` | a commit that lands a derived or decreed fact | `ratify` |
| `threshold-crossed` | the limit counts at commit | `split-view`, `abstract-entity` |
| `view-member-gone` | a delete that removes a curated view's member | `retrace` |
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
```

The entry kinds:

- `session`: a goal batch's changeset. Carries `batch`, `mutations`, `rounds`, `tokens`,
  and the model's `reasoning` where a mutation gave one.
- `edit`: a human save that dirtied sections. Written at the start of a build when the
  parse finds changes, one mutation per dirtied or removed section, so the root of every
  ripple is itself a generation. Carries `dirtied`.
- `align`: alignment's mechanical moves and the proposals it left pending, once per build.
- `gc`: the [sweep](#the-sweep)'s deletions, when it deleted anything.
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

Every kind can carry `resolved_goals` (each with its one-line `justification`, and
`evidence` where the gate wanted some) and `opened_goals` (each with its `cause`:
generation, mutation index, and the edge or computation it traveled). The ripple DAG is
derivable, never stored: start at a generation, follow `opened_goals` to the generations
that resolved them, repeat; or walk backward from any node's `updated` marker to the
`edit` entry that started everything.

## Garbage collection

Garbage collection has two halves. The sweep is mechanical: deterministic, at every
commit, never delegated to the model. The GC goals are judgment: restructuring work the
harness derives and a session resolves. Both are named `gc` on every surface.

### The sweep

- A requirement whose source section disappeared and was not re-anchored during
  reconcile is deleted by the store, journaled. An anchor named by a pending
  [alignment proposal](./alignment.md#what-applies-and-what-is-proposed) is exempt until
  the `place-anchors` session decides it.
- An entity mention is pruned when its section is gone or its quote no longer locates
  in it (a stale mention leaks statements the documents no longer make into later
  loaded sets). Quoted attributes pointing at removed sections are pruned.
- An entity with zero mentions, zero requirements, zero attributes, zero children, and no
  derived or decree provenance is deleted, with a tombstone redirect.
- A deleted requirement takes its edges and its transition with it: relationships and
  state machines recompute; default and query views recompute. A curated view keeps its
  member list, and a `view-member-gone` record opens [`retrace`](./goals/retrace.md) on
  it. A derived node whose `from` names the dead node gets a `node-deleted` record and a
  `retrace` of its own. The sweep never deletes derived or decreed nodes: their fate is
  judgment.
- Deleting a node settles the open judged diagnostics naming it as a subject: one whose
  subjects are all gone is resolved by the store, journaled; one with surviving subjects
  writes a record on them, so a session re-judges the finding. This runs on every
  deleting commit, session and sweep alike.

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
bounds the alternation.

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
| `children-per-entity` | 10 | 20 | `abstract-entity` |
| `members-per-structural-view` | 20 | 30 | `split-view` |
| `edges-per-view` | 40 | 60 | `split-view` |
| `members-per-flow-view` | 12 | 20 | `split-view` |
| `participants-per-sequence-view` | 8 | 12 | `split-view` |
| `instances-per-object-view` | 15 | 25 | `split-view` |
| `states-per-state-machine` | 12 | 20 | `abstract-entity` (on the subject) |

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

Session budgets (24 rounds, 64 mutations, 24000 characters of loaded context, at least 8
rounds per section in the batch), the build cap (3 × derived goals + 8 sessions), the
document quality thresholds (6000 characters per section, 40 sections per document), and
the alignment thresholds (move 0.5, split 0.6) are registry constants too, not settings.
See [budgets](./sessions.md#budgets).

## Concurrency

- Writers take `.lock`. One changeset commits at a time.
- Compilation is sequential: one build at a time (the build lease enforces it), one
  session at a time within it. See [workers and leases](./control-plane.md#workers-and-leases).
- Human paths (chat dual writes, decrees, triage, answers) commit between sessions under
  the same lock. A running session's staged creates are reconciled against them at
  commit by natural key ([changesets](#changesets)).
- Readers (e.g. the [MCP server](../frontends/mcp.md)) do not lock. They read the generation
  counter, load shards, and retry if the counter moved mid-read.
