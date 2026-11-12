---
purpose: >-
  Instruction fragment: Validate and land the draft in the docs tree: path is
  project-relative and must match the docs glob.
used_by:
  - "bootstrap/src/mcp/dispatch.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Validate and land the draft in the docs tree: path is project-relative and must match the docs glob; content is the full markdown (one H1, short declarative sentences, no em dashes, every statement citing its evidence in backticks). Records the draft hash for ratification and consumes the scope's release. The compiler picks the file up like any hand-written document.