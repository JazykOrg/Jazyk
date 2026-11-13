---
name: edit-documentation
description: "Edit prose, requirements, criteria, or intended behavior without implicit compilation."
requiredCapabilities: "document writes"
---
# Edit documentation

Use for prose, requirements, criteria, and intended behavior changes.

- Read current text with `read_document`. Preserve unrelated prose and use its
  `hash` as `baseHash` with `write_document`. New files omit `baseHash`.
- Use format-specific editing for binary documents. Do not overwrite DOCX or PDF
  with text. A stale hash requires a fresh read before retrying.
- For an identified compiled requirement, prefer `revise_requirement`,
  `add_requirement`, or `retract_requirement`. Use `edit_fact` with `id`, `field`,
  and `value` for an identified entity, requirement, or view fact.
- Use `update_criterion` for a criterion. It can commit in chat without
  `begin_goals`. `edit_fact` does not accept criterion IDs.
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
