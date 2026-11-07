# Change code

Use for bug fixes, refactors, and features in an existing deliverable.

- Read the requested files, relevant requirements and criteria, and tests. Find
  recorded ownership from the source read. Separate implementation detail from
  a change to intended observable behavior.
- For a repair within requirements, keep the docs' intent and test assertions.
  Limit work to the requested files and directly affected tests.
- For changed behavior, read `edit-documentation` and update prose first. Resolve
  ambiguity before choosing behavior. If the graph needs reconciliation, read
  `compile` and obtain any missing cost approval before that prerequisite.
- If source reads return `owner` and `begin_generation`, claim that entity with
  `begin_generation` before writing. Its package supplies current requirements,
  criteria, file inventory, hashes, and gates. Read `generate` for recording.
- Implement real code and tests within those requirements. Do not remove a
  failing assertion to manufacture evidence. Refactoring must preserve behavior.
- Use guarded source writes where available. External file tools must preserve
  the same ownership and requirements boundaries.
- Record the changed entity with `record_generation` and its complete manifest.
  Recording ends the claim. Do not regenerate unrelated entities or drain the
  goal board for a narrow repair.
- Read `verify` and run the relevant authorized checks. Inspect unexpected
  failures before changing more code.
- If code is undocumented or requirements do not cover the change, read
  `reverse-compile` to inspect observed behavior or propose missing docs. Do not
  treat a draft as ratified intent. Explain any blocked synchronization step.
- Report source changes, docs changes, recorded evidence, and remaining sync.
