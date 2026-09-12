# Plan: acceptance criteria

Status: the design and execution plan for acceptance criteria as a node kind of their
own. Owner decision 2026-09-12: build it, with the convergence rule below (criteria gate
the ledger tier, never compile convergence). Written for a fresh session: read this file,
then the pages it names, then work the checklist in order. Commit and push after every
step.

## Start here

- Read the repo root `CLAUDE.md` first (docs first, the writing style, commit as you go).
- The concept: [criterion](../docs/compiler/model/criterion.md). The goal that writes
  them: [derive-criteria](../docs/compiler/goals/derive-criteria.md) and its contract
  under `goals/prompts/`. The method: [criteria skill](../docs/compiler/skills/criteria.md).
- Where they land downstream: [bind](../docs/consumers/bind.md),
  [gen](../docs/consumers/gen.md#tests-tie-criteria-to-the-deliverable),
  [docsgen](../docs/consumers/docsgen.md#the-requirements-document).
- The machinery they ride: [graph](../docs/compiler/graph.md) (shard, mutations, natural
  key, change records, sweep), [reconciler](../docs/compiler/reconciler.md#tiers)
  (readiness), [compilation](../docs/compiler/compilation.md#checks) (checks),
  [tools](../docs/compiler/tools.md#write-tools).

## Why

Requirements engineering keeps intent and evidence apart. A requirement states what the
system shall do or hold; an acceptance criterion states the observable evidence that it
does: concrete inputs, a boundary, the failure path, a measured threshold. Tests are the
executable form of the criteria, and the chain requirement → criteria → tests → code is
what traceability means. Jazyk has been asking one node to play both roles: a statement
is tuned to "one honest test name", a test case in the docs is extracted as a
requirement, and bind maps one requirement to one test. Splitting the evidence out
lets a requirement carry several criteria, lets a criterion quote its own sentence
(an example, a case, a threshold, in another section or document), lets the model
propose criteria the docs never wrote as derived facts with a ratification proposal, and
lets generation write one test per criterion.

## The design in one page

- A `criterion` is an authored node, `crit:<doc-stem>-<n>`, owned by exactly one
  requirement (`requirement`), with a one-condition `statement`, an optional
  `given`/`when`/`then`, a `kind` (`positive`, `negative`, `boundary`, `quality` with a
  `measure`), and one provenance like any fact: `source` (quote), `derived` (with a
  ratification proposal), or `decree`. Natural key: requirement plus normalized
  statement. A requirement's `entities` are the criterion's unless it lists its own.
- Stated criteria come out of extraction: a test case, an example with concrete values,
  a bullet giving a threshold under a rule, are criteria of the requirement they
  exercise, quoting their own sentence, not requirements of their own.
- `derive-criteria` is a tier 3 goal per entity in the generation scope: it reads the
  entity's criterion-less or revised requirements, extracts what the documents state,
  proposes derived criteria where they state nothing (each with a sentence for the docs
  and, for a value the docs do not give, an `invented-choice` prompt), and never
  restates the requirement as its own criterion.
- Readiness: `bind` on a requirement waits for at least one criterion. Compile
  convergence does not: a project nobody generates from converges with
  `criterion-missing` findings riding as information. The check is `warning` when the
  project configures a `[gen]` deliverable and `info` otherwise.
- Binding and generation key tests by criterion. A requirement's ledger row aggregates
  its criteria; the requirement verifies when every criterion is green; `existingTests`
  keys on criterion id and hash.
- Docsgen lists the criteria under each requirement with their provenance and test
  status; the diagrams do not change.

## Phases

1. Docs: this plan, `model/criterion.md`, the model, graph, schema, reconciler,
   compilation, tools, diagnostic, sessions, skills, goal page and contract, extraction
   changes, consumers pages. Commit.
2. Store: the shard, the natural key, the mutations and their gates, change records,
   journal, the sweep (a criterion goes with its requirement; a statement change on the
   requirement marks its derived criteria revised), `version` unchanged (an absent shard
   reads as empty). Tests. Commit.
3. Tools: `upsert_criterion`, `update_criterion`, `delete_criterion` in the
   reconcile-section and derive-criteria slices; the loaded set renders a requirement's
   criteria under it; `get_requirement` (if any) lists them. Extraction skill and
   reconcile-section gate: a test-case section is covered when its case is a criterion.
   Tests; f2 and example-sort compiled through the loop. Commit.
4. The `derive-criteria` goal kind: derivation, readiness, hints, gate, prompt, skill.
   The `criterion-missing` and `criterion-restates` checks. Tests; the loop on f2. Commit.
5. Bind, generate, verify per criterion; the ledger row shape; `existingTests` by
   criterion hash; docsgen. Tests; a generation run on f2 and example-sort. Commit.
6. VALIDATION.md records the measured effect (criteria per requirement, tests per
   criterion, cost).

## Checklist

- [x] Phase 1 docs land and link-check clean.
- [x] Phase 2 store with tests.
- [x] Phase 3 tools and extraction with tests (the two loop runs follow phase 4).
- [ ] Phase 4 goal kind and checks with tests and a loop run.
- [ ] Phase 5 ledger with tests and two generation runs.
- [ ] Phase 6 VALIDATION.md.

## Decisions taken

- Criteria are nodes, not a facet list on the requirement: they quote their own
  sentences, are ratified on their own, and are what tests bind to.
- One owner per criterion. A shared example splits into one criterion per requirement.
- The convergence rule gates the ledger tier, not compile. A future registry toggle can
  make compile wait when a project wants it; nothing in `jazyk.toml`.
- The llm test setup file keeps its name (`gen/criteria/<slug>.md`) for now; the page
  says it is a test artifact, not an acceptance criterion.
