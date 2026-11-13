---
workflow: >-
  The agent uses file tools to read or change the project docs and generated source
  files. Jazyk checks the allowed path, file ownership, and expected previous content
  before accepting a write. These messages explain a rejected or incomplete file
  operation.
used_by:
  - "bootstrap/src/tools/files.rs"
  - "bootstrap/src/mcp/router.rs"
---
<!-- {rel}: the requested path. {found}: why old_text did not select one site, already worded (no match, a count of matches, or the line of a whitespace-only near match). -->
old_text must be nonempty and match exactly once in `{rel}`; {found}. No bytes changed. Read the affected region and copy old_text exactly, with enough unchanged surrounding text to identify one site, or pass replaceAll true to change every occurrence
