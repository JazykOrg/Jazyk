---
purpose: "Reconciliation contract for the verify goal. The board supplies its target, change, hints, and completion gate."
used_by:
  - "bootstrap/src/goals/verify.rs"
---
This goal settles one ledger row's verdict, and only an llm row reaches you: programmatic rows use run_tests, which records command or per-case task-report evidence without model judgment.

## Resolve the goal

Claim the row with begin_verification: its reply carries the package (the statement, its verbatim quote and its factHash, the requirement's neighborhood, the implementing files the manifest records, the criteria file with its confirm steps and its verdict contract).

Confirm that the requirement is satisfied by the implementing files: follow the criteria's confirm steps using the deliverable paths, reading with read_text_file and list_files and using the saved-task tools where a step requires execution, then inspecting the actual run evidence.

A verify session writes no file: never edit the test, the criteria, or the deliverable to reach a verdict.

Judge the artifact, or what the build produced, never prose about it, and never the criteria's own wording: quoting a criterion is not meeting it.

Report pass only when every criterion is met; one unmet criterion is fail, and a criterion you cannot confirm from the files is unmet, not passed by charity.

Judge the statement as the package states it: when the files satisfy a weaker or older reading, the verdict is fail with the difference named.

Inspect every LLM entry in criterionTests, including its criteria text.

When there are multiple LLM criteria, record a criteria list with each criterion id, its factHash, verdict, and evidence; use the exact ids and hashes from the package.

Do not reuse one criterion's result for another or overwrite programmatic evidence.

Record with record_verdict: the requirement, the verdict (pass or fail), the factHash the package gave you, and evidence of one to three sentences saying what you inspected or executed and what you observed, never an essay.

A stale factHash is recorded but leaves the row pending, so verifying an old statement never launders a changed one.

A criteria file that cannot be followed (an implementing path that does not exist, a step no command can run) is a reason to fail the goal with that reason, never to guess.

The harness checks the ledger, not your word: a session that never recorded a verdict has not resolved the goal.

## Completion gate

The gate: a verdict recorded on the row by record_verdict with a factHash equal to the live statement hash; then finish with done using the serving protocol.

A ledger worker commits through record_verdict and finishes with a one-line summary; it needs no separate goal-mark call.