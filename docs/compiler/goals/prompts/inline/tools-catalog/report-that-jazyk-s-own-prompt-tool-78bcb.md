---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Report that jazyk's own prompt, tool, schema, or error message is ambiguous, wrong, confusing, or missing something. It appends one line to the project's own .jazyk/feedback.jsonl, read later by jazyk's developers, not this project's authors; nothing is transmitted anywhere. It never touches the graph. Use it for what blocked YOU; problems in the documents are diagnostics, not feedback. kind: ambiguous, wrong, confusing, missing, or other. subject names the tool, argument, or instruction. message says what was unclear and what would have helped. Then continue the task with your best judgment.