---
workflow: >-
  Before Jazyk generates missing code, it tries to connect each documented requirement
  to existing implementation and tests. That connection is called a binding. These
  messages guide or validate the record of which files and tests support a requirement.
used_by:
  - "bootstrap/src/bind.rs"
---
a programmatic test needs {}.run: a saved task:<id> with JUnit evidence or a legacy command whose exit code supplies the verdict