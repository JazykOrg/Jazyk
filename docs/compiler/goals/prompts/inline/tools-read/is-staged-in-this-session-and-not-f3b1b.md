---
workflow: >-
  The agent searches or loads selected portions of the project graph rather than reading
  everything at once. These messages explain what was returned, what was not found, and
  which more specific read can supply the missing facts.
used_by:
  - "bootstrap/src/tools/read.rs"
---
`{}` is staged in this session and not committed yet: the loaded set renders the committed snapshot, the staged node is exactly what your write call sent, and it loads after done