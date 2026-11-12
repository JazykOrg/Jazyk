---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Write one deliverable file atomically. For a small edit, pass old_text matching exactly once and expectedHash from the last read or write; content replaces that literal text. old_text requires expectedHash and cannot combine with append true. An optional expectedHash guards any write mode; a conflict changes nothing. With append true, content goes at the end. Replies include the whole-file hash. path is relative to the deliverable. Source and ownership gates apply to every mode; preserve unrelated content.