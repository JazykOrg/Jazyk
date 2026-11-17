# Authored diagrams

Status: implementation plan, 2026-11-17. This change commits the plan only.
The specification, engine, store migration, and editor contracts below are planned
work, not implemented behavior. No dogfood build or paid model session is needed
to prepare this plan.

## Decisions

- A diagram exists only after a model or human authors its graph node. Recompute
  derives relationships and state machines, never diagrams. A new graph has zero
  views even when it contains systems, flows, transitions, and instances.
- Keep the authored `View` node, `view:` IDs, `graph/views/`, and existing view tool
  names. A view is the authored diagram definition; `.puml`, `.svg`, and `.png`
  remain disposable renderings. Do not introduce a parallel `Diagram` node.
- Extend views with intent, an explicit area, selected relationships, annotations,
  grouping, and bounded layout hints. Rendering remains deterministic through
  `plantuml-little`. Diagram annotations cannot add semantic edges or transitions.
- Add `diagram-coverage`, a GC goal derived by static analysis. Models decide
  which diagrams answer a question, or suppress the finding with a durable reason.
  No analysis rule creates a view, even as a temporary placeholder.
- Containment defines levels and navigation independently of views. Cards and
  level pages exist without diagrams. There is no mandatory primary diagram.
- Store version becomes 6. Upgrade only version 5. Drop untouched defaults;
  preserve every curated view and its ID. Never relabel automatic output as human
  or model authorship. Version 4 follows the older-store archive policy.
- Keep the current exemption from sentence ratification for presentation nodes.
  Require provenance on every authored diagram and its authored content. Semantic
  claims still require entities and requirements with their ordinary provenance
  and ratification rules. An annotation cannot bypass those rules.
- Cloud UI work remains in `../jazyk-cloud`. Define and test portable data and
  mutation contracts here; do not implement a second editor in this repository.

## Current seams and concurrent work

The baseline inspected is master `392324be`. `derive::recompute` calls
`recompute_default_views_with`; its rules also supply level identity to rendering,
flow participants, docsgen, cards, and web APIs. Removing that call alone breaks
navigation and check assumptions.

The current catalog has 13 kinds: `class`, `object`, `package`, `component`,
`composite`, `deployment`, `use-case`, `activity`, `state`, `sequence`,
`communication`, `timing`, and `overview`. This work supports all 13 through the
same tools. `composite` and `overview` retain their existing stored spellings.
UML profiles are outside this catalog and are not added by this change.

`git branch` and `git worktree list` were inspected before planning. No local
branch had unmerged commits touching `model/view.md`, `diagrams.md`,
`derive/views.rs`, `derive/levels.rs`, or `render/` at inspection time.
`/Users/matus/dev/jazyk-requirement-concepts` had uncommitted edits in
`derive/levels.rs`, `derive/order.rs`, derivation tests, and renderer tests.
Recheck that worktree before implementation and integrate its final changes
without overwriting its edits. The main checkout's unrelated `AGENTS.md` edit
remains untouched. Planning uses `plan/authored-diagrams` in an isolated worktree.

Read and update these contracts before engine work:

- [View model](../docs/compiler/model/view.md), [diagrams](../docs/compiler/diagrams.md),
  [levels](../docs/compiler/concepts/levels.md), and [docsgen](../docs/consumers/docsgen.md).
- [Graph](../docs/compiler/graph.md), [folder layout](../docs/compiler/jazyk-folder.md),
  [tools](../docs/compiler/tools.md), and both YAML schemas.
- [Reconciler](../docs/compiler/reconciler.md), [compilation](../docs/compiler/compilation.md),
  and the curate-view, split-view, retrace, and abstract-entity contracts.
- [Prompt guide](../docs/compiler/goals/prompts/README.md) before payload edits.
  Current payloads are under `goals/prompts/goals/`; verify paths at implementation.

## Node schema

Keep `kind`, `title`, ordered `members`, `query`, `excluded`, `collapse`, `limits`,
`created`, and `updated`. Remove `default`. Keep identity as kind plus normalized
title to preserve existing natural keys. Titles must be qualified when two areas
need the same title; never silently repurpose a view in another area on upsert.
Changing title preserves the ID. Changing kind requires a new view because ID
kind segments and kind-specific fields are stable contracts.

Add these fields to `View`, graph schema, read models, and mutation schemas:

