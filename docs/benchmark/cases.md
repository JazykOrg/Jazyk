# Cases

A case is one predefined test: a fixture, one goal to resolve in one session, and
deterministic assertions about what the session did. Each case lives under
[`cases/`](./cases) as a markdown file: an H1, a paragraph stating the skill it grades,
and a fenced `yaml` block holding the case definition per
[`case.schema.yaml`](./case.schema.yaml). A file may hold more than one `yaml` block;
each block is one case.

Cases exist for ten goal kinds: the four the landing graded first
(`reconcile-section`, `review-entity`, `generate`, `verify`) and six added after it
(`rejudge-pair`, `declare-edges`, `dedupe-candidates`, `curate-view`, `split-view`,
`abstract-entity`). The kinds still without a case are listed under
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
    document path. The session holds one batch: every section of that document, in
    document order, as it would in a build
    ([batching](../compiler/reconciler.md#batching)).
  - [`review-entity`](../compiler/goals/review-entity.md): `target` is an entity id.
  - [`generate`](../compiler/goals/generate.md): `target` is an entity id; the session
    generates into a temp deliverable.
  - [`verify`](../compiler/goals/verify.md): `target` is a requirement id; the
    llm-judge path over a planted ledger row.
  - [`rejudge-pair`](../compiler/goals/rejudge-pair.md): `target` is a requirement
    pair, `req:a~req:b`, the smaller id first.
  - [`declare-edges`](../compiler/goals/declare-edges.md): `target` is a requirement
    id.
  - [`dedupe-candidates`](../compiler/goals/dedupe-candidates.md): `target` is an
    entity pair, `ent:a~ent:b`, the smaller id first.
  - [`curate-view`](../compiler/goals/curate-view.md) and
    [`split-view`](../compiler/goals/split-view.md): `target` is a view id.
  - [`abstract-entity`](../compiler/goals/abstract-entity.md): `target` is an entity
    id, or `scope:<scope>` for the fan-out on a scope's top level
    ([the scope root](../compiler/concepts/levels.md#the-scope-root)).
  These six kinds derive their goal from the seeded fixture; see
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
    - `coverage`: map of section reference → state.
  - `deliverable` (optional, verification cases): map of deliverable-relative path →
    content, the implementing files the judged row names. The harness builds the
    ledger row and its criteria file from the target requirement.
  - `lint` (optional): project [lint rules](../compiler/project-settings.md#docs)
    the session runs under, as `warnings` and `errors` lists.
- `assert`: an array of checks. All must pass. Each check is deterministic and runs over
  the staged mutations and the resulting graph. Patterns are regular expressions,
  matched case-insensitively. See [checks](#checks).

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
- `requirementExists {statementPattern, entity}`: a requirement whose `statement`
  matches the pattern references the entity.
- `requirementCount {entity?, min?, max?}`: the number of requirements, optionally
  only those referencing the entity, is within bounds.
- `relationshipExists {a, b, type?}`: a derived relationship between the two entities,
  optionally carrying the type.
- `edgeDeclared {requirement, a, b, type?}`: the requirement's `edges` carry an edge
  from `a` to `b` (direction counts), optionally of the type.
- `edgeAbsent {requirement, a, b}`: the requirement's `edges` carry no edge between
  the two in either direction.
- `mutationCount {min?, max?}`: the number of mutations the session staged is within
  bounds.
- `diagnosticExists {rule, subject?, subjects?}`: an open diagnostic with the rule
  exists; with `subject`, its subjects include it; with `subjects`, its subjects
  include every listed id.
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
[verify-judge](./cases/verify-judge.md).

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
sandbox graph. The six other kinds carry a `change` the harness computes (a fan-out's
coupling candidates, a pair's shared entity and tokens, a lookalike score, a query's
new matches, a crossed limit), so their goal must be derived, never assembled:

- The fixture is seeded through one commit: every entity, requirement, and view of
  `given.graph` lands as a `create` mutation in one changeset, parents before
  children, under the same commit path a build uses
  ([mutations](../compiler/graph.md#mutations)). The commit writes the change records
  the fixture implies: `threshold-crossed` on a level or a view over a limit,
  `edges-missing` on a multi-entity requirement without edges, `requirement-created` on
  every requirement, `query-match` on a seeded query view whose query picks up seeded
  entities; the lookalike score is computed at derivation. A fixture the commit
  refuses (an unknown parent, a quote that does not locate) fails the case as a
  fixture error, never as a model failure.
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

Verification tier:

- [verify-judge](./cases/verify-judge.md): judged pass and judged fail, as a pair.
