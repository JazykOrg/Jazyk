# Reconciler

The reconciler drives [compilation](./compilation.md). It compares the documents
(desired state) against the graph (observed state), derives the goals that separate the
two, and schedules [sessions](./sessions.md) until no mandatory goal remains. It is
deterministic code. The model never decides what is stale, what work exists, or what
runs next: it resolves, fails, or parks the goals it is handed
([division of labor](./compiler.md#division-of-labor)). What a build does with the
scheduled work is [compilation's](./compilation.md) to describe; whether anyone may act
on it is the [control plane's](./control-plane.md).

The loop is level-triggered, not edge-triggered. Nothing about pending work is stored
([what needs building](./graph.md#what-needs-building)). Every derivation reads the
current documents, the current graph with its decisions and sources, and the
[ledger](../consumers/gen.md#the-ledger), so a missed or duplicated change notification
is harmless, any process computes the same board, an interrupted build resumes from any
consumer, and a clone or a git merge needs no bookkeeping. Initial compilation is not a special case:
it is reconciliation against an empty graph. A rebuild with nothing changed derives zero
goals and makes zero LLM calls.

## Dirty set

Staleness is computed, never judged:

- [Parse](./parsing.md) every matched document into a section tree with per-section
  content hashes.
- [Align](./alignment.md) the trees against the sources, the section trees the graph
  last reflected ([graph store, `sources/`](./graph.md#storage-layout)), across all
  documents:
  - added or changed section → dirty,
  - moved section (same title and body, new reference) → not dirty; the store
    rewrites anchored references mechanically,
  - edited, split, merged, or removed section → dirty where it still exists; its anchors
    are relocated to their best candidate as proposals for the
    [`place-anchors` goal](./goals/place-anchors.md), and anchors with no candidate
    become stale anchors.
- Anchors whose quote fails to locate in their section on disk join the stale anchors of
  their document.
- Map dirty sections to affected graph nodes through mentions and requirement sources.

The dirty set is computed, never stored. Each finding becomes one
[change record](./graph.md#change-records) in memory for the derivation that found it:

- `section-dirty`: the subject is the section reference. `detail` carries the hash in
  `sources/` and the line diff against that body: the counts `added` and `removed` and
  the lines `addedLines` and `removedLines`
  ([created when](./goals/reconcile-section.md#created-when)).
- `section-removed`: the subject is the reference the section had in `sources/`.
  `detail` lists the nodes anchored there. It derives no goal of its own; the sweep
  handles the nodes anchored there.
- `anchor-stale`: the subject is the section reference the anchor points to, even when
  that section is gone. `detail` lists the anchored nodes and why each is stale
  (`no-candidate`, `quote-missing`).
- `alignment-pending`: the subject is the document path. `detail` carries the proposals
  alignment computed for it
  ([what applies and what is proposed](./alignment.md#what-applies-and-what-is-proposed)).
  A derivation recomputes them; a `place-anchors` session's placements change the graph,
  so a placed anchor is no longer proposed.

A build writes the result of its own alignment: the exact moves it applied and the
sections it synced land in `sources/` under one `align` generation, and a human save
that dirtied sections journals as an `edit` generation, so every ripple roots in one
([journal](./graph.md#journal)). Neither writes a record.

Sections whose coverage is still `unprocessed` need no finding of their own:
[coverage](./compilation.md#coverage) is graph state, and `reconcile-section` goals
derive from it directly. A store whose version does not match the binary is upgraded,
refused, or archived by [store version](./graph.md#store-version); an archived one starts
from an empty graph, which is an ordinary dirty set with every section dirty.

## Goal derivation

Compilation is a goal board. A goal is one unit of work the harness holds evidence for:
a dirty section, a revised statement, a dangling view member, a crossed limit. The board
is derived, never stored. `derive_goals` recomputes it from disk whenever it is
consulted: at the start of a build, after every commit, on every `goals` call over
[MCP](../frontends/mcp.md#compilation-over-mcp), and for the
[GUI board](../frontends/gui.md#board). The inputs are the documents, the graph with its
decisions and sources, the ledger, and, inside a running build, that build's progress
([a build's progress](./graph.md#a-builds-progress)): the goals it parked and the reopen
counts. A derivation outside a build (`jazyk status`, `preview`, `release`, `monitor`,
the GUI board, a `goals` call) aligns the documents in memory and derives from that, so
the board shows the goals the next build will open; it writes nothing. The synced
sources and the `edit` and `align` journal entries are written by the build that aligns
for real, or by the serving's `done` before its commit. A read-only
command never moves the graph. The graph
never stores goals, so it cannot grow with them. A derivation builds its
[derived indices](./graph.md#derived-data) (document order, parent to children, entity
to requirements, the referrer index of the cone walk) once from the store and reads them
for every goal; it stores none of them.

E.g.:

```yaml
g:retrace:view:usecase/holds:
  kind: retrace
  class: compile
  mandatory: true
  target: view:usecase/holds
  unit: view
  change: {deleted: req:orders-6, in: g409}
  cause: {generation: 409, mutation: 2, via: members}
  state: open
  hints:
    - load view:usecase/holds
    - skill flow-views
```

- `kind` names a row of the [catalog](#the-catalog). `class` is `compile` or `gc`.
- `mandatory` goals block convergence; optional goals advise.
- `target` is a node id, a section reference (`doc.md#/ref`), a document path, a pair
  (`req:a~req:b`, smaller id first), or `scope:<scope>` (the top level of a scope, its
  parentless entities; `scope:public` for the default scope, see
  [the scope root](./concepts/levels.md#the-scope-root)). The goal id is
  `g:<kind>:<target>`.
- `change` is the attached evidence and the goal's identity: re-deriving the board
  matches a goal to its predecessor by the change, so a goal survives across builds and
  processes without being stored.
- `cause` names the committed change that made the finding true: the generation, the
  mutation index within it (`0` for store-level causes), and the edge or computation
  that carried the dirtiness. It is read from this machine's journal, the newest entry
  whose mutations touched the subject ([change records](./graph.md#change-records)), and
  is absent when no local entry did (a fresh checkout, work merged from another branch).
- `state` is `open`, `blocked {on}` (a human answer, a ratification, a release, a
  `goal-failed` diagnostic on the target), or `parked {reason}` (inside the build that
  parked it).
- `hints` are computed and honest: what to load, which skill explains the shape, which
  tool typically resolves the kind. They are suggestions; the gate is the truth.

### The registry

One Rust trait, one static registry compiled into the binary, one implementation per
kind. The trait surface is small because the store, gates, context engine, journal,
trace, and control plane are generic underneath it:

- `kind`: the name. `class`: compile or GC. `unit`: what one target is (document,
  section, pair, entity, node, requirement, ledger row, view), so the board and the GUI
  can render it.
- `derive_goals(store, findings) -> Vec<Goal>`: this kind's goals from the store and
  the derivation's computed change records. Deterministic, idempotent, cheap.
- `ready(goal, board) -> Ready | Blocked(reason)`: the [readiness](#readiness) rule,
  the reason rendered as a sentence because every surface shows it.
- `pack(store, batch)`: a batch's initially [loaded set](./context.md#the-loaded-set).
- `toolset()`: the kind's slice of the one [tool registry](./tools.md#toolsets).
- `gates(changeset)`: the batch gate `mark_goal_done` and `done` validate, on top of the
  store's [per-mutation gates](./graph.md#validation-gates).
- `prompt`: the contract paragraph, a payload file under `goals/prompts/` embedded at
  compile time ([the prompt](./sessions.md#the-prompt)).

Adding a capability is one module: a kind, its gate, its hint computer, its skill. No
tool enqueues work: the model writes graph state, the harness derives the goals.

### Change records

Current graph state alone cannot say "revised since last judged". The decisions can: a
judgment made on purpose is stored in the graph with a hash of what it judged
([decisions](./graph.md#what-needs-building)), and a derivation compares the hash with the
graph now. Every derivation computes its change records in memory
([change records](./graph.md#change-records)) from the documents and the sources (the
dirty set), the graph and its decisions, the counts and the name index, the ledger
comparison, and the [checks](./compilation.md#checks). Nothing writes a record, so
nothing clears one.

- One finding per kind and subject. Several findings on one target with the same goal
  kind fold into one goal whose `change` merges their details.
- A goal is resolved when its finding stops: the session's commit changes the graph
  (the section is reconciled and its source synced, the dead member is retraced) or
  writes the decision the kind's gate asks for. The decision a resolved goal writes:

  | goal | writes |
  |---|---|
  | `rejudge-pair` | `graph/decisions/pairs/<a~b>` with both statement hashes |
  | `review-entity` | `graph/decisions/reviews/<entity>` with the entity's fact hash |
  | `conform-instance` | `graph/decisions/conformance/<instance>` with the instance's and the type's fact hashes |
  | `declare-edges` judged not structural | `graph/decisions/structure/<requirement>` with the statement hash |
  | `dedupe-candidates` kept apart | `graph/decisions/lookalikes/<a~b>` with the score |
  | `curate-view` | `graph/decisions/curation/<view>` with the nodes its query matches |

- A decision lapses by itself when what it judged changes, and its finding returns.
  The sweep deletes a decision whose subject left the graph.
- A finding whose evidence lapses on its own goes without a session: a
  `threshold-crossed` count falls back under its threshold, a requirement gains edges by
  any path.

The kinds, what each is computed from, and what derives from it:

| kind | subject | computed from | derives |
|---|---|---|---|
| `alignment-pending` | document | alignment of `sources/` against disk | `place-anchors` |
| `section-dirty` | section reference | a section hash differing from `sources/`, or a section `sources/` lacks | `reconcile-section` |
| `anchor-stale` | section reference | a quote that no longer locates | `reconcile-section` |
| `section-removed` | section reference | a section in `sources/` gone from disk | nothing (the sweep) |
| `requirement-revised` | requirement | a candidate pair with no `pairs/` decision matching both statements | `rejudge-pair` per [neighbor](#pairs) |
| `node-deleted` | a live node naming a dead one | a reference to no live node | `retrace` (view, instance, derived fact); `rejudge-pair` (a surviving diagnostic subject) |
| `entity-changed` | entity | a fact hash differing from the entity's `reviews/` decision, or none | `review-entity` |
| `instance-changed` | instance entity | fact hashes differing from its `conformance/` decision, or none | `conform-instance` |
| `view-member-gone` | view | a curated view listing a dead node | `retrace` |
| `ledger-stale` | requirement (`bind`, `verify`), entity (`generate`) | the ledger comparison, `detail.goal` naming the kind | `bind`, `generate`, `verify` by `detail.goal` |
| `prompt-unanswered` | diagnostic | a prompt without an answer | `answer` |
| `provenance-pending` | derived or decreed node | the node's provenance | `ratify` |
| `threshold-crossed` | the node over a limit | the limit counts | `split-view`, `abstract-entity` |
| `edges-missing` | requirement | several entities, no `edges`, no `structure/` decision on the statement | `declare-edges` |
| `lookalike` | entity pair | the name index over the threshold, no `lookalikes/` decision at or above the score | `dedupe-candidates` |
| `query-match`, `flow-unplaced` | view | the view's query against its `curation/` decision, the flow placement check | `curate-view` |

No goal derives on a target an open `goal-failed` diagnostic names
([parked and failed](#parked-and-failed)): the derivation keeps the finding and renders
the goal `blocked {on: goal-failed}`.

`via` names how dirtiness reached the subject. The set is closed here, and every goal
page draws its values from it:

- a stored reference: `section` (a requirement created from, or a `sources/` entry on,
  that section), `quote` (a source quote that moved, changed, or stopped locating),
  `mentions`, `entities`, `edges`, `parent`, `members`, `collapse`, `excluded` (the
  list of a curated view a dead node sat in), `from`, `subjects` (a surviving
  diagnostic subject), `requirements` (a retracted requirement left the entity's
  set), `attributes` (an instance reached through its type's attributes);
- a change on the node itself: `fields` (an entity's or requirement's own fields
  changed, a transition or an attribute among them, or the node was created or
  deleted by a commit), `merge` (another entity was merged into it), `provenance` (a
  derived or decreed fact landed);
- a computation: `alignment`, `ledger`, `limits`, `lookalike`, `query`,
  `flow-placement`, `flip-detection` (the check that parks a flipping pair or child),
  `sweep` (a node the sweep deleted), `decision` (a decision lapsed because what it
  judged changed), or the rule name of the check that found it;
- the path that filed the prompt behind a `prompt-unanswered` finding:
  `report_diagnostic`, `update_diagnostic`, `ratification` (the store filing a
  ratification proposal on a derived or decreed fact), or the rule name of a check that
  files a prompt.

The ledger goals share one change kind, `ledger-stale`, which is derived and never
stored: the ledger comparison runs at every derivation and yields a `ledger-stale`
change wherever the ledger and the graph disagree, `detail.goal` naming
the kind and `detail.reason` the
[derived status](../consumers/gen.md#status-is-derived-never-stored) behind it:

- `bind` on a requirement with no current row (`unbound`, `requirement-changed`,
  `artifact-gone`).
- `generate` on an entity whose facts differ from the ledger (`facts-changed`) or one
  of whose requirements has an `unimplemented` row: the requirement is bound and
  nothing implements it, so the bound test is the acceptance gate
  ([the cascade](../consumers/gen.md#the-cascade)).
- `verify` on a requirement whose row says action (`never-run`, `test-changed`,
  `code-changed`, `runner-failed`).

A `failing` row (a `fail` verdict on implementing files) is a finding for the author,
a diagnostic and never a goal. `reconcile-section` also derives from coverage: every
section with a body of its own whose coverage is `unprocessed`.

### Pairs

A `rejudge-pair` goal targets one pair of requirements. Neighbor selection is the
reconciler's, never the model's. For every requirement, the neighbor set is computed
deterministically, and a neighbor is a candidate when no `pairs/` decision holds the
statement hashes of both sides now (a new requirement has none; a changed statement or
a source `quote` changed in substance, normalized text and not punctuation, no longer
matches its hash):

- candidates are requirements sharing an entity with it,
- each is scored by overlapping content tokens (statement tokens minus stop words and
  the shared entities' own name tokens, reduced to crude stems, so "reverses" meets
  "reverse" and "sorting" meets "sort") plus the count of shared entities. The stop
  words are the function words (articles, prepositions, pronouns, auxiliaries,
  quantifiers such as `their`, `has`, `once`, `every`, `within`), and a token under
  three letters drops unless it is a number, since a five-document handbook once
  derived 113 pair goals on shared tokens like `s`, `at`, and `their`. One tokenizer
  and one stemmer serve every token comparison. The tokens are the words of the
  statement's [normalized form](./graph.md#mutations). The stem drops one suffix
  (`ing`, `ed`, `s`) from a word that keeps three letters, then a trailing `e` from a
  stem longer than three letters, so "files" and "filed" both land on `fil`. The goal
  hints' token lists (the shared tokens of a `rejudge-pair` change, the cohesion
  groups of `abstract-entity`) and the lookalike name tokens of
  [`dedupe-candidates`](./goals/dedupe-candidates.md#the-lookalike-score) stop at the suffix stage,
  because their stems ride in goal changes and remembered lookalike scores,
- neighbors sharing at least two content tokens qualify, and so do neighbors sharing
  at least two entities and one content token: a restatement built from the same
  entities can share every noun and few other words, because the shared names leave
  the token pool, but two entities alone are what every statement of one page shares
  (Payment and Order sit together in each payment rule), and a same-entity
  restatement with no word in common is review-entity's catch, which reads the
  entity's statements whole and deletes the same-document duplicate.
- two transitions of one subject with different `from` or `to` never pair: they are
  siblings of one state machine, distinct by construction, and the machine's checks
  (`nondeterministic-transition`) judge their conflicts; six transition bullets of a
  hiring page once opened twenty-eight pairs, every one consistent.
- a pair whose requirements both carry a `transition` with the same subject and the
  same `from` and `to` qualifies on its own and scores two more: a restated transition
  is the likeliest duplicate (`expenses-1` against `expenses-15`, one sentence in the
  opening paragraph and one in the steps).
- an intro-versus-steps restatement (both sourced in one document, one in its opening
  section, the other in a later section) qualifies with one shared entity and one
  shared content token, typically the verb stem, and scores one more: an opening
  paragraph summarizes the steps below it (`expenses-5` against `expenses-6` share
  `expense claim` and `file` and nothing else).
- the graph's hub never counts as a shared entity: a pair whose only shared entity is
  the hub counts no entity toward its score and qualifies on three shared content
  tokens, never two, and a pair sharing the hub plus one other entity counts that one
  entity, so it needs the two shared tokens like any single-entity pair. The hub is
  the entity co-referenced with the most other entities across requirements; a tie
  names none. Sharing the hub says little: one hub-sharing flood derived forty-nine
  pair goals with one real pair among them, and on a ten-document shop the hub plus
  Product qualified every stock, catalog, and order statement against each other with
  no word in common.
- an entity's boilerplate never counts as a shared token: a content token carried by
  at least four of the statements naming the entity, and by at least half of them, is
  the wording every statement of that entity shares, not a sign that two of them say
  the same thing. The pair's shared entities each contribute their boilerplate, and
  the pair hint drops those words too. Six operation bullets of one user management
  page ("The user management system supports a `createUser` operation that ...",
  `deleteUser`, `passwordReset`, ...) once opened twenty-three pair goals on
  `supports` and `operation` alone; with the boilerplate out, the two bullets about a
  password still pair on `password`, and the rest derive nothing.
- Best six by score.

The scorer's inputs over the whole graph (the hub, each entity's statement count and
token counts, the requirements naming each entity, each requirement's content tokens)
are computed in one walk of the requirements and shared by every requirement the pass
scores, once per goal derivation. A requirement's candidates are then the requirements
naming one of its entities, so a pass over `n` requirements costs `n` candidate sets,
not `n` walks of every requirement and its tokens.

Open `contradiction` and `duplicate-requirement` diagnostics are sticky pairs: a
requirement also pairs with every partner such a diagnostic ties it to, so editing one
side of a known pair always re-judges the other. A requirement's candidates are its
neighbors and sticky partners without a current `pairs/` decision. Resolving a pair
writes the decision with both statement hashes (`verdict: consistent`, or `diagnosed`
when the session filed a diagnostic on the pair), so the pair derives from neither side
again until one statement changes. An `edit` option's answer on a pair's diagnostic is
that pair's judgment and writes the decision the same way
([answers](./model/diagnostic.md#answers)). A goal that leaves the board without a
resolution (a pair waiting under the cap below, a side deleted by another goal) writes
no decision. A requirement revised in substance again no longer matches any of its
decisions, so every candidate derives again.

The degree is capped. Each changed requirement picks six, but a broad statement is
picked by many (on a sixteen-document corpus one requirement sat in forty pairs and a
hundred and fifty sat in more than six, 1336 pairs over 418 requirements), and a pair
session settles about eight. A requirement takes part in at most
`pairs-per-requirement` open pair goals at once, 8 in the
[registry](./graph.md#budgets-and-thresholds): the count a pair session settles, so one
requirement's open pairs fit one session in its locality, with room beside its own six
picks for the pairs that picked it. The derivation places the pairs in one
deterministic order and a pair opens when both sides have a slot:

- Pinned pairs go first, in target order, and never wait: a pair an open
  `contradiction` or `duplicate-requirement` diagnostic ties (sticky), a pair a
  `node-deleted` finding derives for a surviving diagnostic subject, a pair the build
  parked. A pair blocked on a `goal-failed` diagnostic holds no slot: it is not open.
- The rest follow by score, highest first, ties by target. The score is the neighbor
  score above, the same from either side. A pair whose side already holds
  `pairs-per-requirement` open pairs waits; it is no goal until a slot frees, and its
  finding stands.
- A re-derivation after a commit opens the best waiting pairs into the slots the
  resolved ones freed. A commit that brings higher-scoring candidates to a saturated
  side can send an open pair back to waiting; nothing is lost, its finding stands, and
  the loop treats the vanished goal as unresolved.

The hints on an open pair carry the score and, per side, how many of its pairs wait.
The cap bounds the open board and orders the judgments by lexical closeness; it never
drops a candidate. Deletion propagates the same way: a
`node-deleted` finding on a surviving subject of an open judged diagnostic derives a
`rejudge-pair` goal for it, and that open diagnostic alone is reason enough. The session
sees the deleted subject marked and either resolves the diagnostic or refiles it against
the surviving statements. A diagnostic left with no existing subjects at all is resolved
by the store at commit, journaled as `settle-diagnostics`; no session is needed to bury
it. The sweep is level-triggered: the [checks](./compilation.md#checks) settle every
stranded diagnostic the same way, so a store deleted into a stranded state by hand edits
heals at the next build.

A requirement with no undecided neighbor, no sticky partner, and no open diagnostic
naming it derives no pair goal. A pair is one
goal, `g:rejudge-pair:req:a~req:b`, carried by the smaller id: judging A against B is
judging B against A, so two changed requirements that neighbor each other derive one
goal, not two. The reach is lexical by design: a contradiction expressible only through
concrete example values shares no tokens with its opposite and derives no pair.
`review-entity` is the net for those; it sees the entity's whole statement set. A pair
waiting under the cap is not one of those: it opens later, and the review waits for it
like any open pair.

### Fan-out

An [`abstract-entity`](./goals/abstract-entity.md) goal derives on a node whose direct
children exceed the `children-per-entity` limit, or on the scope root when its
parentless entities do ([levels](./concepts/levels.md#levels),
[the scope root](./concepts/levels.md#the-scope-root)). This is the fan-out variant of
the goal, beside the caps variant on requirement and state counts. The harness counts
and computes coupling; the model names and judges.

- The finding is `threshold-crossed` with `detail.limit: children-per-entity`, computed
  by the limit counts ([the registry](./graph.md#the-registry): soft 9, hard 15). Its
  subject is the node, or `scope:<scope>` for the scope root; `via` is `limits`. It is
  level-triggered like every threshold finding: crossing soft derives the goal
  optional, hard makes it mandatory at every derivation, and a count back under soft
  ends it without a session ([escalation](#escalation)).
- The goal is `g:abstract-entity:<ent>` or `g:abstract-entity:scope:<scope>`. Its
  `change` is `{fan_out: n, limit: {soft, hard}, candidates: [[id, ...], ...]}`: the
  count, the thresholds in force, and the coupling partitions the hint computer
  proposes. A finding for another limit on the same node folds into the same goal, as
  findings do ([change records](#change-records)).
- The cone is the node's subtree, the downward walk from the node ([cones](#cones)).
  The root form's cone is the whole scope. The goal is ready under the GC rule, when no
  compile goal is open or parked in the cone ([GC gating](#gc-gating)), so a level is
  regrouped once, over settled children.
- Hints: the fan-out count, the candidate partitions with their cohesion scores, the
  members' stereotypes, the section of the node's own document that lists each
  member and the document each member is mentioned in most (documents and
  headings are strong naming hints, see [naming](./concepts/levels.md#naming)), and
  any existing grouping under the node ([groupings](./concepts/levels.md#groupings)).

The hint computer (the `abstract-entity` kind's, in the [registry](#the-registry))
works over the target's direct children:

- `weight(a, b)` is the number of requirements referencing both `a` and `b` plus the
  number of derived relationships between them. Descendants count: a requirement or a
  relationship on a descendant lifts to the child it sits under, so a leaf's edges reach
  the level.
- Greedy agglomeration: start with every child as a singleton cluster; repeatedly merge
  the pair of clusters with the highest total weight between them (the sum of `weight`
  over their member pairs); stop when the cluster count is at or under the soft
  threshold and every cluster has at least two members or is a singleton no other
  cluster touches (zero weight to every other cluster). A merge never produces a
  cluster larger than half the soft threshold (four at soft 9): a candidate is a
  cohesive subset the model can name, and two candidates the documents treat as one
  area merge in the session, so a level whose weights would fuse everything yields
  several candidates, never one tautological whole; a singleton no merge can take
  stays out of the output.
- Ties break by id: among merges of equal weight, the pair whose ids sort first.
- Output: each cluster as an ordered id list with its internal weight, largest first,
  capped at the soft threshold of clusters and at 12 ids per cluster (the rest
  summarized as a count).
- Deterministic: a re-derivation over the same graph yields the same candidates, so the
  goal's `change` is stable and matches its predecessor across builds and processes.

The candidates are suggestions; the gate is the truth. The model may accept a
candidate, adjust it with reasons, or decline it with a reason. The harness never names
a grouping, and the model never counts. A grouping lands through `group_entities` and
undoes through `dissolve_entity` ([write tools](./tools.md#write-tools)); a level that
is genuinely flat fails the goal with that reason rather than gaining invented tiers.

### The catalog

Compile goals bring the graph in line with the documents. GC goals restructure and tidy.
`M` mandatory, `O` optional, `O→M` optional until the hard threshold, `B` blocked on a
human. A kind derives only when its input exists in the graph; nothing enumerates
features. Each kind's page states its gate, its hints, its contract paragraph, and its
tools.

| kind | class | tier | m | unit | derives from |
|---|---|---|---|---|---|
| [`place-anchors`](./goals/place-anchors.md) | compile | 0 | M | document | `alignment-pending` |
| [`reconcile-section`](./goals/reconcile-section.md) | compile | 1 | M | section | `section-dirty`, `anchor-stale`, `unprocessed` coverage |
| [`rejudge-pair`](./goals/rejudge-pair.md) | compile | 2 | M | pair | `requirement-revised` (no current `pairs/` decision), sticky pairs, `node-deleted` |
| [`review-entity`](./goals/review-entity.md) | compile | 2 | M | entity | `entity-changed` |
| [`retrace`](./goals/retrace.md) | compile | 2 | M | node | `view-member-gone`, `node-deleted` |
| [`conform-instance`](./goals/conform-instance.md) | compile | 2 | M | instance | `instance-changed` |
| [`derive-criteria`](./goals/derive-criteria.md) | compile | 3 | M | entity | a requirement with no criterion, or whose statement no longer matches the statement hash its derived criteria record, in a set with a nonempty `source` |
| [`bind`](./goals/bind.md) | compile | 3 | M | requirement | `ledger-stale` (`goal: bind`: no current row) |
| [`generate`](./goals/generate.md) | compile | 3 | M | entity | `ledger-stale` (`goal: generate`: facts differ from the ledger, `unimplemented` rows) |
| [`verify`](./goals/verify.md) | compile | 3 | M | ledger row | `ledger-stale` (`goal: verify`: the derived status says action) |
| [`ratify`](./goals/ratify.md) | compile | 3 | B | fact | `provenance-pending` |
| [`answer`](./goals/answer.md) | compile | 3 | B | diagnostic | `prompt-unanswered` |
| [`declare-edges`](./goals/declare-edges.md) | gc | | O | requirement | `edges-missing` |
| [`dedupe-candidates`](./goals/dedupe-candidates.md) | gc | | O | entity pair | `lookalike` |
| [`curate-view`](./goals/curate-view.md) | gc | | O | view | `query-match`, `flow-unplaced` |
| [`split-view`](./goals/split-view.md) | gc | | O→M | view | `threshold-crossed` (view limits) |
| [`abstract-entity`](./goals/abstract-entity.md) | gc | | O→M | entity | `threshold-crossed` (entity limits, state limit; the [fan-out](#fan-out) form on a node or on `scope:<scope>`) |

`retrace` is one kind: delete a requirement, and the flow view that stepped through it,
the instance that conformed to it, and the derived fact that cited it each surface as a
`retrace` goal with the same cause, each hinting what to load to see the damage. Derived
data needs no retrace: [relationships](./model/relationship.md#recompute), state
machines, and [default views](./model/view.md#default-views) recompute at commit.
Decompilation stays outside the board: draft work is released per scope
([decompilation](../consumers/decompile.md#triggering)).

## Scheduling

One build runs one loop, sequentially: one session at a time, each with one batch.

- Derive the board. Take the highest ready [tier](#readiness) that has an open goal;
  goals an interrupted run of the same build parked resume first.
- Group the ready goals by [locality](#batching) and fill one batch under the context
  budget.
- Run one session for the batch ([execution](./sessions.md#execution)). The session
  prompt lists exactly the batch's goals and one summary line for the rest.
- Commit the changeset: derived data recomputes, the decisions the resolved goals asked
  for land, the journal entry records `resolved_goals` with their justifications and
  `opened_goals` with their causes ([commit](./sessions.md#commit)).
- Re-derive the board. GC goals whose cone the commit quieted are ready; they run next,
  in a burst, before the loop returns to the compile tiers, unless the build budget is
  tight, in which case compile outranks GC.
- Repeat until no goal is ready or a budget is spent. Then the deterministic tail runs:
  [checks](./compilation.md#checks), rendering, docsgen, the verdict. The tail needs no
  model, so whichever consumer emptied the board runs it.

A consumer that claims the next batch over MCP (`begin_goals`) and finishes it (`done`)
walks the same path the internal loop walks
([compilation over MCP](../frontends/mcp.md#compilation-over-mcp)). The trace shows each
step as `batchStart`, `sessionStart`, `goal`, and `gcBurst` events
([trace events](./sessions.md#trace-events)).

## Readiness

Ordering inside compile is small and internal: alignment before ingest, ingest before
judgment, judgment before ledger work. Tiers carry it. GC goals sit outside the tiers
and wait for their cone instead.

### Tiers

| tier | kinds | ready when |
|---|---|---|
| 0 | `place-anchors` | always |
| 1 | `reconcile-section` | no tier 0 goal is open or parked; the document's link level is reached; the document has no `alignment-pending` finding |
| 2 | `rejudge-pair`, `review-entity`, `retrace`, `conform-instance` | no tier 0 or 1 goal is open or parked |
| 3 | `derive-criteria`, `bind`, `generate`, `verify`, `ratify`, `answer` | no tier 0, 1, or 2 goal is open or parked |

- Open and parked goals hold the later tiers. Blocked goals do not, a goal blocked on a
  `goal-failed` diagnostic included: a section the model could not reconcile, or a
  document awaiting a release, must not wedge every judgment behind it. They count in
  the [verdict](./compilation.md#convergence) instead.
- Within tier 3, `bind` on a requirement waits until the requirement carries at least
  one [criterion](./model/criterion.md) and its entity owes no `derive-criteria` (the
  evidence must exist before a test encodes it; the goal is `blocked {on: criteria}`),
  `generate` waits for its entity's `derive-criteria`, but not for `bind`.
  A generation batch starts before binding-only work in the same locality, so
  source and tests can develop together. Explicit binding remains available.
  `verify` waits until the
  row's entity is generated. `ratify` and `answer` are never ready for
  a session: they are `blocked {on: human}` and resolve through the human paths their
  pages describe.
- A release gate ([modes and releases](./control-plane.md#modes-and-releases)) renders
  as `blocked {on: release}`.

### Link levels

Link levels order tier 1: breadth-first over the document link graph from the
[roots](./project-settings.md#roots), resolved from `jazyk.toml` against the documents
on disk at every derivation and never stamped into the store.

- The root documents are level 0. A document's level is one more than the lowest level
  of any document linking to it.
- The root documents run first, so the core vocabulary exists before anything else asks
  for it; then their children; then the next level. A document's level is reached when
  every document in an earlier level has no open or parked tier 1 goal.
- Documents unreachable by links run last, in path order.

### Selection

A [selected build](./compilation.md#selected-builds) computes readiness over its
selected goals alone. Tier counts, link levels, alignment, and GC cones count only
goals in the selection, so a dirty section outside it holds no selected goal. A goal
outside the selection is never ready: its readiness reads `outside the selection` and
it forms no batch. Its state does not change, so it is neither parked nor failed by the
build. A session cap that parks the remaining work parks only selected goals.

### GC gating

A GC goal is ready when no compile goal is open or parked in its target's
[cone](#cones). Blocked compile goals do not count, a goal blocked on a `goal-failed`
diagnostic included: a ratification proposal
that a GC goal itself created must not hold its own cone, and a section the model could
not reconcile is as settled as it will get. Restructuring therefore always sees settled
content: an entity is abstracted knowing every requirement this build gives it, never a
stream of partial states. Nothing waits for a global phase: as each cone settles, its GC
goals become ready and run right there, while the graph is loaded. Mandatory GC goals
run before optional ones within a burst.

One GC goal yields to another: a [`split-view`](./goals/split-view.md) goal on a
level view or on a lifted flow view of a level is blocked while an
[`abstract-entity`](./goals/abstract-entity.md) goal on that level is open or parked,
its readiness reason naming the fan-out goal (`fan-out first:
g:abstract-entity:<target>`). Grouping the level is the structural answer to a picture
over its limit (the flow lifts to the groupings and draws with fewer participants);
splitting the picture is the last resort, so it is never batched before the fan-out.
The rule is one-directional: a fan-out goal never waits for a view goal.

### Cones

The cone of a target is the set of nodes reachable from it through stored references.
The references are the stored edges of the [model](./model.md#edge-summary): `parent`
(entity to its containing entity) and `parents` (section to section), `mentions`
(entity to section), `entities`, `edges`, and `transition` (requirement to entity),
`members` and `collapse` (view to node; a `query` counts as listing what it matches),
and `from` (derived provenance to upstream node). Every reference has a referrer and a
referent, and the walk keeps one direction at a time:

- Upward, transitively: every referent of the target, then every referent of those.
  For an entity that is its `parent` chain, its `mentions` sections and their `parents`
  chains, the sections its quoted attributes point at, and its `from` nodes. For a
  requirement it is its `entities`, the endpoints of its `edges`, its `transition`
  subject, their parents, its `source` section and that section's `parents` chain, and
  its `from` nodes. For a view it is its `members`, `collapse`, and `excluded` nodes
  and its `from` nodes. For a section it is its `parents` chain and its document.
- Downward, transitively: every referrer of the target, then every referrer of those.
  For an entity that is its descendants over `parent`, the requirements naming it or any
  descendant in `entities`, `edges`, or `transition`, the views listing any of them in
  `members` or `collapse`, and every node whose `from` names any of them.
- Never sideways: a node reached upward contributes no referrers, and a node reached
  downward contributes no referents. Without this rule the cone of one entity would be
  its whole connected component.

A compile goal is in the cone when its target is a node in the cone, when its target is
a pair with a member in the cone, when its target section anchors a node in the cone (a
requirement `source` or an entity mention points into that section), or when its target
document contains such a section.

The cone of a leaf entity is its requirements, its views, the derived facts built on it,
and the sections that anchor them. The cone of a top-level «system» entity is most of
the graph, which is correct: abstracting the top of the tree must wait for everything
under it. The target `scope:<scope>` names the scope root
([the scope root](./concepts/levels.md#the-scope-root)); its cone is the downward walk
from every parentless entity of the scope, which is the whole scope, so the top level
is regrouped only when everything in the scope has settled ([fan-out](#fan-out)). The
same computation answers `jazyk explain <target>`: the goals a change to the target
would open ([`jazyk explain`](../frontends/cli.md#jazyk-explain)).

## Batching

The model never sees the whole board. Each session gets one batch. The scheduler forms
a batch from goals of one class and one tier that resolve to one
[executor](./control-plane.md#executors), grouped by locality:

- document locality: `place-anchors` is one goal for the document; `reconcile-section`
  goals of one document batch in document order, adjacent sections together.
- node locality: `rejudge-pair`, `review-entity`, `retrace`, and `conform-instance`
  goals whose targets share an entity, a requirement, or a view batch together, so a
  judgment sees the merges and diagnostics of its neighbors. Entities that share
  requirements or relationships form one group.
- view locality: view goals batch with the goals on the view's members.
- level locality: an `abstract-entity` goal on a node or on `scope:<scope>` has node
  locality through the level's members, the direct children or the scope's parentless
  entities ([fan-out](#fan-out)).
- ledger locality: `bind`, `generate`, and `verify` goals join through their entity's
  component group root, so the ready goals of one component subtree form one batch
  ([grouping by component](../consumers/gen.md#grouping-by-component)). A flat graph
  has no groups, and the ledger goals batch per entity.

A batch is labeled by the key most of its goals share (a document, an entity, a
requirement), never by the representative of the locality group it was cut from: the
group of every pair over two characters once labeled forty-one batches with a scene
none of them touched. The batch fills until the budget says stop. A locality group is
where a batch starts, not where it must end: the document and ledger localities are walls (two documents
never share a batch, nor two component subtrees), but the node, view, level, and
criteria localities of one class, tier, and executor pack on into the same batch,
neighborhood after neighborhood in locality order, until the budget is spent. Criteria
goals pack at most four to a batch (`CRITERIA_GOALS_PER_BATCH`, a
[registry constant](./graph.md#limits)), since each lists several requirements with
their source sections and grows the changeset's mutation budget. A pair batch charges
each requirement once: a statement that sits in several of the batch's pairs loads
once, so each further pair adds only its other side. Charging both sides per pair
counted one changed requirement's six pairs as twelve statements and cut pair sessions
at five to seven pairs where twelve fit. A corpus of hundreds
of small pair judgments therefore runs as a few full sessions, never one session per
shared entity. The skills of the batch's goal kinds render
into the same session context budget (24000 chars, a
[registry constant](./graph.md#limits)), so the registry's `pack` computes the batch's
initially loaded set from the goals' hints under what the skill payloads leave; a goal
whose loads do not fit waits for the next batch. The initial set never passes the
high-water mark: a goal's own target always loads full, and a supporting item that
would land the set past the mark enters as a stub instead
([policy](./context.md#policy)). The round budget bounds the count too: at least 8 of the session's
24 rounds per section in the batch, so a reconcile batch holds at most three sections.
A batch is one to a handful of goals; the count is a consequence of budget and locality,
never a fixed number. Goals the build parked are batched first.

Formed is not the same as carried. A batch is formed of one class and one tier; the
session that runs it may resolve more. A previewed goal of either class joins the
running session under the join conditions of [bubbling](#bubbling) (it lies in the
batch's locality, the session's toolset covers its kind, its kind resolves to the same
executor, the remaining budget fits it), so a compile session can end by resolving the
GC goal its own commit made ready.

A batch has an id, `b<generation>-<n>`: the generation the board derives from and the
batch's index within it. A re-derivation of the board renames its batches, so an id
never outlives the board it came from. Numbering skips the ids of batches a live lease
holds ([workers and leases](./control-plane.md#workers-and-leases)), so a batch formed
while another is being worked never takes the working batch's id. It names the session, its lease
([workers and leases](./control-plane.md#workers-and-leases)), its trace file, and the
`{target}` of the worker protocol line. The toolset of the session is the union of the
batch's kinds' toolsets ([toolsets](./tools.md#toolsets)); the skills are the union of
their skills ([skills](./sessions.md#skills)). `jazyk preview` renders the next batch's
prompt exactly as the model would receive it
([preview](./sessions.md#preview)).

## Bubbling

Downstream work is never silent and never model-invented. Staged mutations are validated
when staged ([staged mutations](./sessions.md#staged-mutations)), and the same
computation previews what the mutation will open and what it will make ready. The tool
reply says so. E.g.:

```
ok: delete_requirement req:orders-6 staged
this delete will open: retrace view:usecase/holds (member gone), retrace ent:order (statement gone)
```

A goal line previewed once in a session is not repeated on later replies: the reply
counts it under `opensRepeated` instead, since an extraction batch of nineteen
statements about one entity once read the same review line on every upsert.

- At commit the previews become real goals with causes, recorded as `opened_goals` in
  the journal entry, and the trace prints a `goal` event per goal
  ([trace events](./sessions.md#trace-events)). `jazyk watch` prints one line per goal
  opened or resolved.
- A previewed goal joins the running session when it lies in the batch's locality, the
  session's toolset covers its kind, its kind resolves to the session's executor, and
  the remaining budget fits it: the reply lists it as available, the session resolves it
  with `mark_goal_done` like any goal in the batch, and the commit records it as opened
  and resolved in one generation. This is how a GC burst often lands in the session that
  just settled the cone: the staged resolution of the cone's last compile goal previews
  the GC goal as ready.
- Everything else waits for a later batch. A previewed goal that never materializes
  (the mutation was dropped before `done`) opens nothing.

## Escalation

Every [limit](./graph.md#limits) carries a soft and a hard threshold. Crossing the soft
threshold is a `threshold-crossed` finding and derives an optional goal (`split-view`
for view limits, `abstract-entity` for entity limits and the state limit, the
`children-per-entity` row on a node or on `scope:<scope>` included, see
[fan-out](#fan-out)). Crossing the
hard threshold escalates the same goal to mandatory: `mandatory` is recomputed at every
derivation from the current count, so a count that drops back under the hard threshold
de-escalates, and one that drops under the soft threshold ends the finding without a
session. The build cannot report `converged` while a mandatory limit goal is open or
failed; the diagram renders meanwhile with collapse applied and marked as such
([over-limit views](./diagrams.md#over-limit-views)).

Dismissing a limit goal is a graph write, not goal state. The node's own limit is
raised (`limits: {<limit>: n}`) with decree provenance recorded in the journal
(`kind: decree`); the goal derives only when the count crosses `n`, and escalates when
it crosses `n` plus the registry's distance between soft and hard. Nothing tunes limits
in `jazyk.toml`; the registry is built into the binary, and bumps are per node.

The other escalations are states, not thresholds: a mandatory goal a session fails files
a `goal-failed` diagnostic and blocks convergence ([parked and failed](#parked-and-failed));
a goal that waits on a human is `blocked` and counted; a build that exhausts its cap
parks what is open, and the verdict counts it as open work. A dead endpoint trips a breaker before the cap: five
consecutive failed sessions that spent no tokens park what is open the same way, the
last error named in the reason, because an endpoint answering only errors (a rate
limit, an outage) must not grind the session cap into hundreds of futile attempts. A
failure naming a rate limit (HTTP 429, a rate, usage, or quota limit, an agent's "hit
your limit") trips the breaker at once, since a quota does not reset between sessions,
and so does one saying the prompt does not fit the model's context ("too large", "too
long", "context length"), since the fixed part of a session's prompt is the same for
every batch and a retry cannot help (a browser's built-in model once failed every
batch of a sync that way before the cap). A session that failed without
spending a token never counts as its goals' attempt: the model never saw them, so their
four attempts stay theirs. A novel build under an exhausted quota had parked goals on
attempts no model made, 302 in its last minutes.

## Parked and failed

Parked is a build's progress, failed is the graph's. Neither is a stored work queue.

- Parked goals live in the build that parked them
  ([a build's progress](./graph.md#a-builds-progress)): in memory, in `local/run.yaml`
  while a local build runs so an interrupted `jazyk compile` resumes them first, or in
  the host's database for a hosted build paused and resumed. The build that ends drops
  them. The next build derives the same goals again from the same findings, and they
  count as open in the verdict meanwhile (`incomplete`). Unfinished work is never
  silent, and it needs no record.
- A goal a session marks failed becomes a `goal-failed`
  [diagnostic](./model/diagnostic.md) on its target, with the goal kind and the reason in
  its message. It is graph state: committed, seen by every checkout, answered by a
  person. While it is open, no goal of that kind derives on that target; the board
  shows the goal `blocked {on: goal-failed}`.

The park `reason` says which road ran out, in the words the trace uses: the retry
(`left open after 4 attempts in session b12-3`), the build cap or the `--sessions`
limit, the dead-endpoint breaker with the last error, a reopen cycle
(`reopened-after-resolve: ...`), or a flip (`cross-class flip: ...`,
`reparent flip: ...`). A cancelled build keeps the goals it parked before the stop and
parks nothing for the stop itself. The reason rides wherever a parked goal is shown
while its build lives: `jazyk explain` (a `parked:` line), the whole-build report of
`jazyk ripple`, the board's goal payload (`parkedReason`), and the `parked` list of
`GET /api/status`.

- Parked means "ran out of road": a session that exhausted its rounds without meeting
  the gate gets up to four attempts, each with a fresh session, then its goals park;
  a build that hits its cap (8 × the distinct goals derived so far in the build + 64
  sessions, the count cumulative over every board of the build) parks every open goal.
- Parked also means "the same work came back". A finding the commit leaves standing
  deterministically (a `threshold-crossed` count still over its limit, a `ledger-stale`
  row the ledger still disagrees on) derives again when the measured condition still
  holds, so a goal a session just resolved derives again under the same id with the
  same `change`. The build counts these cycles per goal id and `change` across its
  sessions: a goal resolved and re-opened with the same `change` for the second time
  in one build parks without spending the remaining unfinished-goal attempts, and the
  trace names the cycle (`reopened-after-resolve`, with the count). A goal re-opened
  with a different `change` is new work under an old id, not a cycle: its count starts
  over. The count is the `REOPEN_CYCLES` constant of the registry
  ([budgets](./graph.md#budgets-and-thresholds)), two. Without it only the build cap
  ends the loop (a session marks done, the board re-opens the identical goal, repeat),
  and one goal spends the whole build's budget.
- Failed means the session said so: `mark_goal_failed({goal, reason})` is always
  available, because a goal that cannot be accomplished (documents too deeply
  contradictory, a target that makes no sense any more) must be failable, or the board
  fills with dishonestly resolved goals. The `goal-failed` diagnostic sits on the target
  itself, so the failure surfaces everywhere the target renders. It is a warning on a
  mandatory goal, which blocks convergence while it is open, and info on an optional one.
- A failed goal never re-runs by itself. Its diagnostic carries the hash of the evidence
  it failed on (the goal's `change`); when the target changes so that the derivation
  computes a different `change`, the store resolves the diagnostic at the next commit and
  the goal derives again. A person can also answer or resolve it, from the board or any
  diagnostic surface, which is a retry. `jazyk explain <goal>` shows the reason. For an
  [`abstract-entity`](./goals/abstract-entity.md) goal whose change is a count over a
  limit, growth alone is not a new change: the diagnostic stands until the count has
  grown by a fifth over the count at failure (or a person bumps the limit or retries),
  since a novel lead's goal once re-fired on every chapter's new statements and failed
  three times, at up to fifty calls each, with the same honest reason. The
  [fan-out](#fan-out) variant's count is its child count: its coupling candidates move
  with every requirement on the level and change nothing on their own.
- A ledger goal (`bind`, `generate`, `verify`) the ledger no longer derives is simply
  gone, parked or not: the ledger judges what landed, so a verify goal parked mid-run
  whose row reads verified since has nothing to resume. A `goal-failed` diagnostic on a
  ledger goal resolves the same way when the ledger no longer derives it.

## Flip detection

Oscillation is caught on natural keys, never on ids. No dedicated key history is
stored: the journal is the history, and tombstone redirects mark deletions
([garbage collection](./graph.md#garbage-collection)).

- Recreation within one class is caught on tombstoned slugs. A deleted entity leaves a
  tombstone redirect on its slug, so recreating the natural key mints the new id with a
  collision suffix. An entity id carrying a collision suffix while a tombstone holds
  the base slug files `unstable-extraction` ([checks](./compilation.md#checks)).
- A flip between the classes is a GC commit undoing what a compile commit established on
  a key, or the reverse: `abstract-entity` introduces `ent:order-pricing` with derived
  provenance, `review-entity` merges it back into `ent:order`, `abstract-entity` derives
  again and re-creates it. The check replays the journal's `session` entries into a
  per-key event list: entity creates, deletes, merges, and decree retractions, each
  stamped with its generation, the class of the session's resolved goal, and that
  goal's justification from `resolved_goals`. Two flips of one natural key between the
  classes stop the pair: both goals park for the rest of the build, and one
  `unstable-derivation` diagnostic is filed on the key's surviving node with both
  justifications side by side (the latest from each direction). The diagnostic carries a
  [prompt](./model/diagnostic.md#prompts): keep the split, keep the merge, or a
  freeform ruling. While the diagnostic is open, the derivation renders both goals
  `blocked on human: unstable-derivation ruling` in every build, so no batch takes them.
  Answering it records the ruling as graph state and the next build resumes the chosen
  direction.
- A reparent flip is a child that moves between the same two parents across
  generations of this machine's journal: `ent:cache` moves from `ent:backend` under `ent:storage` in one
  generation (a `group_entities`, a `dissolve_entity`, an `update_entity` or
  `edit_fact` on `parent`, or the sweep's dissolve) and back in a later one. The check
  replays every journaled `parent` change, each mutation carrying the prior parent
  ([journal](./graph.md#journal)), into a per-child event list keyed on the child's
  natural key with the two parents matched by natural key too, so a grouping dissolved
  and re-minted under a new id counts as the same parent. The second move stops like a
  cross-class flip: the goal behind it parks for the rest of the build, and one
  diagnostic is filed on the child with both justifications side by side and a
  [prompt](./model/diagnostic.md#prompts): keep it under the first parent, keep it
  under the second, or a freeform ruling. The goal is blocked on that answer the same
  way in every build, never batched (a novel's `conform-instance` goal behind a reparent flip was
  once batched, resolved, and re-derived six times in one build with no mutation
  between; the reopen cycle count in [parked and failed](#parked-and-failed) parks
  such a goal at its second cycle). Answering it records the ruling and the next build
  resumes the chosen direction.

Flip detection and the budgets bound the alternation between the classes and the
alternation of a child between parents. With them,
idempotence makes convergence a fixed point rather than a loop: a session that re-derives
an unchanged conclusion stages a no-op upsert, no mutation lands, no finding changes,
and that branch of the cascade dies.
