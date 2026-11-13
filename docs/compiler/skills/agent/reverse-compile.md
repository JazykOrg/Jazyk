---
name: reverse-compile
description: "Draft documentation from observed implementation and surface intent conflicts."
requiredCapabilities: "decompilation or authorized code inspection and drafting"
tools:
  - decompile_tasks
  - begin_decompile
  - submit_draft
  - preview_document_set
  - save_document_set
  - create_document_set
  - update_document_set
  - remove_document_set
  - inspect_asset_link
---
# Reverse compile implementation

Use to document observed implementation or reconcile an authorized code-first
change with intended behavior.

- Inspect the requested implementation and its tests. Existing docs and graph
  facts remain the source of current intended behavior.
- Use `decompile_tasks` and `begin_decompile` when available. The package names
  released scope, inventory, and tests. Stay within that scope.
- Read code and tests with the available file tools. Draft prose describing
  observable behavior, concrete outcomes, and limits. Separate observation from
  inference. Do not claim that the implementation proves stakeholder intent.
- Submit the document with `submit_draft` when using the decompile lifecycle.
  The draft is compiler input. Compilation and binding supply later checks.
- If no decompile tools are served, do not imitate their results. Explain the
  missing capability. An explicitly requested prose draft can still be produced
  from inspected evidence, without claiming a recorded decompile result.
- Surface conflicts with existing requirements. Accepting an implementation
  deviation as new intended behavior requires the user's decision.
- Read `edit-documentation` for accepted intent changes, `compile` for authorized
  reconciliation, and `bind` for observed implementation links. Each still
  obeys approval and cost rules.
- Report drafts, confirmed observations, unresolved intent conflicts, and sync
  steps that remain. A draft alone does not establish convergence or coverage.
