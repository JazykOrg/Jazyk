---
workflow: >-
  Jazyk assigns an agent a bounded piece of work and checks the evidence it gives when
  finishing that work. These messages explain valid completion claims, status values, or
  the facts already supplied in the session.
used_by:
  - "bootstrap/src/tools/claims.rs"
---
stale anchors left untouched: {}; for each: if the document still states the fact, re-record it with upsert_requirement quoting the new sentence verbatim (it resolves to the anchor and updates in place); if the statement changed meaning, revise it with update_requirement carrying the id, the new statement, and the new section plus quote; if the fact is gone, delete_requirement