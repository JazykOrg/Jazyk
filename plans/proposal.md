# The jazyk proposal

Status: proposal for iteration. This document is the whole design.

Jazyk compiles prose documentation into a persistent semantic graph and consumes
the graph downstream. This proposal extends the graph from one semantic layer
(entities and requirements) to the whole engineering process: requirements, use
cases, a domain model, worked examples, components and contracts, lifecycles and
interactions, and verification, all in one graph. Every UML diagram type renders
from it. One generic agent brings it to convergence by resolving goals the
harness derives. Every fact in the graph answers to a sentence in the documents,
and every change replays as a causal chain a human can inspect.

## Doctrine

The invariants everything below obeys:

- The documents are the source of truth. Anything the deliverable needs that the
  documents do not state is an ambiguity: resolved with best judgment, recorded,
  and pushed back toward the documents ([ambiguity](#ambiguity)).
- One persistent graph per project, edited in place, never regenerated. Ids are
  minted once and immutable; natural keys make retries harmless; merges leave
  redirects. Nothing enters the graph without provenance.
- Division of labor is strict. The harness owns everything that must never be
  wrong: parsing, identity, dirtiness, goal derivation, scheduling, validation
  gates, derived data, budgets, causality. The model owns everything that needs
  judgment: extraction, same-vs-different, severity, wording, abstraction.
- Diagrams are projections. There are no diagram elements, only graph nodes and
  view definitions; a rendering can never drift from the graph because it is
  recomputed from it.
- Stages order dependencies for the scheduler; they are never authoring
  phase-gates. Any document edit at any time dirties whatever its sentences
  anchor, at every stage, and reconciliation flows through the trace edges.
- Incrementality: a no-op rebuild derives zero goals and makes zero LLM calls. A
  change reaches exactly its cone, never the pyramid.
- The design assumes a capable model. Navigating a graph, managing loaded
  context, and working a goal board are agentic behaviors; small local models
  are not the target executor. The harness holds regardless: gates bounce bad
  calls, and junk never lands.

## The graph

### Node kinds

| id | kind | stage | natural key | one per |
|---|---|---|---|---|
| `ent:<slug>` | entity | 1 | name + scope | concept |
| `req:<doc-stem>-<n>` | requirement | 1 | source section + statement | statement |
| `rel:<a>~<b>` | relationship | 1 (derived) | member pair | entity pair |
| `uc:<slug>` | use case | 2 | goal + actors | actor-goal |
| `inst:<slug>` | instance | 4 | name + `of` entity | concrete example |
| `comp:<slug>` | component | 5 | name | part |
| `iface:<slug>` | interface | 5 | owner + name | contract |
| `adr:<n>` | decision | 5 | normalized title | decision |
| `sm:<entity-slug>` | state machine | 6 | subject entity | stateful entity |
| `ixn:<uc-slug>` | interaction | 6 | subject use case | multi-part use case |
| `view:<kind>/<slug>` | view | any | kind + title | rendered diagram |
| `diag:<rule>-<n>` | diagnostic | any | rule + subjects | finding |

Sections stay the structural kind beneath all of this: a tree per document,
carrying no meaning of their own, holding the verbatim text every quote locates
in. Every semantic kind takes an optional `stereotype` from the
[profile](#profiles) vocabulary, and the standard `confidence`, `reasoning`,
`created`, and `updated` fields.

### Provenance

Every semantic fact carries exactly one provenance:

- `quote`: extracted from prose. `{doc, section, quote}`, the quote verbatim,
  located whitespace-insensitively. Dies when its section text changes (the
  stale-anchor machinery).
- `derived`: synthesized by the compiler from upstream IR.
  `{from: [node ids], reasoning}`. A derived fact is invented until ratified:
  docsgen proposes the sentence the docs should gain, the owner accepts, the
  next reconcile flips it to `quote`. It dies when an upstream node dies or
  changes; dirtiness re-derives it.
- `decree`: authored by a human directly on the graph (a diagram edit with no
  prose behind it). `{author, at, note?}`. A decree outranks derivation (the
  compiler must not undo it) but not the documents: prose that contradicts a
  decree draws a `contradiction` diagnostic, and ratification proposals stand
  until the decree is written into the docs or retracted.

The invariant: the graph never holds a fact that cannot say where it came from,
and ratification pressure pushes every fact toward `quote`. The documents remain
the single source of truth.

### Entity

A domain concept, one node per concept, with one living `definition`.

- `name`, `aliases`, `definition`, `mentions` (each with a verbatim quote).
- `scope`: bounded context. Same-named entities in different scopes are
  deliberately distinct.
- `parent`: the containing node, an entity or a component. One containment
  tree, unlimited depth, crossing kinds: a database contains its tables, a
  microservice its modules, a module its classes (`comp:order-service`
  contains `comp:checkout-module` contains `ent:checkout-session`). See
  [containment and lifting](#containment-and-lifting).
- `role`: `aggregate-root`, `value`, `actor`, `service`, or unset.
- `attributes`: `{name, type?, provenance}` where prose states structure ("an
  order carries a total and a currency"). Behavior is never an attribute; it is
  a requirement.

### Requirement

One atomic obligation, free-form. The model writes the statement in whatever
wording carries the obligation best; extraction guidance points at clarity in
the EARS tradition (specific, testable, entity-anchored) without prescribing a
syntax.

- `statement`: the free-form text.
- `entities`: the entity ids the statement is about, at least one. Multi-entity
  statements are encouraged: a statement tying two concepts is what makes the
  graph a graph.
- `edges`: `{a, b, type?, cardinality?}` pairs the statement ties together
  (`cardinality` from `1`, `0..1`, `1..*`, `*`; the more specific claim is the
  stronger one), directional: `{a, b}` reads a-acts-on-b ("A calls B to list directory
  content" declares `{a: ent:a, b: ent:b, type: dependency}`). A multi-entity
  requirement with no edges draws the `declare-edges` goal.
- `source`: `{doc, section, quote}`, the verbatim sentence.
- Facets (behavior, constraint, failure mode, quality attribute per ISO 25010)
  are model judgments recorded at extraction with reasoning, where downstream
  stages want them. One deterministic check rides on the quality facet: a
  quality requirement with no measurable bound ("shall be fast") draws a
  warning, because it cannot be verified.

### Relationship

A derived, typed edge between two entities. There is no write tool for it: on
every commit the store recomputes relationships from requirement `edges`, so an
edge cannot exist without a statement behind it. That property is what makes
every rendered arrow explainable.

- One node per unordered pair (`rel:<a>~<b>`, lexical), contributions recorded
  per direction: `a→b` and `b→a` each keep their own type, cardinality, and
  contributing requirements. "A calls B" and "B notifies A" are two arrows
  between one pair; promotion never merges across directions.
- `type` per direction, strongest across contributing edges:
  generalization → realization → composition → aggregation → association →
  dependency → reference. `cardinality` per member, strongest claim wins.
- The reading is medium-neutral: "a House is composed of Rooms" and "an Act is
  composed of Chapters" are the same `composition` edge with different quotes
  behind them.

### Use case

Who does what for which goal, lean: no ceremony fields.

```yaml
uc:customer-checks-out:
  name: Customer checks out
  actors: [ent:customer]
  goal: complete a purchase
  preconditions:
    - {text: cart is not empty, refines: [req:cart-2]}
  steps:
    - {n: 1, text: customer submits the cart, refines: [req:checkout-1]}
    - {n: 2, text: the system reserves stock, refines: [req:inventory-3]}
    - {n: 3, text: the system empties the cart, refines: [req:catalog-2]}
  extensions:
    - condition: payment is declined
      refines: [req:payment-4]
      steps: [{text: the order is held and the customer notified, refines: [req:payment-5]}]
  includes: [uc:reserve-stock]      # a shared sub-flow, rendered as «include»
  provenance: {derived: {from: [req:checkout-1, req:inventory-3, req:catalog-2], reasoning: ...}}
```

Every step refines at least one requirement; that is the gate. Under the
organization profile a use case is a business process; under the narrative
profile, a plot thread (actor `ent:heroine`, goal: expose the betrayal).

### Instance

Concrete examples, checked against the model: worked examples in the docs,
enumerated concrete things, test fixtures the ledger names.

```yaml
inst:gold-tier-cart:
  name: gold tier cart
  of: ent:shopping-cart
  values: {items: "3", currency: EUR}
  links:
    - {to: inst:ana-the-gold-customer, via: rel:customer~shopping-cart}
  provenance: {quote: {doc: docs/catalog.md, section: /catalog/examples, quote: "Ana, a gold customer, carries 3 items priced in EUR."}}
```

The payoff is the conformance check: `values` must name declared entity
attributes, `links` must respect relationship types and cardinality. An example
that contradicts the model is a documentation bug found deterministically.

### Component and interface

A component is a modular part with provided and required interfaces, nesting to
any depth for coarse-to-fine structure.

```yaml
comp:order-service:
  name: Order Service
  stereotype: service                # from the profile
  parent: comp:commerce-platform     # nesting, any depth
  responsibilities: owns order lifecycle and checkout
  technology: {value: Go, provenance: {quote: {doc: docs/arch.md, section: /arch/services, quote: "The order service is built with Go."}}}
  provides: [iface:order-service.checkout]
  requires: [iface:inventory.stock]
  deployedOn: {node: aws-us-east, provenance: {quote: ...}}   # optional facet
  satisfies:
    - {target: req:checkout-1, provenance: {derived: {from: [adr:2], reasoning: ...}}}
    - {target: uc:customer-checks-out, provenance: {...}}

iface:order-service.checkout:
  name: checkout
  owner: comp:order-service
  operations:
    - {name: submitOrder, inputs: [{name: cart, type: ent:shopping-cart}], outputs: [{name: order, type: ent:order}], errors: [payment-declined], satisfies: [req:checkout-1], provenance: {...}}
```

Operation inputs and outputs reference entities where they carry domain types:
one node, not a copy, joining the contract layer to the domain model. Under the
organization profile a component is a business unit and its provided interfaces
are the capabilities it offers other units.

### Decision

An architecture decision record, append and supersede, never rewrite.

```yaml
adr:2:
  title: Checkout is synchronous
  question: Does checkout confirm inline or via async callback?
  options: [...]
  decision: inline confirmation; the order service owns the whole flow
  status: accepted           # proposed | accepted | superseded {by}
  consequences: [payment latency bounds the checkout requirement]
  constrains: [comp:order-service, iface:order-service.checkout]
  provenance: {quote: ...}   # stated in docs: born accepted
                             # invented by the compiler: born proposed
```

A `proposed` decision is a question to the owner through the diagnostic prompt
machinery, rendered wherever prompts render. Decisions are not
software-specific: which department owns onboarding, whether the novel is told
in first person, one node each.

### State machine

One machine per entity that has a lifecycle, whole-node upserts.

```yaml
sm:order:
  subject: ent:order
  initial: placed
  states:
    - {name: placed, provenance: {...}}
    - {name: paid, provenance: {...}}
    - {name: held, provenance: {...}}
  transitions:
    - {from: placed, to: paid, trigger: payment succeeds, refines: [req:payment-2]}
    - {from: placed, to: held, trigger: payment declined, refines: [req:payment-4]}
```

Triggers are read from the requirements' statements by judgment. The lifecycle
reading is universal: an order, a hiring pipeline, a relationship arc (strangers
to rivals to lovers, each transition refining a plot requirement). Semantics are
flat statecharts; hierarchy only if real projects demand it.

### Interaction

The message exchange that realizes one use case.

```yaml
ixn:customer-checks-out:
  subject: uc:customer-checks-out
  participants: [ent:customer, comp:order-service, comp:inventory]
  messages:
    - {n: 1, step: 1, from: ent:customer, to: comp:order-service, operation: iface:order-service.checkout#submitOrder}
    - {n: 2, step: 2, from: comp:order-service, to: comp:inventory, operation: iface:inventory.stock#reserve}
```

Each message rides a use case step and names an interface operation; both must
exist. Where composition is off (the narrative profile's dialogue scenes),
participants are entities and messages refine requirements directly. The gates
keep sequence views, use cases, and contracts mutually consistent.

### View

The stored half of a diagram: what it includes, never how it looks.

```yaml
view:class/commerce-overview:
  kind: class            # any renderable kind from the catalog
  title: Commerce overview
  members: [ent:catalog, ent:shopping-cart, ent:order, comp:order-service]
  query: {scope: commerce, depth: 1}   # alternative or additional: membership by rule
  collapse: [ent:order]  # show as one node even though it has children
  provenance: {derived: {from: [...], reasoning: one view per scope by default}}
```

- Default views derive on every build (one class view per scope, one use case
  index per actor, one sequence view per interaction), so nothing must be
  curated to get diagrams. Curated views come from `curate-view` and
  `split-view` goals or from humans (a decree like any other).
- A view renders its members plus every edge among them, direct or
  [lifted](#containment-and-lifting). Views are how diagrams stay readable: a
  view has [size limits](#size-limits), and the way to satisfy them is
  membership, `collapse`, and sub-views, never silently omitting edges. Views
  nest by reference: a collapsed node links to the sub-view that details it, so
  one overview fans out into readable detail at every depth.
- A member that dies opens a `retrace` goal on the view. Query-based membership
  recomputes by itself and needs none.

### Diagnostic

A recorded judgment: a contradiction, an ambiguity, a conformance finding.
Sticky by construction (diagnostics are nodes), with severity, subjects,
`prompt` (a question to the owner with optional suggested edits), `answer`, and
human triage that the compiler never touches. Diagnostics record findings;
[goals](#goals) carry work. The two reference each other and nothing is
recorded twice.

### Edge algebra

Edges are stored on the downstream node (the one being written) pointing
upstream, and the store maintains derived reverse indexes on commit, the same
way requirement sources become entity mentions.

| edge | stored on | points to | kind |
|---|---|---|---|
| `parents` | section | section | structural |
| `parent` | entity, component | containing node | structural |
| `members` / `collapse` | view | any semantic nodes | structural |
| `mentions` | entity | section | provenance |
| `entities` / `edges` | requirement | entities | semantic |
| `members` | relationship | entities | derived |
| step / extension `refines` | use case | requirements | trace |
| `includes` | use case | use cases | trace |
| `of` | instance | entity | trace |
| `links` | instance | instances, via relationships | trace |
| `satisfies` | component | requirements, use cases | trace |
| `provides` / `requires` | component | interfaces | semantic |
| operation `satisfies` | interface | requirements | trace |
| transition `refines` | state machine | requirements | trace |
| message `step` + `operation` | interaction | use case step, interface op | trace |
| `constrains` | decision | any semantic node | trace |
| `verifies` | ledger row | requirement | trace |
| `subjects` | diagnostic | any node | meta |
| `from` | derived provenance | upstream nodes | provenance |

Trace edges carry a typed `kind` from the vocabulary the standards agree on
(SysML v2, ISO 29148, DO-178C): `refines` (same content, more detail),
`satisfies` (a design element answers a requirement), `verifies` (a test checks
a requirement), `derives` (a downstream fact justified by upstream nodes),
`constrains` (a decision or quality attribute limits an element).

The context engine walks four axes, each with a hop quota: `parents`,
`mentions`, `requirements`, and `traces` (both directions through the reverse
indexes, with an optional kind filter). Dirtiness propagation walks the same
indexes. The traceability matrix regulated industries maintain by hand is a
free projection over these edges plus the ledger.

### Containment and lifting

Containment is the structural answer to scale. Entities and components form
containment trees through `parent`, unlimited depth, and the trees cross
kinds: an entity lives inside the component that owns it, which is what lets
the composite-structure projection render a component's owned entities as
parts.

- Where the docs state the whole-part ("the database has an orders table"), the
  statement yields a `composition` relationship, and the domain goal sets
  `parent` to match; a `parent` contradicting a stated composition edge is a
  check failure.
- Where structure is invented to tame scale (an `abstract-entity` goal
  splitting a 60-requirement service into modules), the new parents and
  children carry `derived` provenance and ratify toward prose like every
  invented fact.

Lifting keeps coarse diagrams true without drawing every leaf. When a view
shows an ancestor and hides its descendants (by membership or `collapse`),
every edge touching a hidden descendant lifts to the nearest shown ancestor:

- `comp:a` contains `comp:a1`, `comp:a2`; `comp:b` contains `comp:b1`;
  `ent:a1-client` is parented under `comp:a1` and `ent:b1-listing` under
  `comp:b1`. A requirement ("A1 calls B1 to list directory content") declares
  `{a: ent:a1-client, b: ent:b1-listing, type: dependency}`.
- The system view shows only `comp:a` and `comp:b`: the renderer lifts the
  concrete edge to one `a depends-on b` arrow.
- The lifted arrow's justification is the set of concrete edges beneath it;
  clicking it lists them, and each walks to its requirement and quote. Lifting
  stores nothing: it is aggregation at render time over `parent` chains, so it
  can never drift. The lifted type is the strongest across the underlying
  edges, and the label carries the count past one ("3 dependencies").

### Size limits

Limits make readability a computed property, not taste. All configurable under
`[limits]`, joining the existing section and entity thresholds:

- per view kind: maximum members and rendered edges (on the order of 20 members
  for class and component views, 10 for use case indexes, participants and
  messages for sequence views),
- per entity: maximum requirements, maximum direct children,
- per state machine: maximum states before hierarchy is advised.

Every limit carries two thresholds. Crossing the soft one (getting big) opens
an optional goal (`split-view`, `abstract-entity`); crossing the hard one (too
big) makes that goal mandatory, so the build that tipped it also restructures
and cleanup debt cannot accumulate. Dismissing a size goal is a graph write,
not goal state: the node's own limit is raised, recorded with decree
provenance, and the goal simply stops deriving until the raised threshold is
crossed in turn. A violation never truncates a rendering silently: the diagram
renders meanwhile with collapse applied to the largest subtrees, marked as
such.

The documents have scalability limits of their own, on the human side of the
mirror: a sentence overrunning, a paragraph or section too big, a file too big.
Those surface as document-quality diagnostics (`section-too-large`,
`doc-too-large`, finer-grained prose lint), because prose is the human's to
restructure. The boundary is genuinely subjective: an oversized entity can mean
the graph should split the entity, or that the docs should split the section
feeding it. Both paths stay open, and where the line sits is a declared
experiment.

### Justification closure

The check that makes every diagram answerable. For every semantic node and
every trace edge: walking provenance and trace edges upward terminates in a
verbatim quote in a live section, or the fact is `derived`/`decree` with live
upstream nodes and an open ratification proposal. No orphan facts, no
unjustified edges, enforced in the deterministic checks (the DO-178C rule,
computed instead of audited).

The walks are also the GUI inspector's click paths and the LSP's hover targets:

- Class diagram edge `ent:catalog ~ ent:shopping-cart`, label `aggregation 1..*`
  → `rel:catalog~shopping-cart` → its contributing requirements (never empty)
  → each requirement's quote → section → line range in the source file.
- Sequence message `reserve` → `iface:inventory.stock#reserve` → the operation
  `satisfies` `req:inventory-3` → quote. The message also names step 2 of
  `uc:customer-checks-out`, which refines the same requirement: two paths, one
  sentence, and the checks verify they agree.
- Object diagram value `currency: EUR` → the `currency` attribute on
  `ent:shopping-cart` (its own quote) and the example sentence the instance was
  read from. A conformance failure names both sentences.
- Component box `comp:order-service` → its `satisfies` list → requirements and
  use cases → quotes; its `technology` value → its own quote; a constraining
  decision → its question and either its quote or its awaiting-answer state.

## Diagrams

### How a diagram is stored

A diagram is three layers, and only the first two are stored:

- The semantic facts: nodes and edges in the graph. The only editable truth,
  behind the gates, with provenance.
- The view: a `view:` node saying which facts one diagram includes.
- The rendering: a PlantUML file per view, written to
  `<out>/diagrams/<kind>/<view-slug>.puml` on every build, deterministically,
  the way docsgen pages are. With a PlantUML binary configured, the build also
  emits the rendered `.svg` beside it. These files are build output: diffable,
  viewable in any PlantUML tooling, never hand-edited, never read back.
  Deleting them loses nothing.

PlantUML is the renderer; its UML coverage is full and faithful across the
catalog, where Mermaid cannot render object, component, composite structure,
deployment, timing, or communication diagrams properly. The GUI renders its
interactive projections straight from the graph
and does not go through the `.puml` files. Geometry, layout, and styling are
never stored anywhere.

A PlantUML block inside a source document is the opposite thing: input. Parsing
treats it as a `diagram` section and extraction reads its obligations as prose.
Hand-written diagrams in docs are statements to compile; generated diagrams in
the out directory are projections of the result.

### Profiles

Jazyk does not assume the documents describe software. The node kinds are
medium-neutral: things, obligations, goals, examples, parts, contracts,
lifecycles, interactions. A profile, UML's own specialization mechanism adopted
as configuration, supplies the medium:

- a stereotype vocabulary, rendered guillemet-style («service», «character»,
  «department», «slide»), stored as the `stereotype` field with provenance like
  any fact,
- stage defaults: which stages earn their keep in this medium (the narrative
  profile turns composition off; the organization profile leans on activity
  views),
- rendering labels: what the projections call things (the class diagram is an
  org chart under the organization profile; a sequence diagram is a scene under
  the narrative one).

Built-in profiles: `software` (default), `organization`, `narrative`, `slides`.
A custom profile is a table in the project file. The graph, gates, and checks
are identical under every profile. The same ladder reads:

| stage | software | slide deck | company organization | romance novel |
|---|---|---|---|---|
| requirements | system obligations | content obligations per slide | policy and process rules | narrative obligations |
| use cases | actor goals | audience takeaways | business processes per actor | plot threads per character |
| domain | domain model | deck structure and elements | org chart, roles, capabilities | characters, settings, themes |
| instances | fixtures, worked examples | sample data on slides | named teams and offices | concrete scenes and events |
| composition | services and their contracts | (off by default) | business units and capabilities | (off by default) |
| dynamics | lifecycles, message flows | presentation flow | pipelines, cross-unit procedures | character arcs, dialogue scenes |
| verification | tests | render and content checks | audit checks | continuity checks |

### The UML catalog

All 14 UML 2.5 diagram types render from the graph. Six have stored semantics,
seven are projections of facts other stages store, one is a mechanism. None is
stored as a drawing.

Structure diagrams:

- Class: stored (stages 1 and 3). Entities with roles and attributes; edges
  from derived relationships with type and cardinality; one view per scope.
  Editing an edge edits the requirement behind it; drawing a new edge becomes
  an `add_requirement` dual write, the chat tool that inserts the sentence
  into the docs and stages the requirement in one changeset.
- Object: stored (stage 4). Instances with values and links, conformance
  checked against the class model.
- Package: projection. Scopes are the packages; the view groups the class
  projection by scope with dependencies summarized from cross-scope
  relationships.
- Component: stored (stage 5). Components, nesting, provided and required
  interfaces (lollipop and socket). Nesting depth carries the coarse-to-fine
  levels.
- Composite structure: projection over a component's internals: nested
  components and owned entities as parts, connectors from the interaction
  messages that cross them.
- Deployment: on-evidence. `deployedOn` facets and stated topology render as
  nodes and artifacts; nothing synthesizes topology. Under the organization
  profile: offices and locations.
- Profile: the mechanism above, not a picture. The project settings page is its
  honest form.

Behavior diagrams:

- Use case: stored (stage 2). Two renderings: the index (per actor, with steps
  and requirement links) and the classic oval diagram («include» from
  `includes`, «extend» from extensions).
- Activity: projection of one use case's scenario: steps as actions, extensions
  as decision branches, `includes` as sub-activity frames. Under the
  organization profile this is often the primary view (workflows); under the
  narrative profile, plot flow.
- State machine: stored (stage 6), for entities with lifecycles.
- Sequence: stored (stage 6), one per interaction.
- Communication: projection. The same interaction as a wiring view with
  numbered messages. One stored fact, two layouts.
- Timing: on-evidence projection. Renders for a state machine whose transitions
  refine requirements carrying time measures: lifelines of states against the
  stated bounds. No measures, no view.
- Interaction overview: projection. A use case's steps as an activity frame
  where each step that owns messages embeds a reference to its sequence view.

Adjacent notations: a C4-styled rendering of the component tree is a docsgen
style option (C4's one-model-many-views insight is this proposal's first
principle); ER is a style option on the class projection; BPMN is out of domain
as IR (a BPMN-shaped document is prose input like any other); statecharts are
the adopted `sm` semantics. Two boundaries hold beyond notation: the
KerML/SysML v2 metamodel is not adopted beyond the trace vocabulary (metamodel
maximalism, sparsely represented in LLM training data), and formal methods
(TLA+, Alloy) are not a stage, because LLM-generated TLA+ is fluent syntax
with wrong semantics; flat statecharts are the formal ceiling.

## The stage ladder

Stages in dependency order. Each names its unit of work, its trace edges, its
deterministic checks, and the projections it feeds. Stages are opt-in per
project with defaults from the profile; the shipped default is stage 1 alone.

### Stage 0: sections

Structural and deterministic: parse every matched document into a section tree
with per-section content hashes, align against the stored trees, compute the
dirty set. Unchanged from the current compiler.

### Stage 1: requirements and entities

The foundation, as implemented today, with these extensions: free-form
statements with judged facets, directional edges with cardinality, entity
attributes, roles, `parent`, and stereotypes, all as specified in
[the graph](#the-graph).

- Unit of work: one document.
- Checks: coverage (every section covered or marked non-normative with a note),
  reachability from the roots, flip detection, duplicate and contradiction
  judgment, the quality-measure warning.
- Projections fed: class (with stage 3), package.

### Stage 2: use cases

- Derivation: behavior-stating requirements cluster by actor and trigger
  tokens. The reconciler computes candidate clusters deterministically (shared
  actor entity, overlapping content tokens, the same machinery pair review uses
  for neighbors), so one session sees one bounded cluster, never the whole
  requirement set.
- Unit of work: one actor-goal cluster.
- Trace: each step `refines` its requirements; extensions `refine`
  failure-mode requirements.
- Checks:
  - every step references at least one existing requirement,
  - every behavior-stating requirement is refined by at least one step, or
    carries a per-stage coverage mark saying why not (each stage marks what it
    consciously skips),
  - every extension either traces to a failure-mode requirement or draws a
    `missing-error-requirement` diagnostic. Enumerated failure paths are where
    missed requirements hide; this check surfaces them,
  - flip detection on the actor + goal natural key.
- Projections fed: use case index, oval diagram, activity, interaction overview
  (with stage 6). A trigger-response statement maps naturally onto
  `Given/When/Then`, so scenarios render from the statements and the
  scenario-to-requirement `verifies` link is exactly checkable.

### Stage 3: domain model

On the existing entities, not beside them: scopes are the bounded contexts,
entities and typed relationships are the model.

- Content: attributes refined with types where stated, relationship
  cardinality, entity roles, `parent` alignment with stated composition.
  Invariants are not a new kind: an invariant is an always-holding requirement.
- Unit of work: one scope, or a relationship-connected cluster of the public
  scope when it is large. The reconciler computes the partition.
- Checks: cardinality contradictions become diagnostics; same-named entities
  across scopes stay distinct without diagnostic; an aggregate-root cycle is an
  error.
- Projections fed: class per scope, package, ER style option.

### Stage 4: instances

- Sources: example sections (detected by the same cheap signals the
  `suspicious-non-normative` check uses), enumerations of concrete things,
  fixtures the ledger names.
- Unit of work: one example section or one fixture group.
- Checks: conformance of values and links against the class model, each
  violation naming both sentences (the example and the attribute or cardinality
  it contradicts).
- Projections fed: object diagrams.

### Stage 5: composition

Components, contracts, and the decisions behind them. This is where generation
stops re-deciding structure on every run.

- The first composition build runs one root session that proposes the partition
  from the derived relationship graph plus explicit prose statements, recorded
  as a `proposed` decision; component detail follows once it is accepted.
- Unit of work: one component with neighbors' interfaces summarized.
- Trace: component `satisfies` requirements and use cases; operations `satisfy`
  the requirements they realize; decisions `constrain` their subjects.
- Checks:
  - every requirement is satisfied by at least one component or marked
    `cross-cutting` with a note,
  - an operation no requirement realizes is a warning (invented surface),
  - every required interface has exactly one provider,
  - every component-to-component connection is exercised by at least one use
    case that crosses it; an unexercised connection is invented structure,
  - a change contradicting an `accepted` decision is a diagnostic,
  - a component satisfying requirements from two bounded contexts draws a
    warning naming the split,
  - pinned-fact drift: operation names appearing in no bound file once the
    ledger exists.
- Projections fed: component, composite structure, package dependencies,
  deployment (on-evidence), decision log, traceability matrix.

### Stage 6: dynamics

Demand-driven; the stage costs nothing when its triggers are absent.

- `sm:` derives only for entities referenced by two or more state- or
  failure-describing requirements. `ixn:` derives for use cases whose steps are
  satisfied by two or more components, or, with composition off, whose steps
  involve two or more actors.
- Unit of work: one entity's machine; one use case's interaction.
- Checks:
  - every message names an operation the target component provides (or refines
    a requirement directly with composition off), and rides an existing step,
  - every transition trigger traces to a behavior-stating requirement,
  - unreachable states and dead ends are warnings,
  - event completeness: every event the entity's requirements name is handled
    or explicitly ignored in every state. An unhandled event-state pair is a
    requirements gap detector,
  - nondeterminism (two transitions, one state, one trigger, overlapping
    guards) is an error.
- Projections fed: state machine, sequence, communication, timing
  (on-evidence), interaction overview.

### Stage 7: verification

Binding, generation, and verification as currently designed (the ledger, the
two test kinds, derived statuses), with the upstream stages feeding them:

- Generation can group by component (parts ordered by interfaces); the entity
  is the unit otherwise.
- Test derivation reads scenarios: steps and extensions shape acceptance tests;
  statement shape guides test shape for requirements no use case traces;
  instances feed fixtures.
- The traceability matrix (requirement → use case → component → files → test →
  verdict) is a projection; nothing new is stored.
- The graph stops at interface operations. Below that altitude the code is the
  source of truth, the ledger verifies it against the graph, and round-trip
  engineering is not attempted: syncing method-level structure back into a
  model is how model-driven engineering died.

### Divide and conquer

Three rules every stage obeys, which is what keeps a large project inside small
sessions:

- Bounded unit of work: a session sees one document, one cluster, one scope,
  one example section, one component, one entity, one use case. Its context is
  assembled under budget with expansion handles.
- Deterministic pre-partitioning: whenever a stage transition needs a global
  view (clustering requirements into use cases, partitioning entities into
  components), the reconciler computes candidate partitions cheaply and hands a
  session one part. The model judges within a part; it never surveys the whole
  graph.
- Dirtiness flows down `traces`: a changed section dirties its requirements; a
  changed requirement dirties exactly the steps, allocations, instances,
  transitions, and messages tracing to it. Each stage reconciles only its dirty
  units, in stage order. A one-line docs edit touches a narrow cone through all
  stages.

### Configuration

```toml
[profile]
name = "software"        # software | organization | narrative | slides | custom

[stages]                 # overrides the profile's defaults
usecases = true
domain = true
instances = false
composition = true
dynamics = false
```

Everything off is exactly the stage-1 compiler. Verification has no toggle:
stage 7 is driven by the ledger and the gen and test commands. `[limits]`
holds the size thresholds. Runtime modes, releases, workers, and leases stay with the control
plane as implemented.

## Compilation

### The agent

One agent. Task variety lives in goal kinds, which are data: a contract
paragraph, a resolution gate, hints, skills. The agent's session prompt is one
fixed contract (resolve the listed goals with the tools, over the loaded graph,
finish with `done`); everything task-specific arrives as data. The executor is
pluggable per ACP profile (the embedded agent, Claude Code, OpenCode), chosen
per goal family (the goal kinds one stage contributes, keyed by the stage name
in configuration), so extraction can run cheap while composition judgment runs
on the strongest model available.

The model never creates, routes, or prioritizes goals. It resolves them, fails
them, or parks them. Derivation, grouping, readiness, gates, budgets, and
causality are harness code.

### Goals

Compilation is a goal board. The reconciler derives goals from disk state the
way it derives the dirty set; a build is converged when no mandatory goal is
open or failed and the checks pass.

```yaml
g:retrace:uc:order-expires:
  kind: retrace                    # from the catalog below
  mandatory: true                  # mandatory blocks convergence; optional advises
  target: uc:order-expires
  change: {deleted: req:orders-6, in: g409}   # the disk evidence; also the identity
  cause: {generation: 409, mutation: 2, via: traces/refines}
  state: open        # open | resolved {generation, justification}
                     # | failed {reason} | parked | blocked {on}
  hints:
    - load uc:order-expires
    - load view:usecase/customer (the use case appears there)
    - skill usecase-editing
```

- Mandatory goals are correctness debts: something changed and the graph no
  longer agrees with itself or the docs. Optional goals are pressure: getting
  big opens one, too big escalates it to mandatory (the two thresholds on every
  limit). Restructuring happens inside the same convergence loop, in whatever
  build tipped the threshold; there is no separate cleanup pass.
- `change` is the attached evidence (the section diff, the deleted node, the
  crossed threshold) and the goal's identity: re-deriving the board matches a
  goal to its predecessor by the change.
- `cause` names the committed change that spawned the goal: the generation, the
  mutation within it, and the edge or computation that carried the dirtiness.
- `hints` are computed and honest: what to load, which skill explains the
  shape, which tool typically resolves the kind. Suggestions; the gates are
  the truth.
- `blocked` goals wait on a human (an unanswered decision, a ratification
  proposal, a gated release) and render on every status surface.

Goals are derived, not stored. The board recomputes from disk whenever
consulted, so any process computes the same board, an interrupted build
resumes anywhere, and a no-op rebuild derives zero goals. The inputs are the
documents, the graph, the ledger, and the durable change records: every commit
writes the typed dirtiness it caused (which sections changed, which
requirements were created, revised, or deleted, which thresholds crossed) into
`status.yaml` beside the parked work, and `derive_goals` reads a goal's
`change` from exactly that record. Current graph state alone cannot say
"revised since last judged"; the change records can, and resolving a goal
clears its record. What else is recorded is progress: resolutions with
justifications and failures with reasons in the journal, parked and failed
goals in `status.yaml`, dismissals as the limit bumps described under
[size limits](#size-limits). The graph never stores goals, so it cannot grow
with them.

### A compile, end to end

`jazyk compile` on a project with edits pending:

- Parse and diff the documents, derive the board. The terminal prints the
  summary first: `board: 47 goals (12 reconcile-section, 9 rejudge-pair,
  6 retrace, ...), 3 blocked, 5 optional`.
- The agent never sees the whole board. Sessions run one at a time, and each
  gets one batch: the scheduler takes the highest ready tier, groups open goals
  by locality (one document, one entity neighborhood, one view), and fills the
  batch until the context budget says stop. A batch is one to a handful of
  goals; the count is a consequence of budget and locality, never a fixed
  number.
- The session prompt lists exactly the batch's goals, the loaded graph for
  their locality, and one summary line for the rest ("41 goals elsewhere, not
  this session's").
- Each commit re-derives the board. Goals a mutation opened either join the
  running session (same locality, fits the budget) or wait for a later one. The
  live trace and the GUI board show counts ticking down, each resolution
  landing with its justification.
- The loop ends when the board derives empty of mandatory goals and the checks
  pass. The verdict carries what remains (`converged, 2 blocked on answers,
  5 optional advised`), and `jazyk ripple` replays how any goal came to exist.

Compilation is sequential: one build at a time (the build lease enforces it),
one session at a time within it. Parallel sessions are a later optimization;
nothing in the design depends on them.

### Sessions

One session per goal batch, fresh context, retries clean. The prompt is
assembled, never authored per task: the fixed contract, the goal list with each
kind's contract paragraph and hints, the initially loaded graph for the batch's
locality, and the active skills. The toolset is the union of what the batch's
goal kinds need, computed by the harness, so a batch of extraction goals still
sees a small toolset.

Session mechanics carry over from the current turn design: staged mutations
validated as staged, reads showing the session's snapshot with a note when
staged work shadows them, the repeated-call guard, budgets on rounds and
mutations, retry once then park, and the finish contract (`done` runs the batch
gates; a session that ends with valid staged work commits it).

### Loading the graph

The context is an explicit working set, not an accident of what was prompted.
The serving maintains the loaded set and renders its status into every round,
so the agent always knows what is loaded, what could be loaded next, and what
it costs.

```
## Loaded (14.2k/24k chars)
- view:class/commerce   12 entities, 18 edges shown; 9 members unloaded  [h:view:class/commerce:members]
- ent:order             full: 7 requirements, parent ent:commerce        [3 more edges: h:ent:order:traces]
- ent:customer          stub (definition only)                           [5 edges loadable: h:ent:customer]
- docs/orders.md#/orders/holds   section body
Consider unloading: ent:customer (not referenced in 6 rounds, no open goal touches it)
```

Tools:

- `load({target, depth?})`: load a node and its immediate neighborhood. Targets
  are any node id, section reference, or view id.
- `expand({handle})`: load the frontier behind a handle; every truncation emits
  a handle with a size estimate.
- `unload({target})`: drop an item from the loaded set.
- `graph_status({})`: re-render the status block on demand (a condensed form
  rides on every mutating reply).
- `search`, `read_section`, `get_entity`, `diagnostics`: reads; a read's
  subject joins the loaded set as a stub.

The policy is deterministic and budget-driven. Loading A brings A in full, its
edges, and each neighbor as a stub (name, one definition line, stereotype, its
own edge count); neighbors' neighbors are counts only. The walk stops at the
budget and emits handles, so overload is impossible by construction: the agent
sees "9 members unloaded" and chooses. Unloading frees budget for the rest of
the session: unloaded items leave the status, their handles close, and later
replies stop rendering them. The serving suggests unload candidates (least
recently referenced, not named by any open goal) and, past a high-water mark,
refuses further `load` calls until something is unloaded. Loading an
already-loaded target is a repeat, answered by the repeated-call guard.

### Skills

A skill is a prompt payload with the working knowledge for one shape: the use
case format and its invariants, how to edit a state machine node, what a good
abstraction split looks like, the profile's vocabulary. Skills are payload
files embedded at compile time, like the goal contracts.

- Auto-load: loading a node kind brings its skill once per session (load a
  `view:usecase/...` and the usecase-editing skill appears).
- Manual: `load_skill({name})`, with a skill index line in the status.
- Skills render once per session, count against the context budget, and are
  capped. Unloading the last node of a kind marks the skill inactive: the text
  already in context stands, the status just stops advertising it.
- Profiles contribute skills: the narrative profile's usecase skill speaks plot
  threads and scenes.

### The goal catalog

`M` mandatory; `O` optional; `O→M` optional, escalating to mandatory past its
hard threshold; `B` blocked-on-human. A goal kind exists only when its stage
is active.

| kind | m | stage | created when | resolved when (the gate) |
|---|---|---|---|---|
| `place-anchors` | M | 1 | alignment proposals pending for a document | every proposal decided |
| `reconcile-section` | M | 1 | a section is dirty or unprocessed | coverage mark staged or recorded; stale anchors addressed; extractions honest |
| `rejudge-pair` | M | 1 | a requirement was created or revised; sticky pairs | a verdict per neighbor in `evidence` (duplicate, contradiction, consistent) |
| `review-entity` | M | 1 | an entity's fact set changed | definition current; lookalikes judged; diagnostics filed or resolved |
| `declare-edges` | O | 1 | a multi-entity requirement has no `edges` | edges declared, or justification says the statement is not structural |
| `dedupe-candidates` | O | 1 | cross-document lookalikes score high | merged, or kept with reasoning |
| `derive-usecases` | M | 2 | a cluster's membership changed | every cluster requirement refined by a step or marked |
| `retrace` | M | any | any node's upstream trace died or changed | broken links repaired, re-derived, or the node deleted; nothing dangling |
| `extend-usecase` | O | 2 | a failure-mode requirement is unrefined by any extension | extension added or `missing-error-requirement` filed |
| `model-domain` | M | 3 | structural facts changed in a scope cluster | attributes, roles, cardinalities current; contradictions filed |
| `conform-instance` | M | 4 | an instance or the model under it changed | values and links conform, or the finding is filed |
| `partition` | M | 5 | composition on, no accepted partition decision | partition proposed and recorded; the decision answerable afterward |
| `design-component` | M | 5 | allocation candidates, proposed operations, or answered decisions pending | every candidate accepted or marked; operations satisfy or carry reasoning |
| `derive-statemachine` | M | 6 | a stateful entity's triggering requirements changed | transitions refine requirements; machine current |
| `derive-interaction` | M | 6 | a use case's steps, allocation, or interfaces changed | messages ride steps and name operations (or refine requirements) |
| `curate-view` | O | any | new nodes match a view's scope; a view has no members for its query | membership decided (added, or excluded with note) |
| `split-view` | O→M | any | a view crosses its soft limit | sub-views created and linked, or members collapsed under parents |
| `abstract-entity` | O→M | any | an entity crosses its requirement or child soft limit | sub-entities introduced with `parent`, detail moved, docs proposals staged |
| `ratify` | B | any | a derived or decree fact awaits its prose | human accepts the docsgen proposal (dual write) or retracts the fact |
| `bind` | M | 7 | a requirement owes a binding | ledger row recorded |
| `generate` | M | 7 | an entity or component's facts differ from the ledger | `record_generation` landed |
| `verify` | M | 7 | a row's derived status says action | verdict recorded |
| `answer` | B | any | a diagnostic or decision carries an unanswered prompt | the human answers; applying the answer is a new goal with the answer as cause |

Notes on the load-bearing rows:

- `retrace` is one kind, not five. Delete a requirement and the entity, the use
  case, and the class view that referenced it each surface as a `retrace` goal
  with the same cause, each hinting what to load to see the damage. The gate is
  uniform: nothing may keep pointing at the dead node.
- `abstract-entity` and `split-view` are where containment is exercised:
  introduce a parent, distribute children, let lifting keep coarse views true.
  Their skill carries the judgment guidance: split by cohesion of requirements,
  respect scopes, never invent concepts the docs cannot support, propose docs
  sentences for the new structure.
- Judgment gates verify completeness, not correctness: a `rejudge-pair` gate
  checks that a verdict with reasoning exists per pair; it cannot know a
  "consistent" verdict is true. Verdict quality is a benchmarking concern,
  taken up after the first implementation.
- `ratify` and `answer` are the human seams. They keep the report honest: a
  build with open blocked goals is "converged, awaiting 2 answers", never
  silently done.

Each kind ships a contract paragraph (a payload file, embedded like all
prompts), a gate implementation, and a hint computer.

### Resolving, failing, bubbling

- `mark_goal_done({goal, justification, evidence?})`: the justification is
  mandatory and concise, one or two sentences of why the goal is complete; the
  prompt demands brevity, the journal records it, and `jazyk ripple` shows it
  beside each step. The serving validates the claim against the kind's gate and
  rejects a false one with the gate named.
- `mark_goal_failed({goal, reason})`: always available. A goal that cannot be
  accomplished (documents too deeply contradictory, a target that no longer
  makes sense) must be failable, or the board fills with dishonestly resolved
  goals. A failed goal keeps its target, so the failure surfaces on the thing
  itself everywhere it renders. A failed mandatory goal blocks convergence; a
  failed optional goal is recorded and stands. Parked is different: "ran out of
  budget", resumed next build.
- Bubbling: staged mutations are validated when staged, and the same
  computation previews the goals a mutation will open; the tool reply says so
  ("this delete will open: retrace uc:order-expires (step 2), retrace
  view:class/orders (member gone)"). At commit the previews become real goals
  with causes. They join the running session when they fit its locality and
  budget; otherwise they wait. Downstream work is never silent and never
  model-invented.

### What the model sees

Every session prompt is assembled deterministically, so it can be shown before
it is spent. `jazyk preview` renders the next session's prompt exactly as the
model would receive it (`jazyk preview <goal|target>` for the batch that goal
would join), and the GUI shows the same pane before a release in manual mode.
The transcript records the same rendering per round, so post-hoc review sees
what the model saw, verbatim.

```
[agent contract, fixed]

[skill: requirement-extraction (active)]

## Project
- build 12, generation 412, manual mode
- diagnostics: 1 error (contradiction diag:contradiction-3), 4 warnings
- board: 2 goals in this session; 41 elsewhere; 3 blocked on human answers

## Goals
- [g:reconcile-section:docs/orders.md#/orders/holds] mandatory
  The section changed (diff in the loaded body). Bring the graph in line:
  extract, update, cover. Gate: coverage marks staged, stale anchors addressed.
- [g:retrace:uc:order-expires] mandatory
  Step 2 refines req:orders-6, deleted in g409 (reason: duplicate). Repair,
  re-derive, or delete. Gate: nothing dangling. Hint: load uc:order-expires.

## Loaded (9.8k/24k chars)
- docs/orders.md#/orders/holds   section body, with the diff marked
- ent:order    full: 7 requirements, parent ent:commerce   [3 more edges: h:ent:order:traces]
- uc:order-expires   stub   [loadable: h:uc:order-expires]
```

Contract paragraphs are short and imperative: what the goal means, what
evidence the gate wants, what not to do (the review asymmetry: a wrong delete
destroys information, a missed duplicate only leaves a finding; when in doubt
keep both and file), and that justifications and failure reasons are one or two
sentences, never essays. The feedback contract rides once, high: confusing
instructions and tools go to `report_feedback`, and the session continues on
best judgment.

### Ordering and convergence

- Readiness tiers order the work: a goal is ready when the goals it depends on
  are closed in its cone. The ladder gives the tiers (alignment before ingest
  before judgment before use cases before domain before instances before
  composition before dynamics before ledger goals); document link levels order
  stage-1 batches, roots first.
- Convergence: no open or failed mandatory goals, checks clean. The verdict
  carries the counts: `converged`, or
  `incomplete: 3 open, 1 failed, 2 blocked, 5 optional advised`.
- Budgets: per session (rounds, mutations, context), per build (goal
  resolutions), earlier tiers first when tight. Parked goals resume first next
  build.
- Oscillation: two goals resolving each other back and forth (a proposed
  operation bouncing between dynamics and composition) is caught by flip
  detection on the target's natural key; the pair parks as one
  `unstable-derivation` diagnostic with both justifications side by side,
  blocked on a human.

## Ripple

The target is a stable system: docs, graph, diagrams, deliverable, and tests as
one fixed point. A human edits any surface; compile detects the change, the
agent converges the rest, and the whole run is explainable afterward.

### Edit paths

Every human edit enters as one of four paths, and all four end the same way:
goals derived, sessions scheduled, fixed point restored.

- Edit prose. Parse, align, dirty sections, reconcile, cascade.
- Edit a quote-provenanced fact through a diagram or the GUI inspector. The
  fact's provenance names the sentence, so the edit is a dual write: the model
  proposes the sentence rewrite, the human accepts it, and the prose
  replacement commits with the graph mutation in one changeset (changing a
  class-diagram edge's cardinality rewrites the sentence behind the
  requirement that declared it). The commit absorbs the new section hashes, so
  the edit does not dirty the document it just changed; downstream goals
  derive from the graph change. When no proposed rewrite is accepted, the edit
  falls through to a decree plus a ratification proposal: the compiler never
  rewrites a source document without an accepted proposal.
- Edit a derived fact, or add a fact with no prose behind it: a decree. The
  changeset lands graph-only with `decree` provenance. Downstream goals derive
  normally; upward, the decree queues a ratification proposal (docsgen renders
  the sentence the docs should gain, as a diagnostic prompt with a suggested
  edit). Accepting writes the prose and flips the provenance to `quote`;
  rejecting retracts the decree. The compiler never overwrites a decree; it
  files diagnostics against it.
- Edit the deliverable or tests. Ledger statuses flip (`code-changed`,
  `test-changed`), verification reruns, and the unclaimed report feeds
  decompile drafts.

Deletion runs the same paths in reverse: dead prose kills quotes, which kills
quote-provenanced facts (garbage collection with tombstone redirects), which
opens `retrace` goals through the trace edges; derived facts whose upstream
died are re-derived or collected.

### Ambiguity

Anything the deliverable needs that the documents do not state is an ambiguity.
Generation does not stall on one: it chooses with best judgment, records the
choice, and raises it, graded by the scope of what had to be invented. "Build
me a Facebook" is an error (the invention is the whole product); an unspecified
out-of-memory behavior is a warning; an unspecified background color is info a
human may suppress. Ratification is how the debt is repaid: every derived and
decreed fact carries a proposal for the sentence the docs should gain, and the
graph converges toward fully quoted.

The docs absorb the detail by dividing, not bloating: a document states the
high level, sub-documents carry the detail, every one readable on its own.
`doc-too-large` and `section-too-large` tell the human where to split, incoming
links keep the parts bound to the whole, and ratification proposals can target
a new sub-document rather than cramming a parent.

Measuring the grade has a promising instrument: the deliverable itself.
Generated mass attached to no requirement is exactly the invented detail, and
the ledger with the unclaimed report already computes attachment. "App like
Facebook", three words, shows up as an enormous unattached remainder; docs
written near pseudo-code leave almost none. Measured at generation time, the
unattached remainder grades the ambiguity, and a later pass can bubble those
emerged details up into the IR and the docs.

### Causality

Every goal carries its cause, and that record is the whole ripple story. Every
committed changeset (a session, a dual write, a decree, garbage collection)
appends a journal entry with a generation number; the entry records
`resolved_goals` (each with its one-line justification) and `opened_goals`.
Human edits are generation-stamped the same way: a prose save that dirties
sections journals an `edit` entry, so the root of every ripple is itself a
generation.

The ripple DAG is derivable, never stored: start at a generation, follow opened
goals to the generations that resolved them, repeat; or walk backward from any
node through its `updated` markers to the human edit that started everything.
The journal is the ground truth; the DAG is a rendering over it.

### Observing a run

Realtime:

- The live trace: session lifecycle events, tool rows with condensed arguments,
  model text, per-session token counts, and `goal` events (opened or resolved,
  with cause). `--verbose` shows the cascade as it happens.
- The GUI board: stages as columns, goals as cards (open, blocked with reason,
  parked, failed), arrows lighting up as causes fire. A card click opens the
  live session (the follow-session machinery).
- `jazyk watch` prints one line per goal: what opened, why, what session took
  it.

Post compile:

- `jazyk ripple <target|generation|doc>`: the ripple DAG rooted at a change.
  For a target, the last cascade that touched it; for a generation, the full
  tree forward; `--back` shows causes instead of consequences.
- The build report: the causality DAG for the whole build, per-family cost
  beside it, parked and failed goals with reasons.
- Journal diffs between builds remain the release-diff surface for project
  management.

The trace a one-sentence edit leaves (`orders.md`: "held orders expire after
21 days" becomes "30 days"):

```
edit g87 docs/orders.md /orders/holds (human)
└─ reconcile-section docs/orders.md g88: req:orders-6 revised (quote and statement updated)
   ├─ rejudge-pair (req:orders-6 ~ req:payment-9) g89: consistent
   ├─ derive-statemachine ent:order g90: transition held→expired guard updated
   │  └─ checks: event completeness ok
   ├─ derive-usecases cluster:customer/holds g91: uc:order-expires step 2 requote
   └─ bind req:orders-6: row stale (requirement-changed)
      └─ verify req:orders-6: fail (test asserts 21) → generate ent:order-expiry g92
         └─ verify req:orders-6: pass
converged: 5 sessions, 2 stages touched, 38k tokens
```

Every line is a journal entry; every indent is a goal with its cause and
justification on record.

### Termination

Ripple must not mean runaway:

- The cone: goals open only along trace edges and computed derivations, so a
  change reaches exactly the nodes with a justification path through it.
- Idempotence: a session that re-derives an unchanged conclusion stages a no-op
  upsert; no mutation, no new goal, and that branch of the cascade dies. This
  is what makes convergence a fixed point rather than a loop.
- Budgets: per session and per build, earlier tiers first when tight; tier
  priority under the per-build cap is what bounds a runaway stage. Exhaustion
  parks with an `incomplete-build` diagnostic, resumed next build. Unfinished
  work is never silent.
- Flip detection catches oscillation and parks it for a human.

## Implementation

### The registry

One Rust trait, one registry, every goal kind an implementation. The trait
surface stays small because the shared machinery (store, gates, context engine,
journal, trace, control plane) is generic underneath:

- `kind`: the goal kind name.
- `unit`: what one target is (a document, an entity, a cluster, a view), so the
  board and the GUI can render it.
- `derive_goals(store, status) -> Vec<Goal>`: this kind's open goals from disk
  state. Deterministic, idempotent, cheap; the board stays derivable from disk,
  which is what lets any consumer resume any build.
- `ready(goal, board) -> Ready | Blocked(reason)`: the tier ordering and
  gating, with the reason rendered as a sentence because the visibility
  surfaces show it.
- `pack(store, batch) -> Pack`: a batch's initially loaded graph, through the
  context engine.
- `toolset() -> &[ToolId]`: the kind's slice of the one tool registry.
- `gates(changeset) -> Vec<Violation>`: batch checks at `done`, on top of the
  store's per-mutation gates.
- `prompt`: the contract payload file, embedded at compile time.

Registration is compiled in, a static list: the dependency policy hand-rolls
domain logic, and a dynamic plugin system is infrastructure nobody needs.
Adding a stage is goal kinds plus gates plus skills in the registry, a module
and a config line.

### Write tools

Joining the existing catalog, all staging into changesets behind the same
gates: `upsert_usecase`, `update_usecase`, `delete_usecase`, `upsert_instance`,
`update_instance`, `delete_instance`, `upsert_component`, `update_component`,
`delete_component`, `upsert_interface`, `update_interface`, `delete_interface`,
`upsert_statemachine`, `delete_statemachine`, `upsert_interaction`,
`delete_interaction`, `report_adr` (upsert by natural key; supersede, never
rewrite), `set_trace_coverage({stage, target, state, note?})` (the per-stage
coverage mark; `not-applicable` requires the note), and the goal tools
(`mark_goal_done`, `mark_goal_failed`). Chat gains `answer_adr` on the
prompt-and-answer machinery. Relationships keep having no write tool, and no
tool enqueues work: the model writes graph state, the harness derives the
goals.

### Executors

One global ACP profile with per-family overrides:

```toml
[acp]
agent = "embedded"

[stages.composition]
agent = "claude-code"
```

Per-family cost accounting (below) is what makes the choice informed.
Benchmarking per goal kind follows the first implementation, once real use
shows what needs grading.

### Visibility

- `jazyk explain [goal|target]`: for a goal, which change produced it, what its
  readiness says, what blocks it; for a target, the cone of goals a change to
  it would open, stage by stage. A rendering over derivable state.
- The GUI board (above), the `jazyk preview` pane, and the follow sessions.
- Cost accounting: per-session token counts aggregate per goal kind, per
  family, per build, and per document into `status.yaml` and the GUI ("this
  build: 41 sessions, 310k tokens, 78% in reconcile-section").
- OpenTelemetry export, off by default: one span per build, session, and tool
  call, GenAI semantic-convention attributes on session spans, OTLP endpoint
  from config. The GenAI conventions are pre-stable (their repository split out
  in June 2026 with no tagged release), so the attribute set is pinned in one
  module and treated as an export detail. The journal stays the source of
  truth; OTel is an export, not a store.

### Alternatives considered

- Orchestration frameworks (LangGraph, CrewAI, AutoGen, Mastra, the agent
  SDKs). Rejected as the core: they are Python or TypeScript against a single
  Rust binary, their center of gravity is LLM-decided control flow while
  deterministic scheduling is jazyk's thesis, and the part they would replace
  (queue, leases, resume, gates) is the part jazyk already has. External agents
  fit at the executor seam instead, and ACP already carries that.
- Durable-execution engines (Temporal, Restate) for retries, leases, resume.
  Rejected: Temporal's Rust SDK is a public preview, Restate is credible (a
  single-binary Rust server with a real Rust SDK) but still a second process
  journaling execution history. Jazyk's durability is stronger for its shape:
  the board recomputes from disk rather than replaying an event log, so there
  is no history to corrupt. Their concepts (heartbeats, lease expiry,
  idempotent steps) are present already.
- Rust agent frameworks, Rig foremost (active, 20+ providers, MCP support,
  OTel instrumentation, mock-model testing). Rig's agent is a model-loop
  concept: preamble, tools, provider. Jazyk's task definition lives above it
  (contracts, gates, packs) and the loop below it is already pluggable via
  ACP, so Rig competes only with the embedded agent's endpoint client, where
  it is async (tokio) against a deliberately sync binary, 0.x with breaking
  changes, and without the text codec and downgrade stickiness that exist for
  weak local models. The right entry point is embeddings: one index over docs
  and graph serving the `search` tool, lookalike candidates, and the
  reconciler's clustering. The boundary stands regardless: embeddings are a
  similarity signal inside deterministic machinery, never the context path;
  retrieval-over-raw-prose is what jazyk exists to replace.

### Landing

One coordinated change, docs first per the repo rule: the `docs/compiler/` tree
is rewritten to this design (model pages per node kind, goal pages with
contract paragraphs as payload files, skills as payload files; `reconciler.md`,
`turns.md`, and `control-plane.md` absorb goals, the registry, and the
visibility surfaces), then `bootstrap` follows the docs. Validation holds the
line: `cargo test`, runs on `bootstrap/example/f1` and `f2`, and the dogfood
compiled under the full ladder. The non-software profiles get fixture projects
(`example-slides` exists; an organization and a narrative fixture join it).
Stage toggles and profiles are runtime configuration, not a delivery sequence.

## Open questions

- The entity natural key under containment: `name` plus `scope` collides when
  two parents each contain a same-named child (two modules, each with a
  `Config`), and an upsert would wrongly merge them. Whether scope generalizes
  to a namespace derived from the containment chain, or `parent` joins the key,
  is undecided; a wrong merge is the failure to avoid.
- Where the docs-split vs graph-split line sits: the same size pressure can be
  answered by splitting a section or splitting an entity, and which is right is
  subjective. A declared experiment.
- Whether stage 2 should start as a rendering (scenarios projected from
  requirement clusters) before use cases become stored nodes; rendering first
  is cheaper to validate.
- Whether instances earn their keep outside media rich in examples; the
  conformance check may justify defaulting the stage on once measured.
- Whether the glossary deserves promotion: entities plus scopes already are a
  per-context glossary, but making every noun phrase in every stage resolve to
  a term (undefined term as diagnostic) is the cheapest check with the highest
  leverage.
- How much of stage 3 belongs in stage 1: attributes and cardinality could land
  as ordinary extraction improvements without a stage boundary.
- Per-family executor defaults, and whether the profile should default from the
  recorded medium decision.
- Ratification review granularity: proposals could aggregate per target
  document (one reviewed draft carrying many facts) rather than one prompt per
  fact; the draft-document machinery points that way.
- Whether goal derivation needs a cheap dirtiness probe beside the full
  computation, serving `await_changes` and watch mode without a full board
  recompute per poll; and whether the change records outgrow `status.yaml`
  into sibling files.

## References

- [UML 2.5.1 specification](https://www.omg.org/spec/UML/2.5.1/) (the diagram
  taxonomy and the profile mechanism);
  [SysML v2](https://www.omg.org/news/releases/pr2025/07-21-25.htm) and its
  [textual notation](https://github.com/Systems-Modeling/SysML-v2-Release) (the
  trace vocabulary; models as git-diffable text).
- Practitioner UML usage:
  [Dobing and Parsons](https://www.researchgate.net/publication/220373821_Dimensions_of_UML_Diagram_Use_A_Survey_of_Practitioners),
  [Ozkaya 2020](https://www.sciencedirect.com/science/article/abs/pii/S0950584920300252):
  class, sequence, and state carry the real use, which is why those are stored
  stages and the rest are projections.
- Traceability discipline:
  [DO-178C](https://www.parasoft.com/learning-center/do-178c/requirements-traceability/),
  [ISO 26262](https://www.parasoft.com/learning-center/iso-26262/requirements-traceability/).
- EARS and ISO/IEC/IEEE 29148 inform the extraction guidance (atomic, testable,
  entity-anchored statements).
- [Structurizr](https://docs.structurizr.com/) (one model, many views),
  [Context Mapper](https://contextmapper.org/) (DDD as a textual DSL),
  [why MDE fails](https://www.infoq.com/articles/8-reasons-why-MDE-fails/) (the
  drift the provenance kinds and the ratification loop answer).
- Spec-driven development, validating the living-graph thesis by contrast:
  [Kiro](https://kiro.dev/docs/specs/) (staged specs, no persistent model
  behind them),
  [Spec Kit criticism](https://github.com/github/spec-kit/discussions/1784)
  (generated spec volume is a cost; the graph must stay denser than the
  prose),
  [OpenSpec](https://hashrocket.com/blog/posts/openspec-vs-spec-kit-choosing-the-right-ai-driven-development-workflow-for-your-team)
  (living specs plus deltas, the manual version of reconciliation),
  [Fowler's comparison](https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html),
  [Brooker on SDD vs waterfall](https://brooker.co.za/blog/2026/04/09/waterfall-vs-spec.html).
- [Temporal Rust SDK](https://temporal.io/changelog/rust-sdk-public-preview),
  [Restate](https://restate.dev/), [Rig](https://github.com/0xplaygrounds/rig),
  [OTel GenAI conventions status](https://john-hodge.com/blog/opentelemetry-genai-semantic-conventions/).
