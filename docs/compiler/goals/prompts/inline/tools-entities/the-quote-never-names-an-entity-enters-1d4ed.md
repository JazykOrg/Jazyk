---
workflow: >-
  The project graph stores named concepts from the documentation as entities. The agent
  can create, merge, or reorganize those concepts. These messages ensure those edits
  preserve their source evidence and the distinction between genuinely different
  concepts.
used_by:
  - "bootstrap/src/tools/entities.rs"
---
the quote never names `{}`; an entity enters with a sentence that names it (a word of its name or an alias). An entity the documents never name enters with provenance {{derived: {{from, reasoning}}}}, or not at all: "the system" is a placeholder, never a new entity