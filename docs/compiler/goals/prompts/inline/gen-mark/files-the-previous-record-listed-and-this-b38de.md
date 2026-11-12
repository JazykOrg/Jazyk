---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Tell the agent where to recover files that its new completion report removed.
shown_when: >-
  A successful record omits files that belonged to the previous record, so Jazyk removes
  those files and returns this note.
---
files the previous record listed and this manifest omits were removed from the deliverable (their last content is under deliverable-baseline/ in the out directory)