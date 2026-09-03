# Plan: levels

Status: in progress on the `levels` branch (branched from `uml`). Landed: stage 1
(docs), stage 2 (harness), stage 3 (tools, level views, lifted flows, drill-down
links), stage 4 (docsgen, GUI, viewer), stage 5 (the abstraction skill and the
fan-out contract, rewritten with the docs stage and embedded by the release build),
and stage 6 (`example-saas` with ten traps). Running: stage 7, validation, by the
owner's short-loop method (2026-09-02): never converge a whole corpus and grade hours
later; snapshot a real project at one moment and run one goal's session from it
with `jazyk benchmark --project <dir> --goal <id> [--force]`, read the transcript,
fix the docs and the harness, rerun, then move to the next interaction in the chain.
Done so far on a three-document mini project (twelve root entities): the root
fan-out resolves in seven rounds to the groupings the documents name (Orders,
Shipping, Platform) and rejects the coupling candidate that mixed two areas; the
review of a fresh grouping judges right in four rounds. The loops fixed: the snippet
toolset for GC goals, skill auto-load inside a batch, a move alone reopening every
child's review, the root view flipping to class under groupings of components, bare
document reads, stereotypes on component members, resolutions with nothing staged.
The chain then ran through a namesake collision (a front door naming Checkout beside
a stated Checkout process: the node is the area, its document's entities move under
it, stated once) and the node-form fan-out (eleven leaves under Checkout from one
flat document: Pricing and Funds groupings in six rounds once the doctrine said a flat
glossary is the common case and the candidates came in halves). Fixed along the way:
parked entries never cleared on resolution, reachability blind to containment, the
tautological whole-level candidate. Next in the chain: the view GC kinds the chain
opened (`split-view`, `curate-view`), the review of a grouping's ratification,
`example-saas` traps one interaction at a time, then `f2` and `example-org`
regressions and the dogfood, all as snippets. Written for a fresh session
with no prior context: read this file first, then work the stages. The owner's ask, verbatim in
spirit: requirements stay per entity as they are today; entities summarize into
higher-level entities, and those into higher still; each level gets its own
diagrams (structure, use cases, sequences) describing the relationships among that
level's entities; digging into an entity shows the level below it; a level that grows
too large or shrinks too small draws garbage collection to regroup, split, or dissolve;
and the harness guides the model with optional and mandatory goals until the graph
converges on a navigable architecture of the project.

## The owner's picture

The first diagram shows three boxes: User, Frontend, Backend, with the use cases and
sequences among them (signup, create data). Digging into Backend shows the server,
the database, the queue, the cache. Digging into the server shows its modules (a
model, view, controller split is an example, not a requirement). Digging into the
model shows the classes behind the database tables. Every level carries several
diagrams. Nothing here is a fixed depth: the tree is as deep as the documents and
the model's judgment make it.

## What already exists

This is the design's missing top half, not a change of strategy. Most of the
machinery stands:

- The `parent` field on entities is one containment tree
  ([entity](../docs/compiler/model/entity.md)). It is the hierarchy.
