# Generate a deliverable

Use for implementing documented behavior or recording an owned source repair.

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
- LLM criteria remain unjudged until evidence-based verification. Report what
  exists and what is still unverified. Do not infer publication from generation.
