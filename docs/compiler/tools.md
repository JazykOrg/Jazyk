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

The catalog is deliberately small. Weak models handle few, simple tools better than many
clever ones. No tool enqueues work: the model writes graph state, the harness derives the
[goals](./reconciler.md#goal-derivation).

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
  ([policy](./context.md#policy)).
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
  `{id, name, definition}` entries. No embeddings, no LLM. A miss is an answer, not a
  dead end: `hits` is empty and the result carries `entityCount`, the graph's entities
  by id and name (up to 25), and a `next` line saying the search will keep returning
  this and to create the entity instead. A bare empty list reads as "ask again", and
  models loop on it; naming the whole graph lets the caller decide without another
  call.
- `read_section({ref})`: one section's raw body and its child titles. A bare document
  name (`orders.md`, or `orders.md#`) reads the document's root section, the way a
  reader opens a file to see what it is about.
- `get_entity({id})`: one entity with its definition, stereotype, parent, attributes,
  mentions, requirements, relationships, and its derived state machine when one exists.
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
  message, and whether it carries an unanswered prompt. This is the read surface for
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
  the wording as an alias; a `note` overrides. A name that looks like syntax rather
  than a concept is rejected unless `note` explains it
  ([what is an entity](./model/entity.md#what-is-an-entity)). Omit `scope` unless the
  documents explicitly name a bounded context.
- `update_entity({id, name?, definition?, add_aliases?, stereotype?, parent?, attributes?})`:
  a rename keeps the id. `attributes` upserts by name; attributes not named stand.
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
  the existing statement is the same fact reworded: it also updates in place. A stale
  anchor in the same section whose statement subsumes or is subsumed by the new
  statement resolves the same way, so re-recording a reworded statement lands on the
  anchor's id. A sentence carrying several atomic facts is unaffected, since those
  statements are not subsets of each other. `edges` are `[{a, b, type?, cardinality?}]`,
  directional, both ends among `entities` ([edges](./model/requirement.md#edges)).
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
  change.
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
  on its old document. Same `id` rule.
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
  most 4 options, each with a `label` and exactly one of `edit` or `answer`, and an
  `edit`'s `old_text` must locate in its section when non-empty. A diagnostic with an
  unanswered prompt opens the blocked [`answer`](./goals/answer.md) goal. The reply
  carries the finding's `id` (a re-report answers with the id it updates), and the
  session's own staged findings are visible to `diagnostics`, `update_diagnostic`,
  and `resolve_diagnostic`.
- `update_diagnostic({id, prompt})`: replace the prompt on an open diagnostic (pass
  `null` to remove it). The finding itself is edited through `report_diagnostic`'s
  natural-key upsert; this tool only maintains the question. Never touches a human-set
  `answer` or `triage`.
- `resolve_diagnostic({id, reason})`.
- `set_coverage({section, state, note?})`: `state` is `covered` or `non-normative`.
  `non-normative` requires the `note`; a placeholder note (`<nil>`, `none`, `n/a`)
  counts as absent. `non-normative` is rejected for a section that already yielded a
  requirement, in the store or in this session's own staged work: the mark and the
  statement contradict each other, and the statement is the evidence. The rejection
  names the statements and asks for `covered`. A repeated call for the same section
  within one session replaces the earlier mark: a changeset carries at most one
  coverage mark per section. A `covered` claim requires a requirement sourced from
  that section; the `done` gate enforces it ([coverage](./compilation.md#coverage)).

There is no write tool for relationships, state machines, or default views. They are
[derived data](./graph.md#derived-data), recomputed on every commit from requirement
`edges` and `transition` facets and from the graph's structure.

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
  grouping left with fewer than two children ([the sweep](./graph.md#the-sweep)).

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
  it; `exclude` adds one `{id, note}` pair. On a default view any field clears the
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
  the kind's page under `goals/`). `evidence` carries what the gate reads for kinds
  that ask for it, e.g. a verdict per neighbor for
  [`rejudge-pair`](./goals/rejudge-pair.md). A goal outside the batch is rejected.
- `mark_goal_failed({goal, reason})`: always accepted. A goal that cannot be
  accomplished (documents too deeply contradictory, a target that no longer makes
  sense) must be failable, or the board fills with dishonestly resolved goals. A failed
  goal keeps its target, so the failure surfaces on the thing itself everywhere it
  renders. A failed mandatory goal blocks convergence; a failed optional goal is
  recorded and stands ([parked and failed](./reconciler.md#parked-and-failed)).
- `load_skill({name})`: bring one [skill](./sessions.md#skills) into the session. The
  skill index line of the status block lists the names. An unknown name is rejected
  with the index; a call past the per-session cap (four, the registry constant
  `skills-per-session`) is refused naming the cap and the skills already rendered.
- `done({summary, beginNext?})`: end the session and request commit. Runs the batch
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
([file and command tools](./goals/generate.md#file-and-command-tools)).

- `generation_tasks({})`: entities whose facts differ from the ledger, the targets of
  the open `generate` goals: `{entity, reason, changed}` where `changed` lists the
  requirement ids added, removed, or reworded since the entity was last generated.
- `begin_generation({entity})`: the full package for one entity: the instructions (the
  generation contract: both halves per entity, traceability markers, the two test
  kinds, the parts protocol), the entity's loaded set, its requirements in generation
  groups (by component where containment exists), the change diff, the deliverable
  directory, the `factHash`, the manifest of already generated files with what each
  holds, and the deliverable's
  [medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)
  and build. Layout, file names, and run commands are the worker's choices, recorded
  in the manifest; the medium is not, it is already decided. Stateless on the server:
  beginning claims nothing and holds nothing; the lease is the claim.
- `record_generation({entity, factHash, manifest, choices?})`: record the entity
  generated. The worker writes the deliverable files itself; the `manifest` binds
  them to the graph: `{files: [...], tests: [{requirement, kind, label, artifact, name,
  run, cwd}], build?}`. Marking strips every single-line marker comment from the
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

- `binding_tasks({})`: requirements owing a binding, the targets of the open
  [`bind`](./goals/bind.md) goals, with a `reason` (`unbound`, `requirement-changed`,
  `artifact-gone`). Deterministic; no model involved.
- `begin_binding({requirement})`: the package for one requirement: the instructions
  (the [bind contract](../consumers/bind.md#the-bind-goal): search before write, both
  directions get a test, the two test kinds, falsifiability, the naming scheme), the
  statement, quote, and hash, the loaded set, the deliverable directory, the decided
  [medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)
  and build when they exist, and the test conventions already recorded in the ledger.
  Stateless on the server; the lease is the claim.
- `record_binding({requirement, files, test, verdict, evidence?})`: record the row: the
  implementing files (an empty list is a finding), the test row
  (`{kind, label, artifact, name, run, cwd}`), and the first verdict. Rejects a test
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
- `run_tests({requirements?})`: execute the recorded programmatic tests: the
  [build](../consumers/gen.md#the-build) first, once, then each selected row's command.
  Verdicts and evidence are recorded as a side effect and returned. `llm` rows are
  skipped here; they go through `record_verdict`. This is how a worker verifies without
  hand-rolling the harness, and it serves in the generation toolset too.
- `record_verdict({requirement, verdict, factHash?, evidence?})`: record a `pass` or
  `fail` verdict with its evidence, rebaselining the test and files hashes. A stale
  `factHash` is recorded but the row stays pending, the same protection
  `record_generation` has. The `verify` goal's gate reads the landed verdict.

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
as one that omits them.

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
- A write tool or goal tool outside an open batch: rejected toward `begin_goals`.
- A tool outside the batch's toolset is not served; a call to it is an unknown tool.

## Toolsets

Sessions see subsets, not the whole catalog: the union of the batch's goal kinds'
toolsets ([toolsets](./sessions.md#toolsets)). Every subset carries the
[read tools](#read-tools), the [goal tools](#goal-tools), and
[`report_feedback`](#feedback-tool); they are listed once here, not per kind. Each
kind's page under `goals/` repeats its slice.

Compile goals:

- [`place-anchors`](./goals/place-anchors.md): `place_anchor`, `orphan_anchor`.
- [`reconcile-section`](./goals/reconcile-section.md): `upsert_entity`,
  `update_entity`, `delete_entity`, `upsert_requirement`, `update_requirement`,
  `delete_requirement`, `set_coverage`. No `report_diagnostic`: extraction records,
  judgment goals judge.
- [`rejudge-pair`](./goals/rejudge-pair.md): `update_requirement`,
  `delete_requirement`, `report_diagnostic` (`contradiction`,
  `duplicate-requirement`), `resolve_diagnostic`.
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
  the [chat tools](#chat-tools), `update_diagnostic`, and `answer_diagnostic`. No raw
  write tools: a chat edit moves the prose and the graph together or not at all.

Tool input and output shapes are specified in [`tools.schema.yaml`](./tools.schema.yaml).
