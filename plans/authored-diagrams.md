# Authored diagrams

Status: implemented in the engine and specifications, 2026-11-17. Validation
uses cargo tests and scripted fixtures. No dogfood build or paid model session
is run for this change. The authored diagram editor in `../jazyk-cloud` is in scope
for this delivery, with its specifications under `docs/editor/` in that repository.

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
- Add an explicit diagram-review compilation step after requirement judgment and
  structural review, before criteria and implementation. Its `diagram-coverage`
  tasks come from static analysis. Models choose diagrams that answer the raised
  questions, or suppress a finding with a durable reason.
  No analysis rule creates a view, even as a temporary placeholder.
- Containment defines levels and navigation independently of views. Cards and
  level pages exist without diagrams. There is no mandatory primary diagram.
- Store version becomes 7. Upgrade only version 6. Drop untouched defaults;
  preserve every curated view and its ID. Never relabel automatic output as human
  or model authorship. Version 5 and older follow the older-store archive policy.
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
  [tools](../docs/compiler/tools.md), both YAML schemas,
  [context](../docs/compiler/context.md), and
  [session toolsets](../docs/compiler/sessions.md#toolsets).
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
| `labels` | Ordered `{target, text}` entries for member entities or requirements. Overrides display text only. Canonical names and source IDs remain in read data and captions. |
| `notes` | Ordered `{id, targets, text, sources}` entries. Structural entity targets receive note connectors; other notes remain diagram-wide; a diagram-wide note has no targets. Sources cite graph facts or document sections. |
| `groups` | Ordered `{id, title, members, parent?}` entries. Local presentation groups have no semantic containment effect. Groups form an acyclic forest; an element belongs to at most one group. |
| `layout` | `{direction?: left-to-right \| top-to-bottom}`. Relative hints only, no coordinates, fonts, colors, or raw PlantUML. Unsupported hints for a kind are rejected. |
| `fragments` | Sequence-only ordered tree of `{id, operator, operands}`. Operators are `alt`, `opt`, `loop`, `par`, `break`, `critical`, `ref`. Operands carry sourced guard text and ordered requirement IDs or nested fragments; `ref` names an existing sequence view in `view`. |
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
concurrency. Timing continues to read criterion bounds and transitions; it cannot
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
change coordinated with the Cloud editor, without a fallback diagram.

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

## Entity context and diagram discovery

Every entity context fetch makes its available authored diagrams discoverable.
Use a shared reverse index over the snapshot for `load`, `get_entity`, initial
context packs, `jazyk context`, cards, and portable API reads. Do not discover
availability from rendering files or well-known default-view IDs.

Match these associations and explain each in the returned summary:

- Direct effective membership, including query membership after exclusions.
- Participation in a flow through a member requirement's entities or message ends.
- Subject of a state or timing diagram, and instance/type association for object.
- Representation through a shown collapsed ancestor, naming that ancestor. Do not
  claim that the hidden descendant is drawn as its own element.
- The entity's own area and its immediate parent's containment area, even when
  the diagram does not draw the entity. Mark these `area` or `parent-context`,
  distinct from actual membership. A root's parent-context is its scope.
- Explicit linked detail views of these diagrams, available as references on the
  diagram summary. Do not recursively inline all linked diagrams or all ancestors.

Return a `diagrams` section of references by default. Each entry contains `id`,
`kind`, `title`, `purpose`, `area`, association reasons, member/participant counts,
`factHash`, and whether it is already loaded full. Optional related goal and
render-failure status are compact flags. Deduplicate diagrams that match several
associations. Order direct membership/participation/subject first, collapsed
representation second, own area third, parent-context fourth; break ties by kind,
normalized title, and ID. References and source identities are useful even when
rendering failed or the entity has no directly containing diagram.

The same entity read exposes compact area coverage status and the count of active
suppressions, with references to expand. Do not inline suppression histories or
mislabel a suppressed absence as an available diagram. A truly empty diagram
result says `diagrams: none`; a truncated result says how many remain and gives
its expansion handle. Never silently replace a large list with `none`.

Keep this bounded under the existing context budget. Reuse the `members` axis
for reverse diagram associations and its paged `h:<entity>:members[:<offset>]`
handles; add no parallel loading protocol. Read responses outside the loaded set
use the same bounded list with a continuation accepted by `list_views`.
`list_views({entity})` uses the same relevance semantics, with an optional
association filter, not only direct stored membership. Counters and continuation
metadata distinguish complete, partial, and empty results.

Use references rather than full definitions on ordinary entity fetches. A
sequence definition may contain many statements and nested fragments, and several
entities may share it. Automatically loading them all would duplicate context
and make unrelated entity work expensive. `get_view({id})` reads a definition;
`load({target: id})` makes it a full persistent loaded-set item under existing
budget rules. `expand` on an entity's members handle discovers more references;
it does not secretly load every discovered diagram fully. Existing explicit
`depth: 2` loading can load neighboring diagram definitions within the budget;
ordinary depth-1 loads remain summaries only. Full loads include members as
stubs and expansion handles, not recursively full semantic neighborhoods.

A diagram that is the target of curate-view, split-view, or retrace loads full
in that goal's initial pack. Diagram-coverage starts with relevant diagram
summaries and explicitly loads candidates before editing; it does not need every
existing diagram in full. A model-requested full load appears once in the loaded
set even when several entities reference it. Compaction, reload, and unload use
the normal item lifecycle. Read summaries reflect the session snapshot and mark
staged edits/deletes consistently with the current context contract.

Fetching an entity or loading a diagram never authorizes edits or starts
diagram review or maintenance.
Generation, verification, review, and read-only agents can consult the same
references without receiving diagram write tools.

## Tool availability and prompting lifecycle

The harness computes tool availability from the open batch's goal kinds and
source subtype, not from a model's desire to draw or its loading a domain skill.
All mutations retain the batch scope and commit-time gates. A mixed batch receives
the union of its authorized slices. Refresh the tool catalog at batch transitions,
and reject cached out-of-scope writes through the existing wrong-toolset gate.

| Consumer or goal | Diagram reads | Diagram writes and suppression |
|---|---|---|
| Read-only context, inspection, generation, verification, ordinary entity review | Entity summaries, list/get views, load/expand, coverage and suppression reads | None from diagram discovery. Existing non-diagram permissions remain unchanged. |
| `diagram-coverage` | All diagram and graph reads | Upsert/update/delete views and suppress/unsuppress for its area/findings, plus goal completion/failure and atomic done. |
| `curate-view`, `split-view`, view `retrace` | All diagram and graph reads | Existing view mutation slice, extended to authored fields and linked details. No suppression tool unless a coverage goal is also in the batch. |
| `abstract-entity` | All diagram and graph reads | Existing upsert/update view slice for affected coarse/detail diagrams. No diagram deletion or suppression from abstraction alone. |
| Source PlantUML `diagram` input | Existing semantic extraction tools first; full diagram reads in the subsequent mandatory source review | `diagram-coverage` authors the quoted definition after semantic extraction, without heuristic suppression. |
| Explicit human diagram request in writable chat/editor | All diagram and graph reads | Validated graph-edit mutations needed for the request, including suppression only when requested or otherwise authorized. No compiler batch required. |

Source-diagram reconciliation may author the diagram stated by its input; this
is an explicit source-backed review after semantic extraction. The source section
opens a mandatory diagram-coverage finding without broadening ordinary prose
ingest writes. Quoted provenance must locate in that source block.

The `diagrams` domain skill guides judgment and does not grant tools. Automatically
activate it for diagram-coverage and diagram-targeted curate/split/retrace work,
and when reconciling a source diagram. For abstraction, activate it when that
batch has affected diagram hints or loads a diagram. The existing structural-views
and flow-views skills provide kind-specific detail when relevant. Read-only and
deliverable agents may load the skill by name without gaining mutation authority.
In writable conversation, the relevant workflow skill exposes authorized tools;
loading the diagram domain skill alone never widens the workflow capability.

Add a short shared context instruction in docs payloads: entity context lists
relevant authored diagrams; read the purpose and association, load a relevant
definition when it helps answer the current question, and treat its membership
and annotations as authored interpretation backed by the cited semantic facts.
Do not copy every available diagram into the prompt. Preserve actual provenance
and verify requirements directly for implementation or verification decisions.

Each coverage goal prompt contains the area, the reason it fired, measured
counts, soft/hard values, optional/mandatory status, missing candidates, existing
diagram summaries, suppression status, and the completion gate. It then loads
the `diagrams` skill for question selection and good/bad examples. The model
loads the relevant facts, sources, and candidate diagrams, authors or repairs
what is useful, or records a concrete suppression. Goal hints carry runtime
evidence; shared instructions and corrective guidance stay in docs payloads.
There is no unsolicited diagram-authoring prompt on an ordinary entity read.

## Compilation step and per-kind selection

The owner accepted this visible compilation sequence:

Extract requirements → review requirements and structure → review and author
diagrams → criteria and implementation → verification and final output.

Use existing task/session machinery for this step. Insert diagram-review at
readiness tier 3 and move current tier-3 criteria/ledger work to tier 4. Change
readiness rules explicitly rather than retaining the old GC gate: waiting for
all compile work would wait for the very implementation this step should precede.
Mandatory diagram reviews hold downstream work in their affected area until
answered by diagrams or valid suppression. Unrelated settled areas may proceed.
Optional diagram work is offered before downstream work within the selected
scope and budget; unresolved optional advice cannot hold implementation forever.
Keep optional tasks visible when skipped or left unfinished. This local rule
must coexist with the existing upstream tier barrier and concurrent batches.

Existing maintenance tasks still handle abstraction, duplicate cleanup,
curation, splitting, and repairs. Structural abstraction needed for an area
runs before its diagram review. Curation and split-view may run inside the diagram
step as follow-up work. A later semantic edit reopens the affected upstream work
and diagram review, never the whole project's diagram step without cause.
Pure rendering changes only regenerate output. Preserve explicit release and
cost gates; adding this step does not start otherwise unauthorized model work.

The program chooses areas and questions using graph facts and fixed thresholds.
The model chooses the smallest useful set of diagrams for each task. A suggestion
is a candidate for judgment, not a command to create that kind. Mandatory means
that the explanation question must be answered or explicitly suppressed. Size
alone does not justify drawing all eligible kinds.

| Kind | When to suggest it to the model | When review becomes mandatory | What the kind-specific prompt asks |
|---|---|---|---|
| `class` | A level of domain concepts has at least 4 children; attributes, associations, inheritance, or composition make the structure worth explaining. | Structural question at 10 children, when no suitable explanation covers it. Component/package/composite may be better choices depending on the facts. | Explain the concepts, relevant attributes, and selected relationships at one abstraction level. Do not use class notation to pretend every actor or service is a data class. |
| `component` | Any stated system needs a boundary/context review; internal levels with services, interfaces, or components also qualify for structural review. | System boundary question at 8 children/interactors, or internal structural question at 10 children. Explicit context requests require context, not merely an internal component picture. | For context, show the subject system and external actors/systems. For container/component, show the stated internal units and their responsibilities/interfaces. Keep perspective explicit and link detail. |
| `package` | At least two existing containment groups have children and relationships crossing their boundaries. | No separate quota. It can answer a mandatory structural question if package boundaries are the question. | Show existing group boundaries and cross-group dependencies. Do not invent architectural layers to make packages look tidy. |
| `composite` | One stated whole has at least two parts with connectors/interactions among them or across its boundary. | No separate quota. It can answer mandatory internal-structure review when connected parts are the issue. | Draw one boundary, its parts, and sourced connectors. Distinguish presentation groups from real containment. |
| `use-case` | An actor participates in a behavior cluster of at least 4 statements, or the source explicitly asks for an actor-goal overview. | The behavior review is mandatory at 12 statements. Use-case is suitable only for actor-goal scope, not sufficient for a required process or interaction question. | Identify the externally meaningful intent and actors from statements. Never turn each sentence into a separate user goal or draw an internal subsystem as an invented actor. |
| `activity` | A behavior cluster has at least 4 statements; sourced guards, triggers, or competing outgoing transitions particularly suggest paths and decisions. | Behavior review at 12 statements; for a process-path finding this is the preferred kind. | Show actions, sourced ordering, alternatives, and recovery. Source every guard; do not invent a branch, join, or concurrency because the picture looks better. |
| `state` | A machine has at least 3 distinct states. A smaller machine can be drawn when explicitly requested. | Lifecycle review at 8 states. A sequence diagram cannot replace it. | Explain one subject's states and allowed transitions, using its actual machine. Cite triggering requirements; expose missing information rather than invent transitions. |
| `sequence` | At least 4 behavior statements involve at least 2 message participants. Source order, causal language, and message edges suggest the candidate. | Behavior review at 12 statements. For a message-order finding, use sequence or a justified communication alternative. | Select one scenario, identify initiator/receiver per step, and source message order and fragments. Add failure paths only where stated. Do not reproduce every statement as a message. |
| `communication` | At least 3 participants interact and understanding their connections is more useful than vertical chronology. | No independent quota. It can answer mandatory interaction review when topology plus numbered order answers the question. | Show who communicates with whom and number the interactions. Prefer sequence when temporal order or nested alternatives dominate; avoid redundant twins. |
| `object` | A type has at least 2 known instances, especially instances with values or links worth comparing. | Instance-example review at 8 instances, with suppression appropriate if the examples are repetitive. | Choose a meaningful concrete snapshot, show instance/type identity and known values/links. Never invent example values or turn types into instances. |
| `deployment` | Any valued placement fact exists in an area. | Placement review at 6 placement facts. A component diagram without placements does not answer it. | Show only artifacts and placements stated by the graph. Do not assume hosts, replicas, networks, or cloud services from conventional architecture. |
| `timing` | A stateful subject also has bounds in criteria that might express time constraints. The model must read their source and confirm temporal relevance. | No numeric rule forces timing solely from the number of criteria. An explicit timing-diagram request or stated diagram obligation requires review. | Use actual state changes and stated durations, deadlines, or measures. If no meaningful time scale is stated, explain the missing facts and suppress rather than guessing a trace. |
| `overview` | At least 3 authored interaction diagrams explain the same area and could be related phases of one scenario. | No automatic count-based requirement. An explicit scenario-map request requires review; a mandatory readability task can require a linked coarse explanation, with overview as one option. | Verify phases and their order from sources, then link the existing interactions. Do not invent a workflow merely because three diagrams share a parent or title. |

The positive signals for package/composite/communication/timing/overview can
open optional review below the family size threshold. Fold them into the same
area task as other findings. Use stable question keys such as `time-constraints`
and `interaction-map`; put their presence/count tests in `limits.rs` alongside
the family limits. Do not introduce one model call per candidate type. False
positives are expected: a criterion bound can concern throughput rather than time,
and three interactions can be unrelated. The model reads evidence and suppresses
that suggestion. Do not add undocumented keywords, regex classifications, or a
hidden model call to the static analyzer.

For behavior clusters, static evidence exposes supported questions: an actor
suggests actor-goal scope, directed edges suggest interaction, guards and transitions suggest process paths. Generic large clusters get a behavior
review without a forced notation. A session may produce both activity and sequence
when they explain distinct questions; it states both purposes and evidence.
The harness never requires all alternatives and the model cannot replace a
specific question with an easier unrelated one.

Explicit human requests for a kind are instructions to execute within their
scope, even below automatic thresholds. A source diagram or a quoted requirement
that explicitly requires a diagram is not waived by an ordinary heuristic
suppression. Fulfill it, or report missing prerequisites/unsupported requirements.
Human reversal of that instruction must be explicit. Extracted diagram
obligations must be represented through the normal source/requirement machinery,
not detected by scanning arbitrary prose during diagram analysis.

## Coverage analysis

Run analysis after relationship and state-machine calculation and query expansion,
at commit and again when preparing work. Present its work as the named
**Review and author diagrams** compilation step, not as a GC burst. Loading an
entity remains read-only and never starts this step.

Analysis produces findings and counts only. Use immutable graph IDs for area identity,
never a title, name, generated view ID, or document basename.

An area key is a structured `{family, target, document?}` encoded reversibly for
`g:diagram-coverage:<area-key>`. Families separate independent questions about the
same target. One goal folds all findings for that area, including diagram drift;
its mandatory flag is the maximum severity of its unsuppressed findings.
Each finding has a stable question code and its own eligible diagram kinds;
counting the same facts in an unrelated kind cannot answer the question.
No synthetic entity is stored for an area.

Thresholds are initial product defaults, not benchmark-derived values. Put named
constants in `limits.rs` and document them in the registry. A count must be
strictly greater than soft or hard, matching existing limit semantics.

| Family | Deterministic candidates and count | Soft | Hard | Eligible authored kinds |
|---|---|---:|---:|---|
| `structure` | Direct children of an entity or scope root, excluding instances | 3 | 9 | class, component, package, composite |
| `system` | A `system` entity; count is max(1, direct children plus distinct external interactors) | 0 | 7 | component with context/container/component perspective |
| `flow` | Requirements with dependency edges, transitions or scenario criteria reaching one containment level, clustered by source document | 3 | 11 | use-case, activity, sequence, communication, timing, overview |
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

A diagram matches by explicit area, question, and compatible kind, never by title.
Use-case diagrams can cover actor-goal scope, but cannot close a message-order
finding. Activity diagrams cover process paths; a simple message list cannot
claim branch coverage. Sequence and communication diagrams cover interactions;
state diagrams cover lifecycle; timing diagrams cover stated time constraints.
The family table gives the candidate catalog, not interchangeability between
all kinds in one row. Record covered question codes in goal evidence and the
computed diagram coverage summary; do not accept self-declared coverage alone.

 For
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
area containment identity, related authored interaction IDs/hashes for overview
suggestions, and applicable coverage rules. It excludes generation,
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

A heuristic suppression may remain valid across optional-to-mandatory escalation if growth
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

- Kind: `diagram-coverage`. Class: compile, in the new diagram-review step.
  Optional above soft; mandatory above hard. This is the explicit exception to
  the existing statement that all compile goals are mandatory.
- Unit: one area. ID: `g:diagram-coverage:<area-key>`.
- Created by: `diagram-undercovered` and `diagram-drift` findings from the analyzer.
- Ready after extraction, requirement/entity judgment, reference repair, and
  structural abstraction in the affected area settle. Do not wait for criteria,
  binding, generation, or verification that belong after this step. Include
  descendant entities, relevant requirements, source sections, and related
  diagrams in the affected area. Human ratification and answer work keep their
  existing explicit blocked state; do not make them an invisible global barrier.
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

## Session prompt assembly and worked example

Assemble three layers through the existing prompt parser and partial mechanism:

- A shared authoring contract: inspect before create, name the reader's question,
  reuse diagrams, load semantic sources, keep all facts sourced, and close each
  finding with actual saved work or a valid suppression. Explicitly distinguish
  completing a mandatory review from being forced to create a pointless diagram.
- The `diagrams` skill: the selection table above, UML/C4/4+1 question lenses,
  abstraction-level guidance, good/bad examples, and when text is clearer. Load
  kind-specific instructions only for relevant candidates or selected kinds.
- Runtime evidence: area and question IDs, why review opened, optional/mandatory
  status, counts and thresholds, statements/source handles, relationships and
  machine summaries, current diagram purposes/coverage, active suppression
  baselines, and the exact evidence schema for completion. These are data fields,
  not new authored instructions generated inside Rust.

Draft session instruction for the new compilation step:

```text
You are reviewing diagrams after the requirements and structure of this area
have been reviewed. Address each listed question. First inspect existing diagram
summaries and load any candidate for reuse. Read the relevant statements and
source sections before deciding what to draw. Choose the smallest set of diagrams
that answers the questions; the suggested kinds are alternatives unless the
request explicitly requires a kind. Give each diagram a reader's question as its
purpose and explain why its kind, members, ordering, and omissions serve it.

Use upsert_view or update_view to save definitions. Use sourced graph IDs for
members, relationships, notes, and fragments. Do not invent domain facts or raw
PlantUML. If a heuristic suggestion would produce no useful explanation, call
suppress_diagram_coverage with a concrete reason. Missing facts needed to fulfill
an explicit requirement must be reported, not silently suppressed.

Claim completion only after each question has eligible saved diagrams with
verified coverage, or a valid suppression where permitted. Cite the saved IDs.
If you cannot complete the authorized work, fail it with the specific reason.
```

Worked runtime evidence, shown separately from the shared instructions:

```text
Area: ent:checkout, document docs/checkout.md
Question: How do the participants complete checkout and recover from failure?
Review: mandatory, 14 behavior statements (mandatory above 11)
Evidence: 4 message participants, 2 guarded transitions
Candidates: activity for paths; sequence for messages and order
Existing: view:usecase/checkout, purpose "What can a customer do?"
Coverage: actor-goal scope covered; paths and interactions not yet covered
Sources: requirement IDs and section handles supplied below
```

Here the model should inspect the use-case view and its sources, then choose an
activity diagram, a sequence diagram, or both for distinct needs. Creating another
use-case view does not close the missing explanation. If sources do not state
recovery order, the model reports that gap or omits unsupported order with a
specific disposition; it never invents a retry loop to complete a pretty picture.

Rechecks verify eligible kinds, question-specific evidence and coverage, live
references, saved mutations, provenance, sourced fragments, and suppression
permission/current baseline. Mark each finding as `heuristic` or `obligation`,
with source/requirement IDs for obligations, and reject ordinary suppression of
the latter. The program, not the model, determines that classification from the
recorded request or source-backed requirement. They do not pretend to prove visual communication
quality. The model's justification and human inspection judge whether the picture
is useful. Fixture replays include both good and poor choices to test guidance
and gate boundaries. No new paid sessions are authorized by this plan.

## Existing goals and checks

- Curate-view keeps only query matches on existing authored structural diagrams.
  No flow-placement change record or classification-based placement check exists.
- Interaction coverage uses dependency edges, transitions and existing scenario
  criteria within an area. Missing diagrams and unplaced statements are not
  placement errors. A reasoned suppression waives only the area's diagram advice.
- Keep semantic checks, including inconsistent transitions. Concrete bounds and
  test shapes live in criteria, without requirement classifications.
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

Portable API contract for the editor:

- `GET /api/views` pages authored definitions with effective member counts, edge
  counts, intent, provenance and limit state. `GET /api/views/{id}` also resolves
  drawing data, drill-down links and renderer status. The editor fetches every
  page independently for each document set.
- `POST /api/diagrams/edit` accepts one human view or suppression operation.
  It reuses gated atomic graph edits, with no model session. The editor sends
  changed fields only, so unrelated concurrent field edits remain intact.
  Human creates and updates carry an explicit decision note.
- `clear_excluded: true` clears prior exclusions before applying `exclude`.
  Empty collections clear authored presentation fields. `query: null` removes
  automatic membership selection.
- `GET /api/diagram-coverage` exposes current findings above their soft threshold,
  their measures and suggested kinds, current area hashes, covered or suppressed
  status, and mandatory status. `GET /api/diagram-suppressions` includes active
  and expired decisions. Human suppression uses the current area hash and reason.
- Native, WASM and hosted transports share these operations. Cloud owns the
  gallery, forms, member ordering, presentation controls, confirmation, and
  suppression management. Engine builds and tests remain independent of Cloud.
- The coordinated release verifies the engine and Cloud revisions, builds the
  native and WASM artifacts, and publishes the API and editor together.

## Migration and persistence

Set `STORE_VERSION = 7`, `UPGRADABLE_VERSION = 6`. Upgrade at the next writable
build opening, preserving the current read-only behavior before upgrade. Never
run the migration on the dogfood to test it.

- Untouched defaults live in version 6's derived/default-view projection. Discard
  them and their stale rendering outputs. Do not promote them into graph/views.
- Preserve all curated views, including views converted by a previous edit to a
  default. Preserve provenance, identity, membership, query, exclusions, collapse,
  and limit bumps. Missing provenance receives derived migration provenance citing
  surviving members and stating preservation of a curated version-6 view.
- Fill missing purpose with the existing title as a visibly migration-supplied
  intent. Infer an area only when version-6 level association or query.parent is
  unambiguous. Otherwise choose the members' common scope with extent selection;
  use the first subject for state/timing. Never guess a whole-area coverage claim.
- Migrate explicit split links where existing exclusions or containment identify
  them unambiguously. Leave ambiguous phase linkage for authored repair, surfaced
  through diagnostics; do not infer links from matching titles alone.
- Version 7 writes no `default` field or default-view files. Rebuild derived
  relationships, machines, query membership, and coverage findings after upgrade.
  Do not run a model during migration. Opening a store can therefore expose new
  goals without creating diagrams.
- Keep atomic persistence and failure recovery. A failed upgrade leaves version 6
  authoritative; retry is deterministic. A second successful opening is a no-op.
- Version 5 and older archive/rebuild under the existing one-previous-version
  policy. Newer stores stay read-only and are never archived. Document this
  consequence explicitly because only version 6 upgrades to version 7.

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
   Add the named diagram-review step, tier-3/tier-4 scheduling, local downstream
   gating, and explicit optional-review exception to compile-goal policy.
6. Extend render scene/emitter support for labels, notes, groups, selection,
   direction, and fragments across the catalog. Keep inert text and deterministic
   output; preserve size accounting and explicit omission visibility.
7. Extend `context.rs`, entity reads, context packing, reverse association indexing,
   members-axis handles, and context/session specifications as defined above.
   Implement version-6 migration, store round trips, output pruning, docsgen/card/
   viewer/LSP updates, webapi contracts, and native adapters. Version bump lands
   with a working migration and all changed readers, never as a standalone release.
8. Update fixtures and recorded tool sessions, run validation, commit coherent
   work, merge locally with `git merge --no-ff`, and push master. Use local-only
   work branches. Hand verified engine and Cloud revisions to the coordinated
   release owner for API, editor and schema publication. Do not run paid model
   sessions without per-action approval.

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
- Scheduling: diagrams wait for extraction/judgment/structural review, never for
  downstream criteria or generation; mandatory review holds its area, optional
  review cannot deadlock implementation, and unrelated areas proceed. Exercise
  explicit obligations versus heuristic suppression, mixed batches, and reopen
  after upstream edits. Show the named step in status and trace output.
- Analyzer: table-driven tests at soft, soft+1, hard, hard+1 for every family;
  scope roots, deep uneven containment, grouping-only flows, document-path
  collisions, internal flows without diagrams, instances excluded from structure,
  and zero/singleton areas. Assert no analysis mutation and stable IDs/order.
- Coverage: unrelated kinds/areas and one empty diagram do not satisfy coverage;
  partitioned diagrams do; collapsed descendants count correctly; 80% boundary,
  selected relationships, missing phases, and exact drift boundaries behave as
  documented. Focused selection diagrams do not spuriously drift.
  Actor-goal coverage cannot close message-order/path work; type suggestions are
  alternatives, explicit requests honor the named kind, and timing/overview
  opportunities do not invent temporal facts or phase order.
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
  expose interaction evidence without placement diagnostics; suppression has bounded effect. Hard fan-out and
  semantic transition/containment failures still block as before.
- Rendering: snapshot every kind with applicable intent fields, stable aliases,
  order, labels, group and arrow selection, sequence fragments, inert malicious
  text, over-limit notes, render failure cleanup, and deleted-view file pruning.
  Use the in-process renderer locally; official native renderer parity only when
  available, with no unsolicited download or hosted validation dispatch.
- Migration: a real version-6-shaped disk fixture with untouched defaults plus
  curated views, including a curated former default. Assert defaults disappear,
  IDs/provenance and edits survive, no models run, failed-write recovery, second
  opening no-op, older archive, newer read-only, and suppression round trips.
- Update `bootstrap/example/e2e-gc` scripted expectations to explicitly create
  diagrams through tool sessions. Cover coverage → author or suppress → split/
  curate/retrace → rebuild. Keep f1, f2, and relevant example-* traps; replace only
  assertions that specifically promise automatic defaults.
- Context: entity reads and loads expose direct, flow, subject, collapsed, own-area,
  and parent-context associations with truthful reasons. Cover exclusions, query
  matches, deduplication, stable ordering, zero/large lists, members-axis paging,
  full load on demand, explicit depth-2 behavior, reload after compaction, staged
  notes, budget ceilings, and native/WASM parity. Read-only loading exposes no
  mutation tools; mixed-batch slices, source-diagram ingest, skill loading, cached
  wrong-toolset calls, and coverage-only suppression authority have registry tests.
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
version-6 fixture migration, explicit tool-session coverage for every catalog
kind, integration into local master, and verified remote master SHA. Report
local validation, push, artifact publication, and deployment separately. Publication uses the coordinated release handoff after both repositories pass
validation.

Implementation choices remaining are mechanical: exact Rust module factoring,
HTTP route spelling within the existing dispatcher, and support of optional
layout hints by each emitter. Unsupported hints must be rejected explicitly;
required catalog tooling must not be deferred. The thresholds above are concrete
initial defaults to revisit with fixture evidence, not unresolved parameters.
Cloud UI adaptation is included and validated against the shared portable API.
