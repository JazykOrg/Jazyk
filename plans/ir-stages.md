# Plan: the software engineering process as IR

Status: draft for iteration. Companion plan: [orchestration](./orchestration.md), which
defines the stage registry this plan's stages plug into.

This file is the overview: motivation, the stage ladder, prior art, migration. The
detailed design lives in three companions:

- [ir-graph](./ir-graph.md): the node kinds and edge algebra, how all 14 UML diagram
  types project from the one graph, the justification chain from any diagram element
  to a document sentence, and the profile mechanism that serves any medium.
- [agent](./agent.md): the one agent and the goal system: the goal catalog (what
  opens each goal, its prompt contract, its resolution gate), loading and
  unloading the graph in context, skills, and multi-stage convergence.
- [ripple](./ripple.md): the stable system: editing docs, diagrams, or the graph and
  converging back to a fixed point; causality-carrying effects; observing a build in
  realtime and post hoc (`jazyk ripple`).

## The idea

Today the graph holds one semantic layer: entities and EARS requirements, with
relationships derived from requirement `edges`. Generation jumps from EARS statements
straight to the deliverable. Everything between (who uses the thing and for what, how
its domain is structured, what parts it has, how the parts talk, what state they
hold) is implicit, decided fresh inside each generation turn, and recorded nowhere.

The plan: model the engineering process itself as ordered IR stages in the same
graph. Requirements stay the foundation. Above them, each stage is a set of node
kinds reconciled through its own goal kinds (resolved by the one agent, see
[agent](./agent.md)), traced to the stage below, checked
deterministically, and rendered as diagrams on demand. The diagram vocabulary is
authentic UML: every UML 2.5 diagram type has a rendering path from the graph, and
UML's own profile mechanism carries the medium specialization. Diagrams are
projections of the graph, never stored drawings.

## Any deliverable, one ladder

