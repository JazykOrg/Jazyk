---
name: bind
description: "Connect existing implementation and test evidence to requirements."
requiredCapabilities: "binding"
tools:
  - binding_tasks
  - begin_binding
  - record_binding
  - verification_tasks
  - begin_verification
  - run_tests
  - record_verdict
  - release_work
  - task_context
  - update_tasks
  - run_task
  - inspect_run
  - wait_run
  - cancel_run
  - read_run_log
  - search_run_log
  - read_run_changes
  - preview_document_set
  - save_document_set
  - create_document_set
  - update_document_set
  - remove_document_set
---
# Bind existing implementation

Use when asked to connect existing code to documented requirements and criteria.

- Inspect `binding_tasks` for the requested scope. Claim it with `begin_binding`.
- Read the package's requirements, criteria, existing code inventory, and tests.
  Inspect actual implementation and executable evidence with available tools.
- Record observed implementation and test links with `record_binding` using the
  returned hashes and schema. A filename or test name alone proves no coverage.
- Distinguish satisfied, missing, stale, and uncertain behavior. Do not rewrite
  requirements or mark evidence passing merely to make binding succeed.
- For an authorized repair, read `change-code`. For undocumented observed
  behavior, read `reverse-compile`. Those are separate procedures and authority.
- Follow the active package's completion and lease rules. Report the recorded
  bindings and any remaining generation or verification work.
