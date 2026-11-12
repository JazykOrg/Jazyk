---
workflow: >-
  Jazyk imports test results from retained task runs and checks that they still refer to
  the current test inputs. These messages explain why a result cannot yet be applied or
  which test run must be repeated.
used_by:
  - "bootstrap/src/verify/artifacts.rs"
---
Test inputs changed or run {run_id} was not applied. Inspect the retained run, then rerun the selected tests.