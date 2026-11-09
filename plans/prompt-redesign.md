# Prompt redesign

Status: proposed, not started. Every step that runs a model is paid and needs the
owner's approval per run. Tracked as L5 in [the 2026-11 review](./review-2026-11.md).

Two payloads have grown rule by rule: the `review-entity` contract asks one broad
question, and the `reconcile-section` round resends a large fixed prefix. This plan
restructures both and measures every change with `jazyk benchmark` before it lands.
Nothing here changes the harness, the gates, or the tools.

## The problems

### review-entity asks one diffuse question

`docs/compiler/goals/prompts/review-entity.md` (6.3k characters, one paragraph) asks the
model to judge an entity whose fact set changed. The paragraph bundles nine duties:

- refresh the definition, stereotype, and parent when the statements no longer fit;
- set `parent` on the part of a stated composition;
- judge every lookalike (merge, or keep separate with a reason);
- reference the entity on statements about it that do not reference it;
- delete same-document duplicate requirements, file cross-document ones;
- file contradictions, duplicates, ambiguity, missing links, and lint;
- leave edges to `declare-edges` except where a staged change moved them;
- resolve or refile lapsed diagnostics;
- fill the evidence object the gate checks.

A capable model handles it. A weaker one answers shallowly and skips duties: the gate
catches a missing evidence entry, but not a shallow verdict inside one. The hints
already list the suspects (lookalikes, unreferenced statements, open diagnostics,
composition parts), yet the prompt states the duties in prose, not as one worklist.

### reconcile-section resends a large fixed prefix

A reconcile round resends about 48k characters before any history
([docs/TODO.md](../docs/TODO.md), "The fixed cost of a round"):

- the extraction skill, `docs/compiler/skills/extraction.md`, 24.9k;
- the reconcile toolset's schemas, about 16.4k;
- the agent contract, about 5k;
- the goal block with the loaded set, 3k to 4k, of which the `reconcile-section`
  contract paragraph is 7.7k in three paragraphs.

The rules accreted one at a time, so related rules sit apart and the examples sit
inside the rules. The prefix is byte-identical across a session's rounds, so a
provider's prompt cache may absorb most of its cost; whether LocalRouter forwards the
cached figures is unmeasured.

## Proposed changes

### review-entity: a worklist, then optional split

1. Render the duties as a numbered worklist built from the hints: one line per
   lookalike, unreferenced statement, open diagnostic, and composition part, each
   naming the evidence entry it fills. The contract paragraph keeps the judgment rules
   (what is the same concept, what "about it" means) and drops the restated
   procedure.
2. Only if step 1 does not close the gap: split the review into two goals, structure
   (definition, parent, lookalikes, parts) and statements (references, duplicates,
   diagnostics, lint). This changes the goal catalog, so it is a docs-first change to
   `reconciler.md` and `goals/review-entity.md` with its own approval.

### reconcile-section and extraction: split by concern

1. Regroup the extraction skill by concern: what to extract, how to phrase a
   statement, entities and containment, transitions and attributes, coverage.
2. Move the worked examples into one block at the end of the skill, each pointing at
   the rule it shows.
3. Remove rules the contract paragraph and the skill both state; keep one copy, in
   the skill.
4. Measure cached input through LocalRouter before cutting anything for size alone.
   If the prefix is cached, the token cost argument is weak and only the clarity
   argument stands.

No wording changes beyond the regrouping in the first pass. Shortening tool
descriptions is out of scope: they are the owner's exception to the payload rule.

## How each change is measured

- The benchmark grades session capability with deterministic checks
  ([docs/benchmark/benchmark.md](../docs/benchmark/benchmark.md)). The relevant cases
  are:
  - review-entity: `review`, `review-duplicate`, `review-lookalike`, `review-lint`;
  - reconcile-section: `extract`, `declarative`, `density`, `enumeration`,
    `properties`, `steps`, `stated-containment`, `machine-subject`,
    `narrative-behavior`, `paragraph-layout`, `reuse`, `tech-constraint`.
- One run per configuration and model:
  1. A baseline on current master.
  2. One run per proposed change, each on its own branch.
- Two models: the local default (to see the weak-model gap) and one capable remote
  model (to confirm no regression where the gate already holds).
- Record per case: pass or fail, the gate refusals, the session tokens
  (`jazyk status --costs`), and cached input where the transport reports it.
- A change lands only when no graded case regresses on either model and at least one
  improves. Tokens alone do not justify a change.
- Refresh `known-results.yaml` after a change lands, on the new case set hash.

## Cost and approval

- The local model runs are free but slow. A remote model run bills per token: the
  owner approves each remote run with the model and the case list named.
- A full benchmark pass is about 16 graded cases, a few sessions each. Estimate the
  tokens from the baseline before approving the change runs.

## Steps

1. Baseline: run the listed cases on both models, record the results here.
2. Measure cached input through LocalRouter on one reconcile session.
3. review-entity worklist on a branch, run, compare, land or drop.
4. Extraction skill regroup on a branch, run, compare, land or drop.
5. Decide on the review-entity split from step 3's numbers.
6. Record the outcome in `bootstrap/VALIDATION.md` and close L5 in the review plan.