| Field | Shape and meaning |
|---|---|
| `purpose` | Required nonempty question or intent, e.g. “How does checkout recover after payment fails?” |
| `description` | Optional short explanation for the reader. |
| `area` | `{target, extent: area \| selection, document?}`. Target is an entity or `scope:<scope>`. A flow may additionally name its source document. An area view claims to explain its area; a selection view intentionally shows a subset. |
| `perspective` | Optional `context`, `container`, or `component` for C4 framing. This is intent metadata, not a new kind or a fixed global tree depth. |
| `relationships` | `{mode: all \| selected, include: [selector], exclude: [{selector, reason}]}`. Default is all. Each selector is canonical `{a, b, type}` over a derived directional relationship group before lifting. Include is used only in selected mode. |
| `labels` | Ordered `{target, text}` entries for member entities, requirements, or relationship selectors. Overrides display text only. Canonical names and source IDs remain in read data and captions. |
| `notes` | Ordered `{id, targets, text, sources}` entries. Targets resolve to drawn elements or relationship selectors; a diagram-wide note has no targets. Sources cite graph facts or document sections. |
| `groups` | Ordered `{id, title, members, parent?}` entries. Local presentation groups have no semantic containment effect. Groups form an acyclic forest; an element belongs to at most one group. |
| `layout` | `{direction?: left-to-right \| top-to-bottom, together?: [[entity id]]}`. Relative hints only, no coordinates, fonts, colors, or raw PlantUML. Unsupported hints for a kind are rejected. |
| `fragments` | Sequence-only ordered tree of `{id, operator, operands}`. Operators are `alt`, `opt`, `loop`, `par`, `break`, `critical`, `ref`. Operands carry sourced guard text and ordered requirement IDs or nested fragments; `ref` names an existing sequence view. |
| `links` | Ordered `{view, relation: detail \| phase, member?}`. Explicit sub-view and overview links, independent of title matching. |

Example authored definition, omitting generation markers and optional fields:

```yaml
view:sequence/checkout-recovery:
  kind: sequence
  title: Checkout recovery
  purpose: How does the shop recover from a declined payment?
  area: {target: ent:shop, extent: selection, document: docs/checkout.md}
  members: [req:pay, req:declined, req:retry]
  fragments:
    - id: retry-path
      operator: opt
      operands:
        - guard: {text: Payment is declined, sources: [req:declined]}
          members: [req:declined, req:retry]
  provenance:
    derived:
      from: [ent:shop, req:pay, req:declined, req:retry]
      reasoning: The alternate payment path needs an ordered explanation.
```

The renderer emits unfragmented requirements at their list positions and replaces
each fragment's contiguous member interval with its fragment tree. Each message
occurs exactly once. Crossing intervals, cyclic refs, unknown members, duplicate
fragment IDs, and unsourced guards are refused. `par` expresses only sourced
concurrency. Timing continues to read quality measures and transitions; it cannot
invent time coordinates. State diagrams continue to read the subject's derived
machine. Deployment diagrams continue to read valued attributes.

Provenance is mandatory. Model creation and edits record derived sources and
reasoning; explicit human choices use decree; a source PlantUML block uses quote
provenance on its `diagram` section with a verbatim whitespace-insensitive quote.
An edit to quoted content must carry new derived provenance or a human decree;
it cannot retain an unsupported quote. The view-level provenance covers the
entire definition, including display choices. New semantic assertions in notes,
labels, fragments, or groups must first exist as sourced graph facts. Pure display
wording is not treated as another domain requirement.

`children` remains computed read data, never a stored field. Extend it to entries
`{member, target, views: [id]}` for drawn members with children. `views` can be
empty. The target identifies the member's containment level. Return plural lists
because several authored diagrams can explain one area. This is a deliberate API
change coordinated with Cloud later, not an invented fallback diagram.

## Tool surface and gates

Extend `upsert_view` and `update_view` with all fields above. Preserve ordered
membership replacement and add/remove operations. Omitted update fields mean
unchanged; `null` clears an optional scalar, and `[]` clears a collection. Reject
ambiguous replacement plus delta operations on the same field. Require reasoning
for every model mutation, including deletion. Human paths record an explicit
decision through the existing decree mechanism.

- `upsert_view` searches the existing natural key, validates area compatibility,
  and returns `{id, created}`. It accepts every catalog kind.
- `update_view` validates the complete resulting definition, including queries,
  reference closure, kind restrictions, and derived source support.
- `delete_view` accepts every authored view with a reason. It never deletes facts.
  Incoming `links` and fragment refs derive retrace findings on their owners.
- `get_view` returns the complete authored definition, computed effective members,
  relationships shown and omitted, coverage summary, navigation, and fact hash.
