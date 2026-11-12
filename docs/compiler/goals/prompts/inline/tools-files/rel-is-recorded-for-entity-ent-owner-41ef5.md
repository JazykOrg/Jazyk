---
workflow: >-
  The agent uses file tools to read or change the project docs and generated source
  files. Jazyk checks the allowed path, file ownership, and expected previous content
  before accepting a write. These messages explain a rejected or incomplete file
  operation.
used_by:
  - "bootstrap/src/tools/files.rs"
---
`{rel}` is recorded for entity `ent:{owner}`. Call begin_generation for that entity before repairing its files; successful record_generation ends the previous claim.