---
workflow: >-
  The agent records which generated files implement each documented requirement and how
  each requirement is tested. These tools save that accounting, called the generation
  and verification ledger. These messages explain what a valid recording must include.
used_by:
  - "bootstrap/src/tools/ledger.rs"
---
manifest is required: {files: [...], tests: [{requirement, kind, label, artifact, name, run}]}