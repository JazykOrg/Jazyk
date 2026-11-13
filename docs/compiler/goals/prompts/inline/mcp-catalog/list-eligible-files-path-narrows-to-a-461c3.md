---
workflow: >-
  An external agent discovers Jazyk operations from the tool server before calling them.
  These texts describe those operations or their available capabilities; they are
  tool-facing instructions, not results of executing the operation.
used_by:
  - "bootstrap/src/mcp/catalog.rs"
---
List eligible files. path narrows to a subdirectory; pattern is a glob such as **/*.rs (without / it matches file names at any depth). A cut reply carries nextOffset; pass it as offset with the same query to continue. Returned paths use the same root.