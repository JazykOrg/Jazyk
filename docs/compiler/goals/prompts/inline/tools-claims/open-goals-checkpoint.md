---
purpose: Offer a valid partial-save path without requiring false goal failures.
used_by:
  - bootstrap/src/tools/claims.rs
inputs: One positional argument listing unclaimed mandatory goal IDs.
---
Goal(s) neither done nor failed: {}. Finish their judgments with mark_goal_done or mark_goal_failed, or call done with checkpoint: true to save valid progress while these goals remain open. Do not mark uninspected goals failed just to save completed work.
