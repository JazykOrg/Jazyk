---
workflow: >-
  The Jazyk tool server receives a named tool call and routes it to the relevant project
  operation. These texts describe available calls or explain how the caller should
  proceed in the current workflow.
used_by:
  - "bootstrap/src/mcp/dispatch.rs"
---
Replace the question attached to an open diagnostic (null prompt removes it). Never touches a human answer or triage.