- `list_views` filters by area, kind, and entity/requirement membership, with stable
  ordering and bounded pagination. It never creates missing views.
- `suppress_diagram_coverage({area, finding, reason, expectedAreaHash})` stages a
  decision, returning its ID and baseline. The harness computes all measures.
- `unsuppress_diagram_coverage({id, reason})` removes the decision. Models may
  revise a suppression only inside their authorized coverage area; humans may
  suppress or unsuppress through the ordinary graph-edit path.
- `list_diagram_suppressions` exposes active and expired status with reasons,
  baseline and current measures, and the exact re-fire condition.

Use the existing session staging, preview, and atomic `done` gates. Graph edits
in chat use the same mutation implementation and authorization, not a bypass.
Check the snapshot hash again at commit. Reject stale suppression measures.
Include new references in merge rewriting, redirect resolution, hashing,
retraction, and dead-reference analysis. A missing relationship selector is
reported as a retrace finding, never reinterpreted as a new arrow.

Every kind retains its existing member rule. Queries remain structural only and
exclude instances except where object membership explicitly permits them.
Preserve current query-match curation. Filter relationships before lifting, then
aggregate shown contributions in deterministic order. Omitted contributions and
reasons remain inspectable. Hidden arrows cannot satisfy the split-view
no-arrow-lost gate: exclusions used to resolve a split must link to a detail view
that draws those contributions. Normal focused diagrams may omit arrows with
reasons; that is an authored explanation, not missing semantic data.

Always escape diagram text through the existing inert-text emitter. Neither
layout nor annotation fields accept executable renderer syntax or includes.

## Coverage analysis

Run analysis after relationship and state-machine derivation and query expansion.
It produces findings and counts only. Use immutable graph IDs for area identity,
never a title, name, generated view ID, or document basename.

An area key is a structured `{family, target, document?}` encoded reversibly for
`g:diagram-coverage:<area-key>`. Families separate independent questions about the
same target. One goal folds all findings for that area, including diagram drift;
its mandatory flag is the maximum severity of its unsuppressed findings.
No synthetic entity is stored for an area.

Thresholds are initial product defaults, not benchmark-derived values. Put named
constants in `limits.rs` and document them in the registry. A count must be
strictly greater than soft or hard, matching existing limit semantics.

| Family | Deterministic candidates and count | Soft | Hard | Eligible authored kinds |
|---|---|---:|---:|---|
| `structure` | Direct children of an entity or scope root, excluding instances | 3 | 9 | class, component, package, composite |
| `system` | A `system` entity; count is max(1, direct children plus distinct external interactors) | 0 | 7 | component with context/container/component perspective |
| `flow` | Behavior and failure-mode requirements reaching one containment level, clustered by source document | 3 | 11 | use-case, activity, sequence, communication, timing, overview |
| `state` | Distinct states of one derived machine | 2 | 7 | state on its subject |
| `object` | Instances of one type through instantiation groups | 1 | 7 | object |
| `deployment` | Distinct `(entity, attribute name, value)` placement facts in one nearest system, or scope when no system contains it | 0 | 5 | deployment |
| `drift` | Percentage of uncovered current candidates or out-of-area shown members of an area diagram | 20 | 40 | The family's eligible kinds |

Structure begins at four children, mandatory at ten. Flow begins at four
requirements, mandatory at twelve. State begins at three states, mandatory at
eight. Object begins at two instances. A system with no neighbors still receives
optional advice that can reasonably be suppressed. A deployment attribute is
recognized by the same placement semantics as the existing deployment emitter;
do not invent an environment-name classifier in the coverage analyzer.

Reuse level topology and document-order indexing independently of views. Preserve
current flow reachability rules, but replace “child has a level view” with “child
has a containment level”: internal behavior stays at the child's level even with
zero diagrams. Exclude grouping-only flows by the existing grouping rule. A
single behavior requirement does not create a goal by itself; it remains visible
in the area report below threshold. Missing documentation source for a derived
behavior groups it into a separate stable `derived` bucket, not a random document.

A diagram matches by explicit area and compatible kind, never by title. For
structure, flow, object, and deployment, union the candidates represented by
matching diagrams, after exclusions. Full-area coverage means at least 80% of
candidates represented. Use integer arithmetic `5 * covered >= 4 * total`.
Area extent `selection` may contribute represented candidates but never claims
coverage of unrepresented ones. Member entities cover themselves or descendants
explicitly collapsed under them, not arbitrary omitted siblings. A state diagram
covers the states of its subject's live machine. An overview covers a phase's
requirements only through valid explicit phase links. System context includes
the system and its external interactors; internal component/container views show
its children. Either answers the system finding, while the structure family
still independently measures children.

