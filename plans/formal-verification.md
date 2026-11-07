# Formal verification and test sensitivity

Status: deferred. The owner does not consider this the right time to implement it.
This file preserves proposals for later evaluation. It authorizes no implementation,
service enrollment, paid verification jobs, or changes to current compiler behavior.
Schemas below are illustrative, not decided contracts.

## Motivation and sources

Extend the trail from prose to requirements, criteria, tests, and current evidence.
The first useful experiment is evidence that a generated test detects its intended
defect. Formal properties and proof integration are separate later experiments.

- [Anthropic research](https://www.anthropic.com/research/formal-verification).
- [Theorem add-project skill](https://github.com/theorem-labs/oss-verify/blob/main/.claude/skills/add-project/SKILL.md).
- [Scope of Theorem proofs](https://github.com/theorem-labs/oss-verify/blob/main/docs/what-gets-proven.md).

The research separates agent-written proofs from independent checking. The skill
requires witnesses that checks execute and mutations that make them fail. A proof
can establish a formal statement without establishing that it captures human intent.

Existing foundations:

- [Criteria](../docs/compiler/model/criterion.md) associate observable examples with requirements.
- [Generation and verification](../docs/consumers/gen.md) bind tests to criteria and track evidence.
- [The verify goal](../docs/compiler/goals/verify.md) settles current ledger verdicts.

## 1. Evidence that a test detects a defect

Example:

- Requirement: sorting preserves duplicate values.
- Criterion: sorting `[2, 1, 2]` produces `[1, 2, 2]`.
- Bound test: `sort_preserves_duplicates`.
- Witness: the input `[2, 1, 2]` that reaches the relevant assertion.
- Mutation: a patch that inserts deduplication into the implementation.

Attach evidence to the existing criterion test entry in the ledger. The criterion
id identifies the obligation; the test selector identifies its executable check.
The witness and mutation are deliverable-relative artifacts, not new graph facts.

E.g.:

```yaml
crit:sort-3:
  # Existing test binding and verdict remain here.
  sensitivity:
    witness: fixtures/duplicates.json
    mutation: mutations/drop-duplicates.patch
    assertionReached: true
    mutationDetected: true
    # Record hashes of the implementation, test, witness, and mutation.
```

Proposed flow:

1. Generate or bind the test and run the original successfully.
2. Measure that the witness reaches its relevant assertion.
3. Apply the mutation in a disposable isolated copy.
4. Run the same test and confirm that its relevant assertion fails.
5. Record the result and the exact checked inputs. Discard the mutated copy.

Compilation failures, setup failures, and unrelated failures do not count as
detecting the defect. Changed relevant inputs invalidate the sensitivity evidence.
This starts as an explicit audit for selected programmatic tests, not a mandatory
step for every build.

Benefit: a test that checks only sortedness fails this audit because deduplicated
output is still sorted. Mutation detection demonstrates sensitivity to one defect;
it does not establish completeness or the absence of other defects.

## 2. General properties alongside concrete criteria

Keep concrete criteria. Explore property obligations associated with requirements
when the documented intent applies across an input domain.

E.g.:

```text
Requirement: Sorting preserves every input element.
Concrete criterion: [2, 1, 2] produces [1, 2, 2].
Property: For every supported xs, multiset(sort(xs)) = multiset(xs).
```

The executable property lives in the deliverable and binds through the ledger.
Its prose statement, provenance, and assumptions need a graph representation whose
schema remains undecided. Avoid adding a node kind before evaluating the need.

Proposed flow: derive criteria → propose property and domain → bind a harness →
run generated inputs or submit the obligation to a verifier.

Benefit: one obligation expresses behavior across many inputs. Generated input
testing remains testing evidence. A universal claim requires an appropriate proof.
Derived assumptions follow ratification. An agent cannot silently narrow the domain
to lists without duplicates or invent resource bounds.

## 3. Scoped verification evidence

Extend ledger evidence to distinguish the method and scope of a successful check.

E.g.:

- Example test: passed for `[2, 1, 2]`.
- Property test: passed for 10,000 generated lists.
- Bounded verification: established for all lists of at most eight elements in a
  declared integer domain. These bounds are an example, not a proposed default.

Associate each result with its requirement or property binding. Record the checked
source or binary, harness, assumptions, dependencies, verifier version and
configuration, and relevant hashes. Keep method separate from verdict.

Proposed flow: task runs → adapter imports structured evidence → Jazyk displays
method and scope → relevant changes invalidate the result.

Benefit: readers can distinguish passing examples from a scoped proof. Timeout,
unsupported constructs, and incomplete checking remain inconclusive, not demonstrated
defects. A proof task can initially use the existing `programmatic` kind.

## 4. Counterexamples as diagnostics and regression cases

E.g., verification of duplicate preservation returns:

```text
Input: [2, 2]
Expected: [2, 2]
Actual: [2]
```

Save the counterexample against the property binding. A diagnostic references the
requirement and the evidence. Offer to retain it as a criterion and regression test.

Proposed flow: concrete counterexample → diagnostic → resolve implementation defect,
incorrect formalization, or ambiguous prose → rerun → retain regression case.

Benefit: failures become understandable examples and durable tests. Failed proof
attempts need not produce counterexamples. Specification changes follow provenance
and ratification; repair cannot silently weaken the obligation to make a proof pass.
The harness continues to derive goals and validate evidence.

## 5. Verify selected Jazyk invariants

Apply the same mechanisms to small deterministic parts of Jazyk.

| Candidate | Obligation |
| --- | --- |
| File id encoding | Accepted ids round-trip and distinct ids remain distinct. |
| Ledger status | Changed relevant implementation bytes invalidate old passing evidence. |
| Natural-key upserts | Repeating an operation creates no duplicate. |
| Merge handling | References resolve to the surviving identity. |
| Mutation gates | Rejected mutations leave graph state unchanged. |

Proposed flow: document an invariant → bind a harness → establish sensitivity →
optionally attempt bounded or deductive verification.

Benefit: stronger confidence in the machinery that evaluates other deliverables.
Start with file ids or ledger status, not whole-compiler verification. Kani and Verus
are candidates to evaluate later, not selected dependencies. Compatibility with this
repository has not been tested.

## Deferred evaluation sequence

No steps are scheduled or approved for execution.

1. Revisit whether the expected benefit justifies the complexity and cost.
2. If approved, specify the selected behavior in `docs/` before changing code.
3. Demonstrate proposal 1 with a duplicate-preservation test in `example-sort`.
   Confirm the example obligation exists or propose it explicitly before binding it.
4. Assess whether sensitivity evidence catches ineffective generated assertions at
   acceptable cost before expanding the feature.
5. Evaluate properties, scoped proof evidence, counterexamples, and compiler proofs
   separately. They need not ship together.

Open decisions: evidence schema and storage, assertion-to-criterion mapping,
coverage tooling, mutation generation and review, invalidation dependencies,
execution isolation, supported verifier backends, and opt-in policy. Witnesses and
mutations must test the documented obligation rather than the generator's own
invented interpretation. External service eligibility and costs require a fresh
review if enrollment is ever considered.
