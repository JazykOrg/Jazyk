# Tools

The tool registry is the graph's only interface for models. One registry, one set of
schemas, served two ways:

- in-process, to [sessions](./sessions.md) during compilation,
- over stdio as an MCP server (`jazyk mcp <toolsets>`), to external agents. See
  [MCP](../frontends/mcp.md).

Both servings dispatch the same implementations, so the tools an external agent uses are
exactly the tools the compiler uses. Read tools are public. Write tools stage into an
open changeset: a session's batch, an MCP batch claimed with `begin_goals`, or a raw
changeset per call under `jazyk mcp graph --write`. Outside an open batch a write tool
is rejected toward `begin_goals`.

An open batch advertises only its goal kinds' tools, the read and goal tools, and
the compilation lifecycle. Hidden writes still pass the same scope checks and
are refused outside that batch's toolset. Repair reads, `mark_goal_failed`, and
`abandon_goals` remain available after a gate refusal.

The catalog is deliberately small. Weak models handle few, simple tools better than many
clever ones. No tool enqueues work: the model writes graph state, the harness derives the
[goals](./reconciler.md#goal-derivation).

Every reply is one JSON object. Wherever a reply travels as text (the MCP text content,
the tool message the embedded agent hands its model, the chat pane's expanded row) it
is pretty-printed with two-space indentation, one field per line: a person reading a
transcript or watching an IDE's tool call reads the same shape the model does, never a
single line. The trace keeps its condensed one-line summaries beside the full payload.

## Read tools

Every serving carries the read tools. The first four maintain the
[loaded set](./context.md#the-loaded-set); the rest are lookups whose subject joins the
loaded set as a stub.

- `load({target, depth?})`: load a node and its immediate neighborhood into the loaded
  set. `target` is any node id, a section reference (`docs/orders.md#/orders/holds`),
  or a view id. `depth` is the hop count along the [axes](./context.md#axes), default
  `1`. The reply carries the rendered items and the re-rendered status block; whatever
  the budget cut off arrives as [expansion handles](./context.md#expansion-handles).
  Loading an already loaded target is a [repeat](./sessions.md#repeated-calls). Past
  the high-water mark the call is refused until something is unloaded
  ([policy](./context.md#policy)); the refusal's candidates never name the target
  itself, and a target held as a stub reloads in place, its stub's chars discounted. A node staged this session and not yet committed
  is refused with `staged-target`: the loaded set renders the committed snapshot, the
  staged node is exactly what the write call sent, and it loads after `done`.
- `expand({handle})`: load the frontier behind a handle, under the same policy. A
  frontier that does not fit is refused with its size estimate and an unload
  suggestion. Expanding a closed handle is a repeat.
- `unload({target})`: drop an item from the loaded set. Its handles close and later
  replies stop rendering it. Frees budget for the rest of the session.
- `graph_status({})`: re-render the status block in full
  ([rendering](./context.md#rendering)). A condensed form rides on every mutating
  reply, so this call is for a model that lost track.
- `search({query, kind?})`: deterministic lookup over names and aliases: normalized
  exact match, then alias, then substring, then token overlap. `kind` narrows to
  `entity` (default) or `view` (matched on `title`). Returns `{hits}`, up to 8
  `{id, name, definition}` entries. `queries: [..]` in place of `query` answers
  several names in one call as `{results: [{query, hits, ...}]}`, one entry per
  query in order, so the search-before-create doctrine costs one round per section,
  not one per name. No embeddings, no LLM. A miss is an answer, not a
  dead end: `hits` is empty and the result carries an inventory of the requested
  kind, including staged nodes, with up to 25 ids and names. Entity misses return
  `entityCount`, `entities`, and a `next` line guiding reuse or creation of an entity.
  View misses return `viewCount`, `views`, and a `next` line guiding `get_view` with
  a listed id, or reading entities and requirements directly with `get_entity` or
  `load`. A view miss never suggests creating an entity. When the view inventory is
  truncated, the caller can refine the query to find an unlisted title. A bare empty
  list reads as "ask again", and models loop on it; the inventory and next step
  let the caller proceed.
- `read_section({ref})`: one section's raw body and its child titles. A bare document
  name (`orders.md`, or `orders.md#`) reads the document's root section, the way a
  reader opens a file to see what it is about.
- `get_entity({id})`: one entity with its definition, stereotype, parent, children,
  attributes, mentions, requirements, relationships, and its derived state machine
  when one exists; `parent`, `stereotype`, `attributes`, and `children` appear only
  when set, so a review can tell an absent parent from an unknown one.
  The lookup sees the session's own staged entities, same as `search`.
- `get_view({id})`: one view with its members in order, its exclusions, query, and
  collapse list, the relationships among its members (lifted where a member is
  collapsed), the path of its rendering under `diagrams/`
  ([output layout](./diagrams.md#output-layout)), and `children`: the members that
  hold a [level](./concepts/levels.md#levels) of their own, each with the id of its
  level view. These are the drill-down links the rendering carries
  ([drill-down](./diagrams.md#drill-down)). `children` is computed from the
  containment tree and the [level views](./diagrams.md#level-views) at call time,
  never stored on the view.
- `diagnostics({lifecycle?, rule?, subject?})`: the diagnostics, `open` by default
  (`lifecycle` takes `open`, `resolved`, or `all`; `rule` and `subject` narrow
  further). Each entry carries id, rule, severity, lifecycle, triage, subjects,
  message, whether it carries an unanswered prompt, and `owner: harness` on the
  build's own bookkeeping rules (`incomplete-build`, `ratification-pending`,
  `criterion-missing`, `unstable-*`), which no session resolves
  ([lifecycle](./model/diagnostic.md#lifecycle-and-triage)). Without a `rule`, those
  bookkeeping entries are counted per rule under `bookkeeping` rather than listed, and
  `rule` lists them: a novel compile's listing carried 222 `ratification-pending`
  entries, one per derived criterion, and Codex read it as a truncated wall that hid its
  three judgment findings. This is the read surface for
  document health; without it an agent can only learn diagnostic state by reading the
  out directory off disk.

## Write tools

Write tools stage [mutations](./graph.md#mutations) into the open changeset. Every
staged mutation passes the [validation gates](./graph.md#validation-gates) when staged;
a mutating reply previews the goals the mutation will open
([bubbling](./reconciler.md#bubbling)) and re-renders the condensed status block.

- `upsert_entity({name, definition?, aliases?, scope?, stereotype?, parent?, attributes?, mention?, provenance?, note?})`
  → `{id, created}`. Keys on `name` plus `scope`, and `parent` joins the key when
  given; a match updates instead of duplicating. Without `parent`, exactly one entity
  with that name and scope may exist, whatever its parent; several matches is an error
  naming the candidates and asking for `parent`
  ([the natural key under containment](./concepts/identity.md#the-natural-key-under-containment)).
  Exactly one of `mention: {section, quote}` (the quote provenance) or
  `provenance: {derived: {from, reasoning}}` (an entity the documents do not state,
  as [`abstract-entity`](./goals/abstract-entity.md) creates) is required; a session
  never stages a decree. `stereotype` is free-form judgment. `parent` must resolve
  and keep the tree acyclic ([containment](./model/entity.md#containment)).
  `attributes` are `[{name, type?, value?, provenance?}]`, keyed by `name` within the
  entity; an attribute without its own provenance takes the call's quote. A name
  variant of an existing entity is rejected toward reusing that entity and recording
  the wording as an alias; a `note` overrides and is kept on the new entity as
  `distinct_from`, so the pair is never asked about again. A name that looks like syntax rather
  than a concept is rejected unless `note` explains it
  ([what is an entity](./model/entity.md#what-is-an-entity)); a name starting with an
  article passes when a document writes it capitalized mid-sentence ("upstairs at The
  Lantern Room"), since the article is then part of a proper name. Omit `scope` unless the
  documents explicitly name a bounded context.
- `update_entity({id, name?, definition?, add_aliases?, remove_aliases?, stereotype?, parent?, remove_parent?, attributes?})`:
  a rename keeps the id. `add_aliases` records a wording; `remove_aliases` takes one
  back, so a session repairs an alias it judged unsupported instead of leaving an
  invented wording in the graph. `remove_parent: true` takes the entity out of the
  containment tree: the judgment skill keeps a generic rule-carrier (Department beside
  Operations, Sales, Finance) parentless, and a session that had set one could not
  undo it, since the contract forbids an empty string as a value. `attributes` upserts by name; attributes not named
  stand.
  `parent` obeys the same gates as on create. No gate compares `parent` with committed
  `composition` edges: composition consistency is the `containment-mismatch`
  [check](./compilation.md#checks).
- `delete_entity({id, reason})`: rejected while any requirement references the entity
  or any entity names it as `parent`; the error lists them. Deleting an entity a view,
  a transition, or a `from` list references is allowed: the commit writes
  `view-member-gone` and `node-deleted` [change records](./graph.md#change-records),
  and [`retrace`](./goals/retrace.md) derives. The reply says so before commit.
- `merge_entities({keep, absorb, reason})`: the store rewires every reference
  (requirement `entities`, `edges`, `transition.subject`, entity `parent`, view
  `members`, `excluded`, `collapse`, diagnostic `subjects`, provenance `from`),
  unions aliases, mentions, and attributes, and leaves a redirect. A merge that would
  make the survivor its own ancestor is rejected. See [mutations](./graph.md#mutations).
- `upsert_requirement({statement, entities, section?, quote?, provenance?, edges?, transition?, facets?})`:
  the store mints the id; any id supplied is ignored. Exactly one of `section` plus
  `quote` (the source) or `provenance: {derived: {from, reasoning}}` is required.
  Idempotency comes from the natural key (the source section plus the
  punctuation-insensitive `statement`; a derived requirement keys on the statement
  within its `from` set), resolved the moment the call is staged, against the store and
  the session's own staged statements alike
  ([identity](./model/requirement.md#identity)). A match returns the existing id with
  `updated: true` and refreshes its `statement` and `quote` in place, never minting a
  duplicate; the model sees the resolution, not a misleading fresh id. A statement
  re-extracted from the same source sentence whose content subsumes or is subsumed by
  the existing statement is the same fact reworded: it also updates in place, and the
  reply carries `matched` with the statement it landed on. Subsumption is tight: the
  longer statement adds at most two content words to the shorter and adds no negation
  (`not`, `never`, `no`, `unless`, `except`, `otherwise`, `cannot`), so "An Order is paid within
  21 days of placement" and "When an Order is not paid within 21 days of placement,
  Orderly cancels it", two facts from one sentence, stay two requirements. The words
  compare with the obligation modals (`shall`, `must`, `will`) set aside and a plural
  or verb ending (`-s`, `-ies`) reduced, so "the system empties the Cart" is "the system
  shall empty the Cart" restated, not a second requirement. An added `cannot`, `can`,
  `may`, `should`, `might`, or `could` blocks the match like a negation: a permission
  or a recommendation is not the obligation. A stale
  anchor in the same section whose statement subsumes or is subsumed by the new
  statement resolves the same way, so re-recording a reworded statement lands on the
  anchor's id; there the dead quote is the signal, so any subsumption counts. `edges` are `[{a, b, type?, cardinality?}]`,
  directional, both ends among `entities`, `cardinality` the multiplicity at `b` (how
  many `b` per `a`) ([edges](./model/requirement.md#edges)).
  `transition` is `{subject, from, to, trigger?, guard?}` with `subject` among
  `entities` ([transition](./model/requirement.md#transition)). `facets` are
  `[{facet, reasoning, measure?}]`, `measure` on `quality` only
  ([facets](./model/requirement.md#facets)). Real revisions go through
  `update_requirement`.
- `update_requirement({id, statement?, entities?, edges?, transition?, facets?, section?, quote?})`:
  a revision keeps the id. A field given replaces the stored one whole. `section` plus
  `quote` re-anchor the provenance; the quote must locate verbatim in the section.
  This is the path for a stale anchor whose statement changed meaning: new
  `statement`, new `quote`, same id. Omitting both leaves the anchor untouched, which
  is what a call that only adds an entity reference or declares `edges` wants. Passing
  the `statement` as the `quote` is the common miscall, so every rejection on
  `section` or `quote` names the requirement's existing anchor and says to drop the
  two fields when only `entities`, `edges`, `transition`, or `facets` were meant to
  change. Under an [`abstract-entity`](./goals/abstract-entity.md) goal a re-point is
  a swap: `entities` may replace the goal's target with a sub-entity and never drops
  another entity the statement names (`entities-dropped`); the refusal carries the
  list to send, the stored entities with the target swapped for the sub-entity.
- `delete_requirement({id, reason})`: allowed while views or `from` lists reference the
  requirement; the commit writes the change records and `retrace` derives. The sweep
  prunes the mentions the requirement carried
  ([garbage collection](./graph.md#garbage-collection)).
- `place_anchor({id, section, quote?, reevaluate})`: move one anchor (a requirement's
  source, or every proposed mention of an entity) to `section`, in a
  [`place-anchors`](./goals/place-anchors.md) session only. A `quote` must locate
  verbatim in the section and replaces the stored one; omitted, the stored quote stays.
  An entity with several proposed mentions takes the section alone, each mention
  keeping its own quote. A placed requirement carries the mentions derived from its
  source with it. `reevaluate: true`, or a stored quote that does not locate in the new
  section, lists the anchor as a stale anchor on the target section's
  [`reconcile-section`](./goals/reconcile-section.md) goal. The `id` must be one of
  the batch's proposals (`unknown-anchor` otherwise).
- `orphan_anchor({id})`: leave one proposed anchor homeless; it stays a stale anchor
  on its old document while that section still exists, for the
  [`reconcile-section`](./goals/reconcile-section.md) goal there to re-record or
  delete. An anchor whose section is gone has nothing to stay on: the
  [sweep](./graph.md#the-sweep) behind the commit deletes a requirement anchor
  (`requirement-deleted`, journaled) and prunes a mention anchor. Same `id` rule.
- `report_diagnostic({rule, severity, subjects, message, reasoning, prompt?})`. `rule`
  is one of the judged rules: `contradiction`, `duplicate-entity`,
  `duplicate-requirement`, `missing-link`, `ambiguity`, `lint` for violations of the
  project's own [lint rules](./project-settings.md#linting), `decision` for a choice
  the documents leave open, or `nonconformant-instance` for an instance whose values
  or links its type's statements rule out. Free-form rule names are rejected, so
  findings stay comparable across builds ([rules catalog](./model/diagnostic.md#rules-catalog)).
  Keys on rule plus subjects (`invented-choice` adds the choice sentence to the key):
  re-reporting the same finding updates it. `prompt`
  attaches a question with suggested resolutions
  ([prompts](./model/diagnostic.md#prompts)); a `decision` requires one. Its gates: at
  most 4 options, each with a `label` and exactly one of `edit` or `answer`, an
  `edit`'s `old_text` must locate in its section when non-empty, and each of an
  `edit`'s `facts` (`{id, field, value}`, the fact changes the rewrite implies) names a
  requirement quoted from that section and passes the gates `update_requirement` runs
  on the field. A diagnostic with an
  unanswered prompt opens the blocked [`answer`](./goals/answer.md) goal. The reply
  carries the finding's `id` (a re-report answers with the id it updates), and the
  session's own staged findings are visible to `diagnostics`, `update_diagnostic`,
  and `resolve_diagnostic`.
- `update_diagnostic({id, prompt})`: replace the prompt on an open diagnostic (pass
  `null` to remove it). The finding itself is edited through `report_diagnostic`'s
  natural-key upsert; this tool only maintains the question. Never touches a human-set
  `answer` or `triage`.
- `resolve_diagnostic({id, reason})`. Refused on a harness-owned rule
  (`incomplete-build`, `ratification-pending`, `criterion-missing`, `unstable-*`),
  naming the owner: the build settles those itself (a `criterion-missing` clears when a
  [`derive-criteria`](./goals/derive-criteria.md) goal lands a criterion) ([lifecycle](./model/diagnostic.md#lifecycle-and-triage)).
- `set_coverage({section, state, note?})`: `state` is `covered` or `non-normative`.
  `non-normative` requires the `note`; a placeholder note (`<nil>`, `none`, `n/a`)
  counts as absent. `non-normative` is rejected for a section that already yielded a
  requirement, in the store or in this session's own staged work: the mark and the
  statement contradict each other, and the statement is the evidence. The rejection
  names the statements and asks for `covered`. A repeated call for the same section
  within one session replaces the earlier mark: a changeset carries at most one
  coverage mark per section. A `covered` claim requires a requirement sourced from
  that section; the `done` gate enforces it ([coverage](./compilation.md#coverage)).
  A section reference naming a fenced block (`docs/steps.md#/steps/code-block-1`)
  resolves to the section holding the block, in this tool and in every other: a block
  owns no mark and derives no goal, its parent keeps the whole body, so the step
  quotes anchor there and the parent's one mark covers the block
  ([section tree](./parsing.md#section-tree)).

There is no write tool for relationships, state machines, or default views. They are
[derived data](./graph.md#derived-data), recomputed on every commit from requirement
`edges` and `transition` facets and from the graph's structure.

- `upsert_criterion({requirement, statement, kind, measure?, given?, when?, then?, entities?, section?, quote?, provenance?})`
  → `{id, created}`: the observable evidence one requirement is met
  ([criterion](./model/criterion.md)). Keys on `requirement` plus the
  punctuation-insensitive `statement`. Exactly one of `section` plus `quote` (a stated
  criterion: a test case, an example, a threshold bullet, quoted verbatim from its own
  section) or `provenance: {derived: {from: [the requirement id], reasoning}}` (a
  criterion the documents do not state, ratified toward prose). `kind` is `positive`,
  `negative`, `boundary`, or `quality` with `measure`. Refused: a `requirement` that
  does not resolve (`unknown-requirement`), a statement equal after normalization to
  the requirement's or to any other requirement's (`criterion-restates`, naming the
  requirement it restates), a quote that does not locate.
- `update_criterion({id, statement?, kind?, measure?, given?, when?, then?, entities?})`:
  a wording or kind repair; the id and the owner stay.
- `delete_criterion({id, reason})`: a criterion the documents no longer support.

### Grouping tools

Two write tools in the graph group build and unbuild
[levels](./concepts/levels.md#levels). A [grouping](./concepts/levels.md#groupings) is
an entity with derived provenance from its members and no mentions. Each tool stages
one changeset under the same gates as the other write tools. They serve the fan-out
variant of [`abstract-entity`](./goals/abstract-entity.md)
([fan-out](./reconciler.md#fan-out)).

- `group_entities({name, definition, members, stereotype?, reasoning})` → `{id, moved}`:
  stage one derived entity and reparent every member under it, as one changeset. The
  new entity takes provenance `derived` with `from` exactly the `members` and
  `reasoning` why, the members' shared current parent as its `parent` (none for a
  grouping at the [scope root](./concepts/levels.md#the-scope-root)), and the
  members' scope. `stereotype` comes from the existing vocabulary or is omitted;
  there is no grouping stereotype. Gates: at least two `members`; every member
  resolves; all members share one current parent (a grouping never crosses levels);
  `name` passes the `near-duplicate` gate against existing names, the same gate as
  `upsert_entity` ([validation gates](./graph.md#validation-gates)), so a lookalike
  of an existing area reuses that entity ([naming](./concepts/levels.md#naming));
  the refusal's advice follows where the lookalike sits: a sibling of the members
  that already holds children is the area, and the members move under it; a sibling
  with no children is a peer that carries the area's word, never the members'
  parent, so the grouping takes the heading's name; a lookalike elsewhere in the tree
  names another level's concept, and the grouping qualifies its name, since a move
  under it would cross levels; `definition` and `reasoning` are non-empty. The reply
  carries the new `id` and `moved`, the member ids reparented under it.
- `dissolve_entity({id, reason})`: the inverse, for a grouping with derived provenance
  and no mentions. Its children reparent to its parent (they become parentless when
  the grouping was at the scope root), and the entity tombstones with a redirect to
  its parent. Refused on an entity a document states (`stated-entity`): revise the
  documents instead. The deterministic sweep applies the same operation to a derived
  grouping left with fewer than two children ([the sweep](./graph.md#the-sweep)). On
  an entity this session staged and has not committed, the call withdraws it from the
  changeset instead: the create, every move under it, and every requirement re-pointed
  to it drop, so a sub-entity created and then found empty leaves nothing behind. The
  reply names what dropped (`withdrawn`).

`update_entity`'s `parent` stays the single-move path: one child under one new parent,
under the same gates.

## View tools

Views are the stored half of a diagram: what it includes, never how it looks
([view](./model/view.md)). The view tools stage view mutations under the same gates as
the write tools. They serve the [`retrace`](./goals/retrace.md),
[`curate-view`](./goals/curate-view.md), [`split-view`](./goals/split-view.md), and
[`abstract-entity`](./goals/abstract-entity.md) goals.

- `upsert_view({kind, title, members?, query?, collapse?, excluded?, reasoning})`
  → `{id, created}`. Keys on `kind` plus `title`
  ([identity](./model/view.md#identity)). `kind` is one of the
  [catalog](./model/view.md#kinds). `members` are ordered node ids: entities for
  structural kinds, requirements for flow kinds; order is the flow order
  ([membership](./model/view.md#membership)). `query` is
  `{scope?, parent?, stereotype?, depth?}`, membership by rule; matches join `members`
  at every commit. `collapse` lists entities shown as one node despite their children.
  `excluded` is `[{id, note}]`. `reasoning` becomes the view's derived provenance
  (`from` the members, `reasoning` why); every id must exist. A call that lands on a
  default view's `kind` and `title` clears its `default` field. A view crossing a
  [limit](./graph.md#limits) is accepted and renders with collapse applied
  ([over-limit views](./diagrams.md#over-limit-views)); the `split-view` goal follows.
- `update_view({id, title?, members?, add_members?, remove_members?, query?, collapse?, exclude?, reasoning?})`:
  `members` replaces the whole ordered list; `add_members` and `remove_members` edit
  it; `exclude` adds one `{id, note}` pair or a list of them, so one view's exclusions
  batch into one call. On a default view any field clears the
  boolean `default` field: the store stops rewriting the view's `title` and `members`
  from the rule, and the view is curated from then on
  ([default views](./model/view.md#default-views)).
- `delete_view({id, reason})`: refused on a default view, because the next commit
  would derive it again. Exclude its members instead, or collapse them.

## Goal tools

Every session sees the goal tools. They record goal resolutions in the journal and
clear or keep change records; they are not graph mutations
([resolving, failing, parking](./sessions.md#resolving-failing-parking)).

- `mark_goal_done({goal, justification, evidence?})`: claim one goal of the batch
  resolved. `justification` is mandatory and concise, one or two sentences of why the
  goal is complete; the journal records it and `jazyk ripple` shows it beside each
  step. The serving validates the claim against the goal kind's gate when staged and
  again at commit, and rejects a false one with the gate named (each gate is stated on
  the kind's page under `goals/`). The ledger kinds read the landed ledger, never the
  changeset: a [`bind`](./goals/bind.md#gate) claim needs a current row recorded by
  `record_binding`, a [`generate`](./goals/generate.md#gate) claim a record with the
  live `factHash`, existing files, and `run_tests` run since, a
  [`verify`](./goals/verify.md#gate) claim a verdict standing on a current row; the
  rejection (`bind-gate`, `generate-gate`, `verify-gate`) names what is missing and
  the call that lands it. `evidence` carries what the gate reads for kinds
  that ask for it, one shape per kind: [`rejudge-pair`](./goals/rejudge-pair.md)
  `{verdict: duplicate | contradiction | consistent, carried_by?}`;
  [`review-entity`](./goals/review-entity.md) `{definition: current | refreshed,
  lookalikes: [{id, verdict: merged | separate, reason}], unreferenced: [{id, verdict:
  referenced | word-match, reason}], diagnostics: [{id, verdict: stands | resolved |
  refiled}], parts: [{id, verdict: parented | edge-fixed | kept, reason}], lint:
  [{rule, verdict: clean | filed}]}` (one entry per item the hints list; omit a list
  the hints leave empty, and `lint` when the project declares no lint rules; an empty
  list is a placeholder, as everywhere); [`declare-edges`](./goals/declare-edges.md)
  `declared` or `not-structural`; [`dedupe-candidates`](./goals/dedupe-candidates.md)
  `merged` or `separate`; [`curate-view`](./goals/curate-view.md) `[{id, verdict:
  added | excluded}]`, one per matched node; [`conform-instance`](./goals/conform-instance.md)
  `{attributes: [{name, verdict}], links: [{to, verdict, carried_by?}]}`, one per
  declared attribute and per link; [`derive-criteria`](./goals/derive-criteria.md)
  `{requirements: [{id, criteria: [ids], verdict: complete | stated-only |
  open-question}]}`, one per listed requirement; the other kinds read the staged work
  alone. Each entry the page's gate requires is checked against the staged work, and a
  missing or unbacked one is refused with the rule the page names (`*-unjudged` for a
  listed item without a verdict, `*-unbacked` for a verdict the staged work does not
  carry), since a model checks exactly what the evidence names. The tool's description
  carries the same list, so the shape is never guessed. A goal outside the batch is
  rejected.
- `settle_pairs({verdicts: [{goal, verdict, justification, severity?, message?,
  reasoning?, prompt?}]})`: one claim for several `rejudge-pair` goals, served in the
  [`rejudge-pair`](./goals/rejudge-pair.md#tools) slice only. Each entry is one
  pair's verdict and does what the single calls it stands for would do, under the
  same gates:
  - `consistent`: stages nothing, marks the goal done with the justification.
  - `duplicate`: stages a `duplicate-requirement` diagnostic on the pair (severity
    `info`, `message` when given, else a line saying both are kept), then marks. The
    same-document delete stays on `delete_requirement` plus `mark_goal_done`: a
    delete deserves its own call and reason, and a `duplicate` entry whose two sides
    quote one document is refused (`same-document-duplicate`) naming that route.
  - `contradiction`: stages a `contradiction` diagnostic on the pair with the entry's
    `severity` (`error` or `warning`, required), `message` (required), `reasoning`,
    and `prompt`, then marks; when an open `contradiction` already names both sides
    (filed by an earlier batch, or by this session against a statement it was not
    paired with), that one carries the verdict, no second is filed, and the entry
    needs only `goal`, `verdict`, and `justification`.
  - `resolved`: resolves every open `contradiction` or `duplicate-requirement`
    diagnostic naming both sides, the justification as the reason, then marks. A
    pair with no such diagnostic is refused (`no-pair-diagnostic`); a refile after a
    resolve is a `report_diagnostic` call.

  The reply lists one result per entry, in order: the goal, the verdict, and the
  diagnostic id it staged or resolved, or the violated rule and the repair. An entry
  the gate refuses stops nothing: the other entries land, and a refused entry leaves
  nothing staged. Nothing outside the batch's own goals is accepted, and a goal of
  another kind is refused (`wrong-kind`). Each accepted entry records one resolution
  with its own justification and evidence, so the journal's `resolved_goals`, the
  `goal` trace events, and `jazyk explain` keep one entry per goal, as with
  `mark_goal_done`. The repeated-call guard counts per accepted entry
  ([repeated calls](./sessions.md#repeated-calls)).
- `evidence` may arrive as a JSON string: the tool parses it
  before the gate reads it, and a refusal says what shape arrived (a bare array, a
  string that is not JSON, an object with other keys), since a local model once met the
  same `bad-evidence` line for every shape it tried.
- `mark_goal_failed({goal, reason})`: always available. A goal that cannot be
  accomplished (documents too deeply contradictory, a target that no longer makes
  sense) must be failable, or the board fills with dishonestly resolved goals. The
  reason is a sentence saying what blocked the goal; a placeholder (`placeholder`,
  `TODO`, under twelve characters) is refused as `bad-reason`, since a failed
  mandatory goal blocks convergence and one once landed by a slip. A failed
  goal keeps its target, so the failure surfaces on the thing itself everywhere it
  renders. A failed mandatory goal blocks convergence; a failed optional goal is
  recorded and stands ([parked and failed](./reconciler.md#parked-and-failed)).
- `load_skill({name})`: bring one [skill](./sessions.md#skills) into the session. The
  skill index line of the status block lists the names. An unknown name is rejected
  with the index; a call past the per-session cap (four, the registry constant
  `skills-per-session`) is refused naming the cap and the skills already rendered.
- `done({summary, goals?, beginNext?})`: end the session and request commit. `goals`
  supplies final claims as `{goal, justification, evidence?}` entries. Each runs
  the same gate as `mark_goal_done`. Use it to resolve the final goals and commit
  in one call. A repeated goal or an invalid claim refuses the call. If any claim
  or batch gate fails, none of this call's claims or staged effects remain. Earlier
  staged work and claims stay available for repair. Runs the batch
  gates per goal kind: every goal of the batch marked done or failed, every stale
  anchor from the batch addressed (its quote locates again, or a staged mutation
  re-records, revises, or deletes it), every dirty section of the batch carrying a
  coverage mark, staged or already recorded, and every `covered` claim honest. A gate
  failure leaves the changeset open and names the repair. Goals neither marked done
  nor failed when `done` commits park. The implicit path (a session ending with staged
  work and no `done`) commits under the same gates minus the coverage-mark requirement
  ([budgets](./sessions.md#budgets)). Over MCP, `beginNext: true` claims the next
  ready batch in the same call ([compilation tools](#compilation-tools)). `done` is
  exempt from the repeated-call guard, so repairing a rejected `done` is legitimate.

## Chat tools

The [chat serving](../frontends/mcp.md#toolsets) carries tools built for a conversation
about the project. They are not in any session's toolset: they stage the human paths
(dual write, decree, ratification, answer) that sessions never take.

Native and hosted chat expose `release_work({stage, setId?})` for an explicit user
request to compile or generate. It records approval for the current stage without
changing manual modes. It does not approve ratification, diagnostic answers,
credentials, or publication. See [release handling](../frontends/mcp.md#the-control-plane-over-mcp).

Dual-write tools (see [ACP](../frontends/acp.md#dual-write-tools) and
[edit paths](./compilation.md#edit-paths)): a quote-provenanced fact lives in the prose,
so a chat edit moves the prose and the graph together, or not at all:

- `revise_requirement({id, new_text, statement?})`: locate the requirement's verbatim
  quote in its source section, stage an `edit_doc_prose` replacing it with `new_text`,
  and stage the requirement update (`quote: new_text`, plus `statement` when given) in
  the same changeset. Commits atomically; the document's stored hashes absorb the edit
  ([mutations](./graph.md#mutations)), so the edit does not dirty the document it just
  changed. A quote that no longer locates is rejected with the section's current text
  as repair guidance.
- `add_requirement({doc, section, after_quote?, text, statement, entities})`: insert
  `text` into the section (after the located `after_quote`, or at the section's end)
  and stage the requirement sourced from it. The entities must exist; search before
  naming them.
- `retract_requirement({id, reason})`: remove the requirement's sentence from the prose
  and delete the requirement, one changeset. The deletion writes its change records, so
  `retrace` derives where a view or instance referenced it.
- `edit_fact({id, field, value, note?})`: set one authored field on one node
  (`statement`, `edges`, `transition`, `facets`, `definition`, `stereotype`, `parent`,
  an attribute's `type` or `value` as `attributes.<name>.type` or
  `attributes.<name>.value`, a view's `members`; on a default view the edit
  clears `default`). When the fact is
  quote-provenanced and the person accepted a sentence rewrite in conversation, the
  call carries the accepted sentence as `note`: the serving locates the quote and
  commits the prose replacement with the graph mutation as one dual write. Without an
  accepted sentence, or on a derived or decreed fact, the edit lands graph-only with
  `decree` provenance (`note` becomes the decree's note), a `provenance-pending`
  change record is written, and the blocked [`ratify`](./goals/ratify.md) goal derives
  with its [ratification proposal](./model/diagnostic.md#ratification-proposals). The
  compiler never rewrites a source document without an accepted sentence. Ids,
  `created`, `updated`, `mentions`, and provenance itself are never edited directly.
- `retract_decree({id, reason})`: undo one decree. A node created by decree is deleted;
  a field decreed over a quoted fact returns to the prior value and source the decree's
  journal entry recorded. The open `ratify` goal closes.
- `bump_limit({id, limit, value, note?})`: raise one node's own threshold above the
  [built-in limit](./graph.md#per-node-bumps): `limits: {<limit>: value}` on the
  entity or view, recorded with decree provenance in the journal. The `limit` must be
  a registry name that applies to the node's kind, `value` positive. This is how a
  size goal is dismissed: the goal stops deriving until the count crosses the raised
  threshold.
- `answer_diagnostic({id, option?, text?})`: record a human answer the agent relays
  from conversation. An `edit` option applies as a dual write and resolves the finding
  before the tool returns (a ratification proposal's `edit` option also flips the
  fact's provenance to `quote` in the same changeset); an `answer` option or `text`
  records the reply and the tool's response carries the handling contract back to the
  calling agent. See [questions in chat](../frontends/acp.md#questions-in-chat) and
  the [`answer`](./goals/answer.md) goal.

Project tools (see [ACP](../frontends/acp.md#project-tools)):

- `init_project({dir?})`: scaffold `jazyk.toml` and the starter layout. Offered only
  where no `jazyk.toml` exists.
- `update_project_settings({...})`: typed edits to `jazyk.toml` (workflow modes, the
  docs glob, lint rules, the `[acp]` profile, the `[executors]` overrides), rendered as
  minimal edits ([project settings](./project-settings.md)).

## Feedback tool

`report_feedback({kind, subject?, message})` is the model's channel back to jazyk's own
developers. It reports that a prompt, a skill, a tool, a schema, or an error message is
ambiguous, wrong, confusing, or missing something, not that the project's documents are.
The channel is a local file: each call appends one line to the project's
`<out>/feedback.jsonl`, and nothing is transmitted anywhere. The tool description and
the serving instructions say so, since an agent whose own guard screens outbound data
once refused the call as an export to an external destination.
Findings about the documents are [diagnostics](./model/diagnostic.md); findings about
jazyk are feedback.

- `kind`: one of `ambiguous`, `wrong`, `confusing`, `missing`, `other`. An unknown value
  is recorded as `other` rather than rejected.
- `subject`: what the feedback is about, e.g. a tool name, an argument, an instruction,
  a skill.
- `message`: what was unclear and what would have helped. Trimmed to 4000 characters.

The tool never touches the graph. It stages no mutation, counts against no mutation
budget, and passes no gate beyond a non-empty `message`, so a confused model is never
bounced while asking for help. It is in every [toolset](#toolsets), including the
read-only MCP serving.

Each call appends one JSON line to `<out>/feedback.jsonl`, with the payload plus the
references that identify the caller: the timestamp, the source (`session` or `mcp`), the
batch (the goal ids of the open batch, absent when none is open), the serving mode for an
MCP call, the model, the codec, the store generation, the run's transcript name, and the
MCP client name when one is known. The log is append-only and is never read back by the
compiler. The [GUI](../frontends/gui.md#feedback) renders its history.

A session records at most 5 feedback entries (an MCP serving, 5 per open batch). Beyond
that the call is acknowledged without a record, so a confused model cannot flood the
log.

The reply is `{recorded, note}`. The note tells the model to continue with its best
judgment: feedback is not an escape from the goal.

## Spend tool

`report_spend({tokens, cached?})` records what an external agent's run billed for the
open batch. A session the embedded agent runs measures its own spend from the response
usage ([budgets](./sessions.md#budgets)); a session an external agent drives over MCP
([compilation over MCP](../frontends/mcp.md#compilation-over-mcp)) spends tokens only its
driver can see, so its journal entry carries none unless the agent says what it billed.

- `tokens`: the tokens the agent's endpoint billed for this batch, input and output of
  every round. A whole number above zero.
- `cached`: the input tokens among them the endpoint served from its prompt cache.
  Optional, at most `tokens`.

The figure is the agent's own report of its bill, never an estimate by jazyk; an agent
that does not know its usage does not call the tool. The reply is
`{recorded, tokens, cached}`. A second call in the same batch replaces the first: the
figure is the batch's whole spend so far, not an increment. At the batch's commit the
`session` entry's `tokens` and `cachedTokens` take the reported figure and
`tokensSource: agent` says where it came from ([journal](./graph.md#journal)), so the
cost report can count it apart from measured spend. The tool stages no mutation,
counts against no mutation budget, and passes no gate beyond the number check. It is in
every [toolset](#toolsets), the read-only servings included; outside an open batch it
is refused toward [`jazyk spend`](../frontends/cli.md#jazyk-spend), which stamps a
committed entry after the fact.

## Compilation tools

Compilation over MCP is goal-based: the agent claims goal batches from
[the board](./reconciler.md#goal-derivation), staging graph writes into an open
changeset exactly as an in-process session does. One batch is open at a time per
serving ([compilation over MCP](../frontends/mcp.md#compilation-over-mcp)).

- `goals({})`: the board: every goal with its kind, class (`compile` or `gc`),
  `mandatory`, target, unit, `change`, `cause`, state, hints, and
  [readiness](./reconciler.md#readiness) (`ready`, or the blocking reason as a
  sentence), plus `gated` and `claimedBy` from the
  [control plane](./control-plane.md). The reply groups the ready goals into the
  batches the scheduler would form, each under its id. A batch id is
  `b<generation>-<n>`: the generation the board derives from and the batch's index
  within it. Batch ids are derived with the board, so a commit re-derives both and an
  id from an earlier generation names nothing. When no goal is open the reply carries
  the build [verdict](./compilation.md#convergence) with its counts instead; nothing
  to do is an answer.
- `begin_goals({batch?, goals?, full?})`: claim the named batch (`batch`, an id from
  the `goals` reply), or the named goals as one batch (`goals`, when they share a
  locality and their kinds' executors agree), or with neither the next ready batch
  (the highest ready tier, one locality, filled to the context budget:
  [batching](./reconciler.md#batching)). A `batch` the current board does not hold is
  refused, and the reply carries the current batches so the agent picks again without
  a second `goals` call. The claim reloads the store, syncs section trees in memory,
  opens a changeset, and returns the batch id, its goals, the assembled
  [session prompt](./sessions.md#the-prompt) as `instructions`, and the initially
  [loaded set](./context.md#the-loaded-set) as `package`. The serving's toolset for the
  batch is the union of its goal kinds' [toolsets](#toolsets). The first batch of a
  serving ships the agent contract and the active skills in full; later batches elide
  what the agent already saw. `full: true` repeats everything for a client that lost
  its context. Claiming under a `manual` mode without a release is rejected
  `awaiting-release`. In a session's serving, which already holds its batch, the
  protocol line's `begin_goals({batch})` names that batch and answers with a short
  acknowledgement ([execution](./sessions.md#execution)).
- `done({summary, beginNext?})`: the same tool as in [goal tools](#goal-tools): run the
  batch gates, commit atomically, write the journal entry with `resolved_goals` and
  `opened_goals`, re-derive the board, and return `{committed, next}` naming the next
  ready batch. `beginNext: true` claims it in the same call and carries its
  `instructions` and `package` in the reply. The commit that leaves no open goal on the
  board (blocked and optional goals aside) also runs the deterministic tail
  ([checks](./compilation.md#checks), rendering, docsgen, verdict) and reports the
  verdict and the generation rows that became ready.
- `abandon_goals({reason})`: drop the staged changeset. Leaves no trace; the batch's
  goals return to `open`.

## Generation tools

[Generation](../consumers/gen.md) is a workflow over the graph, so its steps are tools
too. Any agent that speaks MCP can be the generation worker; `jazyk gen` is one such
worker in-process (the [`generate`](./goals/generate.md) goal; its session runs in
the `generate` serving, see [toolsets](./sessions.md#toolsets)). These tools read the graph and the
[ledger](../consumers/gen.md#the-ledger) (`gen/ledger.yaml`); they never mutate the
graph. The agent edits deliverable files and runs commands with its own tools; jazyk
serves no file editing over MCP. For an agent that brings none (the embedded agent),
the serving adds `read_text_file`, `write_text_file`, `list_files`, and `run_command`,
sandboxed to the deliverable, when the profile sets
[`serve_files`](./project-settings.md#acp)
([file and command tools](./goals/generate.md#file-and-command-tools)). The sandbox
holds after symlinks resolve: a path is relative, carries no `..`, and its deepest
existing ancestor canonicalizes under the canonical deliverable directory, so a link
inside the deliverable that points outside is refused (`bad-path`) like `..` would be,
and `list_files` does not list it. A path that starts with the deliverable directory's
own name while the deliverable holds no directory of that name is normalized under
the root, and the reply's `note` says so
([file ownership](../consumers/gen.md#file-ownership-and-conventions)).
The file inventory includes `.github` and `.gitignore` within the selected source
scope. Other hidden paths keep the shared
[file-role exclusions](./document-sets.md#matching-and-file-roles).

- `generation_tasks({})`: entities whose facts differ from the ledger, the targets of
  the open `generate` goals: `{entity, reason, changed, ready, blockedBy?}` where
  `changed` lists the requirement ids added, removed, or reworded since the entity was
  last generated and `blockedBy` is the board's readiness reason for a row that is not
  ready (its requirements still owe a binding, a release). `begin_generation` refuses a
  row that is not ready with the same reason (`not-ready`).
- `begin_generation({entity})`: the full package for one entity: the instructions (the
  generation contract: both halves per entity, traceability markers, the two test
  kinds, the parts protocol), the entity's loaded set, its requirements in generation
  groups (by component where containment exists), the change diff, the deliverable
  directory, the `factHash`, the manifest of already generated files with what each
  holds, and the deliverable's
  [medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)
  and build. Layout, file names, and run commands are the worker's choices, recorded
  in the manifest; the medium is not, it is already decided, or the package carries
  `mediumNote` asking the worker to decide it first with `record_medium`. Stateless on
  the server: beginning claims nothing and holds nothing; the lease is the claim.
- `record_medium({form, produced, toolchain?, artifact?})`: record what the
  deliverable is made of when no session has
  ([the medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)):
  `produced` is `written` or `built`, `artifact` is required when built. Replaces an
  earlier decision until an entity is generated, refused (`medium-settled`) after.
- `record_generation({entity, factHash, manifest, choices?})`: record the entity
  generated. The worker writes the deliverable files itself; the `manifest` binds
  them to the graph: `{files: [...], supportFiles?: [...], tests: [{requirement, kind,
  label, artifact, name, run, cwd}], build?}`, `supportFiles` the deliverable-wide
  files ownership never applies to
  ([file ownership](../consumers/gen.md#file-ownership-and-conventions)). A `files`
  path an entity outside the entity's component group records is refused
  (`bad-manifest`) naming the owner, and so is a `files` list with no implementing
  file while a requirement of the entity has no row naming one. A path that restates
  the deliverable directory's name is normalized under the root and named in the
  reply's `note`. Marking strips every single-line marker comment from the
  manifest files and records each as an anchored site on its requirement's row
  ([traceability](../consumers/gen.md#traceability)), then updates both ledger maps
  and [prunes rows](../consumers/gen.md#deletion-prunes-the-ledger) whose requirement
  left the graph; the `generate` goal's gate reads the landed record. `choices` lists
  what the worker had to invent, each `{choice, scope, reasoning, requirements?}`,
  recorded as [`invented-choice`](../consumers/gen.md#invented-choices) diagnostics
  graded by scope. A `factHash` that no longer matches the live graph is recorded but leaves the
  entity pending, so a graph that moved mid-goal is never masked.

## Binding tools

[Binding](../consumers/bind.md) creates the ledger rows generation and verification
work from. Same worker model as generation: the agent searches and edits with its own
tools; jazyk holds the ledger. These tools never mutate the graph.

- `binding_tasks({})` (rows sorted by entity with a `byEntity` count, since generation
  works per entity and a session once bound an entity's rows by hand to make its task
  useful): requirements owing a binding, the targets of the open
  [`bind`](./goals/bind.md) goals, with a `reason` (`unbound`, `requirement-changed`,
  `artifact-gone`). Deterministic; no model involved.
- `begin_binding({requirement})`: the package for one requirement: the instructions
  (the [bind contract](../consumers/bind.md#the-bind-goal): search before write, both
  directions get a test, the two test kinds, falsifiability, the naming scheme), the
  statement, quote, and hash, the loaded set, the deliverable directory, the decided
  [medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)
  and build when they exist, the test conventions already recorded in the ledger, and
  `existingTests`, the deliverable files that name the requirement by id, slug, or
  statement hash (a test left under an earlier id is found here, never by hand).
  Stateless on the server; the lease is the claim.
- `record_binding({requirement, files, tests?, test?, verdict?, evidence?})`: record the
  row: the implementing files (an empty list is a finding), one test per
  [criterion](./model/criterion.md) under `tests`
  (`[{criterion, kind, label, artifact, name, run, cwd?, verdict, evidence?}]`; a
  requirement with criteria is refused without them, the package lists them with
  suggested names), and for a requirement without criteria the requirement-level
  `test` (`{kind, label, artifact, name, run, cwd}`) with its `verdict`; with criteria
  the first criterion's test stands in as the row's headline and the verdict aggregates. Rejects a test
  whose artifact does not exist or does not contain the declared name, the same shape
  gate `record_generation` applies. The row's
  [derived status](../consumers/gen.md#status-is-derived-never-stored) classifies the
  requirement (`verified`, `unimplemented`, `failing`); the `bind` goal's gate reads
  the landed row.

## Decompilation tools

[Decompilation](../consumers/decompile.md) produces documents, never graph writes. It
stays outside the goal board: released scopes are its worklist.

- `decompile_tasks({})`: the released scopes with their unclaimed files and inventory
  summaries.
- `begin_decompile({scope})`: the package for one draft: the inventory slice, the test
  files with their assertions, the lint rules, and the
  [drafting contract](../consumers/decompile.md#draft-goals).
- `submit_draft({path, content})`: validate the draft (lint rules, extractable
  statements, evidence anchors) and write it into the docs tree. Records the draft hash
  for [ratification](../consumers/decompile.md#ratification) and consumes the scope's
  release.

## Verification tools

Verification runs the tests the ledger records and feeds verdicts back. Same worker
model: `jazyk test` is the built-in worker; any MCP agent is another. These tools write
only the ledger, never the graph.

- `verification_tasks({filter?, entity?})`: rows needing action, the targets of the
  open [`verify`](./goals/verify.md) goals, with their derived
  [status](../consumers/gen.md#status-is-derived-never-stored) and a `reason`
  (`not-generated`, `artifact-gone`, `never-run`, `requirement-changed`,
  `test-changed`, `code-changed`, `runner-failed`). A `failing` row is a finding for
  the author, never a reason: it is listed again only when its test or files change
  ([created when](./goals/verify.md#created-when)). Requirements the graph holds but the
  ledger does not appear as `missing`, so ungenerated work is never silent. Rows whose
  requirement left the graph are not listed: they are not work, and the next
  `record_generation` [prunes them](../consumers/gen.md#deletion-prunes-the-ledger).
  Deterministic; no model involved.
- `begin_verification({requirement})`: the package for one row: the statement, quote,
  and hash; the loaded set; the manifest files; and either the run command
  (`programmatic`) or the criteria and confirm-steps (`llm`).
  A saved-task reference includes instructions to use `run_tests` and inspect
  per-case evidence, not infer a verdict from the container's exit code.
  `criterionTests` lists every bound test with its criterion id, statement hash,
  kind, artifact, and current verdict. LLM entries include the criteria text.
  A mixed row includes both execution guidance and LLM judgment guidance.
- `run_tests({requirements?, all?})`: execute the recorded programmatic tests: the
  [build](../consumers/gen.md#the-build) first, once, then each selected row's command.
  `requirements` selects rows (requirement or entity ids); empty runs every
  non-verified row; `all: true` reruns every programmatic row, verified ones included,
  the green-light check a worker wants before it stops. Verdicts and evidence are
  recorded as a side effect and returned. `llm` rows are
  skipped here; they go through `record_verdict`. This is how a worker verifies without
  hand-rolling the harness, and it serves in the generation toolset too.
  Shared `task:<id>` references execute once per selection, after the build. Mixed
  selections also run their legacy commands locally; hosted responses list those
  commands as pending. Task criteria reuse the imported case evidence.
  An explicitly selected unbound requirement is returned as skipped with binding
  guidance. An empty execution cannot be reported as passing that requirement.
- `record_verdict({requirement, verdict, factHash?, evidence?, criteria?})`: record a `pass` or
  `fail` verdict with its evidence, rebaselining the test and files hashes. A stale
  `factHash` is recorded but the row stays pending, the same protection
  `record_generation` has. The `verify` goal's gate reads the landed verdict.
  When multiple LLM criteria are bound, `criteria` is required. It contains one
  `{criterion, verdict, factHash, evidence?}` entry for each LLM test. Copy the ids
  and hashes from `criterionTests`. Duplicate, missing, stale, or programmatic
  entries are rejected before any verdict changes. A single LLM headline retains
  the short form. A programmatic headline with LLM criteria uses the explicit list.
  The headline verdict summarizes the LLM entries; it cannot replace programmatic
  evidence. Changing implementing files invalidates earlier criterion evidence.
  A fresh verdict or report restores only the tests it actually judged.

## Validation and errors

Every call is validated by the [gates](./graph.md#validation-gates). An error names the
violated rule and how to repair the call. E.g.:

```
quote not found in docs/cli.md#/cli/commands; copy the sentence verbatim from the section
```

Errors are part of the tool design. They are written for a model that will read them and
try again. Resolution is lenient toward intent, and that includes shape: an optional
argument that is empty (an empty string, an empty list, or an object whose fields are
all empty) counts as absent. An all-empty item inside a list argument is dropped the
same way. A model that fills every schema field with empty values makes the same call
as one that omits them. Some endpoints hand the model every schema field as required,
and a model then fills the ones it means to omit.

- The arguments are normalized once, where every call enters, before any gate or the
  repeated-call guard reads them: empty values go at any depth, so two calls that
  differ only in filler are the same call. A top-level `null` stays, since some tools
  read it as "remove".
- A `transition` that names no state (`from` and `to` both empty) is a filled-in blank
  and counts as absent.
- A fact that arrives with its quote and a derivation both (an entity's `mention`, a
  requirement's or a criterion's `section` and `quote`, beside `provenance.derived`)
  keeps the quote, its one provenance, and the derivation is dropped. The reply says
  so in `note`. A quote that does not locate is still refused (`bad-quote`).
- A refusal names the case it met. An exclusive pair that arrives with both says to
  drop one and which; with neither, it says which to give. E.g., a prompt option with
  both `edit` and `answer` is told to keep `edit` when the option changes the document.

The named errors a caller meets beyond the gates:

- `repeated-call`: the third identical call in a session
  ([repeated calls](./sessions.md#repeated-calls)); `load` of a loaded target and
  `expand` of a closed handle count.
- `context-full`: `load` or `expand` past the high-water mark of the context budget;
  the error names the unload candidates ([policy](./context.md#policy)).
- `unknown-handle`: `expand` on a closed or unknown handle; the error names the open
  handles ([expansion handles](./context.md#expansion-handles)).
- `unknown-anchor`: `place_anchor` or `orphan_anchor` on an id outside the batch's
  proposals.
- `wrong-document`: a quote from a document the reconciled section merely links to.
- `undecided-proposal`: `done` in a `place-anchors` session with a proposal undecided.
- `stated-entity`: `dissolve_entity` on an entity a document states; revise the
  documents instead ([grouping tools](#grouping-tools)).
- `awaiting-release`: `begin_goals` or a ledger `begin_*` under a `manual` mode with no
  release ([modes and releases](./control-plane.md#modes-and-releases)).
- `build-running`: `begin_goals` or a ledger `begin_*` while an internal build holds
  the build lease; compilation is sequential
  ([workers and leases](./control-plane.md#workers-and-leases)).
- `claimed`: `begin_goals` on a batch another worker holds; the error names the holder
  ([the control plane over MCP](../frontends/mcp.md#the-control-plane-over-mcp)).
- `session-complete`: a write tool, goal tool, `done`, `abandon_goals`, or
  `begin_goals` on a scoped serving whose batch already committed; the error names the
  batch and says to end the turn ([`--only`](../frontends/mcp.md#mcp-into-acp-sessions)).
- A write tool or goal tool outside an open batch: rejected toward `begin_goals`.
- A tool outside the batch's toolset is not served; a call to it is an unknown tool.

## Toolsets

Sessions see subsets, not the whole catalog: the union of the batch's goal kinds'
toolsets ([toolsets](./sessions.md#toolsets)). Every subset carries the
[read tools](#read-tools), the [goal tools](#goal-tools),
[`report_feedback`](#feedback-tool), and [`report_spend`](#spend-tool); they are listed
once here, not per kind. Each kind's page under `goals/` repeats its slice.

Compile goals:

- [`place-anchors`](./goals/place-anchors.md): `place_anchor`, `orphan_anchor`.
- [`reconcile-section`](./goals/reconcile-section.md): `upsert_entity`,
  `update_entity`, `delete_entity`, `upsert_requirement`, `update_requirement`,
  `delete_requirement`, `upsert_criterion`, `update_criterion`, `delete_criterion`
  (the stated criteria: test cases, examples, thresholds), `set_coverage`. No `report_diagnostic`: extraction records,
  judgment goals judge.
- [`derive-criteria`](./goals/derive-criteria.md): `upsert_criterion`,
  `update_criterion`, `delete_criterion`, `report_diagnostic` (`invented-choice`,
  `ambiguity`).
- [`rejudge-pair`](./goals/rejudge-pair.md): `update_requirement`,
  `delete_requirement`, `update_entity` (a wording the pair's quotes settle, such as a
  nickname, lands as an alias instead of being dropped until a review), `report_diagnostic`
  (`contradiction`, `duplicate-requirement`), `resolve_diagnostic`, and the batch claim
  `settle_pairs` ([goal tools](#goal-tools)).
- [`review-entity`](./goals/review-entity.md): `update_entity`, `merge_entities`,
  `update_requirement` (a review adds missing `edges` when requirements tie entities
  structurally), `delete_requirement`, `report_diagnostic`, `resolve_diagnostic`.
- [`retrace`](./goals/retrace.md): the [view tools](#view-tools), `upsert_entity`,
  `update_entity`, `delete_entity`, `merge_entities`, `upsert_requirement`,
  `update_requirement`, `delete_requirement`, `report_diagnostic` (`missing-link`,
  `decision`).
- [`conform-instance`](./goals/conform-instance.md): `update_entity`,
  `update_requirement`, `report_diagnostic` (`nonconformant-instance`,
  `duplicate-entity`, `ambiguity`), `resolve_diagnostic`.
- [`bind`](./goals/bind.md): the `generate` serving below.
- [`generate`](./goals/generate.md): the `generate` serving below, plus the file and
  command tools when the profile sets `serve_files`.
- [`verify`](./goals/verify.md): the `verify` serving below, plus the read-only file
  tools (`read_text_file`, `list_files`, `run_command`) when the profile sets
  `serve_files`.
- [`ratify`](./goals/ratify.md), [`answer`](./goals/answer.md): no session, no
  toolset. The human path runs through the [chat tools](#chat-tools), the LSP, and the
  GUI.

GC goals:

- [`declare-edges`](./goals/declare-edges.md): `update_requirement`.
- [`dedupe-candidates`](./goals/dedupe-candidates.md): `merge_entities`,
  `update_entity`, `report_diagnostic` (`duplicate-entity`).
- [`curate-view`](./goals/curate-view.md): the view tools.
- [`split-view`](./goals/split-view.md): the view tools.
- [`abstract-entity`](./goals/abstract-entity.md): `upsert_entity` (derived
  provenance), `update_entity`, `upsert_requirement` (derived provenance, the docs
  proposals `ratify` follows), `update_requirement`, `upsert_view`, `update_view`,
  `group_entities` and `dissolve_entity` (the [fan-out](./reconciler.md#fan-out)
  variant, see [grouping tools](#grouping-tools)), and `report_diagnostic`
  (`decision`, for structure the documents cannot yet support). No deletes, no
  merges, no `delete_view`; a dissolve leaves a redirect, it is not a delete.

Servings (`jazyk mcp <toolsets>`, see [MCP](../frontends/mcp.md#toolsets)); every
serving carries the read tools, `report_feedback`, and `await_changes`
([the work loop](../frontends/mcp.md#the-work-loop)):

- `jazyk mcp compile`: the [compilation tools](#compilation-tools) (`goals`,
  `begin_goals`, `done`, `abandon_goals`), the [goal tools](#goal-tools), and the
  write tools and view tools, gated behind an open batch and narrowed to the batch's
  goal kinds.
- `jazyk mcp generate`: the [binding tools](#binding-tools), the
  [generation tools](#generation-tools), and `run_tests`. Binding and generation share
  the serving because they share the worker persona: search and edit the deliverable,
  record into the ledger. A `bind` or `generate` session sees this serving plus the
  goal tools.
- `jazyk mcp verify`: the [verification tools](#verification-tools). A `verify`
  session sees this serving plus the goal tools.
- `jazyk mcp decompile`: the [decompilation tools](#decompilation-tools).
- `jazyk mcp benchmark`: the [benchmark cases](../benchmark/benchmark.md#agent-run-benchmarks)
  against sandbox stores.
- `jazyk mcp graph`: the read tools only; `--write` adds the raw write tools and view
  tools, each call committing as its own changeset with its change records written, so
  the next build derives the goals the write opened.
- `jazyk mcp chat`: the compilation, binding, generation, and verification lifecycles,
  the [chat tools](#chat-tools), `update_diagnostic`, and `answer_diagnostic`. Native
  and hosted chat also expose scoped `release_work` and guarded project file tools.
  There are no raw graph write tools outside an open batch. Prose edits use the dual
  write path. Deliverable writes use the binding and generation ownership gates.

Tool input and output shapes are specified in [`tools.schema.yaml`](./tools.schema.yaml).

## Task runtime tools

[Task execution](task-runtime.md#tools-and-access) supplies the shared task and run
tools to generation, binding, verification, writable MCP, and ACP sessions.

## Conversational document writes

`write_document({path, text, baseHash?})` replaces one prose document. A read
returns the current `hash`, which the write sends as `baseHash`. Omitting the
hash creates a new document exclusively. A stale hash refuses the write. The
tool works before compilation and never releases compile work. It validates
document-set membership and safe paths, refuses binary documents, and uses the
same document writer as the editor. ACP document edits use the editor sink when
one is attached. Normal document saves dirty the affected sections for the next
build. Editing an identified compiled fact uses the existing dual-write tools.

General questions need no project discovery. The conversational contract is
embedded from `goals/prompts/chat-conversation.md`. Detailed lifecycle packages
apply to requested lifecycle work. A document edit does not authorize compilation,
generation, task execution, or a workflow release. A tool repair hint grants no
new authority.
