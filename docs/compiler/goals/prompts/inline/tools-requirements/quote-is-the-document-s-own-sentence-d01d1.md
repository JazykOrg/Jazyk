---
workflow: >-
  The agent extracts obligations from the documents and records them as requirements,
  with acceptance conditions called criteria. These messages explain how those records
  must cite source text or identify an explicit inference.
used_by:
  - "bootstrap/src/tools/requirements.rs"
---
`quote` is the document's own sentence, never the statement. To change only the entities or edges, omit `section` and `quote`