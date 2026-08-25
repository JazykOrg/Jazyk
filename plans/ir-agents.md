# Plan: the stage agents

Status: draft for iteration. Detailed design under [ir-stages](./ir-stages.md).
Companions: [ir-graph](./ir-graph.md) (the shapes these agents write),
[ripple](./ripple.md) (how their effects chain), and
[orchestration](./orchestration.md) (the registry they are registered in).

An agent here is one task kind: a declared unit of work, a trigger, a context pack,
a toolset, gates, and the effects it emits. The executor (which model loop runs it)
is a separate, per-stage choice through
[ACP profiles](./orchestration.md#per-stage-executors). Every agent below is one
`Stage` implementation in the [registry](./orchestration.md#the-stage-registry).

## The roster

| agent | stage | unit of work | triggered by | emits |
|---|---|---|---|---|
| `align-doc` (exists) | 1 | one document's proposals | alignment proposals pending | dirty sections |
| `reconcile-doc` (exists) | 1 | one document's dirty sections | section-tree diff | req/ent changes |
| `review-requirement` (exists) | 1 | one changed-statement pair set | req created/revised | req changes, diagnostics |
| `review-entity` (exists) | 1 | one entity group | entity facts changed | merges, diagnostics |
| `derive-usecases` | 2 | one actor-goal cluster | cluster membership changed | uc changes |
| `review-usecase` | 2 | one changed use case + neighbors | uc changed | uc changes, diagnostics |
| `model-domain` | 3 | one scope cluster | ent/rel facts in scope changed | attributes, roles, cardinality |
| `partition-architecture` | 4 | the project | stage enabled, no accepted partition; or partition invalidated | comp tree, partition ADR |
| `design-component` | 4 | one component + neighbor interfaces | allocation candidates or owned facts changed | satisfies, interfaces, ADRs |
| `review-component` | 4 | one changed component | comp facts changed | diagnostics, repairs |
| `derive-statemachine` | 5 | one stateful entity | its state/event reqs changed, threshold met | sm changes |
| `derive-interaction` | 5 | one multi-component use case | uc, allocation, or ifaces changed | ixn changes, proposed operations |
| `bind-requirement` (exists) | 6 | one requirement | unbound / changed / artifact gone | ledger rows |
| `generate-entity` (exists) | 6 | one entity (or component, see below) | facts differ from ledger | deliverable, manifest |
| `verify-requirement` (exists) | 6 | one ledger row | derived status says action | verdicts |
| `draft-document` (exists) | reverse | one released scope | unclaimed report + release | doc drafts |

The existing eight port onto the registry unchanged in behavior (orchestration plan,
phase 1). The new ones follow, each with trigger, pack, toolset, gates, effects.
Shared by all: `report_feedback`, the repeated-call guard, staged mutations, budgets,
retry-then-park, and the finish contract (`done` runs the gates).

## New agents in detail

### derive-usecases

- Unit: one actor-goal cluster. The reconciler computes clusters deterministically:
  event-driven and state-driven requirements grouped by shared actor entity and
  trigger-token overlap (the pair-review scoring machinery; embeddings upgrade the
  similarity later, see the orchestration plan's Rig note). Clusters are capped;
  an oversized cluster splits by trigger similarity before any turn runs.
- Trigger: a `Dirty(derive-usecases, cluster)` effect. Emitted when a requirement
  enters, leaves, or changes within a cluster, or a use case's refined requirement
  is deleted. Ready when stage 1 has no pending work for the documents in the
  cluster's cone.
- Pack: the cluster's requirements (full statements and quotes), the use cases
  currently refining any of them, the actor entity with definition, per-stage
  coverage marks, and expansion handles into neighboring clusters.
- Toolset: `context`, `expand`, `search`, `read_section`, `get_entity`,
  `upsert_usecase`, `update_usecase`, `delete_usecase`, `set_trace_coverage`,
  `done`.
- Gates: every step and extension `refines` existing requirements; natural key is
  goal plus actors; a cluster requirement neither refined nor marked
  (`not-applicable` with note) rejects the explicit `done` (the coverage contract,
  one stage up).
- Emits: `Dirty(review-usecase, uc)` per changed use case;
  `Dirty(derive-interaction, uc)` when stage 5 is on and the use case's
  requirements span components.

### review-usecase

- Unit: one changed use case beside its neighbors (use cases sharing refined
  requirements), computed by the reconciler like pair-review neighbors.
- Trigger: `Dirty(review-usecase, uc)` after derive commits. Ready when no
  derive-usecases task is pending.
- Pack: the use case, each neighbor side by side, open diagnostics tying them,
  unrefined unwanted-behavior requirements naming the same actors (extension
  candidates).
- Toolset: reads plus `update_usecase`, `delete_usecase` (merge duplicates toward
  the better-derived one), `report_diagnostic`, `resolve_diagnostic`,
  `set_trace_coverage`, `done`.
- Gates: verdict per neighbor (duplicate, conflict, consistent), the same
  asymmetry rule as entity review: when in doubt keep both and file a diagnostic.
- Emits: `Dirty(derive-interaction, uc)` on flow changes.

### model-domain

- Unit: one scope; the public scope partitions into relationship-connected
  clusters first. A turn sees one cluster.
- Trigger: `Dirty(model-domain, cluster)` when an entity's requirement set,
  attributes candidates, or relationships changed in the cluster. Ready when
  stage 1 reviews for those entities are done.
- Pack: the cluster's entities with current `role` and `attributes`, their
  requirements (summaries with quotes on structural statements), derived
  relationships with current type and cardinality, contradiction candidates the
  reconciler precomputed (two requirements implying different cardinality on one
  edge).
- Toolset: reads plus `update_entity` (attributes, role), `update_requirement`
  (add `edges` with `type` and `cardinality` a statement implies),
  `report_diagnostic`, `resolve_diagnostic`, `set_trace_coverage`, `done`.
- Gates: attributes carry provenance; an `edges` addition only ties entities the
  statement references (existing gate); role changes carry reasoning.
- Emits: relationship recompute happens at commit (derived data);
  `Dirty(design-component, comp)` for components whose satisfied requirements
  gained domain structure; `Dirty(derive-statemachine, ent)` when roles or
  requirement sets cross the threshold.

### partition-architecture

- Unit: the whole project, one turn, rare by design.
- Trigger: stage 4 enabled and no `accepted` partition ADR; or invalidation (the
  share of unallocated or misallocated requirements crosses a threshold, or the
  owner asks). Gated in `manual` mode like any release; the partition ADR's
  prompt/answer is the human approval either way. `auto` mode still stops here:
  the partition ADR is never auto-accepted
  ([open question resolved toward safety](./ir-stages.md#open-questions)).
- Pack: the scope list, the derived relationship graph condensed (clusters, edge
  weights), explicit architectural prose (sections whose entities have `service`
  roles or stated technologies), existing components if any.
- Toolset: reads plus `upsert_component`, `update_component`, `delete_component`,
  `report_adr`, `done`. No interface tools: the partition names boxes, not
  contracts.
- Gates: the component tree is connected and acyclic; every scope maps into some
  component's cone; the partition ADR is staged with the changeset.
- Emits: `Dirty(design-component, comp)` for every component once the partition
  ADR is `accepted`.

### design-component

- Unit: one component, with neighbors summarized.
- Trigger: `Dirty(design-component, comp)`: allocation candidates changed (the
  reconciler routes new or changed requirements to candidate components by
  similarity to what each already satisfies), an operation was proposed from an
  interaction turn, a constraining ADR was answered, or domain structure under
  its satisfied set changed. Ready when the partition is accepted and stages 1 to 3
  are quiet for its cone.
- Pack: the component, its `satisfies` set (summaries), candidate allocations with
  scores, its interfaces, neighbor interfaces (signatures only), constraining
  ADRs, proposed operations awaiting ratification.
- Toolset: reads plus `update_component` (satisfies add/remove, responsibilities,
  facets), `upsert_interface`, `update_interface`, `delete_interface`,
  `report_adr`, `set_trace_coverage`, `done`.
- Gates: satisfies targets exist; every operation `satisfies` at least one
  requirement or carries derived provenance with reasoning; a candidate allocation
  neither accepted nor marked draws the coverage rejection; interface operation
  types resolve to entities.
- Emits: `Dirty(review-component, comp)`; `Dirty(derive-interaction, uc)` for use
  cases whose requirement allocation moved; `Dirty(design-component, neighbor)`
  when a shared interface changed.

### review-component

- Unit: one changed component.
- Trigger: after design commits. Ready when no design-component task pending in
  its neighborhood.
- Pack: the component, its interfaces, operations with no satisfying requirement,
  unexercised relations (no interaction message rides them), scope-split findings,
  ADR conflicts.
- Toolset: reads plus `update_component`, `update_interface`,
  `report_diagnostic`, `resolve_diagnostic`, `done`.
- Emits: diagnostics; `Dirty(design-component, comp)` when a repair is beyond
  judgment.

### derive-statemachine

- Unit: one entity. Exists only past the threshold: two or more state-driven or
  unwanted-behavior requirements reference the entity.
- Trigger: `Dirty(derive-statemachine, ent)` when those requirements change.
  Ready when stage 1 reviews for the entity are done.
- Pack: the entity, its state, event, and unwanted-behavior requirements with
  quotes, the existing machine.
- Toolset: reads plus `upsert_statemachine`, `delete_statemachine`,
  `set_trace_coverage`, `done`. Whole-node upsert keyed on the subject.
- Gates: transitions `refine` existing requirements; states carry provenance; the
  deterministic checks (reachability, determinism, event completeness) run in the
  checks wave and file diagnostics rather than bounce the turn, because an
  incomplete machine is usually a requirements gap, not a modeling error.
- Emits: `Dirty(review-entity, ent)` when the machine implies facts the entity
  definition lacks.

### derive-interaction

- Unit: one use case whose refined requirements are satisfied by two or more
  components.
- Trigger: `Dirty(derive-interaction, uc)` from use case changes, allocation
  moves, or interface changes. Ready when stage 4 is quiet for the involved
  components.
- Pack: the use case with steps, the participating components with their
  interfaces (full operation signatures), the existing interaction.
- Toolset: reads plus `upsert_interaction`, `delete_interaction`,
  `update_interface` (propose an operation with derived provenance when a needed
  one does not exist), `set_trace_coverage`, `done`.
- Gates: every message names an existing operation or one staged in this changeset
  as a proposal; every message rides an existing step; participants exist.
- Emits: `Dirty(design-component, owner)` for every proposed operation, so the
  owning component's agent ratifies, reshapes, or rejects it. This is the
  upward handoff: stage 5 may propose into stage 4, never silently decide for it.

### Stage 6 changes

`bind-requirement`, `generate-entity`, `verify-requirement` keep their contracts.
With stage 4 on, generation gains a grouping option: the unit becomes one component
(its entities' requirements ordered by interfaces), falling back to per-entity when
stage 4 is off. Test derivation consumes use case steps and extensions where they
exist (the EARS-to-Gherkin mapping), the pattern rule otherwise.

## New tools, summarized

Write tools added to the registry, all staging into changesets behind the same
gates: `upsert_usecase`, `update_usecase`, `delete_usecase`, `upsert_component`,
`update_component`, `delete_component`, `upsert_interface`, `update_interface`,
`delete_interface`, `upsert_statemachine`, `delete_statemachine`,
`upsert_interaction`, `delete_interaction`, `report_adr` (upsert by natural key;
supersede, never rewrite), and `set_trace_coverage({stage, target, state, note?})`
(the per-stage coverage mark; `not-applicable` requires the note, mirroring
`set_coverage`). Chat gains `answer_adr`, riding the prompt/answer machinery.

Relationships keep having no write tool. Neither do ripple effects: no agent
enqueues another agent directly. Agents write graph state; the reconciler derives
the effects. That single rule is what keeps the cascade deterministic, replayable,
and explainable.

## Ordering and convergence

- Stage order is the ladder: 1 before 2 before 3 before 4 before 5 before 6, as
  readiness predicates over the queue, not as phases. Within stage 1 the existing
  waves stand (alignment, ingest by levels, fix-up, pair review, entity review).
  Within every other stage: derivations in parallel over disjoint units, then
  reviews, the same shape.
- A build converges when the queue is empty across active stages and the checks
  pass: the multi-stage fixed point. `status.yaml` carries a per-stage verdict
  block; a build can be `converged` for stages 1 to 3 with stage 4 parked, and
  says so.
- The per-build turn cap generalizes: the budget spreads over stages with earlier
  stages outranking later ones when tight (the coverage-outranks-review precedent,
  one level up). Parked work resumes first next build, per stage.
- Oscillation across stages (5 proposes an operation, 4 rejects it, 5 proposes it
  again) is caught by flip detection on natural keys per node kind, surfacing an
  `unstable-derivation` diagnostic with the two turns' reasoning side by side, and
  the pair parks until a human answers.
