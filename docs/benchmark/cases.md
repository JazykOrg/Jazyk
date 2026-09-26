# Cases

A case is one predefined test: a fixture, one goal to resolve in one session, and
deterministic assertions about what the session did. Each case lives under
[`cases/`](./cases) as a markdown file: an H1, a paragraph stating the skill it grades,
and a fenced `yaml` block holding the case definition per
[`case.schema.yaml`](./case.schema.yaml). A file may hold more than one `yaml` block;
each block is one case.

Cases exist for twelve goal kinds: the four the landing graded first
(`reconcile-section`, `review-entity`, `generate`, `verify`) and eight added after it
(`rejudge-pair`, `declare-edges`, `derive-criteria`, `dedupe-candidates`,
`curate-view`, `split-view`, `abstract-entity`, `bind`). The kinds still without a case are listed under
[deferred cases](./benchmark.md#deferred-cases).

## Case format

- `name`: unique case name, usually the file stem.
- `description`: one sentence stating the skill the case grades.
- `tier`: `extraction` (the default), `review`, `structure`, `generation`, or
  `verification`. Tier scores and workflow verdicts derive from these; see
  [report](./benchmark.md#report).
- `par` (optional): `rounds`, the rounds a competent model needs; the efficiency
  ratio compares against it. Defaults per tier when omitted.
- `goal`: the goal to run. `kind` is a goal kind and `target` is the goal's target:
  - [`reconcile-section`](../compiler/goals/reconcile-section.md): `target` is a
    document path. The session holds one batch: every section of that document that
    owes a coverage mark, in document order, as it would in a build
    ([batching](../compiler/reconciler.md#batching)). A fenced block follows the
    section holding it and a bare heading owes none
    ([coverage](../compiler/compilation.md#coverage)), so neither is listed: a case
    that listed the block beside its section asked for a mark `set_coverage` stores on
    the holding section, and the gate refused the session four times for it.
  - [`review-entity`](../compiler/goals/review-entity.md): `target` is an entity id.
  - [`generate`](../compiler/goals/generate.md): `target` is an entity id; the session
    generates into a temp deliverable.
  - [`verify`](../compiler/goals/verify.md): `target` is a requirement id; the
    llm-judge path over a planted ledger row.
  - [`rejudge-pair`](../compiler/goals/rejudge-pair.md): `target` is a requirement
    pair, `req:a~req:b`, the smaller id first.
  - [`declare-edges`](../compiler/goals/declare-edges.md): `target` is a requirement
    id.
  - [`derive-criteria`](../compiler/goals/derive-criteria.md): `target` is an entity
    id. The sandbox configures a deliverable so the goal derives.
  - [`bind`](../compiler/goals/bind.md): `target` is a requirement id, unbound in the
    fixture. The session binds it against the fixture's `deliverable`.
  - [`dedupe-candidates`](../compiler/goals/dedupe-candidates.md): `target` is an
    entity pair, `ent:a~ent:b`, the smaller id first.
  - [`curate-view`](../compiler/goals/curate-view.md) and
    [`split-view`](../compiler/goals/split-view.md): `target` is a view id.
  - [`abstract-entity`](../compiler/goals/abstract-entity.md): `target` is an entity
    id, or `scope:<scope>` for the fan-out on a scope's top level
    ([the scope root](../compiler/concepts/levels.md#the-scope-root)).
  These eight kinds derive their goal from the seeded fixture; see
  [derived goals](#derived-goals).
- `given`: the fixture.
  - `docs`: map of document path → markdown text. These are the only source files the
    case sees.
  - `graph` (optional): nodes pre-seeded into the sandbox store before the session
    runs:
    - `entities`: map of id → `name`, `aliases`, `definition`, `mentions` (each a
      `section` reference and a `quote`), plus `stereotype`, `parent`, `scope`, and
      `provenance` (`{derived: {from, reasoning}}` for a grouping) where the case
      needs them.
    - `requirements`: map of id → `statement`, `entities`, and `source` (a `section`
      reference and a `quote`), plus `edges`, `transition`, and `facets` where the
      case needs them.
    - `views`: map of view id → `kind`, `title`, `members`, `query`, `collapse`,
      `excluded`, and `provenance` ([view fields](../compiler/model/view.md#fields)).
      A seeded view is curated: `default` is never set.
    - `criteria`: map of criterion id → `requirement`, `statement`, `kind`, and a
      `source` (a `section` reference and a `quote`) or a `provenance`
      ([criterion fields](../compiler/model/criterion.md#fields)).
    - `coverage`: map of section reference → state.
  - `deliverable` (optional): map of deliverable-relative path → content, written under
    the sandbox's deliverable directory before the session runs. In a verification
    case these are the implementing files the judged row names, and the harness builds
    the ledger row and its criteria file from the target requirement. In a `bind` case
    they are the deliverable as the session finds it, tests included, and no ledger row
    exists.
  - `lint` (optional): project [lint rules](../compiler/project-settings.md#docs)
    the session runs under, as `warnings` and `errors` lists.
- `assert`: an array of checks. All must pass. Each check is deterministic and runs over
  the staged mutations and the resulting graph. Patterns are regular expressions,
  matched case-insensitively. See [checks](#checks).
- `steps` (replays only): the scripted calls of a [replay](./replays.md#replay-format).
  A case with `steps` is a replay: `jazyk benchmark` skips it and `jazyk replay` runs
  it.

## Checks

Every check is one object with one key. An entity is named by id or by exact name or
alias; a view or requirement by id. Patterns are regular expressions matched
case-insensitively; an invalid pattern fails the check.

Graph checks:

- `entityExists {name}`: an entity with this exact name or alias exists.
- `entityAbsent {namePattern}`: no entity name matches the pattern.
- `entityCount {min?, max?}`: the number of entities is within bounds.
- `entityNameCount {namePattern, min?, max?}`: the number of entities whose name
  matches the pattern is within bounds. `max: 1` on a stated name says no twin of it
  was minted.
- `nodeExists {id}`: an entity, requirement, or view with this id exists, not
  tombstoned or redirected.
- `requirementExists {statementPattern, entity?}`: a requirement whose `statement`
  matches the pattern exists, referencing the entity when one is named.
- `criteriaMatching {pattern, requirement?, min?, max?}`: the number of criteria whose
  own statement matches the pattern (of one requirement, when named) is within bounds.
  `max: 0` over a pattern names a kind of criterion that must not appear.
- `attributeExists {entity, name}`: the entity carries an attribute whose name matches
  this pattern, case-insensitive. The wording of an attribute name is the model's
  ("employee count" and "employees" are the same fact), so the check matches a pattern,
  never a literal.
- `criteriaCount {requirement?, statementPattern?, min?, max?}`: the number of
  criteria, of one requirement by id or of the requirements whose statement matches
  the pattern, is within bounds. `max: 1` on a plain fact says no contrived negative
  was added.
- `requirementCount {entity?, min?, max?}`: the number of requirements, optionally
  only those referencing the entity, is within bounds.
- `relationshipExists {a, b, type?, types?}`: a derived relationship between the two
  entities, optionally carrying the type, or any one of the listed `types` (a
  containment stated in passing passes as `composition` or `aggregation`).
- `facetCount {facet, statementPattern?, min?, max?}`: the number of requirements
  carrying the facet, of those whose statement matches the pattern when one is given, is
  within bounds. `min: 1` on a scene's pattern says the scene carries `behavior`;
  `max: 0` says a statement does not carry the facet.
- `transitionSubject {entity, min?, max?}`: the number of requirements whose
  `transition` names the entity as its `subject` is within bounds, `min` 1 when omitted.
  The failure names the subjects the transitions carry instead.
- `edgeDeclared {requirement, a, b, type?}`: the requirement's `edges` carry an edge
  from `a` to `b` (direction counts), optionally of the type.
- `edgeAbsent {requirement, a, b}`: the requirement's `edges` carry no edge between
  the two in either direction.
- `mutationCount {min?, max?}`: the number of mutations the session staged is within
  bounds.
- `diagnosticExists {rule, subject?, subjects?}`: an open diagnostic with the rule
  exists; with `subject`, its subjects include it; with `subjects`, its subjects
  include every listed id. A subject is included when the diagnostic names it or names
  a requirement whose `entities` include it: a contradiction filed on two requirements
  of one entity is on that entity, the shape the judgment doctrine asks for.
- `diagnosticAbsent {rule}`: no open diagnostic with the rule exists.
- `coverageSet {section, state}`: the section's coverage state equals `state`.

Containment checks:

- `childCount {parent, min?, max?}`: the number of direct children of the entity, or
  of parentless entities for `scope:<scope>`, is within bounds
  ([levels](../compiler/concepts/levels.md#levels)).
- `parentIs {entity, parent}`: the entity's `parent` resolves to the named entity.
- `groupingOf {members, namePattern?}`: an entity with derived provenance exists whose
  `from` names exactly the listed members and whose direct children are exactly them,
  optionally with a name matching the pattern
  ([groupings](../compiler/concepts/levels.md#groupings)).

View checks:

- `viewExists {kind?, titlePattern?, excluding?}`: a view of the kind whose title
  matches the pattern exists, leaving out the view id under `excluding` (the original
  a split had to leave behind).
- `viewMember {view, member}`: the id is in the view's `members` and not in its
  `excluded`.
- `viewExcludes {view, member}`: the view's `excluded` lists the id with a note that
  is not a placeholder.
- `viewMemberOrder {view, before, after}`: both are members and `before` precedes
  `after`.
- `viewWithinLimit {view, limit}`: the view's count for the named
  [limit](../compiler/graph.md#limits) is at or under its soft threshold, recomputed
  the way the commit counts it.
- `membersAccounted {view, members}`: every listed id is still in the view's
  `members`, or in its `excluded` with a note, or hidden under an entity the view
  collapses, or a member of another view. The split-view gate's no-member-lost rule,
  checked from the fixture's original membership.

Workflow checks (generation and verification cases): `generationRecorded`,
`rowPerRequirement`, `testsPass`, `testFalsifiable {requirement, replace}`,
`verdictIs {requirement, verdict}`. See [gen-basic](./cases/gen-basic.md) and
[verify-judge](./cases/verify-judge.md). Two more grade a binding over the ledger row,
never the verdict the session claimed:

- `statusIn {requirement, statuses}`: the row's
  [derived status](../consumers/gen.md#status-is-derived-never-stored) is one of the
  listed (`verified`, `unimplemented`, `partial`, `failing`, `unverified`).
- `testsFail {requirement}`: the row records at least one programmatic test, and every
  one exits non-zero when the harness reruns it as recorded. A test that passes against
  a deliverable that carries nothing reads a description. See
  [bind-descriptor](./cases/bind-descriptor.md).

## Execution

- Each case runs in a fresh sandbox store seeded from `given`. The project graph is
  never touched.
- The harness runs exactly one session holding one goal: `goal.kind` on
  `goal.target`, with the kind's [toolset](../compiler/tools.md#toolsets), the
  session [budgets](../compiler/sessions.md#budgets), and the
  [validation gates](../compiler/graph.md#validation-gates). The session prompt is
  the one a build assembles ([the prompt](../compiler/sessions.md#the-prompt)); the
  goal's gate runs at `mark_goal_done` and at `done` exactly as in a build
  ([resolving, failing, parking](../compiler/sessions.md#resolving-failing-parking)).
- Checks run after the session commits. An aborted session fails the case with the
  abort reason; its checks are skipped and count as failed. A goal marked failed fails
  the case the same way, with the model's reason. See [runs](./benchmark.md#runs).

### Derived goals

A `reconcile-section`, `review-entity`, `generate`, or `verify` case builds its goal
from the kind and the target alone: the fixture's nodes are written straight into the
sandbox graph. The other kinds carry a `change` the harness computes (a fan-out's
coupling candidates, a pair's shared entity and tokens, a lookalike score, a query's
new matches, a crossed limit, an unbound requirement and its criteria count), so
their goal must be derived, never assembled:

- The fixture is seeded through one commit: every entity, requirement, criterion, and
  view of `given.graph` lands as a `create` mutation in one changeset, parents before
  children, under the same commit path a build uses
  ([mutations](../compiler/graph.md#mutations)). The commit writes the change records
  the fixture implies: `threshold-crossed` on a level or a view over a limit,
  `edges-missing` on a multi-entity requirement without edges, `requirement-created` on
  every requirement, `query-match` on a seeded query view whose query picks up seeded
  entities; the lookalike score is computed at derivation. A fixture the commit
  refuses (an unknown parent, a quote that does not locate) fails the case as a
  fixture error, never as a model failure. The fixture's `deliverable` files are
  written beside the commit, so a `bind` goal finds them.
- The board derives over the seeded sandbox ([goal derivation](../compiler/reconciler.md#goal-derivation))
  and the case's goal is `g:<kind>:<target>` on it, `change` and hints intact. A
  fixture that derives no such goal is a fixture error.
- Readiness is not consulted: the seeding commit leaves compile goals open in every
  cone (each section is unprocessed), and readiness is scheduling, not a precondition
  of the session ([snippets](./benchmark.md#snippets-from-a-real-project)).
- The turn runs as a snippet does, one goal with its derived change, and the checks run
  over the sandbox after its commit. The `flow-unplaced` record is written only by a
  build's closing check, so a `curate-view` case grades the `query-match` path.

## Index

A case name is the `name` field of its `yaml` block, usually the file stem. A file
holding a pair adds suffixed names: [review](./cases/review.md) holds `review` and
`review-clean`, [verify-judge](./cases/verify-judge.md) holds `verify-judge-pass` and
`verify-judge-fail`, [rejudge-pair](./cases/rejudge-pair.md) holds
`rejudge-pair-contradiction` and `rejudge-pair-duplicate`. `jazyk benchmark
[case...]` takes these names as written.

Extraction tier:

- [extract](./cases/extract.md): extraction sanity.
- [declarative](./cases/declarative.md): declarative extraction.
- [density](./cases/density.md): extraction density on plain declarative
  prose.
- [edges](./cases/edges.md): edge declaration from a sub-system list.
- [steps](./cases/steps.md): code-block extraction, one requirement per
  behavioral step.
- [navigation](./cases/navigation.md): restraint on a glossary and a
  roadmap, both non-normative.
- [reuse](./cases/reuse.md): reuse discipline.
- [converge](./cases/converge.md): convergence discipline.
- [repair](./cases/repair.md): repair.
- [enumeration](./cases/enumeration.md): a parts list is one membership statement
  with an edge per part, and each part's own description its own statement.
- [properties](./cases/properties.md): a sentence listing several properties of one
  subject is one statement with attributes.
- [criteria-density](./cases/criteria-density.md): one criterion on a plain fact, the
  negative only on a conditional.
- [narrative-behavior](./cases/narrative-behavior.md): a narrated scene carries
  `behavior`, a standing character fact `constraint`.
- [machine-subject](./cases/machine-subject.md): a character's stance changes are
  transitions on the character, never on a minted stance entity.
- [stated-containment](./cases/stated-containment.md): a place stated to stand in a
  town is a part of the town, the `parent` following.

Review tier:

- [review](./cases/review.md): review judgment, one planted contradiction and
  one clean entity.
- [review-duplicate](./cases/review-duplicate.md): rephrase-duplicate
  collapse.
- [review-lookalike](./cases/review-lookalike.md): lookalike entity merge.
- [review-lint](./cases/review-lint.md): lint application.
- [rejudge-pair](./cases/rejudge-pair.md): pair judgment, holding
  `rejudge-pair-contradiction` (two statements that cannot both hold) and
  `rejudge-pair-duplicate` (one obligation stated in two documents, both kept).
- [dedupe-candidates](./cases/dedupe-candidates.md): lookalike pair judgment,
  holding `dedupe-candidates` (one concept under two names, merged) and
  `dedupe-candidates-separate` (a shared word and nothing else, kept apart).

Structure tier:

- [abstract-entity](./cases/abstract-entity.md): the fan-out, holding
  `abstract-entity` (twelve children from three documents grouped under nine, no twin
  of the stated namesake) and `abstract-entity-namesake` (a stated process named like
  its document takes that document's entities).
- [split-view](./cases/split-view.md): a sequence view over its participant limit
  split along a section boundary, nothing dropped.
- [curate-view](./cases/curate-view.md): a query match confirmed and an instance
  excluded from a class view.
- [declare-edges](./cases/declare-edges.md): edges of a multi-entity statement,
  holding `declare-edges` (a whole-part sentence yields two composition edges) and
  `declare-edges-none` (a sentence that relates no pair stages nothing).

Generation tier:

- [gen-basic](./cases/gen-basic.md): product and manifest honesty, test
  falsifiability.
- [bind-descriptor](./cases/bind-descriptor.md): a test that reads a descriptor object
  is never bound as the evidence a requirement is met.
- [tech-constraint](./cases/tech-constraint.md): a technology constraint is bound to the
  build's evidence, never to a value the product reports about itself.

Verification tier:

- [verify-judge](./cases/verify-judge.md): judged pass and judged fail, as a pair.
