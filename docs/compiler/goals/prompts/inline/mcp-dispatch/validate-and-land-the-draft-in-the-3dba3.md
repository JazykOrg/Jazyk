---
workflow: >-
  The Jazyk tool server receives a named tool call and routes it to the relevant project
  operation. These texts describe available calls or explain how the caller should
  proceed in the current workflow.
used_by:
  - "bootstrap/src/mcp/dispatch.rs"
---
Validate and land the draft in the docs tree: path is project-relative and must match the docs glob; content is the full markdown (one H1, short declarative sentences, no em dashes, every statement citing its evidence in backticks). Records the draft hash for ratification and consumes the scope's release. The compiler picks the file up like any hand-written document.