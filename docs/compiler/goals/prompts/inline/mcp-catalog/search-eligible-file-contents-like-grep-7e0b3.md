---
workflow: >-
  An external agent discovers Jazyk operations from the tool server before calling them.
  These texts describe those operations or their available capabilities; they are
  tool-facing instructions, not results of executing the operation.
used_by:
  - "bootstrap/src/mcp/catalog.rs"
---
Search eligible file contents, like grep: pattern is a regular expression (literal true for plain text). path narrows to a file or directory, glob filters paths (**/*.ts). Matches come back as {path, line, text}; filesOnly lists matching files, context adds surrounding lines, scanComplete false (even with no matches) means continue by passing next unchanged as cursor with the same query. Search before reading files one by one.
