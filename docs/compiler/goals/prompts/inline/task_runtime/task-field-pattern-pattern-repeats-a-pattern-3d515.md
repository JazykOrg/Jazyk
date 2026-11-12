---
workflow: >-
  A project can save commands such as its test suite as named tasks. Jazyk runs those
  commands in an isolated environment and retains their logs, outputs, and test reports.
  These messages validate task settings or explain how to inspect a retained run.
used_by:
  - "bootstrap/src/task_runtime.rs"
---
Task {} {field} pattern {pattern:?} repeats a pattern in {previous}. Keep each pattern in only one field; put test report patterns in reports.