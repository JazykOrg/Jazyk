---
name: edit-documentation
description: "Edit prose, requirements, criteria, or intended behavior without implicit compilation."
requiredCapabilities: "document writes"
tools:
  - edit_graph
  - edit_fact
  - revise_requirement
  - add_requirement
  - retract_requirement
  - retract_decree
  - bump_limit
  - answer_diagnostic
  - upsert_entity
  - update_entity
  - delete_entity
  - merge_entities
  - group_entities
  - dissolve_entity
  - upsert_requirement
  - update_requirement
  - delete_requirement
  - upsert_criterion
  - update_criterion
  - delete_criterion
  - classify_input
  - set_coverage
  - report_diagnostic
  - update_diagnostic
  - resolve_diagnostic
  - place_anchor
  - orphan_anchor
  - upsert_view
  - update_view
  - delete_view
  - preview_document_set
  - save_document_set
  - create_document_set
  - update_document_set
  - remove_document_set
  - update_project_settings
---
# Edit documentation

Use for prose, requirements, criteria, and intended behavior changes.

- Read current text with `read_document`. For a change to an existing document,
  call `write_document` with `old_text` copied exactly from that read and `text`
  as its replacement. It sends only the changed region, preserves unrelated prose,
  and guards the edit with the hash of your read. Use `replaceAll` to change every
  occurrence. Replace a whole document only for a complete rewrite, with its
  `hash` as `baseHash`. New files omit `baseHash`.
- Use format-specific editing for binary documents. Do not overwrite DOCX or PDF
  with text. A stale hash requires a fresh read before retrying.
- For an identified compiled requirement, prefer `revise_requirement`,
  `add_requirement`, or `retract_requirement`. Use `edit_fact` with `id`, `field`,
  and `value` for an identified entity, requirement, or view fact.
- Use `update_criterion` for a criterion. It can commit in chat without
  opening a compile batch. `edit_fact` does not accept criterion IDs.
  To change an existing criterion's quote anchor, use `upsert_criterion` with its
  current requirement and statement, then `update_criterion` for wording changes
  in the same `edit_graph` transaction. `update_criterion` cannot re-source an
  already quoted criterion.
- For relevant graph repairs, use the authored mutation tools directly in writable
  chat. No build goal or compile release is needed. Use `edit_graph` for related
  mutations that must commit together or for an edit independent of an open batch.
  Supply derived provenance for model judgment. Use `decision` only to record an
  explicit human choice. Keep follow-up reconciliation pending unless authorized.
- For asset reclassification, inspect the input's `quoteReferences`. When
  extracted requirements describe the supplied content rather than this product,
  delete those requirements with `delete_requirement` and classify the input in
  one `edit_graph` transaction. This preserves prose and prunes their cached
  entity mentions. `retract_requirement` removes prose and is unsuitable here.
  Repair independent quotes explicitly. Do not infer a missing capability merely
  from an entity's mention list. Keep referring layout requirements as requirements.
- Do not compile merely to obtain graph IDs. A direct prose edit is valid input;
  the next authorized compile reconciles it.
- Describe a requested behavior in docs before implementing it. If the behavior
  conflicts with existing requirements, surface that conflict. Ask for the
  missing decision rather than silently choosing an interpretation.
- Compilation is a separate action. Read the `compile` skill when requested or
  authorized as a prerequisite. Honor approval and cost rules for that step.
- Report the saved documents and whether graph reconciliation remains pending.
  Saving prose alone does not establish graph convergence or implementation.
