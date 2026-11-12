---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Preserve the test already assigned to a requirement while its implementation is being
  generated.
shown_when: >-
  The report attempts to substitute a different test even though the existing test and
  requirement have not changed.
format_arguments:
  rid: "Identifier of the requirement whose existing test must be reused."
---
requirement `{rid}` already has a current bound test; reuse its artifact, name, and kind instead of replacing the judge