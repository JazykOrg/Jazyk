# Plan: the software engineering process as IR

Status: draft for iteration. Companion plan: [orchestration](./orchestration.md), which
defines the stage registry this plan's stages plug into.

## The idea

Today the graph holds one semantic layer: entities and EARS requirements, with
relationships derived from requirement `edges`. Generation jumps from EARS statements
straight to code. Everything between (who uses the system and for what, how the domain
is structured, what components exist, how they talk, what state they hold) is implicit,
decided fresh inside each generation turn, and recorded nowhere.

The plan: model the software engineering process itself as ordered IR stages in the
same graph. Requirements stay the foundation. Above them, each stage is a set of node
kinds reconciled by its own turn kinds, traced to the stage below, checked
deterministically, and rendered as diagrams on demand. UML is the vocabulary, not the
storage format: diagrams are projections of the graph, never stored drawings.

## What carries over unchanged

The current design's core decisions apply to every new stage:

- One persistent graph, edited in place, never regenerated. New stages are new node
  kinds in the same store, behind the same gates, journal, redirects, and GC.
- Identity: ids minted once, natural keys for upserts, merges leave redirects.
- Division of labor: the harness owns dirtiness, scheduling, validation, and derived
  data; the model owns extraction and judgment.
- Provenance: nothing enters the graph without a source. See
  [two provenance kinds](#two-provenance-kinds) below.
- Incrementality: a no-op rebuild makes zero LLM calls at every stage.

## Two provenance kinds

Requirements are extracted: their provenance is a verbatim `quote`. Higher stages are
increasingly synthesized: the documents rarely state a full interface signature or a
state machine transition table. The graph must record which is which.

- `quote` provenance: extracted from prose, located whitespace-insensitively, exactly
  as today. Used whenever the documents state the fact ("the gateway is built with
  Go", "orders flow through the queue").
- `derived` provenance: synthesized from upstream IR nodes. Records the upstream node
  ids and the `reasoning`. A derived fact is compiler-invented until ratified.

Derived facts feed back to the documents the way
[forced decisions](../docs/consumers/gen.md#forced-decisions) already do: docsgen
proposes the synthesized decision as prose, the owner accepts or edits, and the next
reconcile turns the fact into a quoted one. The documents stay the source of truth;
the IR converges toward being fully stated in them. This is the countermeasure to the
classic model-driven-engineering failure: models that drift from reality. Here the
model cannot drift silently, because every derived fact either gets ratified into
prose or stays visibly marked as invented.

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

Stages in dependency order. Each names its unit of work (the divide-and-conquer grain),
what it traces to, and its deterministic checks. Stages are opt-in per project
(see [configuration](#configuration)); the default set is today's behavior.

### Stage 0: sections (exists)

Structural, deterministic. Unchanged.

### Stage 1: requirements and entities (exists)

Entities, EARS requirements, derived relationships, coverage, diagnostics. Unchanged,
with two small extensions already on the [TODO](../docs/TODO.md):

- Cardinality on derived relationship edges (`1..*`, `0..1`), declared on requirement
  `edges`, promoted like `type`.
- Entity `attributes`: `{name, type?, quote}` captured when prose states them ("an
  order carries a total and a currency"). Attributes are facts about structure, not
  requirements; a statement of behavior stays a requirement.

A derived facet, not a stored field: each requirement classifies as functional or as a
quality attribute (performance, security, reliability, usability, per ISO 25010) from
its pattern and wording, the same way behavior-vs-constraint derives from the EARS
pattern today. Consumers read it off; nothing new to maintain. One deterministic check
rides on it: a quality requirement whose response carries no measurable bound ("shall
be fast") draws a warning, because an unmeasurable quality statement cannot be
verified (the SEI quality-attribute-scenario rule: the response measure is what makes
an NFR testable).

- Unit of work: one document (exists).
- Checks (exist): coverage, reachability, flip detection, duplicates, contradictions.

### Stage 2: use cases

The behavioral grouping layer: who does what for which goal, as
[Cockburn-style](https://en.wikipedia.org/wiki/Use_case) use cases kept deliberately
lean.

- Node kind `usecase:<slug>`:
  - `name`, `actor` (entity ids), `goal`
  - `preconditions`: requirement ids or short text with provenance
  - `steps`: ordered `{text, requirements: [req ids]}`; every step traces to the
    requirements it realizes
  - `extensions`: `{condition, steps}` for the unwanted-behavior branches (these trace
    to `If ... then` requirements)
  - provenance per the two kinds; `confidence`, `reasoning`
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
- Renders as: a use case index per actor, Gherkin-shaped scenarios. EARS and Gherkin
  are near-isomorphic (`When <trigger> ... shall <response>` maps onto
  `Given/When/Then`), so the scenario rendering is close to mechanical and the
  scenario-to-requirement `verifies` link is exactly checkable.

### Stage 3: domain model

The class-diagram layer, on the existing entities rather than beside them. DDD
vocabulary maps onto machinery that already exists:
[scopes](../docs/compiler/concepts/scopes.md) are bounded contexts; entities and
their typed relationships are the model.

- Extensions to existing kinds, no new node kind:
  - entity `attributes` (stage 1 extension) refined with types where stated,
  - relationship `cardinality`,
  - entity `role`: `aggregate-root`, `value`, `actor`, `service`, or unset. Derived
    provenance allowed; prose wins when it states one.
  - invariants are not a new kind: an invariant is a ubiquitous EARS requirement.
    The domain turn may add the `edges` and attributes a statement implies, never a
    parallel invariant store.
- Unit of work: one scope (bounded context), or the public scope partitioned by
  relationship-connected clusters when it is large. The reconciler computes the
  partition; a turn never sees more than one cluster.
- Trace: attributes and roles carry provenance to requirements or quotes.
- Checks:
  - cardinality contradictions (two requirements implying different cardinalities on
    one edge) become diagnostics,
  - same-named entities across scopes stay distinct without diagnostic (exists),
  - an aggregate-root cycle (A composed of B composed of A) is an error.
- Renders as: one Mermaid class diagram per scope in docsgen (the relationships view
  grows attributes and cardinality).

### Stage 4: architecture

The C4-flavored layer: components, their interfaces, and the decisions behind them.
This is where generation stops re-deciding structure on every run.

- Node kind `comp:<slug>`: `name`, `kind` (`container` or `component`), `parent`
  (containers hold components), `responsibilities` (free text), provenance.
- Node kind `iface:<slug>`: `name`, `owner` (component id), `operations`:
  `{name, inputs, outputs, errors}`, provenance. An operation is not an entity
  (the entity doctrine stands: what the system does is not a concept); it lives
  here, in the contract layer built for it.
- Allocation: `satisfies` trace edges: component → the requirements and use cases it
  answers. Every allocation carries provenance, staged by the architecture turn.
- Node kind `adr:<n>`: an architecture decision record. `question`, `options`,
  `decision`, `status` (`proposed`, `accepted`, `superseded` with pointer),
  `consequences`, `subjects` (the nodes it governs). ADRs reuse the
  [diagnostic prompt and answer](../docs/compiler/model/diagnostic.md#prompts)
  machinery for the proposed-to-accepted flow: a `proposed` ADR is a question to the
  owner, rendered everywhere diagnostics with prompts render today. A decision the
  documents state is born `accepted` with quote provenance; a decision the model had
  to invent is born `proposed` with derived provenance.
- Unit of work: one component with its neighbors' interfaces (summarized). The first
  architecture build runs a single root turn that proposes the component partition
  from the derived relationship graph plus explicit prose statements, as `proposed`
  ADRs; component detail turns run after the partition is accepted or auto-accepted
  in `auto` mode.
- Trace: component `satisfies` requirements and use cases; interface operations
  `satisfy` the requirements they realize; ADR `constrains` its subjects.
- Checks:
  - every requirement is satisfied by at least one component, or carries a stage
    coverage mark (`cross-cutting` with note),
  - an interface operation no requirement realizes is a warning (invented surface),
  - every component-to-component relation is exercised by at least one use case that
    crosses it; an unexercised relation is invented structure,
  - a change that contradicts an `accepted` ADR is a diagnostic (flip detection for
    decisions),
  - allocation respects scopes: a component satisfying requirements from two bounded
    contexts draws a warning naming the split,
  - `pinned-fact-drift` generalizes: interface operation names that appear in no
    bound file once the ledger exists.
- Renders as: C4 container and component diagrams (Mermaid), an ADR log, a
  traceability matrix (requirement × component) as a docsgen table.

### Stage 5: behavior dynamics (optional, narrow triggers)

Only where the cheap signals say the structure exists. Most entities need no state
machine; most use cases need no interaction spec. The reconciler derives the work,
so the stage costs nothing when the triggers are absent.

- Node kind `sm:<entity-slug>`: states, transitions `{from, to, trigger, guard?,
  requirements}`. Derived only for entities referenced by two or more state-driven
  (`While ...`) or unwanted-behavior requirements. Every transition traces to the
  requirements that state it; a transition no requirement backs is derived
  provenance, feeding back to docs like any invented fact.
- Node kind `ixn:<usecase-slug>`: an interaction spec: participants (component ids),
  messages `{from, to, operation, requirements}`. Derived only for use cases whose
  steps allocate to two or more components.
- Unit of work: one entity's machine; one use case's interaction.
- Checks (this is where the ladder pays off, all deterministic):
  - every interaction message names an existing operation on an interface the target
    component owns,
  - every transition trigger traces to an event-driven or state-driven requirement,
  - unreachable states and states with no exit (unless terminal) are warnings,
  - event completeness: every event named by a `When`-clause requirement on the
    entity is handled or explicitly ignored in every state. An unhandled
    event-state pair is a requirements gap detector, not a modeling nicety,
  - nondeterminism: two transitions from one state on one trigger with overlapping
    guards is an error.
- Renders as: Mermaid state diagrams and sequence diagrams.

### Stage 6: verification (exists, gains inputs)

Binding, generation, and verification stay as designed. What changes with the stages
above enabled:

- The unit of generation can follow architecture: one component instead of one
  entity, its parts ordered by interfaces. Entity-unit generation remains the
  default and the fallback when stage 4 is off.
- Test derivation reads scenarios: a use case's steps and extensions shape the
  acceptance tests for its requirements, and the EARS-to-Gherkin mapping makes the
  derivation near-mechanical; the EARS-pattern-to-test-shape rule stays for
  requirements no use case traces.
- The traceability matrix (requirement → use case → component → files → test →
  verdict) is a docsgen projection over existing edges plus the ledger. Nothing new
  is stored.

### What is deliberately skipped

- Activity diagrams: use case steps carry the same content at this altitude.
- Deployment diagrams: recorded only if prose states topology; a `deployment` facet
  on containers suffices, no stage.
- Full UML: the 14 diagram types are a rendering vocabulary; practitioner usage
  concentrates on class, sequence, state, and use case, which is what the ladder
  stores semantics for.

## Divide and conquer, stated once

Every stage obeys the same three rules, which is what keeps a large project inside
small turns:

- Bounded unit of work: a turn sees one document, one cluster, one scope, one
  component, one entity, one use case. The unit's pack is assembled by the context
  engine under the same budgets and expansion handles as today. The `traces` axis
  gets hop quotas like the existing three.
- Deterministic pre-partitioning: whenever a stage transition needs a global view
  (clustering requirements into use cases, partitioning entities into components),
  the reconciler computes candidate partitions deterministically and hands a turn
  one part. The model judges within a part; it never surveys the whole graph. Cheap
  lexical machinery already exists (pair-review neighbor scoring, alignment
  shingles) and is reused.
- Dirtiness flows down `traces`: a changed section dirties its requirements (exists);
  a changed requirement dirties exactly the use case steps, allocations, transitions,
  and messages tracing to it, and nothing else. Each stage reconciles only its dirty
  units, in stage order. A one-line docs edit touches a narrow cone through all
  stages, not the pyramid.

Parking, budgets, convergence, and flip detection apply per stage exactly as they do
today; `status.yaml` verdicts and pending blocks gain a stage dimension.

## Configuration

Stages are project configuration, consumed by the
[stage registry](./orchestration.md#the-stage-registry):

```toml
[stages]
usecases = true
domain = true
architecture = true
dynamics = false
```

Default: all off, which is exactly today's compiler. The dogfood turns them on one at
a time as they land.

## Prior art this leans on

Standards and methods:

- EARS (exists) and ISO/IEC/IEEE 29148 for the requirements layer; DO-178C and
  ISO 26262 for the traceability discipline (bidirectional links, no orphan
  requirement, no unjustified artifact, checked by tools because manual matrices do
  not scale).
- Cockburn use cases, kept lean. Practitioner surveys (Dobing and Parsons; Petre,
  ICSE 2013) say heavyweight use cases and use case diagrams see little real use
  while class, sequence, and state diagrams dominate; the stage stores the slim
  fields only.
- DDD: bounded contexts map onto existing scopes; Context Mapper's CML proved the
  pattern language formalizes as a textual DSL.
- C4 for the architecture altitude (context/container/component, never the code
  level, which drifts). Structurizr's load-bearing idea, one model with diagrams as
  projections, is already jazyk's graph philosophy.
- ADRs (Nygard): cheap, append-only, supersede-never-rewrite, so reconciliation is
  trivial. The prompt/answer machinery makes them interactive.
- SysML v2 (finalized 2025): its normative textual notation and git-diffable models
  are the strongest precedent for models-as-text; its `satisfy`/`verify`/`refine`/
  `derive` vocabulary is the trace-edge taxonomy above. Its full KerML metamodel is
  deliberately not adopted: metamodel maximalism is how UML's 800 pages went unread,
  and LLMs are sparsely trained on it.
- Statecharts (Harel, SCXML, XState) for the `sm:` shape: the highest semantic
  density per byte of any modeling artifact, well represented in training data, and
  deterministically checkable. Full formal methods (TLA+, Alloy) are not
  LLM-tractable enough in 2026 to be a stage; research on LLM-generated TLA+ shows
  fluent syntax with wrong semantics.

Spec-driven development tools (2024-2026), and what their reception teaches:

- Kiro: `requirements.md` with EARS acceptance criteria → `design.md` → `tasks.md`,
  with approve-gates. Validated EARS as the LLM-era requirements notation and
  per-feature scoping as a workable grain. Criticized for rigidity and for specs as
  throwaway per-feature documents with no persistent model behind them: no
  cross-feature consistency, no reconciliation. The graph is precisely that missing
  piece.
- GitHub Spec Kit: constitution → specify → plan → tasks → implement. Heavily
  criticized for spec volume (thousands of spec lines per hundreds of code lines,
  hours where iterative prompting takes minutes). Lesson: generated spec volume is
  a cost, not an asset. The graph must stay denser than the prose it reconciles,
  never a bloated restatement. The constitution idea (project-wide principles every
  feature inherits) maps to root-document requirements, which already participate
  in every reconciliation.
- OpenSpec: delta specs merged into living specs, brownfield-first, and the
  strongest external validation of jazyk's thesis: the market converged on
  persistent-spec-plus-deltas over regenerate-per-feature. Jazyk automates the
  merge step OpenSpec makes humans do.
- BMAD: persona agents whose Scrum Master shards the PRD and architecture into
  story files that each embed all upstream context they need. That sharding is the
  manual version of jazyk's budgeted context packs; the criticism (artifacts stay
  unlinked prose, drift goes undetected) is again the missing graph.
- Tessl: the pure spec-as-source bet, still unproven; jazyk's
  generate-then-verify-against-the-graph posture is the hedged version that does
  not require the full bet to pay off.
- The waterfall critique of the whole category (Kent Beck: writing the full spec
  first assumes implementation teaches you nothing) is answered structurally by
  [stages are not phase-gates](#stages-are-not-phase-gates).

The MDA lesson (models drift, round-trip engineering never worked, generated-code
promises break on the first hand edit) is answered by the two provenance kinds, the
ratification loop, and the verification ledger: verification links survive hand
edits; round-tripping is not attempted.

## Migration

Docs first at every step, per the repo rule. Each phase lands with: docs pages
(model page per node kind, turn page per task kind, prompts as payload files), a
benchmark case gating the new turn kind, a run on `bootstrap/example/f1` and `f2`,
then the dogfood.

1. Stage 1 extensions: relationship cardinality, entity attributes, the NFR facet.
   Small, already half-planned in TODO.
2. The `traces` axis, per-stage coverage marks, and the stage dimension in
   `status.yaml` and the queue. No new stages yet; this is the plumbing, landing
   together with the orchestration plan's registry refactor.
3. Stage 2, use cases. First stage with derived provenance and the ratification
   loop; prove it here.
4. Stage 3, domain model.
5. Stage 4, architecture with ADRs. Generation-by-component follows once stable.
6. Stage 5, dynamics, behind its narrow triggers.

## Open questions

- Does the use case node earn its keep for small projects, or should stage 2 start
  as a docsgen projection (scenarios rendered from requirement clusters) before
  becoming stored IR? Rendering first would be cheaper to validate.
- Per-stage model choice: higher stages want stronger models (architecture judgment
  vs extraction). Handled in the orchestration plan via per-stage agent profiles;
  the open question is defaults.
- How much of stage 3 belongs in stage 1: attributes and cardinality could land as
  ordinary extraction improvements without a stage boundary at all.
- Whether the glossary deserves stage status. Entities plus scopes already are a
  per-context glossary (docsgen renders it), so this plan treats it as existing
  machinery; the alternative reading is that promoting it (every noun phrase in
  every stage must resolve to a term, undefined term is a diagnostic) is the
  cheapest new check with the highest leverage, and could land before stage 2.
- ADR auto-acceptance in `auto` mode: silent structural decisions are the thing this
  plan exists to prevent. Likely rule: `auto` accepts component detail, never the
  partition ADR.

## References

- [Kiro specs](https://kiro.dev/docs/specs/) (EARS acceptance criteria, three-stage
  flow) and its
  [criticism](https://dev.to/aws-builders/brilliant-broken-and-frustrating-my-deep-dive-into-amazons-kiro-ai-ide-the-flawed-junior-gn5).
- [Spec Kit criticism](https://github.com/github/spec-kit/discussions/1784),
  [OpenSpec vs Spec Kit](https://hashrocket.com/blog/posts/openspec-vs-spec-kit-choosing-the-right-ai-driven-development-workflow-for-your-team),
  [Fowler on the SDD tools](https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html),
  [Brooker on SDD vs waterfall](https://brooker.co.za/blog/2026/04/09/waterfall-vs-spec.html).
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
