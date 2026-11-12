---
workflow: >-
  The agent searches or loads selected portions of the project graph rather than reading
  everything at once. These messages explain what was returned, what was not found, and
  which more specific read can supply the missing facts.
used_by:
  - "bootstrap/src/tools/read.rs"
---
the build's own records, never a session's to resolve; pass rule to list them