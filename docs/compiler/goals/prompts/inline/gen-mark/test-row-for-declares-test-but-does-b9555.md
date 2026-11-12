---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Make the reported test name point to a real test in its source file.
shown_when: >-
  The named executable test cannot be found in the reported file. A generated XML
  results file is not a substitute for the test source.
format_arguments_in_order:
  - "Identifier of the requirement being tested."
  - "The test name the agent declared."
  - "The file that was supposed to contain that test."
---
test row for {} declares test `{}` but `{}` does not contain that name; artifact must name the test source, not the generated JUnit report; match name to the test in that source and the emitted report case