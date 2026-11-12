---
workflow: >-
  The project graph stores named concepts from the documentation as entities. The agent
  can create, merge, or reorganize those concepts. These messages ensure those edits
  preserve their source evidence and the distinction between genuinely different
  concepts.
used_by:
  - "bootstrap/src/tools/entities.rs"
---
`{}` looks like a name variant of existing `{}` ({}), a sibling at this level that already holds children; a lookalike of an existing area reuses it: reparent the members under {} with update_entity parent, or pick the name the documents use for this area