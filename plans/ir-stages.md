# Plan: the software engineering process as IR

Status: proposal for iteration. Read with [ir-graph](./ir-graph.md) (the graph
and every diagram), [agent](./agent.md) (goals and sessions),
[ripple](./ripple.md) (propagation and observing), [orchestration](./orchestration.md)
(implementation notes).

Jazyk compiles prose documentation into a persistent semantic graph and consumes
the graph downstream. This proposal extends the graph to carry the whole
engineering process in three authored kinds (entities, requirements, views) and
their derivations, renders every UML diagram type from it, and converges it with
one generic agent resolving goals the harness derives. Every fact answers to a
sentence in the documents, and every change replays as a causal chain a human
can inspect.

## Doctrine

- The documents are the source of truth. Anything the deliverable needs that
  the documents do not state is an ambiguity: resolved with best judgment,
  recorded, and pushed back toward the documents
  ([ambiguity](./ripple.md#ambiguity)).
- One persistent graph per project, edited in place, never regenerated. Ids are
  minted once and immutable; natural keys make retries harmless; merges leave
  redirects. Nothing enters the graph without provenance.
- Division of labor is strict. The harness owns everything that must never be
  wrong: parsing, identity, dirtiness, goal derivation, scheduling, validation
  gates, derived data, budgets, causality. The model owns everything that needs
  judgment: extraction, same-vs-different, severity, wording, abstraction.
- Diagrams are projections. There are no diagram elements, only graph nodes and
  view definitions; a rendering cannot drift from the graph because it is
  recomputed from it.
- Incrementality: a no-op rebuild derives zero goals and makes zero LLM calls.
  A change reaches exactly its cone, never the pyramid.

## The two stages

A build has two stages, and only two.

Compile: bring the graph in line with the documents. The reconciler derives
correctness [goals](./agent.md#goals) (dirty sections, changed statements
needing re-judgment, dangling references, instance conformance, stale ledger
rows) and the agent resolves them, batch by batch, to a fixed point. Nothing
restructures here: compile runs its full course so the graph first reflects
what the documents actually say.

Cleanup: restructure holistically, from the converged graph. Only now do the
cleanup goals derive: the limit goals (an entity over its requirement cap, a
view over its member or edge cap), and the judgment sweeps (lookalike
duplicates, missing edges, view curation, the partition proposal). A session
that abstracts an entity therefore sees all of its requirements at once, not a
stream of partial states. If
a compile doubles an entity's requirements, one cleanup session splits it once,
knowing everything. Cleanup mutations can reopen compile goals (a split entity
re-enqueues reviews); the loop runs compile again for that cone, then
re-derives cleanup, with flip detection and budgets bounding the alternation.

The verdict reports both: `converged` only when compile and cleanup are both
quiet (blocked-on-human and standing optional advice ride the verdict as
counts). Ordering inside compile is small and internal: alignment before
ingest, ingest before judgment, judgment before ledger work, document link
levels ordering ingest, roots first.

The optional capabilities are configuration, not phases: features toggle which
extraction, views, checks, and goals are active, with defaults from the
[profile](./ir-graph.md#profiles).

```toml
[profile]
name = "software"        # software | organization | narrative | slides | custom

[features]               # overrides the profile's defaults
usecases = true          # flow views + behavior coverage check
instances = false        # instantiation extraction + conformance checks
composition = true       # partition goal + component and deployment views
dynamics = true          # transition facets + derived state machines + sequence views
```

Everything off is the current compiler. Verification has no toggle: it is
driven by the ledger and the gen and test commands. `[limits]` holds the size
thresholds. Modes, releases, workers, and leases stay with the control plane as
implemented.

## What each feature adds

- Always on: entities, requirements, edges, derived relationships, coverage,
  reachability, duplicate and contradiction judgment, class and package views,
  the glossary.
- `usecases`: flow clustering (behavior requirements grouped by actor and
  trigger, deterministically), use case and activity views over the clusters, a
  coverage check (every behavior requirement in some flow view or marked, every
  failure-mode requirement represented in a branch or flagged as a missing
  error path).
- `instances`: instantiation extraction from example sections and fixtures,
  attribute values, object views, conformance checks (values against declared
  attributes, links against relationship types and cardinalities; a violation
  names both sentences).
- `composition`: component and deployment views over stereotyped entities, the
  partition goal (a cleanup-stage session that proposes the component
  structure as derived entities plus a decision prompt when the docs do not
  state one), provider checks (every required «interface» realized by exactly
  one entity).
- `dynamics`: transition facets at extraction, derived state machines with
  their checks (reachability, determinism, event completeness), sequence,
  communication, and timing views.

Generation and verification consume all of it as designed today: the ledger,
the two test kinds, derived statuses. Generation can group by component
(«service» entities and their subtrees) when composition is on; the entity is
the unit otherwise. The graph stops at interface-level requirements: below
that altitude the code is the source of truth, the ledger verifies it against
the graph, and round-trip engineering is not attempted.

## Landing

One coordinated change, docs first per the repo rule: the `docs/compiler/` tree
is rewritten to this design (model pages per kind, goal pages with contract
paragraphs as payload files, skills as payload files), then `bootstrap` follows
the docs. Validation holds the line: `cargo test`, runs on
`bootstrap/example/f1` and `f2`, and the dogfood compiled with the features on.
The non-software profiles get fixture projects (`example-slides` exists; an
organization and a narrative fixture join it). Benchmarking per goal kind
follows once the design has been exercised.

## Open questions

- The entity natural key under containment: `name` plus `scope` collides when
  two parents each contain a same-named child (two modules, each with a
  `Config`), and an upsert would wrongly merge them. Whether scope generalizes
  to a namespace derived from the containment chain, or `parent` joins the key,
  is undecided; a wrong merge is the failure to avoid.
- Where the docs-split vs graph-split line sits: the same size pressure can be
  answered by splitting a section or splitting an entity. A declared
  experiment.
- Flow view ordering: member order versus document order of the requirements as
  the default flow, and how much reordering judgment `curate-view` should
  exercise over the deterministically clustered flows.
- Whether the glossary deserves promotion: entities plus scopes already are a
  per-context glossary, but making every noun phrase resolve to a known entity
  (undefined term as diagnostic) is the cheapest check with the highest
  leverage.
- Per-feature executor defaults, and whether the profile should default from
  the recorded medium decision.
- Ratification review granularity: proposals could aggregate per target
  document (one reviewed draft carrying many facts) rather than one prompt per
  fact.
- Whether goal derivation needs a cheap dirtiness probe beside the full
  computation, serving `await_changes` and watch mode; and whether the change
  records outgrow `status.yaml` into sibling files.

## References

- [UML 2.5.1 specification](https://www.omg.org/spec/UML/2.5.1/) (the diagram
  taxonomy and the profile mechanism);
  [SysML v2](https://www.omg.org/news/releases/pr2025/07-21-25.htm) (models as
  git-diffable text; its dependency kinds informed the relationship types).
- Practitioner UML usage:
  [Dobing and Parsons](https://www.researchgate.net/publication/220373821_Dimensions_of_UML_Diagram_Use_A_Survey_of_Practitioners),
  [Ozkaya 2020](https://www.sciencedirect.com/science/article/abs/pii/S0950584920300252):
  class, sequence, and state carry the real use.
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
  [Kiro](https://kiro.dev/docs/specs/),
  [Spec Kit criticism](https://github.com/github/spec-kit/discussions/1784),
  [OpenSpec](https://hashrocket.com/blog/posts/openspec-vs-spec-kit-choosing-the-right-ai-driven-development-workflow-for-your-team),
  [Fowler's comparison](https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html),
  [Brooker on SDD vs waterfall](https://brooker.co.za/blog/2026/04/09/waterfall-vs-spec.html).
- [Temporal Rust SDK](https://temporal.io/changelog/rust-sdk-public-preview),
  [Restate](https://restate.dev/), [Rig](https://github.com/0xplaygrounds/rig),
  [OTel GenAI conventions status](https://john-hodge.com/blog/opentelemetry-genai-semantic-conventions/).
