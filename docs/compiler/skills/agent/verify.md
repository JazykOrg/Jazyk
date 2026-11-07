# Verify behavior

Use for running tests, checking requirements, or judging observable criteria.

- Identify the requested scope and current requirements, criteria, test source,
  and deliverable. Read `verification_tasks` when ledger work is involved.
- Programmatic evidence comes from actual execution. Use `run_tests` for recorded
  programmatic rows and the available task tools for the named task. Report
  commands, results, and failures. Preserve assertions when repairing tests.
- A saved task can cost money or affect a remote system. Read `execute-task` and
  honor the specific approval and execution gates before running it.
- For LLM judgment, call `begin_verification`, inspect the criterion and concrete
  deliverable evidence, then `record_verdict` with supported observations. Do
  not treat a successful build or a programmatic pass as an LLM verdict.
- Requirements and test artifacts must still match the current hashes. Stale
  evidence is not a current pass. Follow tool repairs for changed inputs.
- A test-only request does not authorize source repair, docs changes, generation,
  or deployment. Explain unexpected failures and the proposed next action.
- Report executed checks separately from recorded coverage and unjudged criteria.
  State unrun checks and blockers. Never manufacture test output or a verdict.
