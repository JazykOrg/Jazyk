---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Preserve an existing acceptance test while the agent changes the implementation it
  checks.
shown_when: >-
  The report tries to replace the test assigned to an acceptance condition even though
  that existing test still matches the current facts.
format_arguments:
  cid: "Identifier of the acceptance condition whose current test would be replaced."
---
criterion `{cid}` already has a current bound test; reuse its artifact, name, and kind instead of replacing the judge