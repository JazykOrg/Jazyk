---
purpose: >-
  Instruction fragment: Report that jazyk's own prompt, tool, schema, or error message is
  ambiguous, wrong, confusing, or missing something.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Report that jazyk's own prompt, tool, schema, or error message is ambiguous, wrong, confusing, or missing something. It appends one line to the project's own .jazyk/feedback.jsonl, read later by jazyk's developers, not this project's authors; nothing is transmitted anywhere. It never touches the graph. Use it for what blocked YOU; problems in the documents are diagnostics, not feedback. kind: ambiguous, wrong, confusing, missing, or other. subject names the tool, argument, or instruction. message says what was unclear and what would have helped. Then continue the task with your best judgment.