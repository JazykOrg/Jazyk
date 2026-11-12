---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Refuse to overwrite the generation-record counter when no higher value can be
  represented.
shown_when: >-
  Incrementing the saved generation-record sequence would overflow its integer type. No
  new record is saved.
---
The entity generation record sequence is exhausted; no record was changed.