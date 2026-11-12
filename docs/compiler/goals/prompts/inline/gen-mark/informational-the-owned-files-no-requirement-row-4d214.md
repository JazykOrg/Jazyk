---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Explain how much generated content lacks a recorded link to the documentation.
shown_when: >-
  The report has already been accepted. Jazyk returns measurements of files and lines
  not linked to a documented requirement; this note explains those measurements rather
  than rejecting the work.
---
informational: the owned files no requirement row names and the significant lines outside every recorded site; the generation is accepted, and a large ratio suggests the deliverable states behavior the documents do not (docs/consumers/gen.md#the-unattached-remainder)