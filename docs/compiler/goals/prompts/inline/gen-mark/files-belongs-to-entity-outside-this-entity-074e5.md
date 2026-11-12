---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Keep one component from claiming another component's private implementation files.
shown_when: >-
  The agent lists a file owned by a different component group among the files it
  generated.
format_arguments_in_order:
  - "The file the agent is trying to claim."
  - "The component already recorded as owning that file."
  - "The same file path, repeated in the suggested way to reference existing implementation."
---
files: `{}` belongs to entity `{}`, outside this entity's component group; never record another group's file. Write a file of your own for your requirements, or name `{}` on a test row's `files` when it already carries that requirement