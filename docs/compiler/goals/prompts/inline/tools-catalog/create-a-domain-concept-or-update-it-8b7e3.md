---
purpose: >-
  Instruction fragment: Create a domain concept, or update it if the name already exists
  (name plus scope, and parent when given.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Create a domain concept, or update it if the name already exists (name plus scope, and parent when given; several entities under that name need parent to say which). mention cites the section and the verbatim quote that talks about it; provenance.derived {from, reasoning} is the alternative for structure the documents do not state. stereotype is free-form judgment (actor, service, interface, table). parent must exist and keeps the containment tree acyclic. attributes are [{name, type?, value?, provenance?: {section, quote}}], keyed by name; one without its own provenance takes the call's quote. Not for file paths, CLI flags, or markdown terms. Leave scope unset unless the documents name a bounded context. A name that reads as a variant of an existing entity is rejected unless note names how the concepts differ (a field, part, or state of X is a different concept than X).