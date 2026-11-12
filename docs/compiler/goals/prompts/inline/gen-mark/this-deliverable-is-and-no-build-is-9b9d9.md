---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Ensure Jazyk knows how to turn source files into the promised output, such as a PDF or
  compiled program.
shown_when: >-
  The chosen output format requires a build, but neither an earlier record nor this
  completion report supplies a build command.
format_arguments_in_order:
  - "The recorded output format."
  - "The output file the build must produce."
  - "The same output path, repeated for the build declaration."
---
this deliverable is {} and no build is recorded: return `build` with the command that produces `{}`, its `cwd`, and `produces` listing `{}`