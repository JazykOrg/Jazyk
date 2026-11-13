---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
purpose: >-
  Without a content search the model finds code by listing and reading files one by
  one. This is the grep a coding agent reaches for first; it runs in process, so it
  works in a hosted workspace that has no shell
  (docs/compiler/goals/generate.md#file-and-command-tools).
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Search eligible file contents, like grep. pattern is a regular expression (literal true for plain text); ignoreCase, context (lines around each match, at most 5) and filesOnly (paths with match counts) refine it. path narrows to a file or directory, glob filters paths such as **/*.ts. Replies list {path, line, column, text} in path and line order, at most 100 matches. scanComplete false means not every file was searched, even when matches is empty: pass next unchanged as cursor with the same query to continue. Use it to find definitions, usages and requirement ids before reading files.
