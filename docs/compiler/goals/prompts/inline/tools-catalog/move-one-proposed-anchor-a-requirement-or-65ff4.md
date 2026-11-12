---
purpose: >-
  Instruction fragment: Move one proposed anchor (a requirement or an entity mention named
  in this align task) to the section that now holds its text. quote, when given, must
  locate verbatim there and replaces the stored quote. reevaluate true lists the anchor
  for the extraction turn to re-judge.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Move one proposed anchor (a requirement or an entity mention named in this align task) to the section that now holds its text. quote, when given, must locate verbatim there and replaces the stored quote. reevaluate true lists the anchor for the extraction turn to re-judge; false keeps it as an unchanged statement.