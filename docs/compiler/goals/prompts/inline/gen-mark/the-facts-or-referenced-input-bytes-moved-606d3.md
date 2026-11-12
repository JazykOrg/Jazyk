---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Stop a completion report prepared against outdated requirements or input files from
  being accepted as current.
shown_when: >-
  The agent opened a generation package, then its requirements or referenced input files
  changed before it called record_generation.
---
The facts or referenced input bytes moved since begin_generation. Reload the package, use the current exact inputs and record its current factHash.