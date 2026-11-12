---
purpose: >-
  Instruction fragment: stale anchors left untouched: {}.
used_by:
  - "bootstrap/src/tools/claims.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
stale anchors left untouched: {}; for each: if the document still states the fact, re-record it with upsert_requirement quoting the new sentence verbatim (it resolves to the anchor and updates in place); if the statement changed meaning, revise it with update_requirement carrying the id, the new statement, and the new section plus quote; if the fact is gone, delete_requirement