---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Give the AI judge written instructions for deciding whether the requirement is
  satisfied.
shown_when: >-
  The agent declares an AI-judged test without naming the Markdown file containing its
  inspection steps and passing or failing conditions.
format_arguments_in_order:
  - "Identifier of the requirement being judged."
---
test row for {} is llm but names no criteria file; write it (the package names its path under criteria/) and record that path as the artifact