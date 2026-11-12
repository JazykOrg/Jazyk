---
workflow: >-
  The project graph stores named concepts from the documentation as entities. The agent
  can create, merge, or reorganize those concepts. These messages ensure those edits
  preserve their source evidence and the distinction between genuinely different
  concepts.
used_by:
  - "bootstrap/src/tools/entities.rs"
---
`{}` looks like a name variant of existing `{}` ({}); if it is the same concept, reuse that id and add your wording with update_entity add_aliases. A field, part, state, or child concept of it IS a different concept: repeat the call with a `note` naming that relation