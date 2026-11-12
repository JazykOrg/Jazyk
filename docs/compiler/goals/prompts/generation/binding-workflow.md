Bind one requirement to the deliverable.

# Binding workflow

This is the binding scope, not a task, file or container boundary.

Inspect task_context, the project dependency and runner configuration, and existing test commands
before defining execution.

Reuse an ordinary suite task such as test, test-unit or test-browser and extend its discoverable
suite.

Keep distinct test functions and exact JUnit selectors for the criteria.

Separate tasks are justified by a different toolchain, isolation need or independently useful suite,
not by a criterion id.



## Find implementing files

Search the deliverable for an implementation of the statement; record the carrying files,
or none.

Absence is a finding, not a failure.

A file that only restates the statement (a page repeating the sentence, a comment) does not carry
it: list the files a criterion test inspects, and none when no such file exists, so the row reads
unimplemented and generation opens rather than failing. 

<!-- Existing tests identify statements by hash across id changes. The binding contract therefore searches before creating another test. -->
## Reuse an existing test

Search for an existing test that judges
the statement, starting with the package's existingTests (the files that name this requirement by
id, slug, or hash); bind to it when found, never write a duplicate beside it.

The one exception is a test that reads a description of the artifact instead of driving it: it
judges nothing, however well it passes, so rewrite it in place under the same name.

Driving means the test makes the artifact act at run time and checks what it does: an HTTP request
to a started server and its response, a process run with arguments and its output, a function called
with inputs and its result.

Importing a value and asserting its fields (`backendApi.style === 'REST'`, a list of endpoint paths,
a framework name) is reading a description, not driving, even when the value lives in a source file.

Record files as none when nothing the rewritten test drives exists yet.

A test carrying this statement's hash (in its name or its marker comment) under another requirement
id is this requirement's test, since ids renumber between compiles while the hash follows the
statement: rename it to the suggested name and bind to it, never write a second one beside it.

A test named with this id but another hash belongs to whatever statement that hash names now, not to
this one. 

## Write missing tests

When a criterion has no test, write one: the package's criteria are the evidence the
requirement is met, one test per criterion named by its suggestedTestName, and a requirement without
criteria takes one test on the statement.

Implementation found: the test pins the observed behavior.

Implementation absent: the test encodes the criterion and fails by design; it is the acceptance gate
generation must clear, so it must be a test some implementation can pass: assert the refusal or
outcome the criterion states (a constructor returning an error, a validity check answering false, a
message), never a contradiction of the input you passed (asserting that an order built from no items
has items can never pass). 

<!-- A saved task can cover several criteria. Exact JUnit selectors keep each criterion tied to its own evidence. -->
## Run tests and record the binding

Run the tests and record the binding with record_binding: files (empty
list when nothing implements it), tests (one entry per criterion with its verdict and evidence; the
requirement-level test only when there are no criteria), the verdict, the evidence.

Test source files and a saved suite task can cover criteria from several requirements.

Task bindings reuse run="task:<id>" and select each criterion independently by its exact JUnit
classname::name (or a unique case name).

The saved command runs the suite; it does not need to select only this criterion.

The task must emit fresh reports.

For legacy shell bindings, each entry's command selects only its own test by name, since its exit
code is the verdict.

The row's status then reads verified, unimplemented, partial (files exist and the criterion tests
split: generation work for the failing ones), or failing (every test fails against existing files: a
finding for the author).

The test must be falsifiable (its assertion fails when the requirement is violated) and inspect the
artifact, never prose about it; one that asserts the source text contains words or module names
inspects prose, not the artifact, and is the llm case or a real behavioral assertion; so does one
that reads a constant, an object literal, or a table describing the artifact (endpoint paths, a
runtime or framework name), which is a description in another syntax: the test drives the artifact
(starts the server and calls the endpoint, runs the command, opens the output), and a file holding
only such a description implements nothing.

A test of a technology constraint (a framework, a runtime, a database) reads the build's own
evidence: the dependency manifest, the module that imports the framework, the connection the running
server opens.

A response field the product reports about itself (`framework: 'Express'` from a health endpoint) is
a description the code writes, not evidence it runs Express.

Use the suggested test name.

Artifact and criteria paths are relative to the deliverable directory.

Task cwd and product/cache/report globs are project-relative; set cwd to the deliverable directory
when needed.

Legacy shell bindings run from the deliverable directory.

An artifact path never starts with the deliverable's own directory name: when the deliverable is
`handbook`, a page is `hiring.md`, not `handbook/hiring.md`, even where the documents call the
handbook a folder.

Use programmatic tests for command output, exit status, API responses, and automatically inspectable
artifact contents.

A criteriaPath is only an available path for judgment, not a recommendation to avoid execution.

A missing test runner is setup work: inspect the existing project configuration, then reuse or
define one ordinary suite task with fresh JUnit reports.

Never create one task per criterion to work around the absence of per-run arguments.

A requirement with no falsifiable programmatic assertion binds a kind llm test: write the criteria
file and record it with verdict from your own judgment of the deliverable.

Never rewrite implementation files during binding; binding observes, generation changes.



## Programmatic artifacts

For a programmatic test, artifact is the deliverable-relative test source containing
the declared name, such as tests/test_greeter.py. The JUnit XML path belongs in the
saved task's reports globs. Never write report cases to satisfy a binding gate.
Repair a source or selector mismatch without changing executable checks to llm.



## Judgment criteria

For an llm test, create a separate Markdown criteria file. Copy the package's
criteriaFrontMatter exactly, then add the criterion, implementation paths, concrete
inspection steps, and explicit PASS and FAIL outcomes. Read the actual deliverable
before recording a verdict. A Python test file cannot stand in for these criteria.
The package's sourceSelection lists allowed deliverable paths. If no pattern allows
your criteria file, inspect list_document_sets and update_document_set to include
its Markdown path in the set.source array, then write it.

Use baseHash from list_document_sets and preserve the set id, docs, roots and unrelated source
patterns.

Keep documentation out of source.
Never record a binding after a rejected file write until its criteria file exists.
