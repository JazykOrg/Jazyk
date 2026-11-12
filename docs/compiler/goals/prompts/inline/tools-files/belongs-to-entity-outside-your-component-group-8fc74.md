---
workflow: >-
  The agent uses file tools to read or change the project docs and generated source
  files. Jazyk checks the allowed path, file ownership, and expected previous content
  before accepting a write. These messages explain a rejected or incomplete file
  operation.
used_by:
  - "bootstrap/src/tools/files.rs"
---
`{}` belongs to entity `{}`, outside your component group; never write to another group's file. Reference it (import, include, link, read) and pick a path of your own; when it already carries one of your requirements, name it on that requirement's test row (`files: [path]`) in record_generation