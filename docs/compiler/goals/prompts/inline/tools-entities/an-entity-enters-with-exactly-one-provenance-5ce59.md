---
workflow: >-
  The project graph stores named concepts from the documentation as entities. The agent
  can create, merge, or reorganize those concepts. These messages ensure those edits
  preserve their source evidence and the distinction between genuinely different
  concepts.
used_by:
  - "bootstrap/src/tools/entities.rs"
---
an entity enters with exactly one provenance: mention {section, quote} (the sentence that talks about it), or provenance {derived: {from, reasoning}} for structure the documents do not state