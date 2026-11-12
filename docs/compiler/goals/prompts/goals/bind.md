---
purpose: "Reconciliation contract for the bind goal. The board supplies its target, change, hints, and completion gate."
used_by:
  - "bootstrap/src/goals/bind.rs"
---
This goal ties one requirement to the deliverable: the files that carry it (possibly none), the one test that judges it, and the first verdict.

## Resolve the goal

Binding observes and judges; it never changes implementation files, and generate is what changes them.

Claim the requirement with begin_binding, as the last hint says: its reply carries the binding contract as instructions (search before write, both directions get a test, the two test kinds, falsifiability, the naming scheme) and the package (the statement and its verbatim quote, the reason the row is owed, the suggested test name, the medium, the recorded build, the test conventions to imitate, the entity's recorded files, the requirement's neighborhood); follow that contract exactly.

Read the package first.

Inspect task_context and the project runner configuration.

Reuse an ordinary saved suite task across related requirements and give each criterion its own test function and exact JUnit selector.

One requirement per bind goal does not mean one task, test file or container per requirement.

The task command can run the suite; each JUnit case supplies its own verdict.

Reuse the recorded medium; if the package says it is undecided, choose from the documents and call record_medium before writing.

Search the deliverable with the file tools, starting at the entity's recorded files: what carries the statement is recorded as the files, and an empty list is a finding, not a failure.

Search for an existing test that judges the statement and bind to it when found, never writing a duplicate beside it; when none exists, write one under the suggested test name in the recorded conventions and toolchain: implementation found, the test pins the observed behavior; implementation absent, the test encodes the statement and fails by design, the acceptance gate generate must clear.

A test is falsifiable (its assertion fails when the requirement is violated) and inspects the artifact, or what the build produced, never prose about it; when no falsifiable programmatic assertion exists, the kind is llm and the artifact is a criteria file you write.

Run the test and read its outcome.

Record with record_binding: the files, the test row, the verdict, the evidence; it rejects a test whose artifact does not exist or does not contain the declared name, and its reply previews what the row opens (an unimplemented row opens generate on the owning entity, which is expected).

The verdict is not the goal: a failing test on an unimplemented requirement is the intended outcome, and a failing test on implemented files is a finding for the author, never a reason to withhold the row.

Test and criteria files are the only files this goal writes; a write to a path the ledger records as an implementing file is rejected with the owner named.

Fail the goal when the deliverable cannot be searched or no test can be written in its toolchain.

The harness checks the ledger, not your word: a session that never recorded has not resolved the goal.

## Completion gate

The gate: a current ledger row recorded by record_binding for the requirement, whose statement hash equals the live statement and whose test artifact exists and contains the declared name (for an llm row, the criteria file exists); then finish with done using the serving protocol.

A ledger worker commits through record_binding and finishes with a one-line summary; it needs no separate goal-mark call.