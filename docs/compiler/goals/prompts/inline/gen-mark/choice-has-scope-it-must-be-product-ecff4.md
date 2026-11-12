---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Classify a design choice the agent made because the documentation left it unspecified.
shown_when: >-
  The agent reports a choice with a category other than product, behavior, or detail.
format_arguments_in_order:
  - "The first 60 characters of the reported choice."
  - "The category supplied for that choice."
---
choice `{}` has scope `{}`; it must be `product`, `behavior`, or `detail`