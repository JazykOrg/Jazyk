---
workflow: >-
  A project can save commands such as its test suite as named tasks. Jazyk runs those
  commands in an isolated environment and retains their logs, outputs, and test reports.
  These messages validate task settings or explain how to inspect a retained run.
used_by:
  - "bootstrap/src/task_runtime.rs"
---
Task {} cwd {:?} must be a project-relative directory; use "." for the project root, not an absolute deliverable path.