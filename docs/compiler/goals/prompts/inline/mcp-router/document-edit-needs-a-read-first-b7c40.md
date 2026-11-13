---
workflow: >-
  The tool server exposes document reads and other project operations to an external
  agent. These texts explain the operation being offered, including how binary documents
  are read without treating them as editable plain text.
purpose: >-
  A write_document literal edit applies to the document version the agent last saw.
  With no baseHash and no earlier read or write of the path in this serving, there is
  no version to guard, so the edit is refused instead of applied blind
  (docs/compiler/tools.md#conversational-document-writes).
used_by:
  - "bootstrap/src/mcp/router.rs"
---
<!-- {path}: the requested document path, relative to the project root. -->
`{path}` was not read in this session; nothing changed. Call read_document for `{path}` first, then repeat the edit