If no eligible diagram exists, derive absence once the family's count exceeds
soft. If matching diagrams exist but their union covers less than 80%, derive a
coverage-gap finding using the same family's count and thresholds. For a claimed
full-area diagram, also measure drift: missing candidates divided by current
candidates, or foreign members divided by live shown members, whichever is
larger. Compare percentages by cross multiplication, with empty denominators
contributing zero. Drift derives only for `extent: area`; a narrow selection is
not wrong merely because its area is larger. Dead IDs produce retrace first.
No goal is generated for cosmetic title or layout drift.

A valid suppression waives only its named finding, not all diagram work for the
entity. Structure and system findings may share diagrams and batch together.
Coverage is an opportunity heuristic, not a requirement to draw every fact.
Suppressions are the durable way to explain why the heuristic is unhelpful.

## Suppression and re-fire

Persist decisions under `graph/decisions/diagram-coverage/<encoded-key>.yaml`.
A decision contains schema/rule version, area key, finding key, optional diagram
ID for drift, reason, provenance, canonical evidence hash, baseline candidate IDs
and per-candidate semantic hashes, count vector, and generation. Lists and hashes
are harness-computed. No build-local parked state is used as suppression.

The evidence hash covers candidate membership, behavior statement/edge/transition
hashes, state transitions, instance-type assignments, valued placement facts,
area containment identity, and applicable coverage rules. It excludes generation,
formatting, render files, note text, and layout. Canonicalization follows redirects.
A changed hash causes measurement, not immediate invalidation.
Store the full baseline candidate set outside prompt context; expandable handles
and stubs never truncate the persisted measure. Diagram drift uses the area
candidates and its authored-definition hash as evidence. Repairing or replacing
the diagram invalidates its drift suppression immediately, even without area
growth. Expiry never depends on the generated rendering hash.

For each relevant dimension, let `B` be its baseline count and let
`delta = max(1, ceil(B / 5))`. A suppression expires when any of these holds:

- Current count is at least `B + delta` in that dimension. This includes growth
  exactly equal to one fifth, rather than floating-point rounding.
- At least `delta` baseline candidates are removed or semantically revised, or
  `delta` new candidates replace old ones while total count stays flat. Count the
  union of removed/revised/new IDs for churn; do not reset the baseline on small
  commits. For flow use requirement IDs plus semantic hashes, so five new behavior
  statements after a 25-statement decision reopen it.
- The area's identity or containment boundary changes, its machine/type disappears
  and is replaced, its coverage rule version changes, or a human unsuppresses it.

Expired decisions never suppress. Retain an expired decision while its area exists
for inspection; replace it only with an explicit new decision. Sweep decisions
whose areas are gone. An area that shrinks below soft needs no goal regardless
of suppression status. Restoring eligibility is measured against the retained
baseline, not a newly minted baseline. A newly created matching diagram can end
the finding without modifying the suppression. Deleting that diagram does not
revive a previously waived finding before the baseline expires.

A suppression may remain valid across optional-to-mandatory escalation if growth
has not reached its re-fire delta. Severity is waived deliberately; it is not a
failed mandatory goal. Expose this on the board and explain surface. Human and
model suppressions use the same expiry rule. Permanent per-project disable flags
are outside this change.

Failed coverage work remains different from suppression: `mark_goal_failed`
records inability to act; a failed mandatory finding blocks convergence. Apply
the same baseline and one-fifth growth/churn rules to its retry fingerprint so
one new statement cannot retry an honest failure on each chapter. Preserve
explicit human retry, four-attempt parking, and build-local park semantics.
Do not change abstract-entity's existing one-fifth failure behavior as a side
effect of adding coverage decisions.

## Draft goal page: diagram-coverage

Goal: judge the visual explanation needed for one stable area after its semantic
facts settle. Create or repair diagrams that answer specific questions, or record
why this area does not need the requested explanation.

- Kind: `diagram-coverage`. Class: GC. Optional above soft; mandatory above hard.
- Unit: one area. ID: `g:diagram-coverage:<area-key>`.
- Created by: `diagram-undercovered` and `diagram-drift` findings from the analyzer.
- Ready when no open or parked compile goal exists in the area's cone. Reuse the
  existing exceptions for human ratification/answer work; do not introduce a global
  diagram phase. Include descendant entities, relevant requirements, their source
  sections, and diagrams referencing them in the cone.
