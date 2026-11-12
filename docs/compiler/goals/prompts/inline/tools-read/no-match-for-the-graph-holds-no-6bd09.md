---
workflow: >-
  The agent searches or loads selected portions of the project graph rather than reading
  everything at once. These messages explain what was returned, what was not found, and
  which more specific read can supply the missing facts.
used_by:
  - "bootstrap/src/tools/read.rs"
---
no match for `{}`: the graph holds no entities yet. Searching again will return this same answer. Create the entity with upsert_entity.