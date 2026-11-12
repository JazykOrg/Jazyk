---
purpose: >-
  Instruction fragment: One eligible deliverable file's content, capped at 64 KiB
  including a long first line.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
One eligible deliverable file's content, capped at 64 KiB including a long first line. Credentials and private compiler/Git state are excluded. path is relative to the deliverable directory; line (1-based) and limit select lines. hash is the whole file SHA-256, including unread content. truncatedLine identifies a line whose remainder was omitted.