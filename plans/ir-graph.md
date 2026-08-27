# Plan: the IR graph

Status: proposal for iteration. The proposal set:
[ir-stages](./ir-stages.md) (doctrine and the stage ladder), this file,
[agent](./agent.md) (the agent and the goal system), [ripple](./ripple.md)
(convergence and observing it), [orchestration](./orchestration.md) (the
registry, executors, alternatives).

## Node kinds

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

## Provenance

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
  stronger one), directional: `{a, b}` reads a-acts-on-b ("A calls B to list
  directory content" declares `{a: ent:a, b: ent:b, type: dependency}`). A
  multi-entity requirement with no edges draws the `declare-edges` goal
  ([the catalog](./agent.md#the-goal-catalog)).
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
[goals](./agent.md#goals) carry work. The two reference each other and nothing
is recorded twice.

## Edge algebra

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

## Containment and lifting

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

## Size limits

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

## Justification closure

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

## How a diagram is stored

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
interactive projections straight from the graph and does not go through the
`.puml` files. Geometry, layout, and styling are never stored anywhere.

A PlantUML block inside a source document is the opposite thing: input. Parsing
treats it as a `diagram` section and extraction reads its obligations as prose.
Hand-written diagrams in docs are statements to compile; generated diagrams in
the out directory are projections of the result.

## Profiles

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

## The UML catalog

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
