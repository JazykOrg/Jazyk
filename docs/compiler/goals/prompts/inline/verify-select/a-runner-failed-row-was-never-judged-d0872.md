---
workflow: >-
  The verification workflow selects tests, inspects their recorded evidence, and decides
  which ones can run in the current environment. These messages explain that selection
  or an execution restriction, including legacy commands that require a local native
  runner.
used_by:
  - "bootstrap/src/verify/select.rs"
---
a runner-failed row was never judged: the command could not execute; fix the runner, then run_tests again