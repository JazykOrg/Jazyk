---
workflow: >-
  The agent searches or loads selected portions of the project graph rather than reading
  everything at once. These messages explain what was returned, what was not found, and
  which more specific read can supply the missing facts.
used_by:
  - "bootstrap/src/tools/read.rs"
---
no match for `{}`. {} of the graph's {} entities are listed above; searching again will return this same answer. If one of them, or a known entity in your work pack, means the same concept under another name, use its id. Otherwise create the entity with upsert_entity.