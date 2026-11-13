---
name: inspect-project
description: "Inspect project questions, code, docs, reviews, and status without changing them."
requiredCapabilities: "read"
tools:
  - goals
  - await_changes
  - inspect_asset_link
  - task_context
  - inspect_run
  - wait_run
  - read_run_log
  - search_run_log
  - read_run_changes
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
- When file tools are available, locate code by content with `search_files` and
  paths with `list_files` and a glob `pattern` before reading files one by one.
  Continue a search with `scanComplete: false` by passing its `next` as `cursor`. Read the surrounding code with
  `read_text_file`, then edit with `old_text`; the edit applies to that read.
- Reviews and status requests do not authorize edits, compilation, or execution.
- Use section IDs returned by reads or search. Do not assume a Markdown file has
  a section at `/`; that is the whole-file section for opaque inputs and assets.
- Reuse unchanged read results. An error in a tool batch can skip later calls;
  inspect each result and retry only skipped work still needed. Prefer the compact
  goal board before requesting every goal with full hints.
- Stop investigating when the evidence answers the question. Report findings
  and material uncertainty, without turning a question into implementation work.
