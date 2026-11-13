---
name: generate
description: "Implement documented requirements and record deliverables and tests."
requiredCapabilities: "generation and source writes"
tools:
  - generation_tasks
  - begin_generation
  - record_medium
  - record_generation
  - verification_tasks
  - begin_verification
  - run_tests
  - record_verdict
  - release_work
  - classify_input
  - delete_requirement
  - edit_graph
  - edit_fact
  - link_asset
  - inspect_asset_link
  - remove_asset_link
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
  - report_spend
  - update_criterion
---
# Generate a deliverable

Use for implementing documented behavior or recording an owned source repair.

- A request to build or update the deliverable calls for implementation and
  verification, including necessary reconciliation of the requested docs. Read
  `compile` for that prerequisite. Classification and graph edits alone do not
  complete a deliverable request. Continue independent requested work when one
  part is blocked. Honor separate cost and publication rules.
- If the requested stage awaits release and `release_work` is available, call it
  for that stage under the user's explicit build request, then refresh the tasks.
  Finish graph repairs before releasing generation: releases name the current
  generation. Keep manual workflow settings unchanged. If release is unavailable
  or a separate approval is required, name that specific blocker.
- For unchanged supplied files, use `inspect_input`, `classify_input`, and
  `link_asset` at selected source destinations. Keep their links in the complete
  generation manifest and verify the resulting deliverable.
- Inspect `generation_tasks` with `entity` for a named entity. Resolve the returned
  `blockedBy` prerequisites before beginning it. Use the full task list only for
  a request spanning entities. An empty scoped answer is not evidence about other
  entities.
- Inspect `generation_tasks` for requested scope. An empty list does not prove
  an artifact exists or passes checks. Follow returned prerequisites.
- If no source mapping exists, inspect `list_document_sets`, then use the current
  config hash with `update_document_set` to select narrow implementation, test,
  and support paths. Preserve docs and unrelated settings.
- Claim one ready entity with `begin_generation`. Use the returned requirements,
  criteria, fact hash, ownership, and existing inventory as the contract.
- Read existing source and tests. Develop real implementation and tests together
  in the order the work needs. A separate binding pass or failing baseline is
  not required. Do not invent observable behavior beyond requirements.
- For an intended behavior change, read `edit-documentation` and reconcile the
  affected graph when authorized before recording against changed requirements.
- Preserve existing assertions. Reuse related suite tasks with separate JUnit
  selectors where useful. Keep tests traceable to requirements and criteria.
- Call `record_generation` with the package's `factHash` and the complete manifest
  of actual files, tests, support files, and any required build task. Follow its
  schema. Test artifacts are test source, not generated reports.
- Record necessary invented choices honestly with their scope and reasoning.
  Surface product or behavior gaps rather than silently deciding them.
- Recording frees the claim. Complete other assigned entities only when in
  scope. Read `verify` to run programmatic checks at a useful milestone.
- Before ending a deliverable request, compare the requested scope with saved
  implementation files and links, recorded manifests, and verification evidence.
  Metadata edits or feedback alone do not establish a generated deliverable.
  Name a remaining blocker from observed evidence and preserve completed work.
- LLM criteria remain unjudged until evidence-based verification. Report what
  exists and what is still unverified. Do not infer publication from generation.
