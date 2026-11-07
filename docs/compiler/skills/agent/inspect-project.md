---
name: inspect-project
description: "Inspect project questions, code, docs, reviews, and status without changing them."
requiredCapabilities: "read"
---
# Inspect a project

Use for project questions, explanations, reviews, status, and change impact.

- Answer general knowledge questions directly. Do not discover a project just
  because tools exist.
- Read the named document or code before explaining it. Use `project_context`
  for current integration state. Do not infer a selected branch from prose.
- Use search, `get_entity`, `read_section`, `load`, and `expand` for relevant
  graph evidence. Use `read_document` for current source prose.
- Discover a document set only when a set-local query requires it.
- Distinguish documented intent, implemented behavior, derived graph state,
  recorded evidence, and your inference. Name stale or missing evidence.
- Reviews and status requests do not authorize edits, compilation, or execution.
- Stop investigating when the evidence answers the question. Report findings
  and material uncertainty, without turning a question into implementation work.
