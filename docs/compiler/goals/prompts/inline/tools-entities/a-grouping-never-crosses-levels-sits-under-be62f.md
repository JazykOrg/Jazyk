---
workflow: >-
  The project graph stores named concepts from the documentation as entities. The agent
  can create, merge, or reorganize those concepts. These messages ensure those edits
  preserve their source evidence and the distinction between genuinely different
  concepts.
used_by:
  - "bootstrap/src/tools/entities.rs"
---
a grouping never crosses levels: {} sits under {} while {} sits under {}; group members that share one parent, or move one first with update_entity parent