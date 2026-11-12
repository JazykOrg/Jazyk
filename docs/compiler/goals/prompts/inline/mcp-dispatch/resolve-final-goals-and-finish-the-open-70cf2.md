---
purpose: >-
  Instruction fragment: Resolve final goals and finish the open batch in one call. goals
  entries carry {goal, justification, evidence?} under the same gates as mark_goal_done.
used_by:
  - "bootstrap/src/mcp/dispatch.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Resolve final goals and finish the open batch in one call. goals entries carry {goal, justification, evidence?} under the same gates as mark_goal_done. Run all batch gates and commit atomically; a refusal names the repair and preserves earlier work. The reply names the next ready batch (beginNext: true also claims it). The finish that empties the board reports the verdict and counts.