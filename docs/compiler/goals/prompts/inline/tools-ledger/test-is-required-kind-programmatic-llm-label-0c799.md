---
workflow: >-
  The agent records which generated files implement each documented requirement and how
  each requirement is tested. These tools save that accounting, called the generation
  and verification ledger. These messages explain what a valid recording must include.
used_by:
  - "bootstrap/src/tools/ledger.rs"
---
test is required: {kind: programmatic|llm, label, artifact, name, run, cwd?}, or tests: [{criterion, ...}] one per criterion