---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Associate every reported test with the documented obligation it is supposed to check.
shown_when: >-
  A test entry in the completion report has no requirement identifier.
format_arguments_in_order:
  - "The test name, artifact path, or \"?\", used to identify the incomplete entry."
---
test row `{}` names no requirement; every row carries the id of the requirement it tests ({{requirement, kind, label, artifact, name, run, cwd}})