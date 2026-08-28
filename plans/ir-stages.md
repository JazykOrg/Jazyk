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
- No profiles, no feature flags. The model adapts to the medium it reads
  ([any medium](./ir-graph.md#any-medium)), and machinery activates on what
  the graph contains, never on configuration.
- Incrementality: a no-op rebuild derives zero goals and makes zero LLM calls.
  A change reaches exactly its cone, never the pyramid.

## Compile and garbage collection

Goals come in two classes, and a build interleaves them in bursts.

Compile goals bring the graph in line with the documents: dirty sections,
changed statements needing re-judgment, dangling references, instance
conformance, stale ledger rows. Garbage collection (GC) goals restructure and
tidy: decoupling, splitting, combining. An entity over its requirement cap, a
view over its member cap, lookalike duplicates, missing edges, view curation.
GC also names the store's deterministic sweep that already runs at commit
(orphaned facts deleted, tombstone redirects left): the sweep is the
mechanical half, the GC goals the judgment half.

One rule ties the classes together: a GC goal becomes ready only when no
compile goal is open in its target's cone. Restructuring therefore always sees
settled content (an entity is abstracted knowing every requirement this build
gives it, never a stream of partial states), but nothing waits for a global
phase: as each locality's compile goals settle, its GC goals become ready, and
the scheduler runs them right there, often in the session that just finished
the locality, while the graph is loaded and the thinking is warm. A build is
bursts of compile and GC, cone by cone.

GC mutations can reopen compile goals (a split entity re-enqueues its
reviews); the loop runs compile for that cone and returns, with flip detection
and budgets bounding the alternation. The verdict reports both classes:
`converged` only when no mandatory goal of either class is open or failed and
the checks pass, with blocked-on-human and standing optional advice riding as
counts. Ordering inside compile stays small and internal: alignment before
ingest, ingest before judgment, judgment before ledger work, document link
levels ordering ingest, roots first.

## What the content activates

There are no profiles and no feature flags. The model adapts to what it reads,
and machinery activates on what the graph contains:

- Always: entities, requirements, edges, derived relationships, coverage,
  reachability, duplicate and contradiction judgment, class and package views,
  the glossary.
- Stereotypes are free-form judgment («service», «character», «department»),
  recorded with provenance like any fact; nothing enumerates the allowed
  vocabulary.
- Transition facets exist wherever statements describe state changes; wherever
  they exist, the derived state machines and their checks (reachability,
  determinism, event completeness) exist too.
- Instantiation edges exist wherever prose gives worked examples; wherever
  they exist, object views and conformance checks run.
- Flow views derive wherever behavior requirements cluster around actors
  (deterministic clustering by shared actor and trigger tokens); the coverage
  check (every behavior statement placed in a flow or marked, every
  failure-mode statement represented in a branch or flagged) rides with them.
- Component and deployment views derive wherever containment and
  interface-like structure exist, stated by prose or introduced by GC
  abstraction; the provider check (a required «interface»-like entity realized
  by exactly one provider) rides with them.

The model decides what a document's medium calls for, the same way it already
decides what a sentence obliges. What it extracts determines what derives,
renders, and gets checked; configuration never gates it.

Generation and verification consume all of it as designed today: the ledger,
the two test kinds, derived statuses. Generation can group by component
(«service»-like entities and their subtrees) where that structure exists; the
entity is the unit otherwise. The graph stops at interface-level requirements:
below that altitude the code is the source of truth, the ledger verifies it
against the graph, and round-trip engineering is not attempted.

## Configuration

There are no knobs: no profile, no feature flags, no stage toggles, and the
size limits are built into the binary
([the registry](./ir-graph.md#size-limits)), tweaked as dogfooding teaches and
possibly exposed later. Modes, releases, workers, and leases stay with the
control plane as implemented.

## Landing

The execution checklist lives in [implementation](./implementation.md).

One coordinated change, docs first per the repo rule: the `docs/compiler/` tree
is rewritten to this design (model pages per kind, goal pages with contract
paragraphs as payload files, skills as payload files), then `bootstrap` follows
the docs. Validation holds the line: `cargo test`, runs on
`bootstrap/example/f1` and `f2`, and the dogfood compiled in full. Non-software
fixture projects prove the medium adaptation (`example-slides` exists; an
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
- Per-goal-kind executor defaults.
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