- Yield to open or parked fan-out abstraction for the same level, as split-view
  does. Repair stale diagram references through tier-2 retrace before GC judgment.
- Locality: target entity or scope plus source document for flow. Batch overlapping
  areas and existing view goals within the context budget. Never combine unrelated
  scopes just because they share a kind. Newly opened compile work takes precedence.
- Initial context: area identity, findings, thresholds, baseline measures, direct
  child/entity stubs, relevant requirement statements with source handles,
  relationship-group summaries, machine/type/placement summaries when applicable,
  and existing diagram stubs with purpose, kind, area, coverage, and counts.
  Load full nodes and source sections on demand. Large lists use expandable handles;
  the gate always evaluates the entire snapshot, not the truncated context.
- Toolset: graph read tools, diagram mutations, suppression tools, goal tools,
  `report_feedback`. No entity/requirement mutation tools just to pass coverage.
- Skill: `diagrams`, then existing structural-views or flow-views detail as needed.

At `mark_goal_done`, evidence lists one outcome per finding:
`{finding, verdict: covered, views: [id]}` or
`{finding, verdict: suppressed, decision: id}`.
The gate checks complete evidence, live eligible diagrams, compatible areas,
computed coverage, referential integrity, and nonempty purpose/provenance.
A covered claim must eliminate the finding on staged recomputation. A suppressed
claim needs a staged, current-baseline decision with a substantive reason.
Creating one empty diagram never passes. “Done” alone stores no suppression.
Every gate reruns at commit, and `done` commits nodes and decisions atomically.

A focused diagram may exceed a size limit and open split-view. Coverage completion
does not assert readability; split-view remains independently mandatory past hard.
A failed render does not erase the authored node or count as a new coverage gap;
report the renderer failure separately under the existing rendering contract.

Draft payload for `goals/prompts/goals/diagram-coverage.md`:

```text
The harness found an area whose diagrams do not explain its current facts. Load
its entities, statements, relationships, and existing diagrams. Use the diagrams
skill to choose the question worth drawing. Search before creating. Author or
repair only the diagrams needed to answer that question. Every finding needs
computed coverage or a suppression with a concrete reason. Do not invent semantic
facts, copy every kind, or draw a placeholder merely to close the goal. Cite the
facts behind membership, annotations, and ordering. Fail when a required action
cannot be completed; failure is not suppression.
```

Implement this payload with guide-compliant metadata and shared partials. Keep
corrective gate guidance in docs payloads, not Rust strings.

## Existing goals and checks

- Curate-view keeps query matches and unplaced behavior for existing authored
  diagrams. Rank by compatible area, shared entities, same document, source order,
  then ID. There are no automatic twins. An explicit phase/detail link or shared
  area gives locality without depending on equal titles.
- If no suitable flow diagram exists, unplaced behavior feeds flow-area analysis.
  It is not an error and does not fabricate a curate-view target. If a diagram
  exists in that area, its unplaced behavior can still feed curate-view. A behavior
  statement elsewhere in the project must not create an error merely because some
  unrelated area happens to have a sequence diagram.
- Valid area suppression waives corresponding placement advice for this measure.
  Keep any actual semantic failure diagnostic, such as an inconsistent transition.
  Distinguish coverage advice from semantic errors in check results.
- Split-view applies to every authored diagram. Preserve size thresholds, fan-out
  precedence, no-member-lost and no-arrow-lost gates, explicit sub-view linkage,
  and visible renderer over-limit notes. Layout hints never satisfy count gates.
- Retrace repairs dead members, exclusions, collapse IDs, area targets, annotations,
  relationship selectors, source references, group members, fragment refs, and
  links. Deleting the diagram is legal. It may reopen unsuppressed coverage work.
- Level-shape stops checking for a structural diagram. It still checks hard fan-out,
  containment consistency, and dissolution of under-membered derived groupings.
- Existing state-machine consistency and failure-branch checks stay semantic.
  Absence of a state/activity diagram alone is never a semantic check error.
- Update goal catalogs, hint assembly, goal-name serialization, tool capability
  registries, board counts, status/verdict reporting, preview/ripple/explain, and
  no-op expectations. Optional coverage does not block convergence; mandatory
  unsuppressed open/failed coverage does. Suppressed work is reported separately.

## Levels, lifting, docsgen, and API

