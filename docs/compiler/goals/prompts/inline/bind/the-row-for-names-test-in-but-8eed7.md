---
workflow: >-
  Before Jazyk generates missing code, it tries to connect each documented requirement
  to existing implementation and tests. That connection is called a binding. These
  messages guide or validate the record of which files and tests support a requirement.
used_by:
  - "bootstrap/src/bind.rs"
---
the row for `{}` names test `{}` in `{}`, but that artifact is missing or does not contain the name; write the test (or bind to the existing one under its real name) and record_binding again