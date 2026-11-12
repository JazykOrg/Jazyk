---
workflow: >-
  Jazyk imports test results from retained task runs and checks that they still refer to
  the current test inputs. These messages explain why a result cannot yet be applied or
  which test run must be repeated.
used_by:
  - "bootstrap/src/verify/artifacts.rs"
---
Runner {run} also covers tests outside the selection. Select their containing folder or configure separate scoped tasks.