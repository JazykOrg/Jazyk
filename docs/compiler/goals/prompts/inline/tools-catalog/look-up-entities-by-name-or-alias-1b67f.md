---
purpose: >-
  Instruction fragment: Look up entities by name or alias (kind entity, the default) or
  views by title (kind view) before creating one.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Look up entities by name or alias (kind entity, the default) or views by title (kind view) before creating one. Pass query for one name, or queries for every name a section introduces in one call (one result per query, in order). Returns up to 8 possible matches per name; hits can be false neighbors (substrings, shared words), so inspect each before reusing it.