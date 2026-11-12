---
workflow: >-
  Before Jazyk generates missing code, it tries to connect each documented requirement
  to existing implementation and tests. That connection is called a binding. These
  messages guide or validate the record of which files and tests support a requirement.
used_by:
  - "bootstrap/src/bind.rs"
---
test artifacts and build outputs are never implementing files; list the product files a criterion test inspects, or none