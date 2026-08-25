# Plan: the IR graph

Status: draft for iteration. Detailed design under [ir-stages](./ir-stages.md).
Companions: [ir-agents](./ir-agents.md) (who edits this graph and when),
[ripple](./ripple.md) (how edits propagate and how to watch them).

This file defines the shape: the node kinds, the edge algebra, how every UML diagram
type projects from the graph, and how one metamodel serves any deliverable medium.

## One graph, many diagrams

There are no diagram elements. There are graph nodes, and diagrams are deterministic
projections: a query selects nodes and edges, a renderer lays them out. The same
`ent:shopping-cart` is the class in the class diagram, the subject of its state
machine, and the type an interface operation names, because all three views select
the same node. Identity across diagrams is not a synchronization feature; it is the
absence of copies.

All 14 UML 2.5 diagram types render from the graph ([the catalog](#the-uml-25-catalog)).
Six have stored semantics of their own; seven are projections of facts other stages
already store; one (profiles) is a mechanism, not a picture. None is stored as a
drawing.

Consequences:

- A diagram cannot drift from the graph. It is rendered fresh from it, like the
  existing [relationships view](../docs/consumers/docsgen.md#relationships-view).
- Editing a diagram is editing the graph. The GUI edits typed nodes through the
  same write tools turns use; diagram text (Mermaid, PlantUML) is output, never
  input. See [ripple](./ripple.md#edit-paths).
- Every rendered element and edge can answer "why do you exist" by walking its
  provenance chain to a document sentence. See
  [justification closure](#justification-closure).

## Any medium, one metamodel: profiles

Jazyk does not assume the documents describe software
([the subject is whatever the documents describe](../docs/compiler/concepts/ears.md#the-subject-is-whatever-the-documents-describe)).
The node kinds below are medium-neutral: things, obligations, goals, parts,
contracts, lifecycles, interactions. UML's own extension mechanism, the profile,
is the authentic way to specialize a generic metamodel to a domain, and jazyk
adopts it as the medium answer:

- A profile is project configuration
  ([`[profile]`](./ir-stages.md#configuration)), chosen once alongside the
  [medium decision](../docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated),
  or explicitly in `jazyk.toml`.
- A profile supplies three things and changes no schema:
  - a stereotype vocabulary: node-level labels rendered guillemet-style
    («service», «character», «department», «slide»), stored as an optional
    `stereotype` field with provenance like any fact,
  - stage defaults: which [stages](./ir-stages.md#the-stage-ladder) earn their
    keep in this medium (the narrative profile disables composition by default;
    the organization profile enables activity views prominently),
  - rendering labels: what the projections call things (the org chart is the
    class diagram under the organization profile; a scene is a sequence diagram
    under the narrative profile).
- Built-in profiles to start: `software` (default), `organization`, `narrative`,
  `slides`. A custom profile is a table in the project file, nothing more.

The point: the graph stays one metamodel, checks stay uniform, and the medium
lives in vocabulary and defaults, exactly what UML profiles were designed for.

## Provenance kinds

Every semantic fact carries exactly one provenance:

- `quote`: extracted from prose. `{doc, section, quote}`, the quote verbatim,
  located whitespace-insensitively. Exists today. Dies when its section text
  changes (stale anchor machinery).
- `derived`: synthesized by a turn from upstream IR. `{from: [node ids], reasoning}`.
  A derived fact is compiler-invented until ratified into prose; docsgen proposes
  the sentence, the owner accepts, the next reconcile flips it to `quote`. Dies
  when an upstream node it derives from dies or changes (dirtiness re-derives it).
- `decree`: authored by a human directly on the graph (a diagram edit with no prose
  behind it). `{author, at, note?}`. A decree outranks derivation (turns must not
  undo it) but not the docs: prose that contradicts a decree draws a
  `contradiction` diagnostic, and ratification proposals nag until the decree is
  either written into the docs or retracted. See [ripple](./ripple.md#edit-paths).

The invariant across all three: the graph never holds a fact that cannot say where
it came from, and the system pressure (ratification, contradiction checks) pushes
every fact toward `quote`. The documents stay the source of truth.

## Node kinds

Existing kinds unchanged except where noted: `sec` (structural),
`ent`, `req`, `rel` (derived), `diag`.

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
| `diag:<rule>-<n>` | diagnostic | any | rule + subjects | finding |

Ids are minted once by the store, immutable, readable. Merges leave redirects.
Exactly as today. Every kind takes the optional profile `stereotype`.

### Entity (extended)

New optional fields, all provenanced per fact:

- `role`: `aggregate-root`, `value`, `actor`, `service`, or unset. Profiles read
  it through their vocabulary (`actor` is a character in the narrative profile, a
  business role in the organization profile).
- `attributes`: list of `{name, type?, provenance}`. Structure the prose states
  ("an order carries a total and a currency"; "a department has a head and a
  budget"). Behavior stays a requirement.

### Requirement (extended)

- `edges` entries gain optional `cardinality` (`1`, `0..1`, `1..*`, `*`), promoted
  onto the derived relationship like `type` is.

### Relationship (derived, extended)

- Gains `cardinality` per member, strongest-claim promotion, recomputed on commit.
  Still no write tool: an edge cannot exist without a statement behind it. This is
  the property that makes every class-diagram edge explainable, in every medium:
  "a House is composed of Rooms" and "an Act is composed of Chapters" are the same
  `composition` edge with different quotes behind them.

### Use case

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

Lean by design: no ceremony fields (level, stakeholders, guarantees). Every step
`refines` at least one requirement; that is the gate. Under the organization
profile a use case is a business process; under the narrative profile it is a plot
thread (actor: `ent:heroine`, goal: expose the betrayal).

### Instance

Concrete examples, promoted from what today is only non-normative prose: worked
examples in the docs, enumerated concrete things, test fixtures named by the
ledger.

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
that contradicts the model is a documentation bug found deterministically, which
is exactly the trap example values set today
([compilation](../docs/compiler/compilation.md#waves) notes contradictions hiding
in example values). Under the organization profile instances are named teams and
offices; under the narrative profile, concrete scenes and events.

### Component and interface

UML component semantics: a modular part with provided and required interfaces,
nesting for levels (what C4 splits into container and component is nesting depth
here).

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

Interface operation `inputs` and `outputs` reference entities where they carry
domain types; that is the join between composition and the domain model, one node,
not a copy. Under the organization profile a component is a business unit and its
provided interfaces are the capabilities it offers other units; the narrative
profile disables this stage by default (acts and parts are packaging, below).

### Decision (ADR)

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
                             # invented by a turn: born proposed, prompt/answer flow
```

Append and supersede, never rewrite. A `proposed` ADR reuses the
[diagnostic prompt machinery](../docs/compiler/model/diagnostic.md#prompts): it is
a question to the owner, rendered wherever prompts render. Decisions are not
software-specific: which department owns onboarding, whether the novel is told in
first person, one decision node each.

### State machine

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

One machine per entity, whole-node upserts. Triggers map onto EARS
`When`/`While`/`If` clauses near-mechanically. The lifecycle reading is universal:
an order, a hiring pipeline, a relationship arc (strangers to rivals to lovers,
each transition refining a plot requirement).

### Interaction

```yaml
ixn:customer-checks-out:
  subject: uc:customer-checks-out
  participants: [ent:customer, comp:order-service, comp:inventory]
  messages:
    - {n: 1, step: 1, from: ent:customer, to: comp:order-service, operation: iface:order-service.checkout#submitOrder}
    - {n: 2, step: 2, from: comp:order-service, to: comp:inventory, operation: iface:inventory.stock#reserve}
```

Each message rides a use case step (`step`) and names an interface operation where
composition is on; without composition, messages tie participants and refine
requirements directly (a dialogue scene under the narrative profile: participants
are characters, messages are beats, each refining the plot requirement it
delivers). The gates keep sequence views, use cases, and contracts mutually
consistent.

## Edge algebra

Edges are stored on the downstream node (the one a turn writes) pointing upstream,
and the store maintains derived reverse indexes on commit, the same way requirement
sources become entity mentions today.

| edge | stored on | points to | kind |
|---|---|---|---|
| `parents` | section | section | structural (exists) |
| `mentions` | entity | section | provenance (exists) |
| `entities` / `edges` | requirement | entities | semantic (exists) |
| `members` | relationship | entities | derived (exists) |
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
| `verifies` | ledger row | requirement | trace (exists as binding) |
| `subjects` | diagnostic | any node | meta (exists) |
| `from` | derived provenance | upstream nodes | provenance |

Context engine: one new axis `traces`, walkable both directions through the reverse
indexes, with a hop quota and optional kind filter, joining `parents`, `mentions`,
and `requirements`. Dirtiness propagation walks the same reverse indexes; see
[ripple](./ripple.md#causality-effects-carry-their-cause).

## Justification closure

The check that makes every diagram answerable. For every semantic node and every
stored trace edge: walking provenance and trace edges upward terminates in a
verbatim quote in a live section, or the fact is `derived`/`decree` with live
upstream nodes and an open ratification proposal. No orphan facts, no unjustified
edges, enforced in the deterministic checks wave (DO-178C's rule, computed instead
of audited).

Worked walks, which are also the GUI inspector's click paths and the LSP's hover
targets:

- Class diagram edge `ent:catalog ~ ent:shopping-cart`, label `aggregation 1..*`
  → `rel:catalog~shopping-cart` → its `requirements` (never empty)
  → each requirement's `source` quote → section → line range in the source file.
- Sequence message `reserve` → `iface:inventory.stock#reserve` → operation
  `satisfies` `req:inventory-3` → quote in `docs/inventory.md#/inventory/stock`.
  The message also names `step: 2` → `uc:customer-checks-out` step 2 `refines` the
  same requirement: two paths, one sentence, and the checks verify they agree.
- Object diagram instance `inst:gold-tier-cart`, value `currency: EUR` → the
  entity attribute `currency` on `ent:shopping-cart` (its own quote) and the
  example sentence the instance was read from. A conformance failure names both
  sentences.
- State transition `placed → held` → `refines: req:payment-4` ("If payment is
  declined, then the order shall be held") → quote → section.
- Component box `comp:order-service` → `satisfies` list → requirements and use
  cases → quotes; its `technology` value → its own quote. An ADR `constrains` it
  → the decision's question, options, and either its quote or its
  proposed-awaiting-answer state.

## The UML 2.5 catalog

All 14 diagram types, each with its standing: `stored` (a stage writes its
semantics), `projection` (rendered from facts other stages store, zero new IR),
`on-evidence` (renders only when the docs state the facts), or `mechanism`.
Medium readings show the same projection under different profiles.

Structure diagrams:

- Class: stored (stages 1 and 3). Entities with `role` and `attributes`; edges
  from derived relationships with type and cardinality; one diagram per scope.
  Editing an edge edits the requirement behind it (dual write); drawing a new
  edge becomes an `add_requirement` proposal. Readings: domain model (software),
  org chart and role structure (organization), the web of characters, settings,
  and themes (narrative), deck structure (slides).
- Object: stored (stage 4, optional). Instances with values and links, conformance
  checked against the class model. Readings: fixtures and worked examples
  (software), named teams and offices (organization), concrete scenes and events
  (narrative).
- Package: projection. Scopes are the packages; the diagram is the scope grouping
  of the class projection with dependencies summarized from cross-scope
  relationships. Readings: modules or bounded contexts, divisions, acts and
  parts of the book.
- Component: stored (stage 5). UML notation: components, nesting, provided and
  required interfaces (lollipop and socket). Nesting depth carries what C4 splits
  into container and component levels. Readings: services and modules,
  business units and their capabilities; off by default under narrative and
  slides profiles.
- Composite structure: projection over a component's internals: its nested
  components and owned entities as parts, connectors derived from the interaction
  messages that cross them. Renders only where composition is on and the internals
  exist.
- Deployment: on-evidence. `deployedOn` facets and stated topology render as
  nodes and artifacts; no stage synthesizes topology. Readings: infrastructure
  (software), offices and locations (organization); n/a under narrative (settings
  are entities, not deployment).
- Profile: mechanism, not a picture. The medium profiles above are jazyk's use of
  it: stereotypes, stage defaults, rendering vocabulary. A profile diagram is not
  rendered; the project settings page is its honest form.

Behavior diagrams:

- Use case: stored (stage 2). Two renderings: the index (per actor, with steps
  and requirement links) and the classic oval diagram (actors, use cases,
  «include» from `includes`, «extend» from extensions). The oval form is a
  legibility option; the index carries more.
- Activity: projection of one use case's scenario: steps as actions, extensions
  as decision branches, `includes` as sub-activity frames. No new IR and no
  synthesis: the flow is exactly what the steps and extensions already state.
  Readings: process flow (software), workflow (organization, where this is often
  the primary view), plot flow (narrative).
- State machine: stored (stage 6, demand-driven). Flat statechart semantics
  first; hierarchy only if real projects demand it.
- Sequence: stored (stage 6). One per interaction.
- Communication: projection. The same interaction rendered as a wiring view with
  numbered messages instead of a timeline. One stored fact, two layouts; offered
  because it is free.
- Timing: on-evidence projection. Renders for a state machine whose transitions
  refine requirements carrying time measures (the NFR facet): lifelines of
  states against the stated bounds. No measures, no view. Readings: latency
  bounds, SLA timelines, pacing constraints where a narrative states them.
- Interaction overview: projection. A use case's steps as an activity frame where
  each step that owns interaction messages embeds a reference to its sequence
  view. Renders only where interactions exist.

Adjacent notations:

- C4: dropped as the stored vocabulary in favor of UML components with nesting
  (this plan's earlier draft chose C4; the swap buys the generic metamodel that
  profiles can re-read per medium, and UML notation the models know deeply from
  training data). C4's load-bearing insight, diagrams as projections of one
  model, is this file's first principle and is retained. A C4-styled rendering of
  the component tree remains a docsgen style option.
- ER diagrams: a style option on the class projection (entities, attributes,
  cardinalities), not new IR.
- BPMN: out of domain as IR; the activity projection covers workflow rendering,
  and a BPMN-shaped document is prose input like any other.
- Statecharts (Harel): the semantics adopted for `sm`.

## Renderers

- docsgen: Mermaid where it is capable (class, state, sequence, flowchart for
  activity), PlantUML for the rest of the catalog (object, component, composite
  structure, deployment, timing, communication, use case ovals). Both are text
  artifacts in the out directory, diffable like everything else.
- GUI: interactive projections with the justification walks as click paths.
- Never stored, never hand-edited: a diagram file is build output, exactly like
  a docsgen page.
