---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Make a reported executable test runnable by the verification workflow.
shown_when: >-
  The report identifies a test but provides neither a saved task nor a legacy command
  that can execute it.
format_arguments_in_order:
  - "Identifier of the requirement being tested."
---
test row for {} has an empty run command; record task:<id> with its case selector, or a legacy command selecting that test