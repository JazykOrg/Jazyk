---
workflow: >-
  An external agent discovers Jazyk operations from the tool server before calling them.
  These texts describe those operations or their available capabilities; they are
  tool-facing instructions, not results of executing the operation.
used_by:
  - "bootstrap/src/mcp/catalog.rs"
---
Write a Source file, creating parents. Read first: if the read returns owner and begin_generation, claim that entity before writing. Successful record_generation ends the claim. append adds content. Source selection and entity ownership are enforced.