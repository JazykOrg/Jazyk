---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Read existing reusable project tasks, descriptions, configuration revision, execution capabilities and recent runs. Inspect this and project runner configuration before defining a task. Criterion identities belong in JUnit cases, not one task per criterion. Pass next_cursor as before for older runs. task_ids limits full definitions to those tasks and indexes the rest by id, name and description; omit it before update_tasks.