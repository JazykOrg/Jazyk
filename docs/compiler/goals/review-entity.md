# The review-entity goal

`review-entity` judges one entity whose fact set changed. The session loads the entity
in full and reads its requirements, gathered across all documents, as a whole. It
refreshes the `definition` when it drifted (and `stereotype` and `parent` the same way,
only when the statements as a whole support the change), merges lookalike entities that
are one concept, adds missing entity references, declares the edges structural
statements imply, deletes same-document duplicate requirements, files
[diagnostics](../model/diagnostic.md) (`contradiction`, `duplicate-entity`,
`duplicate-requirement`, `ambiguity`, `missing-link`, `lint`, `decision`), and settles
open diagnostics that lapsed. It sees the entity's whole statement set, which makes it
the net for conflicts the [pairwise judgment](./rejudge-pair.md) cannot pair lexically.

- Class: compile. Mandatory. Readiness tier 2.
- Unit: one entity. Goal id `g:review-entity:ent:<slug>`.
- Skill: [`judgment`](../skills/judgment.md).

## Created when

The goal derives from an `entity-changed` [change record](../graph.md#change-records)
on the entity. A commit or the sweep writes one when the entity's fact set changed,
`via` naming how:

- `entities`, `edges`, `transition`: a requirement referencing the entity was created,
  revised, or deleted, including the requirements the sweep prunes when their quotes
  die.
- `fields`: the entity's own `definition`, `aliases`, `stereotype`, `scope`, or
  `attributes` changed, or the entity was created. A move alone (`parent` and nothing
  else) is the parents' business: it writes `parent` on the parent it leaves and the
  one it joins and never `fields` on the child, so a grouping of twelve children opens
  one review, the grouping's own, not thirteen.
- `mentions`: a mention was added or pruned.
- `merge`: another entity was merged into it; its requirements, aliases, attributes,
  children, and edges arrived.
- `parent`: a child was re-parented under it or away from it.

`detail` lists the requirements and fields that changed. A later commit that changes
the same entity again supersedes the record and merges the detail, so one review sees
everything since the last one. The commit that resolves a `review-entity` goal writes
no `entity-changed` record for its target from that session's own mutations: the review
saw the result it staged, and a definition refresh must not re-open the review that
made it. Mutations on other entities (a neighbor added to a statement's `entities`)
open their own goals as usual.

E.g.:

```yaml
- id: c413-4
  generation: 413
  mutation: 4
  kind: entity-changed
  subject: ent:order
  via: entities
  detail: {requirements: [req:orders-6, req:orders-9], fields: []}
```

Lookalike candidates are not stored: they are computed at derivation from the name
index and rendered as hints. A cross-document lookalike whose score is high derives a
`dedupe-candidates` GC goal as well ([dedupe-candidates](./dedupe-candidates.md)); the
review runs first, because its cone is not quiet until it does, and a merge it stages
clears the lookalike record.

## Readiness

- Tier 2: ready when no tier 0 or 1 goal is open or parked
  ([readiness](../reconciler.md#readiness)). The entity's statements are final for this
  build before they are read as a whole.
- Locality is the node neighborhood ([batching](../reconciler.md#batching)): the
  entity's review batches with the `rejudge-pair` goals on its requirements and with
  the reviews of entities sharing requirements or relationships; pairs come first in
  the batch order. The review of a lookalike candidate batches with this one when it is
  open, so one session judges the merge from both sides.
- A merge the session stages rewrites every reference at commit
  ([mutations](../graph.md#mutations)); the absorbed id becomes a redirect and derives
  nothing further.

## Gate

`mark_goal_done({goal, justification, evidence})` carries `evidence` with the state of
the definition and one verdict per lookalike the goal listed:

```yaml
evidence:
  definition: current            # or refreshed
  lookalikes:
    - {id: ent:backend-system, verdict: merged}
    - {id: ent:reorder-point, verdict: separate, reason: "a threshold on Order; statements are about it directly"}
```

The harness validates the claim over the store plus what the session has staged:

- `definition` is `current` or `refreshed`. `refreshed` requires an `update_entity`
  with `definition` on the target staged in this session; `current` requires none. The
  harness cannot judge whether a definition fits; it checks that the claim is made and
  agrees with the staged mutations.
- Every lookalike the goal listed has a verdict. `merged` requires a `merge_entities`
  staged in this session whose `keep` or `absorb` is the candidate; `separate` requires
  a `reason`, or an open `duplicate-entity` diagnostic naming both when the merge is
  not certain.
- Every open diagnostic naming a `(deleted)` subject among the entity's statements is
  resolved in this session, refiled or not.
- The justification is present.

`done` runs the same gate over every goal in the batch and the per-mutation gates on
what was staged ([validation gates](../graph.md#validation-gates)): `parent` stays
acyclic and consistent with a stated `composition`
([containment](../model/entity.md#containment)); a merge across named scopes is
refused ([the natural key under containment](../concepts/identity.md#the-natural-key-under-containment));
an `update_requirement` that passes `section` or `quote` re-anchors provenance and its
quote must locate; a `delete_requirement` carries a reason; a `lint` diagnostic names a
rule the project declares ([linting](../project-settings.md#linting)).

Staging nothing is a correct outcome when everything is coherent: `evidence` with
`definition: current` and every lookalike `separate`. The error asymmetry is stated in
the skill and enforced by nothing but judgment: a wrong merge or delete destroys
information, a missed duplicate only leaves a finding for the next build; when in doubt,
keep both and file a diagnostic. `mark_goal_failed({goal, reason})` is for an entity
whose statement set is too contradictory to say what the entity is. A failed goal keeps
its record and surfaces on the entity; it blocks convergence.

## Hints

Computed by the harness and rendered under the goal block:

- The changed requirements (ids and statements) and the fields that changed, from the
  record.
- Lookalike candidates from the name index, partitioned: name-similar (`ent:<slug>
  (<name>): <definition>`, a variant or a synonym, a merge when they are one concept),
  and related but separate (a name that extends this one or that this one extends: a
  field, a part, a child concept, never a merge suggestion by default). A shared word
  proves nothing.
- Statements whose prose names this entity or an alias (word-bounded, code spans
  stripped) without referencing it, minus matches that belong to a referenced compound
  name, capped at six, with the note that they are word matches, not judgments. A
  missing reference is what strands an entity unreachable from the roots
  (`unreachable-entity`, [checks](../compilation.md#checks)).
- Multi-entity requirements on this entity that declare no `edges`, when the
  statements are structural (the `declare-edges` GC goal advises the same; the review
  may settle it while the statements are loaded).
- Composition edges on the entity's requirements whose part has no `parent`, or whose
  part's `parent` disagrees with the stated whole, with the note that `update_entity`
  on the part sets it.
- Open diagnostics naming the entity's requirements, with `(deleted)` markers; those
  naming the entity itself ride in the loaded set. Harness-owned rules
  (`incomplete-build`, `ratification-pending`, `unstable-*`) are left out: the build
  settles them, and a review never resolves them
  ([lifecycle](../model/diagnostic.md#lifecycle-and-triage)).
- `level views: <view id> (this entity), <view id> (its parent)`: the structural
  level view of the entity when it has children and of the level it sits in, so a
  review reads the drawing without guessing view ids
  ([level views](../model/view.md#level-views)).
- The project's lint rules, restated with their severities
  ([linting](../project-settings.md#linting)).
- The requirement count against `requirements-per-entity` when past the soft limit,
  as information only: splitting is [`abstract-entity`](./abstract-entity.md) work,
  ready once this review resolves ([the limits registry](../graph.md#limits)).
- `load ent:<slug>` in full, `skill judgment`, and the tools per finding.

## What the model sees

The session prompt is [assembled](../sessions.md#the-prompt) like every session's: the
agent contract, the active skills, the project block, the goals block, the loaded set.
The goal block carries the contract paragraph from
[`prompts/review-entity.md`](./prompts/review-entity.md): read the requirements as a
whole; refresh the definition, stereotype, and parent only when the statements support
it; set the part's `parent` where a stated composition leaves it unset; judge every
listed lookalike, merging name variants and synonyms and keeping
fields, parts, states, roles, thresholds, instances, and child concepts apart; add a
missing reference with `update_requirement` passing only `id` and `entities`; delete a
same-document duplicate and file a cross-document one; report only what the author can
act on, with severity `error` only when two statements cannot both hold; add missing
edges with `update_requirement` passing only `id` and `edges`; resolve lapsed
diagnostics and refile those naming a dead subject; when in doubt keep both and file;
mark done with no mutation when everything is coherent. Then the change in one line,
the gate in one line, and the hints.

The `judgment` skill is active from the first round: the review asymmetry, the three
pair verdicts, entity review whole, findings, severity, verdict quality
([the review asymmetry](../concepts/judgment.md#the-review-asymmetry),
[calibration](../concepts/judgment.md#calibration)).

The initially loaded set for the batch holds:

- The entity in full ([policy](../context.md#policy)): `definition`, `aliases`,
  `scope`, `stereotype`, `parent` with its chain, `attributes`, mentions each with one
  parent chain, and every requirement on it across all documents with `statement`,
  quote, section, entities, edges, transition, facets. One hop of related entities as
  stubs with edge counts. An over-budget list becomes a handle
  (`h:ent:order:requirements`, `h:ent:order:related`).
- The lookalike candidates as stubs.
- The unreferenced statements, each with its id, statement, and section.
- The open diagnostics naming the entity or its requirements.

E.g.:

```
## Goals
- [g:review-entity:ent:order] mandatory
  [contract paragraph]
  Change: 2 requirements changed in g413 (req:orders-6 revised, req:orders-9 created).
  Gate: definition current; every listed lookalike judged; findings filed or resolved.
  Hints: lookalikes: ent:order-item (related, separate by default); unreferenced:
  req:payment-3 "Orders on hold are not charged"; 1 open diagnostic; lint: warnings 1;
  skill judgment

## Loaded (11.4k/24k chars)
- ent:order       full: 9 requirements, parent ent:order-service   [3 more edges: h:ent:order:related]
- ent:order-item  stub (definition only)   [2 edges loadable: h:ent:order-item]
- req:payment-3   stub   docs/payment.md#/payment/holds
- diag:contradiction-3   open, error, subjects req:orders-6, req:payment-9
skills: judgment (active); extraction, flow-views, structural-views, abstraction, conformance (load_skill)
```

`jazyk preview <goal>` renders the prompt before it is spent
([preview](../sessions.md#preview)).

## Tools

The `review-entity` toolset ([toolsets](../tools.md#toolsets)):

- The [read tools](../tools.md#read-tools): `load`, `expand`, `unload`, `graph_status`,
  `search`, `read_section`, `get_entity`, `get_view`, `diagnostics`.
- The [goal tools](../tools.md#goal-tools): `mark_goal_done`, `mark_goal_failed`,
  `load_skill`, `done`.
- `update_entity({id, definition?, add_aliases?, stereotype?, parent?, attributes?})`:
  the living definition, refreshed, never forked.
- `merge_entities({keep, absorb, reason})`: the absorbed name becomes an alias; its
  requirements, attributes, children, and edges follow; a redirect remains.
- `update_requirement({id, entities?, edges?})`: a missing reference or a missing edge,
  passing only `id` and the one field.
- `delete_requirement({id, reason})`: the worse-sourced same-document duplicate.
- `report_diagnostic({rule, severity, subjects, message, reasoning, prompt?})`, rules
  `contradiction`, `duplicate-entity`, `duplicate-requirement`, `ambiguity`,
  `missing-link`, `lint`, `decision`.
- `resolve_diagnostic({id, reason})`.
- [`report_feedback`](../tools.md#feedback-tool).

No creation tools: review judges what extraction recorded. No `delete_entity`: a
duplicate is merged, and an entity left with nothing is removed by the sweep with a
tombstone ([garbage collection](../graph.md#garbage-collection)). No coverage and no
view tools. A merge's tool reply previews the goals it opens at commit
([bubbling](../reconciler.md#bubbling)): the reviews of the entities whose statements
gained a reference. A merge opens no `retrace`, because it rewrites every reference
mechanically.
