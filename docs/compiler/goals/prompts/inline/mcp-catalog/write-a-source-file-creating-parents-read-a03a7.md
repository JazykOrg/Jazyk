---
workflow: >-
  An external agent discovers Jazyk operations from the tool server before calling them.
  These texts describe those operations or their available capabilities; they are
  tool-facing instructions, not results of executing the operation.
used_by:
  - "bootstrap/src/mcp/catalog.rs"
---
Write a Source file, creating parents. For an existing file, prefer a literal edit: pass old_text (copied exactly from your last read, matching once, or every match with replaceAll true) and content as its replacement; only the changed region is sent. The edit applies to the file as you last read or wrote it, so expectedHash is optional. Rewrite with content alone only for new files or complete rewrites. Read first: if the read reports an owner entity, the file belongs to generated work; read the generate skill and claim that entity before writing. append adds content. Source selection and entity ownership are enforced.