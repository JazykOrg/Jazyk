# Generation

Generation turns the semantic graph into the end product and the tests that hold it to
its spec, in one workflow. It reads the graph through the
[context engine](../compiler/context.md) and the [read tools](../compiler/tools.md#read-tools),
never the raw source files. The graph, not the prose, is the spec.

The end product is called the deliverable. It is usually code, but the workflow does not
assume software: a book, a schematic, a course. Whatever the requirements describe,
generation produces it, and produces runnable tests beside it. Tests are the tie between
the requirements and the deliverable.

## The deliverable

The deliverable is the project directory itself by default: the generated product lands
beside `jazyk.toml`, and the default docs glob keeps `docs/` as source. A project may
point it elsewhere:

```toml
[gen]
deliverable = "../project2"
```

- `deliverable` resolves relative to the project root. Default `.`. Everything
  generation produces lands under it.
- The deliverable directory is excluded from doc input; the docs glob whitelists paths
  back in ([glob](../compiler/project-settings.md#glob)). Generation metadata (the
  ledger, criteria files) stays in the out directory, never in the deliverable.

That is the only generation setting. What the deliverable is (a Rust crate, a web app,
a book, a schematic) is a fact the documents state, so it reaches the generator through
the graph and the context pack like every other fact. The project file describes where
things go, never what to build; there are no hints.

The generator chooses everything about the deliverable's form: the medium, the layout,
the file names, and the build files that make its recorded commands executable. What
binds the layout to the graph is the manifest: every completed task records which
deliverable files implement which requirements ([the ledger](#the-ledger)).

## The deliverable is the artifact, never a description of it

A requirement naming a format, a medium, or a piece of content is an obligation to
produce that thing. Writing a document that says the thing will be produced satisfies
nothing, and a test asserting that such a document mentions the format verifies nothing:
both sides are prose, and the artifact does not exist. The substitution is the most
common generation failure, because prose is what a language model produces most easily.

So the rule is: whatever the requirements say the deliverable is, that is what lands
under the deliverable directory.

- When the medium is text the requirements describe directly (source code, a manuscript,
  a configuration), the generated files are the deliverable.
- When the medium is a format a tool must produce (a slide deck, a PDF, a rendered site,
  a compiled binary, an image), the generated files are the source that produces it,
  plus [the build](#the-build) that produces it. The produced artifact is the
  deliverable, and it is what the tests inspect.

Content requirements are satisfied by content, not by placeholders. A requirement saying
an artifact shows a title, states a definition, or uses a color is met only when the
artifact carries that exact title, that definition, that color value. A generator that
does not know what to write is missing a requirement, and the honest outcome is a
failing row, not invented filler.

## The medium is decided once, before anything is generated

What the deliverable is made of is one decision for the whole deliverable, not a
judgment each task repeats. A per-task decision is where the substitution above creeps
in: asked to write "the About slide", a generator writes prose about a slide, because
nothing in front of it said the deliverable is a file a tool must produce.

So the first generation run for a deliverable decides the medium first, in its own
step, and records it in [the ledger](#the-ledger):

```yaml
medium:
  form: Microsoft PowerPoint deck            # what the deliverable is, in the model's words
  produced: built                            # written | built
  toolchain: python3 with python-pptx        # what writes or builds it
  artifact: jazyk.pptx                       # deliverable-relative; only when built
```

- The input is the requirements that say what the deliverable is: the same graph every
  task reads, budgeted like a [context pack](../compiler/context.md).
- `produced: written` means the generated files are the deliverable. `produced: built`
  means they are the source that produces `artifact`, and [the build](#the-build) runs
  it.
- Every task package carries the decision, and every task's instructions state it as a
  fact rather than asking again. A task that generates under `produced: built` writes
  source, never the artifact itself and never prose about it.
- The decision is made once and reused, like the toolchain and the build. It is
  re-decided only when nothing is generated: a ledger with no entities decides again,
  so wiping the deliverable is how a project changes its mind. `--force` regenerates
  against the recorded decision.
- Under `produced: built`, recording a build is not optional. The manifest step is
  rejected when it records none and none is recorded yet, with one corrective retry,
  the same gate that catches an unrunnable test command. A generator that cannot name
  the command that produces the artifact has not produced it.

## The build

A deliverable whose medium must be produced by a tool records one build command:

```yaml
build:
  run: python build_deck.py
  cwd: .
  produces:
  - jazyk.pptx
```

- The command runs from the deliverable directory, `cwd` relative to it.
- `produces` lists the artifact paths the command creates, relative to the deliverable.
  They are the deliverable's real output; the ledger keeps them so a reader knows what
  the run was supposed to make.
- The build is per deliverable, not per entity. The first task that needs one records
  it; every later task receives it in its package and reuses it, the same way run
  commands establish one toolchain.
- What the build runs is a [support file](#file-ownership-and-conventions), never an
  entity's own file. One artifact is assembled from every entity's part, and an entry
  point owned by the entity that happened to generate first would freeze the artifact
  at that entity's part: no later task may write into it. So the entry belongs to the
  deliverable, its current content travels in every task package, and each task
  returns it updated so the artifact includes its part too. The convention the entry
  uses to include a part (a function it calls, a list it reads) is the generator's,
  visible to every later task in the entry itself.
- A task that rewrites the entry sees what it is calling. Under a built medium the
  package carries the other entities' part files with their content, not just their
  paths and the statements they hold: an entry is a call site, and a call site needs
  the name of the thing it calls. Without it a task guesses, and the build dies on a
  function that does not exist.
- The entry is checked against the parts, not trusted: every generated file the parts
  live in must be named in it. An entry that re-implements a part instead of calling
  it, or that quietly drops one, gets the same corrective retry a contradicted
  manifest gets. This is the one composition rule the harness can enforce without
  knowing the medium: whether the entry mentions the files at all.
- A generation run ends by running the build. A built deliverable is not generated
  until the artifact exists, so `jazyk gen` produces it as its last step and reports
  it; a failure there is recorded like any other, which is what the next run reads.
- `jazyk test` runs the build once, before any row is judged. A non-zero exit, or a
  missing path in `produces`, fails the run and reports the build, since there is
  nothing to verify when the artifact was not produced. See [runners](#runners).
- A failed build is recorded, not just printed. The ledger keeps the last run's
  outcome under `build.lastRun` (when, whether it succeeded, and the tail of what it
  said), and every task package carries the failure while it stands, together with
  which of this entity's own files the failure names. Regenerating is
  how it gets fixed: the task that owns a file the failure names sees the message and
  writes source that runs. Without that, a generator repeats the same broken part
  every round, because nothing ever told it the artifact was never produced.
- A deliverable that is its own output records no build, and nothing runs.

The build is a fact the generator derives from the requirements, like every other
choice about form. Jazyk holds no list of media and no template per format.

## The entity is the unit of generation

Each [entity](../compiler/model/entity.md) generates in one bounded task. The task's
input is the entity's [context pack](../compiler/context.md#request): its `definition`,
its requirements across all documents, and its relationships. Nothing outside the pack
leaks in, so each task is small, repeatable, and auditable.

One task produces both halves: the entity's part of the deliverable, and the tests for
each of its requirements. Deriving tests in the same task as the product means the tests
exercise the interfaces the product actually got, not the interfaces a separate pass
guessed.

## Order from relationships

[Relationships](../compiler/model/relationship.md) give structure and order:

- `composition` → ownership and nesting.
- `association` → references.
- `dependency` → imports or injection.

Generation runs in topological order over the relationship edges: leaf entities (value
objects) first, then the entities that compose or depend on them. Each task can
reference already generated files through the manifest.

## File ownership and conventions

- Every deliverable file belongs to the entity whose task wrote it, recorded in
  [the ledger](#the-ledger). A task never overwrites another entity's files: the
  harness rejects a file path already recorded for a different entity and asks the
  worker for another path (one corrective retry, then the task fails). Using another
  entity's files goes through references (imports, includes), never through rewriting
  them.
- The task package names those files with the statements they carry, not just their
  paths. A composite deliverable is assembled from parts other tasks wrote, and a path
  alone says nothing about what is inside; the statements do, and they are what the
  graph already knows. So the entry per entity is its `files` and what each set
  `holds`, and the task composing them reads or imports those paths knowing what they
  contain.
- One toolchain per deliverable. The first task establishes it (the language, the test
  runner, the build files); every later task reuses it. The task package carries the
  run commands already recorded in the ledger, so a worker sees the established
  conventions and never introduces a second test runner.
- A recorded run command must execute from the deliverable directory as recorded. When
  it needs a build or configuration file no task has written yet (a `package.json`, a
  `Cargo.toml`), the task returns that file as a support file. Recording a command
  that cannot run is a generation defect; verification surfaces it as a failing row.
- Support files belong to the deliverable, not to an entity. They are what makes the
  recorded commands runnable (a `package.json`, a `Cargo.toml`, the entry point a
  build runs), and every task may rewrite one: a manifest that lists more parts than
  the last task saw is exactly why the file exists. The ledger keeps them in their own
  `support` list, ownership never applies to them, and their content does not enter an
  entity's fact hash.
- The ledger's file lists are sets: the harness deduplicates them on write.
- A corrective retry never costs the parts of an answer the complaint did not name.
  The previous answer goes back with it, the request is to change only what the
  complaint names, and what comes back is merged over it: a retry that returns no
  support files and no build keeps the ones the first answer gave. Weak models answer
  a correction by writing a fresh reply, and a fresh reply drops whatever it forgot;
  the harness holds the rest.
- A step may return several files, and the contract says so. Nearly a third of
  generated replies carry more than one `FILE:` block, because a part that needs a
  dependency manifest, an entry point, or a second module is a normal thing to write.
  The first block is the entity's part; the rest are files the task wrote too.
  Tolerating what the protocol forbade is how a `requirements.txt` header once
  swallowed five files into one.
- Which of those extra files belongs to the entity is decided by the manifest, not by
  the order they arrived in. A file the manifest lists in `supportFiles`, or names as
  the build's entry point, belongs to the deliverable; everything else the task wrote
  belongs to the entity that wrote it. Classifying by arrival would make a second test
  file deliverable-wide, unowned, and rewritable by any later task.
- A support file never lands on a file an entity owns, this task's own product and
  tests included. Support files exist so any task may rewrite them; letting one take
  an owned path would let a manifest step quietly overwrite the module the product
  step just wrote.
- A reply in the wrong shape gets one corrective round before the task fails: a
  product or tests reply whose `FILE:` line never appears, a manifest that is not
  valid JSON. The
  complaint is quoted back with the same request, and the correction shows the shape
  rather than describing it. A reply that opens with a sentence or a fence and then
  gives its `FILE:` line is not a shape failure: the preamble is dropped and the file
  starts at the line. Shape is the harness's contract,
  and a weak model drops it under a long prompt well before it gets the content wrong;
  failing the task over a missing brace throws away work that was otherwise fine.
- The manifest must agree with the artifacts. The harness scans the tests artifact for
  the suggested test names and hands the found list to the manifest step. A manifest
  that contradicts the artifact (a declared programmatic test whose name is absent
  from it, or a present test left undeclared) gets one corrective retry; rows still
  wrong after the retry fall back to `llm` with a criteria file.
- The suggested test name is a suggestion. What the harness requires is that the name
  the manifest declares for a requirement is really in the tests artifact and that the
  command selects it; a generator that named its test its own way has still written a
  test, and the ledger records the name it used. Only a declared name that appears
  nowhere in the artifact is a contradiction.
- A programmatic run command must invoke the tests artifact and select the test: a
  command that names neither the tests artifact nor the test name runs the product,
  not the test, and is invalid. Same corrective retry, same `llm` fallback.

## Dense entities generate in parts

A stringent component legitimately carries 50 or more requirements, and one generation
call has an output ceiling. The generation divides:

- The first part generates the types, state, and the first group of requirements.
- Each further part receives what was generated so far and the next group of
  requirements, and returns only additional content to append.
- Parts concatenate; traceability markers per requirement are unaffected.

The group size defaults to 20 requirements per part. The `entity-too-dense` check warns
the author when an entity approaches the configured ceiling
([limits](../compiler/project-settings.md#limits)), so splitting the documentation into
subsections stays a choice, not an emergency.

## Tests tie requirements to the deliverable

Each [requirement](../compiler/model/requirement.md) derives a test, keyed by the
requirement id. A failing test names the requirement it verifies, and a changed
requirement invalidates exactly the tests keyed to it.

The [EARS](../compiler/concepts/ears.md) pattern of a requirement suggests the test
shape:

- event-driven (`When ...`) → a scenario: arrange, trigger the event, assert the
  response.
- ubiquitous (`The <entity> shall ...`) → a property or invariant check.
- unwanted behavior (`If ..., then ...`) → a negative check.
- state-driven (`While ...`) → a stateful check: enter the state, assert the behavior
  holds throughout.

There are exactly two test kinds. The generator picks the kind per requirement; unit,
integration, and cucumber are prompting examples of the first kind, not a taxonomy the
harness enforces:

- `programmatic`: any test a command can run. The generator writes the test artifact
  into the deliverable and records the exact command that runs it. The command's exit
  code is the verdict, so the artifact must propagate failure: a harness that prints a
  failure and still exits zero verifies nothing.
- `llm`: a test that requires judgment, or a deliverable that is not executable
  software. No programmatic definition exists; the harness gives an agent the
  requirement, its context, and the location of the implementing files, and asks it to
  confirm the behavior. The verdict is the test. See
  [criteria files](#criteria-files-for-llm-tests).

A test inspects the artifact the requirement is about, never a document that describes
it. Asserting that a manifest names a format, that a plan lists a feature, or that a
comment restates the statement is circular: both sides are the generator's own prose,
and the test passes whether or not the artifact exists. When the medium is produced by
[the build](#the-build), the test opens what the build produced. When the deliverable
cannot be inspected by a command at all, the honest kind is `llm`, not a programmatic
test pointed at prose.

A test must be falsifiable: its assertion has to fail when the requirement is violated.
The question that produces one is what change to the artifact would break this
requirement, and the assertion checks exactly that. A test that passes either way is
worse than no test, because the ledger then reports `verified` for a requirement nothing
checked. When no falsifiable assertion is available from the artifact at hand, the row
is `llm`. Choosing `llm` is a correct outcome, not a failure to try harder; inventing a
stand-in assertion is the failure.

## Traceability

Every requirement carries a verbatim `quote`
([shared fields](../compiler/model.md#shared-fields)). The trail from deliverable to
prose has two carriers:

- The test name embeds the requirement id and the first 8 hex characters of the hash of
  its statement: `req_catalog_3_a1b2c3d4`. The name is part of the artifact itself and
  of the recorded run command, so a reworded requirement mechanically breaks the
  recorded command: even a harness that has never heard of Jazyk fails to find the
  stale test.
- Anchored sites in [the ledger](#the-ledger). While writing, a worker puts a
  single-line marker comment directly above each implementing site: `req:catalog-3
  hash:a1b2c3d4` in the medium's comment syntax, nothing else on the line. The marker
  is a wire format, not part of the product: `gen_mark` strips every marker line from
  the written files and records each as a site on the requirement's row: the file, the
  line, and `head`, the verbatim next significant line. The deliverable carries no
  Jazyk metadata; the binding lives in the out directory.

The division of labor: the worker owns localization (it knows where each requirement
lands while it writes), the harness owns anchoring (recording, locating, healing).

Sites relocate defensively. A renderer locates a site by matching `head` against the
current file, whitespace-insensitively. More than one match: the occurrence nearest the
recorded line wins. No match: the site is `lost` and shown as such, never guessed.
Anchoring never parses the medium (no function names, no language syntax), so it works
for code, prose, or any other deliverable. Hand edits that move a site heal on the next
match; staleness stays a [hash comparison](#status-is-derived-never-stored), never an
anchor judgment.

The trail is test or site → requirement id → `quote` → section.

## The ledger

`gen/ledger.yaml` in the out directory is the single generation and verification
metadata file. Two maps:

- `entities`: generation state. What was generated for each entity, against which facts.
  Drives incremental regeneration.
- `requirements`: verification state. How each requirement ties to the deliverable and
  how it is verified.

Two more keys sit beside them: `medium`, the deliverable's
[decided form](#the-medium-is-decided-once-before-anything-is-generated), written by
the first run, and `build`, present only when that medium must be produced by a tool
([the build](#the-build)).

```yaml
support:                                  # deliverable-wide files any task may rewrite
  - build_deck.py                         # the build's entry point
  - requirements.txt

medium:                                   # decided once, carried by every task package
  form: Microsoft PowerPoint deck
  produced: built                         # written | built
  toolchain: python3 with python-pptx
  artifact: jazyk.pptx                    # deliverable-relative; only when built

build:                                    # optional; absent when the files are the output
  run: python build_deck.py               # runs once, before any row is judged
  cwd: .                                  # deliverable-relative working dir
  produces:                               # deliverable-relative artifact paths
    - jazyk.pptx

entities:
  catalog:
    factHash: 9f2ab4c1d0e77a3b            # hash of name, definition, all referencing statements
    requirements: [req:catalog-1, req:catalog-2, req:catalog-3]
    files:                                # deliverable-relative files this entity's
      - src/catalog.rs                    # generation produced or touched
      - tests/catalog.rs

requirements:
  req:catalog-3:
    entity: ent:catalog                   # owning entity (first referenced; follows redirects)
    files:                                # manifest: deliverable-relative files
      - src/catalog.rs                    # implementing this requirement
    sites:                                # anchored implementing sites, from stripped
      - file: src/catalog.rs              # markers (see traceability)
        line: 41                          # 1-based, in the stripped file
        head: "fn add(&mut self, i: Item) {"  # verbatim next significant line
    test:
      kind: programmatic                  # programmatic | llm
      label: unit                         # freeform, the generator's own words
      artifact: tests/catalog.rs          # deliverable-relative; for llm, criteria/req-catalog-3.md under gen/
      name: req_catalog_3_a1b2c3d4        # embeds requirement id + hash prefix
      run: cargo test req_catalog_3_a1b2c3d4    # for llm, jazyk test req:catalog-3
      cwd: .                              # deliverable-relative working dir for run
    hashes:
      requirement: <full statement hash>  # written only at generation time
      test: <hash of test artifact bytes>
      files: <hash over the manifest files, sorted, concatenated>
    verdict: none                         # none | pass | fail (last run outcome)
    lastRun: 2026-07-03T18:40:00Z
    exitCode: 0                           # programmatic runs only; absent for llm rows
    evidence: "cargo test: 1 passed"      # or the llm verdict reasoning, short
```

### Status is derived, never stored

A requirement's verification status is a pure function of the row, the live graph, and
the files on disk, recomputed at every read. First match wins:

1. No row, or the test artifact is missing → `missing`.
2. The live statement hash differs from `hashes.requirement` → `stale-requirement`. The
   test verifies a sentence that no longer exists. Regeneration is needed;
   `jazyk test` refuses to run the row and points at `jazyk gen`.
3. The test artifact bytes differ from `hashes.test` → `stale-test`. Rerun.
4. The manifest files hash differs from `hashes.files` → `stale-code`. Rerun.
5. Otherwise the last verdict: `pass` → `verified`, `fail` → `failing`,
   `none` → `unverified`. A run whose command never executed leaves the verdict at
   `none` and the reason at `runner-failed`, so a broken machine reads as unverified,
   not as a failing deliverable (see [runners](#runners)).

Hashes are written at exactly two moments: generation marks a task done (all three), and
a test run completes (`test` and `files` rebaseline, never `requirement`). Every
staleness flip is a deterministic hash comparison. The model owns three judgments only:
the test kind, the test itself, and the verdict of an `llm` run.

### The cascade

Rewording a requirement flips its row to `stale-requirement` and moves its entity's
`factHash`, so `gen_pending` lists the entity. Generation rewrites the implementing
files and the test (the verdict resets to `none`). If the product does not yet satisfy
the new statement, the fresh test fails. Hand edits to the deliverable flip exactly the
rows whose `files` hash moved to `stale-code`. Reruns update verdicts; when the test
passes, the requirement is `verified`. Nothing in this loop is remembered by a human.

## Criteria files for llm tests

For `kind: llm` rows, generation writes `gen/criteria/req-<slug>.md` in the out
directory (metadata, not deliverable): front matter with the requirement id and the full
statement hash; body with the statement, the verbatim quote, the manifest file paths,
the steps to confirm, and the verdict contract (`PASS` or `FAIL` plus reasoning). It is
the packaged setup for any harness: context, the location of the implemented product,
and what to confirm. Editing it flips `stale-test` like any test artifact.

## Runners

`jazyk test` runs [the build](#the-build) first, once, when the ledger records one. A
non-zero exit, or a path in `produces` that the command did not create, stops the run
before any row is judged and reports the build's own output. A row cannot say anything
true about an artifact that was never produced, so reporting failures per requirement
would name the wrong culprit.

- `programmatic`: `jazyk test` executes `run` in `cwd` under the deliverable. Exit 0 is
  a pass, anything else is a fail. Before running, the runner greps the artifact for the
  test `name`; if absent the row is `stale-test`, not `failing`, and nothing executes.
  The row records the exit code beside the output, so a verdict can be read back
  without rerunning it.
- `llm`: two harnesses, one contract. `jazyk test` packages the criteria file and the
  requirement's context in-process and asks the configured model for a verdict. An
  external agent connected to [`jazyk mcp graph`](../frontends/mcp.md) does the same
  through the [verification tools](../compiler/tools.md#verification-tools), using its
  own abilities to inspect or exercise the deliverable. Whichever harness runs, the
  ledger row comes out the same shape.

### A test that could not run says nothing

A command that never executed has not judged the requirement, so the row reads
`unverified` with reason `runner-failed` and keeps the output as evidence. The run
clears any previous verdict: it moved the row's `lastRun` and learned nothing, so the
honest state is unknown, not yesterday's answer restated with today's timestamp. Recording it
as `failing` would blame the deliverable for a broken machine, and the two are
indistinguishable in a status table.

The harness cannot read a runner's mind, so it uses two signals that need no knowledge
of the tool:

- Exit `127` or `126`: the command was not found, or was not executable. That is the
  runner, never the requirement.
- Every executed row in the run failed, none passed, and their evidence is identical
  once the command line is stripped. One broken runner produces one message, repeated;
  N unmet requirements do not. The run reports the runner once and leaves every row
  unverified.

Anything else is a real verdict. A row that fails on its own assertion, beside rows
that pass, is exactly what verification is for.

`jazyk test --audit` rebuilds the ledger from the artifacts: it scans the deliverable
and the criteria directory for the test names derived from the live statements,
recreates rows the ledger lost, and refreshes the `test` and `files` hashes of rows
whose artifacts still carry their statement hash. Sites are not rebuilt: only
generation records them, so an audit-rebuilt row has none until the next `jazyk gen`.
The `requirement` hash is never rewritten from the live graph: an artifact carrying an
outdated statement hash stays `stale-requirement` until regeneration.

## Incremental regeneration

A rerun skips entities whose `factHash` is unchanged, so a docs edit regenerates only
the entities it touched. `--force` regenerates everything. A regeneration replaces the
entity's recorded file set: files the previous generation recorded that the new
manifest no longer lists are removed from the deliverable, so a renamed test file does
not leave its predecessor behind. Entity ids are stable
([identifiers](../compiler/model.md#identifiers)):

- A merged entity leaves a redirect ([mutations](../compiler/graph.md#mutations)); the
  generator follows it and folds the absorbed files into the survivor's.
- A renamed entity keeps its id, so its files migrate in place.
- A requirement deleted by GC leaves its row listed as `requirement-gone` in
  `verify_pending` until pruned; removals are never silent.

Before a run rewrites or removes a deliverable file, the previous content is
snapshotted to `<out>/deliverable-baseline/` under the file's relative path, once per
run per file. The snapshot is the diff baseline for frontends: the
[GUI](../frontends/gui.md#deliverable-viewer) shows what the last generation changed
against it. A file the run creates fresh has no baseline.

## Command

`jazyk gen [entity...]` runs the built-in generation worker. See
[CLI](../frontends/cli.md).

- With no arguments it generates every entity that has at least one requirement, in
  topological order over the relationship edges.
- `--force` ignores the fact-hash skip.
- `jazyk codegen` and `jazyk testgen` remain as deprecated aliases that print a pointer
  to `jazyk gen`.

`jazyk test [target...]` runs verification. With no arguments it processes every
runnable row; entity ids select their requirements' rows; requirement ids select rows
directly. `--kind` filters `programmatic` or `llm`; `--force` also reruns `verified`
rows; `--list` prints the derived status table without running anything. Exit 0 when
every targeted row is `verified`, 1 otherwise.

## Pluggable workers

Generation and verification are defined by the
[generation tools](../compiler/tools.md#generation-tools) and
[verification tools](../compiler/tools.md#verification-tools), not by the built-in
commands. `jazyk gen` and `jazyk test` are workers: they ask for the pending lists and
the task packages in-process, call the configured model, and mark results. An external
agent connected to [`jazyk mcp graph`](../frontends/mcp.md) is another worker with the
same contract: same instructions, same context, same change diffs, same ledger.

## Forced decisions

Generation sometimes must choose a value the documents never stated (a default, a limit,
a format). Each forced decision is recorded as a diagnostic on the entity and fed back to
the docs by [documentation generation](./docsgen.md), so the spec converges toward
stating what the product does.

## Coverage as a graph query

Coverage is a query over the graph, not over the deliverable:

- requirements with no test row in the ledger,
- entities with no behavior (no event-driven or state-driven requirement references
  them).

Both findings are ordinary [diagnostics](../compiler/model/diagnostic.md), so they land
in the same triage queue as everything else.
