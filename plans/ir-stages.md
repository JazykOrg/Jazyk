# Plan: the software engineering process as IR

Status: proposal for iteration. The proposal set: this file (doctrine and the
stage ladder), [ir-graph](./ir-graph.md) (the graph, diagrams, profiles),
[agent](./agent.md) (the agent and the goal system), [ripple](./ripple.md)
(convergence and observing it), [orchestration](./orchestration.md) (the
registry, executors, alternatives).

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
  and pushed back toward the documents ([ambiguity](./ripple.md#ambiguity)).
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

## The stage ladder

Stages in dependency order. Each names its unit of work, its trace edges, its
deterministic checks, and the projections it feeds. Stages are opt-in per
project with defaults from the [profile](./ir-graph.md#profiles), which also
carries the per-medium readings (services, org charts, slide decks, a novel's
characters and arcs); the shipped default is stage 1 alone.

### Stage 0: sections

Structural and deterministic: parse every matched document into a section tree
with per-section content hashes, align against the stored trees, compute the
dirty set. Unchanged from the current compiler.

### Stage 1: requirements and entities

The foundation, as implemented today, with these extensions: free-form
statements with judged facets, directional edges with cardinality, entity
attributes, roles, `parent`, and stereotypes, all as specified in
[ir-graph](./ir-graph.md).

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

## Divide and conquer

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

## Configuration

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
holds the size thresholds. Runtime modes, releases, workers, and leases stay
with the control plane as implemented.

## Landing

One coordinated change, docs first per the repo rule: the `docs/compiler/` tree
is rewritten to this design (model pages per node kind, goal pages with
contract paragraphs as payload files, skills as payload files; `reconciler.md`,
`turns.md`, and `control-plane.md` absorb goals, the registry, and the
visibility surfaces), then `bootstrap` follows the docs. Validation holds the
line: `cargo test`, runs on `bootstrap/example/f1` and `f2`, and the dogfood
compiled under the full ladder. The non-software profiles get fixture projects
(`example-slides` exists; an organization and a narrative fixture join it).
Stage toggles and profiles are runtime configuration, not a delivery sequence.
Benchmarking per goal kind follows once the design has been exercised.

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
