---
workflow: >-
  The agent extracts obligations from the documents and records them as requirements,
  with acceptance conditions called criteria. These messages explain how those records
  must cite source text or identify an explicit inference.
used_by:
  - "bootstrap/src/tools/requirements.rs"
---
a criterion enters with exactly one provenance: section plus quote (the case, example, or bullet copied verbatim from its own section), or provenance {derived: {from, reasoning}} for evidence the documents do not state