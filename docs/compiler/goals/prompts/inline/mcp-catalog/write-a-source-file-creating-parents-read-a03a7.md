---
purpose: >-
  Instruction fragment: Write a Source file, creating parents.
used_by:
  - "bootstrap/src/mcp/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Write a Source file, creating parents. Read first: if the read returns owner and begin_generation, claim that entity before writing. Successful record_generation ends the claim. append adds content. Source selection and entity ownership are enforced.