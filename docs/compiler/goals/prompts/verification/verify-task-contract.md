---
purpose: "Runs saved task bindings through run_tests and interprets fresh per-case evidence."
used_by:
  - "bootstrap/src/verify/judge.rs"
# Context: saved task references are resolved by the harness. Running a string as a shell command would bypass task execution and its per-case evidence.
---
Run `run_tests` for this requirement. Its `task:<id>` reference names a saved task,
not a shell command. The harness runs the build first and executes each selected
task once. Read the per-case evidence matched to the recorded test name. Container
success does not mean every test passed. Skipped, missing, ambiguous, or incomplete
cases stay unverified. Do not replace their evidence with `record_verdict`.