Extract topology from `derive/levels.rs` into view-independent level enumeration.
A level is a nonempty set of direct children, including a scope's roots. Retain
outside-interactor and document-order helpers. Remove all ID construction and
“diagram exists” tests from topology. Rendering uses the authored `area` target
for flow lifting; a legacy unanchored flow lifts only to its explicit participants.
Structural rendering still lifts hidden descendants to nearest shown ancestors.
Local diagram groups never alter entity ancestry. A reparent changes topology,
not authored intent; queries update, explicit membership persists, and analysis
or retrace requests repair when needed.

- Generate containment level pages for every nonempty level, including one-child
  levels, regardless of diagrams. Render member links and breadcrumbs first.
- A diagram's entity links always open cards. Cards expose parent, children,
  siblings, lifted relationship summaries, and zero or more authored diagrams.
  `In context` and `Inside` can say “No authored diagram” with navigation links.
  Do not label a multi-child node a leaf because it lacks a diagram.
- List diagrams by explicit area and relevance. Prefer structural diagrams for
  context, then purpose/title and ID for stable order. Do not infer well-known IDs
  such as `view:component/<scope>` or `view:state/<entity-slug>`.
- Keep diagram pages and legends only for real nodes. Docsgen's index starts from
  containment and lists authored diagrams. With zero diagrams it still provides a
  complete requirements and relationship walk. Prune obsolete rendering files.
- Cards, LSP hover, terminal viewer, browser/WASM read models, and native HTTP APIs
  consume the same computed topology and diagram lists. No dangling SVG links when
  rendering fails; link to the definition and `.puml` with failure status.

Portable API contract for the later editor:

- List/get views expose definition, effective members, ordered drawing data,
  relationship selection, annotations, fragments, links, provenance, `factHash`,
  renderer status, and coverage. Tree nodes expose `area`, `children`, and `views: []`.
- Mutation requests use the same validated operations and expected snapshot/hash
  concurrency contract as graph tools. Return changed IDs, current hashes, goal
  preview, and validation errors. Implement native dispatch and portable webapi
  support together; no browser-only mutation logic.
- Suppression list/get/suppress/unsuppress returns reasons, provenance, active or
  expired status, baseline/current measures, and re-fire explanation. Coverage
  finding reads expose thresholds and missing candidates even below goal threshold.
- Update `webapi/views.rs`, `webapi/card.rs`, shared `card.rs`, and GUI adapters.
  Inspect the current dispatch/edit_graph surface before selecting HTTP route
  spellings; reuse it rather than adding a competing mutation service.
- Document breaking `children` and tree payload changes for Cloud. This repository
  can build and test without Cloud. No editor deployment is part of implementation.

## Migration and persistence

Set `STORE_VERSION = 6`, `UPGRADABLE_VERSION = 5`. Upgrade at the next writable
build opening, preserving the current read-only behavior before upgrade. Never
run the migration on the dogfood to test it.

- Untouched defaults live in version 5's derived/default-view projection. Discard
  them and their stale rendering outputs. Do not promote them into graph/views.
- Preserve all curated views, including views converted by a previous edit to a
  default. Preserve provenance, identity, membership, query, exclusions, collapse,
  and limit bumps. Missing provenance receives derived migration provenance citing
  surviving members and stating preservation of a curated version-5 view.
- Fill missing purpose with the existing title as a visibly migration-supplied
  intent. Infer an area only when version-5 level association or query.parent is
  unambiguous. Otherwise choose the members' common scope with extent selection;
  use the first subject for state/timing. Never guess a whole-area coverage claim.
- Migrate explicit split links where existing exclusions or containment identify
  them unambiguously. Leave ambiguous phase linkage for authored repair, surfaced
  through diagnostics; do not infer links from matching titles alone.
- Version 6 writes no `default` field or default-view files. Rebuild derived
  relationships, machines, query membership, and coverage findings after upgrade.
  Do not run a model during migration. Opening a store can therefore expose new
  goals without creating diagrams.
- Keep atomic persistence and failure recovery. A failed upgrade leaves version 5
  authoritative; retry is deterministic. A second successful opening is a no-op.
- Version 4 and older archive/rebuild under the existing one-previous-version
  policy. Newer stores stay read-only and are never archived. Document this
  consequence explicitly because version 4 currently upgrades to version 5.

## Implementation sequence

1. Commit specifications first. Rewrite view.md and diagrams.md around authored
   intent. Update graph.md, graph.schema.yaml, tools.md, tools.schema.yaml,
   reconciler catalog/readiness/escalation/failure rules, compilation checks,
   goals/diagram-coverage.md, levels.md, docsgen.md, and jazyk-folder.md. Update
   curate-view, split-view, retrace, abstract-entity, model.md, and main.md wherever
   automatic diagrams remain promised. Search frontends and state-machine docs too.
