---
workflow: >-
  The verification workflow selects tests, inspects their recorded evidence, and decides
  which ones can run in the current environment. These messages explain that selection
  or an execution restriction, including legacy commands that require a local native
  runner.
used_by:
  - "bootstrap/src/verify/select.rs"
---
a failing row has an open document diagnostic: surface it to the document author before changing the deliverable or test; do not silently rewrite prose