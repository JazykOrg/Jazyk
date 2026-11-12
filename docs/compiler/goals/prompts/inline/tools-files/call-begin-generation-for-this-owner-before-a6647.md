---
purpose: >-
  Instruction fragment: Call begin_generation for this owner before write_text_file.
used_by:
  - "bootstrap/src/tools/files.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Call begin_generation for this owner before write_text_file. Repair only the requested files and affected tests, then record_generation. Recording ends the claim.