2. Add the `diagrams` skill and goal payload. Adapt structural-views, flow-views,
   session tool catalogs, curation/split/retrace/abstraction payloads, hint/gate
   guidance, and serving descriptions. Validate parser conventions and registration.
3. Extend model and schemas, tool parsing, staging, provenance, hashing, redirects,
   and validation. Add suppression persistence and decision operations before
   wiring goal completion. Keep native and WASM serialization equivalent.
4. Separate containment topology, coverage candidate computation, and render
   membership. Remove default derivation and ownership code, including default
   write/delete exceptions. Update queries and every consumer in the same coherent
   change so intermediate code never requires a synthesized fallback view.
5. Implement limits and analyzer, `goals/diagram_coverage.rs`, board derivation,
   cones, batching, hints, gates, fingerprints, and suppression expiry. Adapt
   placement checks and existing view goals. Preserve abstract-entity retry rules.
6. Extend render scene/emitter support for labels, notes, groups, selection,
   direction, and fragments across the catalog. Keep inert text and deterministic
   output; preserve size accounting and explicit omission visibility.
7. Implement version-5 migration, store round trips, output pruning, docsgen/card/
   viewer/LSP updates, webapi contracts, and native adapters. Version bump lands
   with a working migration and all changed readers, never as a standalone release.
8. Update fixtures and recorded tool sessions, run validation, commit coherent
   work, merge locally with `git merge --no-ff`, and push master. Use local-only
   work branches. Do not publish artifacts, deploy Cloud/site, or dispatch CI.

Each implementation commit must compile with its applicable tests. Steps 3 to 7
can be combined when needed to avoid exposing an incomplete persistent schema.
Do not preserve compatibility wrappers around removed default-view derivation.
Recheck concurrent work and master immediately before integration.

## Diagrams skill content

The skill is selection guidance, not a scheduling mechanism. Goals decide when
it is loaded. Map questions to the complete local catalog:

| Question | Kinds |
|---|---|
| What concepts and relationships matter? | class, object for concrete instances |
| What are the boundaries, parts, and dependencies? | package, component, composite |
| Where do artifacts run? | deployment |
| What does an actor accomplish? | use-case |
| What are the steps and alternatives? | activity |
| What states and transitions are allowed? | state |
| Who exchanges what, in what order? | sequence, communication |
| What timing constraint governs a lifecycle? | timing |
| How do interaction phases connect? | overview with explicit phase links |

