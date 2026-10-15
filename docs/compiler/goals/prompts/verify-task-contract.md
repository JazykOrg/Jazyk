Run `run_tests` for this requirement. Its `task:<id>` reference names a saved task,
not a shell command. The harness runs the build first and executes each selected
task once. Read the per-case evidence matched to the recorded test name. Container
success does not mean every test passed. Skipped, missing, ambiguous, or incomplete
cases stay unverified. Do not replace their evidence with `record_verdict`.
