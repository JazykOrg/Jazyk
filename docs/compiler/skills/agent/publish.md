---
name: publish
description: "Push, publish, or deploy only with explicit authority and verified integration state."
requiredCapabilities: "publication integration and authority"
tools:
  - task_context
  - update_tasks
  - run_task
  - inspect_run
  - wait_run
  - cancel_run
  - read_run_log
  - search_run_log
  - read_run_changes
---
# Publish changes

Use for an explicitly requested push, publication, release, or deployment.

- Refresh `project_context`. Inspect selected and default branches, available
  integration actions, unsynced files, and project instructions.
- Use the requested destination and authorized scope. Workspace edits do not
  imply permission to push or deploy. Honor per-action cost and approval rules.
- Use only actions the integration actually offers. Missing credentials or an
  unavailable publish action is a concrete blocker, not permission to guess.
- Run required authorized local checks. Preserve other contributors' work and
  follow the project's git and release rules.
- Verify the actual remote result and exact revision. Monitor a deployment to
  success or failure. Report failures and any remaining rollback decision.
- State local edits, pushed revision, and deployed revision separately. Never
  infer deployment from a push or report a remote success from local evidence.
