---
workflow: >-
  The agent extracts obligations from the documents and records them as requirements,
  with acceptance conditions called criteria. These messages explain how those records
  must cite source text or identify an explicit inference.
used_by:
  - "bootstrap/src/tools/requirements.rs"
---
a requirement enters with exactly one provenance: section plus quote (the verbatim source sentence), or provenance {derived: {from, reasoning}} for a statement the documents do not state