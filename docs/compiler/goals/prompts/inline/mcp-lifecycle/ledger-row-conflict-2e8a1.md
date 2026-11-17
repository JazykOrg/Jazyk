---
workflow: >-
  Generation and binding rows are opened with `begin_generation` or `begin_binding` and
  recorded with `record_generation` or `record_binding`. Nothing claims a row, so two
  workers can work the same one; the first recording wins
  (docs/compiler/graph.md#commit-conflicts). This message answers a record when another
  worker recorded the row after this worker began it. The slot is the row's id.
used_by:
  - "bootstrap/src/mcp/chat.rs"
---
`{}` was recorded by another worker after you began it. Nothing was recorded. Call the matching `begin_*` again to read the current row and files, then decide whether your work still adds anything; record again only if it does.
