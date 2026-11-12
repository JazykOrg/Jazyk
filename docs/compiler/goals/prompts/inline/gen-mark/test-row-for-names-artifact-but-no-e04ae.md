---
workflow: >-
  After writing a component and its tests, the coding agent calls record_generation with
  a report listing what it created. Jazyk validates that report before saving which
  files implement the documented requirements and which tests check them.
used_by:
  - "bootstrap/src/gen/mark.rs"
purpose: >-
  Keep a nonexistent test file from being accepted as completed work.
shown_when: >-
  The agent calls record_generation, but a test-file path in its completion report does
  not resolve to an existing file. Jazyk refuses to save that report.
format_arguments_in_order:
  - "Identifier of the documented requirement this test checks."
  - "The reported path to the test source file or Markdown inspection instructions."
  - "Extra search-location text for AI-judged tests: \" or the out directory's gen/\". It is empty for executable tests."

terms:
  test_row: "One entry in the agent's completion report, linking a requirement to a test."
  artifact: "The test source file, or a Markdown file telling an AI judge what to inspect."
  deliverable: "The generated project's root directory, where executable test source paths are resolved."
  JUnit_report: "XML results written after a test run. It records what happened; it does not contain the test to run."
why: >-
  A file path in an agent's report does not prove that the file was written.
  Jazyk checks the filesystem before accepting the report. Executable tests must
  exist in the generated project. AI inspection instructions may also live under
  gen/ in Jazyk's output directory; gen/files.rs::artifact_path checks both places.
---
test row for {} names artifact `{}` but no such file exists under the deliverable{}; write its test source or criteria file before recording, or fix the path. Generated JUnit reports belong in task reports, not artifact