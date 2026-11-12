---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Save the location of an executable test so Jazyk can inspect it and check future
  changes.
shown_when: >-
  The agent reports an executable test without supplying the path to its source file.
format_arguments_in_order:
  - "Identifier of the requirement being tested."
---
test row for {} has an empty artifact; name the test source the row's run command executes, not its generated JUnit report