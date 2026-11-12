---
purpose: >-
  Instruction fragment: The facts or referenced input bytes moved since begin_generation.
used_by:
  - "bootstrap/src/gen/mark.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
The facts or referenced input bytes moved since begin_generation. Reload the package, use the current exact inputs and record its current factHash.