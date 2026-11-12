---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Create a domain concept, or update it if the name already exists (name plus scope, and parent when given; several entities under that name need parent to say which). mention cites the section and the verbatim quote that talks about it; provenance.derived {from, reasoning} is the alternative for structure the documents do not state. stereotype is free-form judgment (actor, service, interface, table). parent must exist and keeps the containment tree acyclic. attributes are [{name, type?, value?, provenance?: {section, quote}}], keyed by name; one without its own provenance takes the call's quote. Not for file paths, CLI flags, or markdown terms. Leave scope unset unless the documents name a bounded context. A name that reads as a variant of an existing entity is rejected unless note names how the concepts differ (a field, part, or state of X is a different concept than X).