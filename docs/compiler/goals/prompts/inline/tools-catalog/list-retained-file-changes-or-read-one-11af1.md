---
purpose: >-
  Instruction fragment: List retained file changes or read one changed path (version:
  changes, the default). version: input requires path and reads the frozen run input,
  including an unchanged file, explicitly labeled historical input.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
List retained file changes or read one changed path (version: changes, the default). version: input requires path and reads the frozen run input, including an unchanged file, explicitly labeled historical input. It does not read the current workspace. Missing contents and partial collection are explicit.