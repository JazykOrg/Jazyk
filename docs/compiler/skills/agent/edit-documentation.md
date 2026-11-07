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
- Do not compile merely to obtain graph IDs. A direct prose edit is valid input;
  the next authorized compile reconciles it.
- Describe a requested behavior in docs before implementing it. If the behavior
  conflicts with existing requirements, surface that conflict. Ask for the
  missing decision rather than silently choosing an interpretation.
- Compilation is a separate action. Read the `compile` skill when requested or
  authorized as a prerequisite. Honor approval and cost rules for that step.
- Report the saved documents and whether graph reconciliation remains pending.
  Saving prose alone does not establish graph convergence or implementation.
