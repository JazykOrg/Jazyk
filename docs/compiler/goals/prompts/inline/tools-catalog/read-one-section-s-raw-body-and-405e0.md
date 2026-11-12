---
purpose: >-
  Instruction fragment: Read one section's raw body and its child section titles. ref is a
  full section reference (doc.md#/ref).
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Read one section's raw body and its child section titles. ref is a full section reference (doc.md#/ref). The body is capped at 400 lines and 16 KiB from line (default 1); a cut body carries truncated, totalLines, and nextLine.