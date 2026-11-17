---
workflow: >-
  Nothing claims work, so the project configuration (document sets, their documents
  and roots) can change while a batch is open (docs/compiler/control-plane.md#workers-and-open-work).
  This message answers `done` when it did. Nothing was committed; the batch stays open
  and the next `done` commits against the new configuration.
used_by:
  - "bootstrap/src/mcp/lifecycle.rs"
---
The project configuration changed while this batch was open (a document set, its documents, or its roots). Nothing was committed. Check that the goals and the staged changes still fit the current document sets, then call `done` again.
