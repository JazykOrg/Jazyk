---
purpose: "Prevent a claimed batch from committing over an independent conversational edit."
used_by:
  - "bootstrap/src/mcp/lifecycle.rs: stale_chat_batch"
---
Another graph edit committed after this batch's snapshot. The staged batch is unchanged and cannot commit. Abandon it with abandon_goals, then begin_goals to load current work and restage. Use edit_graph for further independent user requests.
