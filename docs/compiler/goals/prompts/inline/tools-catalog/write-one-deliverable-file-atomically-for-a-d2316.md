---
purpose: >-
  Instruction fragment: Write one deliverable file atomically.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Write one deliverable file atomically. For a small edit, pass old_text matching exactly once and expectedHash from the last read or write; content replaces that literal text. old_text requires expectedHash and cannot combine with append true. An optional expectedHash guards any write mode; a conflict changes nothing. With append true, content goes at the end. Replies include the whole-file hash. path is relative to the deliverable. Source and ownership gates apply to every mode; preserve unrelated content.