Ground the guidance in [UML 2.5.1](https://www.omg.org/spec/UML/2.5.1).
Use [C4](https://c4model.com/diagrams) for context/container/component intent,
without imposing all levels on every project. Use
[Kruchten's paper](https://arxiv.org/abs/2006.04975) for stakeholder questions
across logical, process, development, physical, and scenario views. These are
selection lenses, not extra stored UML kinds or automatic diagram quotas.

Include paired examples: a small customer/system context versus a class dump;
a payment sequence with a sourced failure branch versus invented concurrency;
an instance snapshot versus types mislabeled as instances; a deployment showing
valued placement facts versus guessed infrastructure. Use the existing soft size
limits as advice and explain split/linked detail. Prefer one question per diagram.
Do not draw when text is clearer, the area is tiny, an existing diagram already
answers the question, or required semantics are absent. Suppress with the actual
reason rather than creating all catalog kinds. Keep all authored instructions in
docs payloads and source rationale beside the rule.

## Tests and acceptance evidence

Use cargo unit/integration tests, deterministic recorded sessions, and fixtures.
Do not call `jazyk compile`, `jazyk check`, `jazyk gen`, or `jazyk test` on dogfood.
Do not start paid model sessions without per-action authority. Replays must show
actual gated tool execution, not model output strings asserted in isolation.

- Fresh graph: seed entities, systems, flows, machine, instances, placement facts;
  recompute repeatedly and round-trip the store. Assert zero views and renderings,
  derived semantic facts intact, topology/cards navigable, expected coverage goals.
- Analyzer: table-driven tests at soft, soft+1, hard, hard+1 for every family;
  scope roots, deep uneven containment, grouping-only flows, document-path
  collisions, internal flows without diagrams, instances excluded from structure,
  and zero/singleton areas. Assert no analysis mutation and stable IDs/order.
- Coverage: unrelated kinds/areas and one empty diagram do not satisfy coverage;
  partitioned diagrams do; collapsed descendants count correctly; 80% boundary,
  selected relationships, missing phases, and exact drift boundaries behave as
  documented. Focused selection diagrams do not spuriously drift.
- Tools: replay create/edit/delete for each of the 13 kinds with appropriate facts.
  Exercise every new field, queries, null/empty updates, quoted source blocks,
  human decrees, reasoning/provenance, natural-key retry, staged references,
  preview, goal evidence, commit conflict, rollback, and atomic `done`.
- Negative gates: wrong member kind, unknown refs, incompatible area, duplicate
  members, group cycles, invalid relationship groups, unsupported layout hints,
  crossing fragments, unsourced guards, cyclic refs, and stale suppression hash.
- Suppression: no re-fire below cumulative delta; exact one-fifth growth; baseline
  zero/one, count-preserving replacement, revised statements, many small commits,
  parent moves, changed rule version, expired decision reads, manual unsuppress,
  restart/round-trip, and optional-to-mandatory escalation. Test failure and park
  separately so neither silently becomes a suppression. Retain abstract-entity's
  existing “grown by a fifth” regression tests unchanged.
- Existing goals: retrace on every new reference category and deletion; curate
  query and behavior matches without default twins; split authored diagrams with
  linked details and no member/arrow loss, including filtered arrows. Test fan-out
  precedence and cone gating over open and parked compile goals.
- Checks: no flow diagram yields coverage advice, not placement errors;
  unrelated flow diagram does not alter that result; authored area diagrams still
  expose unplaced behavior; suppression has bounded effect. Hard fan-out and
  semantic transition/containment failures still block as before.
- Rendering: snapshot every kind with applicable intent fields, stable aliases,
  order, labels, group and arrow selection, sequence fragments, inert malicious
  text, over-limit notes, render failure cleanup, and deleted-view file pruning.
  Use the in-process renderer locally; official native renderer parity only when
  available, with no unsolicited download or hosted validation dispatch.
- Migration: a real version-5-shaped disk fixture with untouched defaults plus
  curated views, including a curated former default. Assert defaults disappear,
  IDs/provenance and edits survive, no models run, failed-write recovery, second
  opening no-op, older archive, newer read-only, and suppression round trips.
- Update `bootstrap/example/e2e-gc` scripted expectations to explicitly create
  diagrams through tool sessions. Cover coverage → author or suppress → split/
  curate/retrace → rebuild. Keep f1, f2, and relevant example-* traps; replace only
  assertions that specifically promise automatic defaults.
- Portable API/docsgen: zero/one/multiple diagrams per level, one-child level,
  member with children but no diagram, authored areas without known IDs, missing
  renderer output, suppression status, navigation links, pagination, native/WASM
  payload parity, and portable mutation routing.

Run applicable focused tests while implementing. Final engine validation, from
`bootstrap/`, uses a private `CARGO_TARGET_DIR` and captures full output to task
logfiles outside the project:

```sh
RUSTC_WRAPPER= cargo test --locked
RUSTC_WRAPPER= cargo fmt --check
RUSTC_WRAPPER= cargo clippy --locked --all-targets -- -D warnings
RUSTC_WRAPPER= cargo check --locked --target wasm32-unknown-unknown --lib
RUSTC_WRAPPER= cargo build --locked --release
```

Run Go tests only if the helper changes. Validate changed Markdown links/anchors,
YAML schemas, prompt parsing/registration, and absence of em dashes. Do not run
engine builds for this plan-only commit. Before implementation handoff, build the
release binary in the owner's expected target directory as well, then restart
running `jazyk gui` if a binary change was installed. Preserve unrelated processes.
Configure Matus Faro's name/email, signed commits, and signing key before each
commit. Stage and commit in one command chain, never amend, stash, or rebase.

## Completion and open items

The implementation is complete only with saved specs, passing local checks,
version-5 fixture migration, explicit tool-session coverage for every catalog
kind, integration into local master, and verified remote master SHA. Report
local validation, push, artifact publication, and deployment separately. No
publication or deployment is requested by this plan.

Implementation choices remaining are mechanical: exact Rust module factoring,
HTTP route spelling within the existing dispatcher, and support of optional
layout hints by each emitter. Unsupported hints must be rejected explicitly;
required catalog tooling must not be deferred. The thresholds above are concrete
initial defaults to revisit with fixture evidence, not unresolved parameters.
Cloud UI adaptation is a separate follow-up with the documented API break.
