---
workflow: >-
  Before Jazyk generates missing code, it tries to connect each documented requirement
  to existing implementation and tests. That connection is called a binding. These
  messages guide or validate the record of which files and tests support a requirement.
used_by:
  - "bootstrap/src/bind.rs"
---
implementing files exist but the tests fail: the deliverable contradicts the statement; the failing-binding check files it for the author at the next check run, never regenerate silently