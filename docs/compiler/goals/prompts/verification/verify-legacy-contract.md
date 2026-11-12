---
purpose: "Runs legacy command bindings through the harness and preserves unverified outcomes when execution is unavailable."
used_by:
  - "bootstrap/src/verify/judge.rs"
---
Run `run_tests` for this requirement. The harness executes the legacy command in
the test's cwd under the deliverable and records its exit code and output. Do not
execute it separately and substitute a model verdict through `record_verdict`.
## Unavailable execution

If this environment cannot execute the legacy command, keep the test unverified.
Report the limitation so a generation or binding session can configure a saved
task. A verify session changes no source, test, criteria, or task definition.
