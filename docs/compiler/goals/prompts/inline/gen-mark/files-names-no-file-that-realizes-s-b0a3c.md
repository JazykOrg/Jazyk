---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Require implementation files as well as tests before accepting a claim that a
  component was generated.
shown_when: >-
  Some documented requirements still have no implementing file, and the proposed file
  list supplies only tests, criteria, or other files that do not implement them.
format_arguments_in_order:
  - "Identifier of the component being recorded."
  - "Identifiers of requirements that still have no implementing file."
---
files names no file that realizes {}'s requirements (test and criteria files are not implementation), and nothing else carries {}; write the part of the deliverable that realizes them and list it in files, or, where a file another entity recorded already carries a requirement, name that file on the requirement's test row (`files: [path]`). Only an entity whose every requirement is carried by another entity's files records files: []