---
workflow: >-
  The project graph stores named concepts from the documentation as entities. The agent
  can create, merge, or reorganize those concepts. These messages ensure those edits
  preserve their source evidence and the distinction between genuinely different
  concepts.
used_by:
  - "bootstrap/src/tools/entities.rs"
---
`{}` already exists without a parent as `{}`; to place it under `{}` call update_entity {{id: {}, parent: {}}}. A second `{}` under that parent is a different concept only with a `note` saying what tells them apart