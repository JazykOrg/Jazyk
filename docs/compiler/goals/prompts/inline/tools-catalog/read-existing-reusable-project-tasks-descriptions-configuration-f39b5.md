---
purpose: >-
  Instruction fragment: Read existing reusable project tasks, descriptions, configuration
  revision, execution capabilities and recent runs.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Read existing reusable project tasks, descriptions, configuration revision, execution capabilities and recent runs. Inspect this and project runner configuration before defining a task. Criterion identities belong in JUnit cases, not one task per criterion. Pass next_cursor as before for older runs. task_ids limits full definitions to those tasks and indexes the rest by id, name and description; omit it before update_tasks.