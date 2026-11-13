---
name: recover-workflow
description: "Diagnose failed tools, conflicts, interrupted work, or missing prerequisites."
requiredCapabilities: "relevant inspection and repair tools"
tools:
  - goals
  - abandon_goals
  - mark_goal_failed
  - release_work
  - await_changes
  - answer_diagnostic
  - task_context
  - update_tasks
  - run_task
  - inspect_run
  - wait_run
  - read_run_log
  - search_run_log
  - read_run_changes
  - cancel_run
  - preview_document_set
  - save_document_set
  - create_document_set
  - update_document_set
  - remove_document_set
---
# Recover a workflow

Use for failed tools, conflicts, interrupted work, and blocked synchronization.

- Inspect the actual error, active batch or run, current files, and returned
  repair. A failed write is not saved work. A stale hash needs a fresh read.
- Repair the named failure within the authorized scope. A suggested lifecycle
  does not authorize compilation, generation, task execution, or publication.
- Preserve successful commits, completed evidence, and active ownership. Resume
  an existing recoverable run instead of repeating uncertain external actions.
- If requirements are ambiguous, surface a diagnostic or ask for the concrete
  missing decision. Do not settle a product choice silently in code.
- A missing capability, credential, release, or approval is a blocker. Name it
  once and stop dependent actions. Do not try unrelated lifecycles to evade it.
- Use `report_feedback` for wrong or confusing Jazyk instructions, tools, or
  errors. It writes local feedback, transmits nothing, and does not resolve the
  work or replace a diagnostic.
- Report the failure, what actually landed, and the exact remaining prerequisite.
