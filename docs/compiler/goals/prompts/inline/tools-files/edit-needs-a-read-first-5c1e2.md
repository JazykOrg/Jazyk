---
workflow: >-
  The agent uses file tools to read or change the project docs and generated source
  files. Jazyk checks the allowed path, file ownership, and expected previous content
  before accepting a write. These messages explain a rejected or incomplete file
  operation.
purpose: >-
  A literal edit applies to the file version the agent last saw. The serving remembers
  the hash of each path it read or wrote; a path it never returned has no version to
  guard, so the edit is refused instead of applied blind
  (docs/compiler/goals/generate.md#file-and-command-tools).
used_by:
  - "bootstrap/src/tools/files.rs"
---
<!-- {rel}: the requested path, relative to the deliverable. -->
`{rel}` was not read in this session; no bytes changed. Call read_text_file for `{rel}` first, then repeat the edit. The edit applies to the version that read returns