Jazyk does not assume software
([the subject is whatever the documents describe](../docs/compiler/concepts/ears.md#the-subject-is-whatever-the-documents-describe)),
and the ladder must not either. The stages store medium-neutral facts: things,
obligations, goals, examples, parts, contracts, lifecycles, interactions. A
[profile](./ir-graph.md#any-medium-one-metamodel-profiles) (UML's specialization
mechanism, adopted as configuration) supplies the vocabulary, the stage defaults,
and the rendering labels per medium. The same ladder reads:

| stage | software | slide deck | company organization | romance novel |
|---|---|---|---|---|
| requirements | system obligations | content obligations per slide | policy and process rules | narrative obligations |
| use cases | actor goals | audience takeaways | business processes per actor | plot threads per character |
| domain | domain model | deck structure and elements | org chart, roles, capabilities | characters, settings, themes |
| instances | fixtures, worked examples | sample data on slides | named teams and offices | concrete scenes and events |
| composition | services and their contracts | (off by default) | business units and capabilities | (off by default) |
| dynamics | lifecycles, message flows | presentation flow | pipelines, cross-unit procedures | character arcs, dialogue scenes |
| verification | tests | render and content checks | audit checks | continuity checks |

A profile turning a stage off is a default, not a wall; the organization profile
that wants deployment views (offices) or the novel that wants none of composition
simply says so. The graph, gates, and checks are identical under every profile.

## What carries over unchanged

The current design's core decisions apply to every new stage:

- One persistent graph, edited in place, never regenerated. New stages are new node
  kinds in the same store, behind the same gates, journal, redirects, and GC.
- Identity: ids minted once, natural keys for upserts, merges leave redirects.
- Division of labor: the harness owns dirtiness, scheduling, validation, and derived
  data; the model owns extraction and judgment.
- Provenance: nothing enters the graph without a source. Three kinds now: `quote`
  (extracted), `derived` (synthesized, ratifies toward prose), `decree`
  (human-authored on the graph). Defined in
  [ir-graph](./ir-graph.md#provenance-kinds); the ratification loop is the
  countermeasure to the classic model-driven-engineering failure, models that
  drift from reality. Here a fact cannot drift silently: it is quoted, or it is
  visibly marked invented and nagging for ratification. An unstated fact is an
  ambiguity, resolved with best judgment and raised at a severity graded by the
  scope of the invention ([ripple](./ripple.md#ambiguity-is-the-debt)).
- Incrementality: a no-op rebuild makes zero LLM calls at every stage.

## Trace edges

Stages connect through one new edge axis, `traces`, with a typed `kind` drawn from the
vocabulary the standards already agree on (SysML v2 dependency kinds, ISO 29148,
DO-178C traceability):

- `refines`: same content, more detail (a use case step refines a requirement).
- `satisfies`: a design element answers a requirement (component ← requirement).
- `verifies`: a test or scenario checks a requirement.
- `derives`: a downstream fact justified by upstream nodes (the derived-provenance
  link).
- `constrains`: a decision or quality attribute limits an element (ADR → component).

The axis gets a hop quota in the context engine like the existing three. Dirtiness
propagation and every cross-stage check below are queries over these edges. The
traceability matrix regulated industries maintain by hand is a free projection.

## Stages are not phase-gates

The lesson from every criticized spec-driven tool (see [prior art](#prior-art-this-leans-on)):
stages order dependencies and trace links for the scheduler; they are never authoring
phase-gates. Any document edit at any time dirties whatever nodes its quotes anchor,
at every stage, and reconciliation flows through the trace edges. Nothing waits for a
phase to "complete", because phases do not exist: only dirty units and their cones.
This is the same rule that already makes rebuilds incremental, restated for the
ladder.

## The stage ladder

Stages in dependency order. Each names its unit of work (the divide-and-conquer
grain), what it traces to, its deterministic checks, and the UML projections it
feeds. Stages are opt-in per project, with defaults from the profile
(see [configuration](#configuration)); the shipped default is today's behavior.

### Stage 0: sections (exists)

Structural, deterministic. Unchanged.

### Stage 1: requirements and entities (exists)

Entities, EARS requirements, derived relationships, coverage, diagnostics.
Unchanged, with extensions already half-planned in the [TODO](../docs/TODO.md):

- Cardinality on derived relationship edges (`1..*`, `0..1`), declared on
  requirement `edges`, promoted like `type`.
- Entity `attributes`: `{name, type?, quote}` captured when prose states them.
  Attributes are facts about structure, not requirements; a statement of behavior
  stays a requirement.
- Entity `stereotype` from the profile vocabulary, captured when prose states it.

A derived facet, not a stored field: each requirement classifies as functional or as
a quality attribute (performance, security, reliability, usability, per ISO 25010)
from its pattern and wording, the same way behavior-vs-constraint derives from the
EARS pattern today. One deterministic check rides on it: a quality requirement whose
response carries no measurable bound ("shall be fast") draws a warning, because an
unmeasurable quality statement cannot be verified. The measures also feed the
timing projection ([catalog](./ir-graph.md#the-uml-25-catalog)).

- Unit of work: one document (exists).
- Checks (exist): coverage, reachability, flip detection, duplicates,
  contradictions.
- Projections fed: class (with stage 3), package (scopes).

### Stage 2: use cases

The behavioral grouping layer: who does what for which goal, as Cockburn-style use
cases kept deliberately lean, plus `includes` for shared sub-flows.

- Node kind `uc:` per [ir-graph](./ir-graph.md#use-case).
- Derivation: event-driven and state-driven requirements cluster by actor and
  trigger. The reconciler computes candidate clusters deterministically (shared
  actor entity, overlapping trigger tokens, the same lexical machinery pair review
  uses for neighbors), so one turn sees one bounded cluster, never the whole
  requirement set.
- Unit of work: one actor-goal cluster.
- Trace: each step `refines` its requirements; extensions `derive` from
  unwanted-behavior (`If ... then`) requirements.
- Checks:
  - every step references at least one existing requirement,
  - every event-driven requirement is refined by at least one use case step, or
    carries a per-stage coverage mark saying why not (the coverage contract
    generalizes: each stage marks what it consciously skips),
  - every extension either traces to an unwanted-behavior requirement or draws a
    `missing-error-requirement` diagnostic. This check is the stage's best payoff:
    enumerated failure paths are where missed requirements hide,
  - flip detection on use case natural keys (actor + goal).
- Projections fed: use case index and oval diagram («include», «extend»),
  activity (steps and extensions as actions and branches), interaction overview
  (with stage 6). EARS and Gherkin are near-isomorphic (`When <trigger> ... shall
  <response>` maps onto `Given/When/Then`), so the scenario rendering is close to
  mechanical and the scenario-to-requirement `verifies` link is exactly checkable.

### Stage 3: domain model

The class-diagram layer, on the existing entities rather than beside them. DDD
vocabulary maps onto machinery that already exists:
[scopes](../docs/compiler/concepts/scopes.md) are bounded contexts (and packages);
entities and their typed relationships are the model.

- Extensions to existing kinds, no new node kind:
  - entity `attributes` (stage 1 extension) refined with types where stated,
  - relationship `cardinality`,
  - entity `role`: `aggregate-root`, `value`, `actor`, `service`, or unset.
    Derived provenance allowed; prose wins when it states one.
  - invariants are not a new kind: an invariant is a ubiquitous EARS requirement.
    The domain turn may add the `edges` and attributes a statement implies, never
    a parallel invariant store.
- Unit of work: one scope (bounded context), or the public scope partitioned by
  relationship-connected clusters when it is large. The reconciler computes the
  partition; a turn never sees more than one cluster.
- Trace: attributes and roles carry provenance to requirements or quotes.
- Checks:
  - cardinality contradictions (two requirements implying different cardinalities
    on one edge) become diagnostics,
  - same-named entities across scopes stay distinct without diagnostic (exists),
  - an aggregate-root cycle (A composed of B composed of A) is an error.
- Projections fed: class (per scope), package, ER style option.

### Stage 4: instances (optional)

The object-diagram layer: concrete examples checked against the model. Sources:
worked examples in the docs (today marked `non-normative` and otherwise inert),
enumerated concrete things, and test fixtures the ledger names.

- Node kind `inst:` per [ir-graph](./ir-graph.md#instance).
- Unit of work: one example section, or one fixture group.
- Trace: `of` names the entity; `links` ride relationships; provenance quotes the
  example sentence or names the fixture.
- Checks (the stage's reason to exist, all deterministic):
  - conformance: every `values` key is a declared attribute of the `of` entity,
  - every link respects a relationship's type and cardinality,
  - a conforming failure names both sentences (the example and the attribute or
    requirement it contradicts). Example values encoding contradictions is a
    known trap today; this stage makes it a computed finding.
- Projections fed: object diagrams.

### Stage 5: composition

The parts layer, in authentic UML component vocabulary: components with nesting,
provided and required interfaces, and the decisions behind the partition. This is
where generation stops re-deciding structure on every run. Under profiles that
disable it (narrative, slides by default), the ladder skips straight to dynamics
with interactions tying entities instead of components.

- Node kinds `comp:`, `iface:`, `adr:` per
  [ir-graph](./ir-graph.md#component-and-interface). Nesting depth carries what
  C4 splits into container and component levels; a `deployedOn` facet carries
  stated topology.
- Allocation: `satisfies` trace edges: component → the requirements and use cases
  it answers. Every allocation carries provenance, staged by the composition
  turns.
- Decisions: a `proposed` ADR is a question to the owner through the diagnostic
  prompt machinery; a decision the documents state is born `accepted` with quote
  provenance.
- Unit of work: one component with its neighbors' interfaces (summarized). The
  first composition build runs a single root turn that proposes the partition
  from the derived relationship graph plus explicit prose statements, as
  `proposed` ADRs; component detail turns run after the partition is accepted.
- Trace: component `satisfies` requirements and use cases; interface operations
  `satisfy` the requirements they realize; ADR `constrains` its subjects.
- Checks:
  - every requirement is satisfied by at least one component, or carries a stage
    coverage mark (`cross-cutting` with note),
  - an interface operation no requirement realizes is a warning (invented
    surface),
  - every required interface is provided by exactly one component; a require with
    no provider is an error,
  - every component-to-component connection is exercised by at least one use case
    that crosses it; an unexercised connection is invented structure,
  - a change that contradicts an `accepted` ADR is a diagnostic (flip detection
    for decisions),
  - allocation respects scopes: a component satisfying requirements from two
    bounded contexts draws a warning naming the split,
  - `pinned-fact-drift` generalizes: interface operation names that appear in no
    bound file once the ledger exists.
- Projections fed: component (lollipop and socket), composite structure
  (internals), package dependencies, deployment (on-evidence, from facets), a
  C4-styled rendering as a docsgen style option, ADR log, traceability matrix.

### Stage 6: dynamics (optional, narrow triggers)

Only where the cheap signals say the structure exists. Most entities need no state
machine; most use cases need no interaction spec. The reconciler derives the work,
so the stage costs nothing when the triggers are absent.

- Node kind `sm:` per [ir-graph](./ir-graph.md#state-machine), derived only for
  entities referenced by two or more state-driven (`While ...`) or
  unwanted-behavior requirements. Every transition traces to the requirements
  that state it; a transition no requirement backs is derived provenance, feeding
  back to docs like any invented fact.
- Node kind `ixn:` per [ir-graph](./ir-graph.md#interaction), derived for use
  cases whose steps are satisfied by two or more components, or, where
  composition is off, whose steps involve two or more actor entities (the
  profile's dialogue-scene reading).
- Unit of work: one entity's machine; one use case's interaction.
- Checks (this is where the ladder pays off, all deterministic):
  - every interaction message names an existing operation on an interface the
    target component provides (composition on), or refines a requirement
    directly (composition off),
  - every message rides an existing use case step,
  - every transition trigger traces to an event-driven or state-driven
    requirement,
  - unreachable states and states with no exit (unless terminal) are warnings,
  - event completeness: every event named by a `When`-clause requirement on the
    entity is handled or explicitly ignored in every state. An unhandled
    event-state pair is a requirements gap detector, not a modeling nicety,
  - nondeterminism: two transitions from one state on one trigger with
    overlapping guards is an error.
- Projections fed: state machine, sequence, communication (same fact, second
  layout), timing (on-evidence, from measures), interaction overview.

### Stage 7: verification (exists, gains inputs)

Binding, generation, and verification stay as designed. What changes with the
stages above enabled:

- The unit of generation can follow composition: one component instead of one
  entity, its parts ordered by interfaces. Entity-unit generation remains the
  default and the fallback when stage 5 is off.
- Test derivation reads scenarios: a use case's steps and extensions shape the
  acceptance tests for its requirements, and the EARS-to-Gherkin mapping makes
  the derivation near-mechanical; the EARS-pattern-to-test-shape rule stays for
  requirements no use case traces. Instance nodes feed fixtures.
- The traceability matrix (requirement → use case → component → files → test →
  verdict) is a docsgen projection over existing edges plus the ledger. Nothing
  new is stored.

### What is still out

Nothing in UML's 14 diagram types is fully out anymore: each has a stored stage, a
projection, an on-evidence rendering, or (profiles) a mechanism role; see
[the catalog](./ir-graph.md#the-uml-25-catalog) for each verdict. What stays out:

- BPMN as IR (the activity projection covers workflow rendering; a BPMN-shaped
  document is prose input like any other).
- Full KerML/SysML v2 metamodel semantics (metamodel maximalism; sparse LLM
  training coverage). The trace vocabulary is borrowed, nothing else.
- Formal methods (TLA+, Alloy) as a stage: not LLM-tractable enough in 2026;
  state machines are the formal-enough sweet spot.
- Method-level design and generated-code round-tripping: the MDA grave. Code is
  the better source of truth at that altitude; the ledger verifies against the
  graph instead.

## Divide and conquer, stated once

Every stage obeys the same three rules, which is what keeps a large project inside
small turns:

- Bounded unit of work: a turn sees one document, one cluster, one scope, one
  example section, one component, one entity, one use case. The unit's pack is
  assembled by the context engine under the same budgets and expansion handles as
  today. The `traces` axis gets hop quotas like the existing three.
- Deterministic pre-partitioning: whenever a stage transition needs a global view
  (clustering requirements into use cases, partitioning entities into
  components), the reconciler computes candidate partitions deterministically and
  hands a turn one part. The model judges within a part; it never surveys the
  whole graph. Cheap lexical machinery already exists (pair-review neighbor
  scoring, alignment shingles) and is reused.
- Dirtiness flows down `traces`: a changed section dirties its requirements
  (exists); a changed requirement dirties exactly the use case steps,
  allocations, instances, transitions, and messages tracing to it, and nothing
  else. Each stage reconciles only its dirty units, in stage order. A one-line
  docs edit touches a narrow cone through all stages, not the pyramid.

Parking, budgets, convergence, and flip detection apply per stage exactly as they
do today; `status.yaml` verdicts and pending blocks gain a stage dimension.

## Configuration

Stages and profile are project configuration, consumed by the
[stage registry](./orchestration.md#the-stage-registry):

```toml
[profile]
name = "software"        # software | organization | narrative | slides | custom
# stereotypes = [...]    # custom vocabulary when name = "custom"

[stages]                 # overrides the profile's defaults
usecases = true
domain = true
instances = false
composition = true
dynamics = false
```

Shipped default: all off, which is exactly today's compiler. The profile supplies
per-medium defaults; `[stages]` overrides them. The dogfood turns stages on one at
a time as they land.

## Prior art this leans on

Standards and methods:

- EARS (exists) and ISO/IEC/IEEE 29148 for the requirements layer; DO-178C and
  ISO 26262 for the traceability discipline (bidirectional links, no orphan
  requirement, no unjustified artifact, checked by tools because manual matrices
  do not scale).
- UML 2.5 as the diagram vocabulary, taken whole but as projections: practitioner
  surveys (Dobing and Parsons; Petre, ICSE 2013) say class, sequence, and state
  carry the real use, which is why those are the stored stages and the rest are
  renderings over the same facts. UML profiles, the specification's own
  extension mechanism, carry the medium specialization; this is the authentic
  answer to a metamodel that must also describe an org chart or a novel.
- Cockburn use cases, kept lean; heavyweight ceremony fields see little real use.
- DDD: bounded contexts map onto existing scopes; Context Mapper's CML proved the
  pattern language formalizes as a textual DSL.
- C4: its notation is dropped in favor of UML components with nesting (an earlier
  draft of this plan chose C4; the swap buys a medium-generic vocabulary and
  deeper LLM training coverage). Its load-bearing insight, one model with
  diagrams as projections, is kept as the first principle of
  [ir-graph](./ir-graph.md#one-graph-many-diagrams), and a C4-styled rendering
  stays a docsgen option.
- ADRs (Nygard): cheap, append-only, supersede-never-rewrite, so reconciliation
  is trivial. The prompt/answer machinery makes them interactive.
- SysML v2 (finalized 2025): its normative textual notation and git-diffable
  models are the strongest precedent for models-as-text; its
  `satisfy`/`verify`/`refine`/`derive` vocabulary is the trace-edge taxonomy
  above. Its full KerML metamodel is deliberately not adopted.
- Statecharts (Harel, SCXML, XState) for the `sm:` shape: the highest semantic
  density per byte of any modeling artifact, well represented in training data,
  and deterministically checkable. Full formal methods (TLA+, Alloy) are not
  LLM-tractable enough in 2026 to be a stage; research on LLM-generated TLA+
  shows fluent syntax with wrong semantics.

Spec-driven development tools (2024-2026), and what their reception teaches:

- Kiro: `requirements.md` with EARS acceptance criteria → `design.md` →
  `tasks.md`, with approve-gates. Validated EARS as the LLM-era requirements
  notation and per-feature scoping as a workable grain. Criticized for rigidity
  and for specs as throwaway per-feature documents with no persistent model
  behind them: no cross-feature consistency, no reconciliation. The graph is
  precisely that missing piece.
- GitHub Spec Kit: constitution → specify → plan → tasks → implement. Heavily
  criticized for spec volume (thousands of spec lines per hundreds of code
  lines, hours where iterative prompting takes minutes). Lesson: generated spec
  volume is a cost, not an asset. The graph must stay denser than the prose it
  reconciles, never a bloated restatement. The constitution idea (project-wide
  principles every feature inherits) maps to root-document requirements, which
  already participate in every reconciliation.
- OpenSpec: delta specs merged into living specs, brownfield-first, and the
  strongest external validation of jazyk's thesis: the market converged on
  persistent-spec-plus-deltas over regenerate-per-feature. Jazyk automates the
  merge step OpenSpec makes humans do.
- BMAD: persona agents whose Scrum Master shards the PRD and architecture into
  story files that each embed all upstream context they need. That sharding is
  the manual version of jazyk's budgeted context packs; the criticism (artifacts
  stay unlinked prose, drift goes undetected) is again the missing graph.
- Tessl: the pure spec-as-source bet, still unproven; jazyk's
  generate-then-verify-against-the-graph posture is the hedged version that does
  not require the full bet to pay off.
- The waterfall critique of the whole category (Kent Beck: writing the full spec
  first assumes implementation teaches you nothing) is answered structurally by
  [stages are not phase-gates](#stages-are-not-phase-gates).

The MDA lesson (models drift, round-trip engineering never worked, generated-code
promises break on the first hand edit) is answered by the three provenance kinds,
the ratification loop, and the verification ledger: verification links survive
hand edits; round-tripping is not attempted.

## Landing

No phases: the whole design lands as one coordinated change. Docs first, per the
repo rule: the `docs/compiler/` tree is rewritten to the new design (model pages
per node kind, goal pages with contract paragraphs as payload files, skills as
payload files), then `bootstrap` follows the docs. Validation is what holds the
line: benchmark cases per goal kind gating executor profiles, runs on
`bootstrap/example/f1` and `f2`, and the dogfood compiled under the full ladder.
The non-software profiles get their own fixture projects (`example-slides`
exists; an organization and a narrative fixture join it). Stages remain
runtime-optional per project through `[stages]` and the profile; that is
configuration, not a delivery sequence.

## Open questions

- Does the use case node earn its keep for small projects, or should stage 2
  start as a docsgen projection (scenarios rendered from requirement clusters)
  before becoming stored IR? Rendering first would be cheaper to validate.
- Does the instance stage earn its keep outside media rich in examples? It is
  optional and off by default everywhere; the conformance check may justify
  defaulting it on once measured.
- Per-stage model choice: higher stages want stronger models (composition
  judgment vs extraction). Handled in the orchestration plan via per-stage agent
  profiles; the open question is defaults.
- How much of stage 3 belongs in stage 1: attributes and cardinality could land
  as ordinary extraction improvements without a stage boundary at all.
- Whether the glossary deserves stage status. Entities plus scopes already are a
  per-context glossary (docsgen renders it), so this plan treats it as existing
  machinery; the alternative reading is that promoting it (every noun phrase in
  every stage must resolve to a term, undefined term is a diagnostic) is the
  cheapest new check with the highest leverage, and could land before stage 2.
- Where the docs-split vs graph-split line sits: the same size pressure can be
  answered by splitting a section or splitting an entity, and which is right is
  subjective. Declared an experiment; see
  [size limits](./ir-graph.md#size-limits).
- Profile inference: the medium decision is already recorded once per
  deliverable; whether the profile should default from it (a slide-deck medium
  implies the slides profile) or stay an explicit setting.

## References

- [Kiro specs](https://kiro.dev/docs/specs/) (EARS acceptance criteria, three-stage
  flow) and its
  [criticism](https://dev.to/aws-builders/brilliant-broken-and-frustrating-my-deep-dive-into-amazons-kiro-ai-ide-the-flawed-junior-gn5).
- [Spec Kit criticism](https://github.com/github/spec-kit/discussions/1784),
  [OpenSpec vs Spec Kit](https://hashrocket.com/blog/posts/openspec-vs-spec-kit-choosing-the-right-ai-driven-development-workflow-for-your-team),
  [Fowler on the SDD tools](https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html),
  [Brooker on SDD vs waterfall](https://brooker.co.za/blog/2026/04/09/waterfall-vs-spec.html).
- [UML 2.5.1 specification](https://www.omg.org/spec/UML/2.5.1/) (the diagram
  taxonomy and the profile mechanism).
- [SysML v2 finalized](https://www.omg.org/news/releases/pr2025/07-21-25.htm) and its
  [textual notation](https://github.com/Systems-Modeling/SysML-v2-Release).
- [Structurizr](https://docs.structurizr.com/) (one model, many views),
  [Context Mapper](https://contextmapper.org/) (DDD as a textual DSL).
- [Why MDE fails](https://www.infoq.com/articles/8-reasons-why-MDE-fails/).
- UML practitioner usage:
  [Dobing and Parsons](https://www.researchgate.net/publication/220373821_Dimensions_of_UML_Diagram_Use_A_Survey_of_Practitioners),
  [Ozkaya 2020](https://www.sciencedirect.com/science/article/abs/pii/S0950584920300252).
- Traceability discipline:
  [DO-178C](https://www.parasoft.com/learning-center/do-178c/requirements-traceability/),
  [ISO 26262](https://www.parasoft.com/learning-center/iso-26262/requirements-traceability/).
