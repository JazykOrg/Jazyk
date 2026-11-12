---
workflow: >-
  The Jazyk tool server receives a named tool call and routes it to the relevant project
  operation. These texts describe available calls or explain how the caller should
  proceed in the current workflow.
used_by:
  - "bootstrap/src/mcp/dispatch.rs"
---
Resolve final goals and finish the open batch in one call. goals entries carry {goal, justification, evidence?} under the same gates as mark_goal_done. Run all batch gates and commit atomically; a refusal names the repair and preserves earlier work. The reply names the next ready batch (beginNext: true also claims it). The finish that empties the board reports the verdict and counts.