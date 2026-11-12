---
purpose: >-
  Instruction fragment: Re-render the loaded set's status block in full: every loaded
  item, its handles, the skill index line, and the unload suggestions.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Re-render the loaded set's status block in full: every loaded item, its handles, the skill index line, and the unload suggestions. A condensed form already rides on every mutating reply.