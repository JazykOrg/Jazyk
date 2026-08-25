# Plan: the IR graph

Status: draft for iteration. Detailed design under [ir-stages](./ir-stages.md).
Companions: [ir-agents](./ir-agents.md) (who edits this graph and when),
[ripple](./ripple.md) (how edits propagate and how to watch them).

This file defines the shape: the node kinds, the edge algebra, how diagrams project
from the graph, and why each UML diagram type is in or out.

## One graph, many diagrams

There are no diagram elements. There are graph nodes, and diagrams are deterministic
projections: a query selects nodes and edges, a renderer lays them out (Mermaid in
docsgen, interactive in the GUI). The same `ent:shopping-cart` is the class in the
class diagram, the subject of its state machine, and the aggregate a component's data
contract names, because all three views select the same node. Identity across
diagrams is not a synchronization feature; it is the absence of copies.

Consequences:

- A diagram cannot drift from the graph. It is rendered fresh from it, like the
  existing [relationships view](../docs/consumers/docsgen.md#relationships-view).
- Editing a diagram is editing the graph. The GUI edits typed nodes through the
  same write tools turns use; Mermaid text is output, never input. See
  [ripple](./ripple.md#edit-paths).
- Every rendered element and edge can answer "why do you exist" by walking its
  provenance chain to a document sentence. See
  [justification closure](#justification-closure).

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
| `comp:<slug>` | component | 4 | name | component |
| `iface:<slug>` | interface | 4 | owner + name | contract |
| `adr:<n>` | decision | 4 | normalized title | decision |
| `sm:<entity-slug>` | state machine | 5 | subject entity | stateful entity |
| `ixn:<uc-slug>` | interaction | 5 | subject use case | multi-component use case |
| `diag:<rule>-<n>` | diagnostic | any | rule + subjects | finding |

Ids are minted once by the store, immutable, readable. Merges leave redirects.
Exactly as today.

### Entity (extended)

New optional fields, all provenanced per fact:

- `role`: `aggregate-root`, `value`, `actor`, `service`, or unset.
- `attributes`: list of `{name, type?, provenance}`. Structure the prose states
  ("an order carries a total and a currency"). Behavior stays a requirement.

### Requirement (extended)

- `edges` entries gain optional `cardinality` (`1`, `0..1`, `1..*`, `*`), promoted
  onto the derived relationship like `type` is.

### Relationship (derived, extended)

- Gains `cardinality` per member, strongest-claim promotion, recomputed on commit.
  Still no write tool: an edge cannot exist without a statement behind it. This is
  the property that makes every class-diagram edge explainable.

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
  provenance: {derived: {from: [req:checkout-1, req:inventory-3, req:catalog-2], reasoning: ...}}
```

Lean by design: no ceremony fields (level, stakeholders, guarantees). Every step
`refines` at least one requirement; that is the gate.

### Component and interface

```yaml
comp:order-service:
  name: Order Service
  kind: container
  parent: null
  responsibilities: owns order lifecycle and checkout
  technology: {value: Go, provenance: {quote: {doc: docs/arch.md, section: /arch/services, quote: "The order service is built with Go."}}}
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
domain types; that is the join between the architecture and the domain model, and
it is one node, not a copy.

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
a question to the owner, rendered wherever prompts render.

### State machine

```yaml
sm:order:
  subject: ent:order
  initial: placed
  states:
    - {name: placed, provenance: {...}}
    - {name: paid, provenance: {...}}
    - {name: shipped, provenance: {...}}
    - {name: held, provenance: {...}}
  transitions:
    - {from: placed, to: paid, trigger: payment succeeds, refines: [req:payment-2]}
    - {from: placed, to: held, trigger: payment declined, refines: [req:payment-4]}
    - {from: paid, to: shipped, trigger: fulfillment ships, refines: [req:fulfill-1]}
```

One machine per entity, whole-node upserts (weak models handle one document-shaped
call better than many granular ones). Triggers map onto EARS `When`/`While`/`If`
clauses near-mechanically.

### Interaction

```yaml
ixn:customer-checks-out:
  subject: uc:customer-checks-out
  participants: [ent:customer, comp:order-service, comp:inventory]
  messages:
    - {n: 1, step: 1, from: ent:customer, to: comp:order-service, operation: iface:order-service.checkout#submitOrder}
    - {n: 2, step: 2, from: comp:order-service, to: comp:inventory, operation: iface:inventory.stock#reserve}
```

Each message rides a use case step (`step`) and names an interface operation. Both
must exist; that pair of gates is what keeps sequence diagrams, use cases, and
component contracts mutually consistent.

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
| step `refines` | use case | requirements | trace |
| extension `refines` | use case | requirements | trace |
| `satisfies` | component | requirements, use cases | trace |
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
- State transition `placed → held` → `refines: req:payment-4` ("If payment is
  declined, then the order shall be held") → quote → section.
- Component box `comp:order-service` → `satisfies` list → requirements and use
  cases → quotes; its `technology` value → its own quote. An ADR `constrains` it
  → the decision's question, options, and either its quote or its
  proposed-awaiting-answer state.

## Diagram catalog

### Projections jazyk renders

Each is a named query plus a renderer, listed with what an edit to it means (the
dual-write or decree it becomes; details in [ripple](./ripple.md#edit-paths)):

- Class diagram, one per scope: entities of the scope with `role` and
  `attributes`; edges from derived relationships with type and cardinality.
  Editing an edge's cardinality edits the requirement behind it (dual-write).
  Adding an edge means writing the statement that implies it; the GUI turns the
  drawn edge into an `add_requirement` proposal.
- Use case index, one per actor: goals, steps, extensions, with per-step
  requirement links. The classic oval diagram renders as a compact index; surveys
  say the picture adds nothing over the list.
- C4 context and container/component diagrams: the `comp` tree with relations
  derived from interactions and cross-component `satisfies` overlap; each arrow
  lists the interface operations that justify it. An arrow with no justifying
  operation cannot render, because it cannot exist.
- Sequence diagram, one per interaction: participants and messages as stored.
- State diagram, one per state machine.
- Deployment view, only when containers carry a stated `deployment` facet;
  rendered as a C4 deployment diagram. No facet, no view, no invented topology.
- Traceability matrix: requirement × (use case, component, files, test, verdict),
  a table projection over `traces` plus the ledger.
- Relationships flowchart and glossary: exist today in docsgen, unchanged.
- Ripple view: not a UML diagram; the causality DAG of a build. See
  [ripple](./ripple.md#observing-a-run).

### The full UML 2.5 catalog, and why each is in or out

Structure diagrams:

- Class: in, as the scope class diagram. The highest-value structural view;
  practitioner surveys rank it first.
- Object (instance): out. Instances are examples; examples live in the docs as
  non-normative sections and in tests as fixtures. A stored object diagram would
  duplicate test data without test semantics.
- Package: out as a diagram. Scopes are the packaging; the class diagram groups by
  scope. A separate package view restates the directory listing.
- Composite structure: out. Its content (parts wired by connectors) is the
  component diagram one level down; practitioner use is marginal and LLM
  extraction reliability is poor for its niche semantics.
- Component: in, but as C4 container/component views rather than UML notation. C4
  is what practitioners draw now; the semantics stored (components, interfaces,
  relations) are the same.
- Deployment: partial. Only stated topology renders; no stage synthesizes one.
  Deployment is an operations fact the docs either state or do not.
- Profile: out. Metamodel machinery with no project content.

Behavior diagrams:

- Use case diagram: partial. The nodes are stage 2; the oval-and-stick-figure
  picture is replaced by the index rendering. Nothing semantic is lost; the picture
  encodes only participation, which the index shows.
- Activity: out. At requirements altitude its content is the use case steps; at
  code altitude it is control flow the code states better. LLMs produce plausible,
  vacuous flowcharts, and no deterministic check can hold one to anything. If a
  document contains one (Mermaid/PlantUML block), parsing treats it as a `diagram`
  section and extraction reads obligations from it as prose, which already works.
- State machine: in, demand-driven (stage 5). Highest semantic density per byte,
  strong deterministic checks, near-mechanical mapping from EARS patterns.
- Sequence: in, per multi-component use case (stage 5), because every message is
  checkable against interfaces and steps.
- Communication: out. Same information as sequence with a worse layout; pick one
  representation per fact.
- Timing: out. Timing bounds are requirements with measures (the NFR facet); a
  timing diagram stores no additional fact worth reconciling.
- Interaction overview: out. It composes sequences the way the use case index
  already composes scenarios.

Adjacent notations:

- C4: adopted as the component-layer rendering (above).
- ER diagrams: covered by the class projection (entities, attributes,
  cardinalities); a separate ER view is a style option in docsgen, not new IR.
- BPMN: out of domain; jazyk models the product's obligations, not the business's
  processes around it. A BPMN-shaped document is prose input like any other.
- Statecharts (Harel): the semantics adopted for `sm` (flat first; hierarchy and
  entry/exit actions only if real projects demand them).