- Rendering lifts a relationship that touches a hidden descendant to the nearest
  shown ancestor and collapses groups to one arrow per direction and type
  ([lifting and collapse](../docs/compiler/diagrams.md#lifting-and-collapse)). A
  three-box top diagram over two hundred leaf entities is a lifting problem, and it
  is solved.
- Views store members, a query (`scope`, `parent`, `stereotype`, `depth`),
  exclusions, and collapse lists, never looks
  ([view](../docs/compiler/model/view.md)).
- Derived provenance (`from` plus reasoning) with a ratification proposal toward
  prose ([provenance](../docs/compiler/model.md)): a grouping no document states is
  a derived entity, and the compiler proposes the architecture chapter the
  documents never wrote.
- Limits carry soft and hard thresholds; soft derives an optional goal, hard
  escalates it to mandatory; per-node bumps are decrees
  ([escalation](../docs/compiler/reconciler.md#escalation)).
- A GC goal is ready only when no compile goal is open in its cone
  ([cones](../docs/compiler/reconciler.md#cones)): leaves reconcile first, groupings
  form cone by cone, each new level's views derive after. Bottom-up convergence
  needs no new scheduler.
- Default views derive per scope and per containment root, flows cluster by actor
  ([defaults](../docs/compiler/model/view.md#defaults)).
- `abstract-entity` exists as the GC goal that creates an abstraction when an
  entity crosses its caps ([abstract-entity](../docs/compiler/goals/abstract-entity.md)).

What is missing is the upward pressure: today abstraction is reactive to one
node's caps, never a drive to build the pyramid, and views exist per scope and per
root, never per level with lifted flows and drill-down links.

## Locked decisions

- A level is a node's children, never a global horizontal slice. The tree is the
  structure; uneven depths are fine. The scope root (the parentless entities of a
  scope) is the top level.
- A grouping is an authored entity with a minted id and derived provenance
  (`from`: its members, plus reasoning), never derived data recomputed at commit.
  It persists across rebuilds; only a crossed limit reopens it. Its ratification
  proposal phrases it as prose for the document that owns its parent (or the front
  door for a top-level grouping).
- No new stereotype for groupings. The model picks a stereotype from the existing
  vocabulary (`system`, `component`, `module`, and so on) or none.
- The harness computes coupling; the model names and judges. Coupling hints are
  deterministic partitions over the derived relationship graph. The model may
  accept, adjust with reasons, or decline with a reason. The harness never names a
  grouping; the model never counts.
- Higher levels carry no requirements of their own in this landing. Lifting covers
  edges and flows. A grouping gets a definition (one sentence stating its
  responsibility) and its ratification proposal. Derived summary statements stay
  out until dogfooding shows a diagram that needs one.
- A grouping never crosses levels: its members share one current parent, and it
  takes that parent.
- A grouping with fewer than two children dissolves in the deterministic sweep
  (children reparent to the grandparent, a tombstone redirect stays). Below two
  there is nothing to judge.
- Level views include the outside entities whose lifted edges touch the level (the
  top diagram shows User beside Frontend and Backend). This settles the open
  question from the `uml` landing about component view membership.
- Limits stay in the registry (`limits.rs`), never in `jazyk.toml`. The new rows and
  their values below are proposals for the owner to tune after dogfooding.

## The design

### Levels and groupings

A node's level is its set of direct children. The top level of a scope is its
parentless entities, addressed as `scope:<scope>` where a goal or view needs a
target for it (`scope:public` for the default scope). A grouping is an entity that
exists to hold a level: derived provenance from its members, a definition, a
parent (the members' former parent), and no mentions. An entity the documents
state (quote provenance) can hold children too; it is a grouping in role, not in
provenance, and the dissolve rule never touches it.

### Limits

New registry rows (`limits.rs`, mirrored in the limits table of
[graph.md](../docs/compiler/graph.md#limits)):

- `children-per-entity`: direct children of one node, soft 9, hard 15. The scope
  root counts its parentless entities under the same row. Crossing soft derives an
  optional `abstract-entity` goal on the node (or the scope) with the `fan-out`
  change; hard escalates it to mandatory. Dropping back under soft clears the
  record without a session.
- Minimum membership is not a limit row: a derived grouping with fewer than two
  children dissolves in the sweep.
- `members-per-structural-view` and `edges-per-view` stay as they are and bound
  every level view; an over-limit level view renders with its largest subtrees
  auto-collapsed as today.

### The fan-out goal

`abstract-entity` gains a second change variant beside the existing caps variant:

- `change`: `{fan_out: n, limit: {soft, hard}, candidates: [[id, ...], ...]}` where
  `candidates` are the coupling partitions the hint computer proposes.
- Target: the node whose children exceed the limit, or `scope:<scope>` for the top
  level. The cone is the node's subtree (the whole scope for the root form).
- Ready when no compile goal is open in the cone (the existing GC rule).
- Hints: the fan-out count, the candidate partitions with their cohesion scores,
  the members' stereotypes, the document each member is mentioned in most
  (documents and headings are strong naming hints), and any existing grouping
  under the node.
- Gate: after the session, the node's direct children count is at or under the
  hard threshold (mandatory) or the soft threshold (optional); every grouping the
  session created has at least two children, a definition, derived provenance
  naming exactly its members, and the node as its parent; no member changed
  scope; or the goal is failed with a reason (`mark_goal_failed`) that names why
  the level is genuinely flat. A session may also lower fan-out by moving a child
  under an existing sibling that already contains it conceptually, with a reason.
- Justification: one or two sentences naming the groupings and why the domain
  would recognize them.

### Coupling hints

The hint computer (`goals.rs`, the `abstract-entity` kind) works over the target's
direct children:

- weight(a, b) = the number of requirements referencing both a and b (descendants
  included, lifted to a and b) plus the number of derived relationships between
  them (lifted the same way).
- Greedy agglomeration: start with singletons, repeatedly merge the pair of
  clusters with the highest total weight between them, stop when the cluster
  count is at or under the soft threshold and every cluster has at least two
  members or is a singleton that no other cluster touches. Ties break by id.
- Output: each cluster as an ordered id list with its internal weight, largest
  first, capped at the soft threshold of clusters and at 12 ids per cluster (the
  rest summarized as a count).
- Deterministic, so a re-derivation yields the same candidates for the same graph.

### Level views

`derive.rs` derives, for every node with at least two children (the scope root
included):

- One structural level view. Kind: `component` when the node or any child carries a
  structural stereotype (`system`, `component`, `service`, `interface`, `actor`),
  `class` otherwise. Id `view:component/<slug>` or `view:class/<slug>` with the
  node's slug (`view:component/public` for the root form). Members: the
  direct children plus every outside entity with a lifted edge into the level, in
  document order. `default: true`; any mutation naming the view clears the default
  as today. The existing per-scope class view and per-root component view fold
  into this rule (the scope root's level view is the per-scope view).
- Flow views per level: the behavior-facet requirements whose entities lift to at
  least one member of the level cluster by the lifted actor (or the lifted first
  entity) and document; a cluster of two or more derives a `use-case` and a
  `sequence` view whose participants are the lifted members, ids
  `view:usecase/<node-slug>-<cluster-slug>` and `view:sequence/...`. A leaf level
  keeps today's clustering unchanged (lifting to a leaf is identity).
- State and object views stay per machine and per instantiated type.

Lifting for flows is the renderer's lifting applied at derivation: the harness
maps each requirement's entities to their nearest ancestor in the level, drops the
requirement from a level none of its entities reach, and dedupes participants.

### Drill-down

Every rendered member whose entity has a level view links to it: PlantUML
`[[...]]` hyperlinks in the `.puml`, anchors in the `.svg` (relative paths under
`diagrams/`), a `children` list on the view in `get_view` and the GUI API. Docsgen
nests a page per level (the node's definition, its level views embedded, its
members with links down and a breadcrumb up). The GUI shows the containment tree
and a breadcrumb over the diagram panel. The terminal viewer prints the tree with
each node's view ids.

### Tools

Two write tools in the `graph` group, both gated at staging
([tools](../docs/compiler/tools.md)):

- `group_entities({name, definition, members, stereotype?, reasoning})`: stages
  one derived entity (provenance `derived` from `members`, the members' shared
  current parent as its parent, the members' scope) and reparents every member
  under it, as one changeset. Gates: at least two members; every member resolves;
  all members share one current parent (a grouping never crosses levels);
  `near-duplicate` against existing names as `upsert_entity`; `definition` and
  `reasoning` non-empty. Answers with the new id and the members moved.
- `dissolve_entity({id, reason})`: the inverse for a grouping with derived
  provenance and no mentions: children reparent to its parent, the entity
  tombstones with a redirect to its parent. Refused on an entity a document
  states (`stated-entity`: revise the documents instead).

`update_entity`'s `parent` stays the single-move path.

### The sweep

The deterministic GC sweep at commit gains one rule: a derived grouping (derived
provenance, no mentions) with fewer than two children dissolves as
`dissolve_entity` would, journaled as a sweep mutation. The flip detector watches
reparents: a child that moves between the same two parents across generations
parks the second move like a cross-class flip.

### Checks and convergence

A `level-shape` check joins the deterministic checks: every node with at least two
children has its structural level view; no node is over the hard fan-out
threshold; no derived grouping has fewer than two children. A mandatory fan-out
goal open or failed blocks `converged`, the existing rule for mandatory GC goals.
`jazyk status` gains a shape line: nodes per depth and the fan-out histogram.

### Skills and prompts

`skills/abstraction.md` is rewritten around groupings:

- A grouping is a concept a reader of the documents would recognize and name,
  never a coupling artifact. Document names and headings are the strongest naming
  hints (a `payment.md` suggests a Payments grouping); the model prefers a name the
  documents already use for the area.
- Boundaries follow the cohesion hints, and the model may split or merge a
  candidate with a reason (a candidate that mixes two responsibilities splits; two
  candidates the documents treat as one area merge).
- A split the documents do not suggest (a model, view, controller split) is a
  choice: state it as one and let the ratification proposal carry it to the owner.
- The definition is one sentence stating the grouping's responsibility, and the
  ratification proposal phrases the grouping as prose for the owning document.
- Declining is honest work: a level that is genuinely flat (nine peers with no
  cohesion) fails the goal with that reason rather than inventing tiers.

`prompts/abstract-entity.md` gains the fan-out contract paragraph. The judgment
skill gains one line: a grouping's name is judged like an entity name (search
before create; a lookalike of an existing area reuses it).

## Docs to change

| page | change |
| --- | --- |
| `docs/compiler/concepts/levels.md` (new) | levels, groupings, the scope root, naming doctrine, drill-down; linked from `model.md` and `main.md` |
| `docs/compiler/model/entity.md` | groupings as entities in role; derived provenance from members; the dissolve rule |
| `docs/compiler/model/view.md` | level views in the defaults; `children` on a view; flow views per level |
| `docs/compiler/graph.md` | the `children-per-entity` rows in the limits table; the sweep's dissolve rule; the reparent flip |
| `docs/compiler/reconciler.md` | the fan-out change record and goal derivation; coupling hints; the `scope:<scope>` target; the reparent flip under flip detection |
| `docs/compiler/goals/abstract-entity.md` | the fan-out variant: created when, gate, hints, what the model sees, tools |
| `docs/compiler/goals/prompts/abstract-entity.md` | the fan-out contract paragraph |
| `docs/compiler/skills/abstraction.md` | the grouping doctrine above |
| `docs/compiler/skills/judgment.md` | one line on grouping names |
| `docs/compiler/tools.md` | `group_entities`, `dissolve_entity`, `children` on `get_view` |
| `docs/compiler/diagrams.md` | level views, lifted flow views, drill-down links, output layout for links |
| `docs/compiler/compilation.md` | the `level-shape` check; the shape line in status |
| `docs/consumers/docsgen.md` | the page per level and its navigation |
| `docs/frontends/gui.md`, `viewer.md`, `cli.md` | tree, breadcrumb, shape line |
| `docs/compiler/tools.schema.yaml`, `graph.schema.yaml` | the two tools, the `children` field |

Every edit follows the docs style in the root `CLAUDE.md`: short declarative
sentences, no em dashes, backticks for identifiers, relative links with heading
anchors, statements extractable (docs is also the compiler's input).

## Code to change

| module | change |
| --- | --- |
| `limits.rs` | `CHILDREN_PER_ENTITY_SOFT` 9, `CHILDREN_PER_ENTITY_HARD` 15, the registry row |
| `goals.rs` | `abstract-entity`: fan-out derivation (nodes and `scope:<scope>`), the coupling hint computer, the fan-out gate, hints |
| `board.rs` | the `scope:<scope>` target form in cones and localities; the fan-out record in the change records; escalation for the new row |
| `store.rs` | the sweep dissolve rule; the reparent flip record; `Op::GroupEntities` and `Op::DissolveEntity` or their composition from existing ops |
| `tools.rs` | `group_entities`, `dissolve_entity`, gates, `children` on `get_view` |
| `derive.rs` | level views (structural per node, the root form), lifted flow clustering per level, `children` links |
| `render.rs` | hyperlinks on members with level views, in `.puml` and `.svg` |
| `reconcile.rs` | the `level-shape` check; the shape line; reparent flip parking |
| `docsgen.rs` | the page per level with navigation |
| `gui/` (axum + `gui/` frontend), `viewer.rs`, `cli.rs` | tree, breadcrumb, shape line |
| `session.rs`, `context.rs` | `scope:<scope>` as a loadable target (renders the top level as stubs) |

Tests to name: fan-out derivation at a node and at the scope root; coupling hints
deterministic and bounded; the fan-out gate accepting a valid grouping and
rejecting a one-member grouping, a cross-level grouping, and a stated-entity
dissolve; the sweep dissolve; level views deriving per node with the root form
folding the per-scope view; lifted flow clustering; hyperlinks in the emitters;
the `level-shape` check; the reparent flip.

## Stages

Each stage is one workflow: parallel agents with disjoint file ownership, a shared
brief (this plan plus `CLAUDE.md`), and an independent verifier (build, test, docs
checks) before commit. Commit per stage on the `levels` branch, docs first.

1. Docs: every page in the table above, one agent per page or page pair, then a
   verifier for links, anchors, em dashes, and agreement with this plan.
2. Harness: `limits.rs`, `goals.rs`, `board.rs`, `store.rs` (sweep and flip),
   `reconcile.rs` (check and shape line), with tests.
3. Tools and derived data: `tools.rs` (the two tools, `children`), `derive.rs`
   (level views, lifted flows), `render.rs` (links), with tests.
4. Frontends: `docsgen.rs`, GUI (API and frontend), `viewer.rs`, `cli.rs`; then
   `cd bootstrap/gui && npm run build` and `cargo build --release`.
5. Skills and prompts: `skills/abstraction.md`, `prompts/abstract-entity.md`, the
   judgment line, embedded by the release build.
6. Fixture: `example-saas`, a corpus written to the owner's picture (User,
   Frontend, Backend; server, database, queue, cache; modules; model classes)
   across fifteen or so documents, with an `EXPECTED.md` naming the level shapes,
   the groupings a good run mints, the diagrams per level, and the planted traps
   (a flat level that must not be tiered, a lookalike grouping name, an
   over-fan-out level that must split).
7. Validation: `f1` and `f2` and `example-org` converge unchanged in verdict and
   gain level views; `example-saas` converges into its pyramid; the dogfood
   converges into a navigable architecture of the compiler; the prompt loop grades
   level shape (fan-out bands, naming, drill-down completeness) and tunes
   `abstraction.md`.

## Definition of done

- `cargo test` green with the named tests above.
- `f1`, `f2`, `example-org` converge with unchanged trap verdicts and gain level
  views; an immediately repeated compile derives zero goals.
- `example-saas` converges into the pyramid its `EXPECTED.md` names, every level
  view renders with drill-down links, docsgen nests the levels, the GUI shows the
  tree.
- The dogfood converges with a top-level view of the compiler under ten members
  and every node with children navigable.
- Docs and binary agree; committed and pushed on `levels`; merged `--no-ff` after
  the owner's review.

## Open questions for the owner

- The fan-out values: soft 9, hard 15 proposed.
- Whether groupings may carry derived summary requirements later (this landing
  says no).
- The kind rule for level views (component when a structural stereotype is
  present, class otherwise) and whether a `package` view should serve module
  levels instead.
- Whether the top level should default to a `component` view even when no
  stereotype is present (the owner's picture suggests yes).
