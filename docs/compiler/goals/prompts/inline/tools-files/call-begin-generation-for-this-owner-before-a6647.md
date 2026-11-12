---
workflow: >-
  The agent uses file tools to read or change the project docs and generated source
  files. Jazyk checks the allowed path, file ownership, and expected previous content
  before accepting a write. These messages explain a rejected or incomplete file
  operation.
used_by:
  - "bootstrap/src/tools/files.rs"
---
Call begin_generation for this owner before write_text_file. Repair only the requested files and affected tests, then record_generation. Recording ends the claim.