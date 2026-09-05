# Generation

Generation makes the semantic graph into the end product and the tests that hold it to
its spec, in one workflow. It reads the graph through the
[loaded set](../compiler/context.md#the-loaded-set) and the
[read tools](../compiler/tools.md#read-tools), never the raw source files. The graph, not
the prose, is the spec.

The end product is called the deliverable. It is usually code, but the workflow does not
assume software: a book, a schematic, a course. Whatever the requirements describe,
generation produces it. Tests are the tie between the requirements and the deliverable,
and they exist before generation runs: [binding](./bind.md) ties each requirement to
its files and its test first, and generation is the step that makes `unimplemented`
bindings pass.

Generation is goal work on the board: a `generate` goal per entity whose facts differ
from the ledger, ready at the ledger tier after the entity's compile goals settle, resolved
when `record_generation` lands ([the generate goal](../compiler/goals/generate.md)).
Anything the deliverable needs that the documents do not state is an ambiguity; generation
chooses, records the choice, and raises it ([invented choices](#invented-choices)).

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
  back in ([docs settings](../compiler/project-settings.md#docs)). Generation metadata
  (the ledger, criteria files) stays in the out directory, never in the deliverable.

`deliverable` is the only setting that says where things go. The other two keys of
[`[gen]`](../compiler/project-settings.md#generation) say nothing about the product
either: `worker` picks the built-in worker's mode, `code` scopes the
[unclaimed report](./bind.md#the-unclaimed-report). What the deliverable is (a Rust
crate, a web app, a book, a schematic) is a fact the documents state, so it reaches the
generator through the graph and the loaded set like every other fact. The project file
never says what to build; there are no hints.

The generator chooses everything about the deliverable's form: the medium, the layout,
the file names, and the build files that make its recorded commands executable. What
binds the layout to the graph is the manifest: every resolved `generate` goal records
which deliverable files implement which requirements ([the ledger](#the-ledger)).

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
judgment each session repeats. A per-session decision is where the substitution above
creeps in: asked to write "the About slide", a generator writes prose about a slide,
because nothing in front of it said the deliverable is a file a tool must produce.

So the first session that needs the medium decides it first, in its own step, and records
it in [the ledger](#the-ledger). Usually that is the first generation run; on a
deliverable that is bound before anything is generated, the first
[bind goal](./bind.md#the-bind-goal) makes the same decision the same way, because a
test is written in the medium's toolchain:

```yaml
medium:
  form: Microsoft PowerPoint deck            # what the deliverable is, in the model's words
  produced: built                            # written | built
  toolchain: python3 with python-pptx        # what writes or builds it
  artifact: jazyk.pptx                       # deliverable-relative; only when built
```

- The input is the requirements that say what the deliverable is: the same graph every
  session reads, budgeted like any [loaded set](../compiler/context.md#the-loaded-set).
- When the deliverable directory already holds files (a planted fixture, an earlier
  run), the decision also reads the tree's file listing: an existing tree pins the
  language and toolchain, and deciding from the statements alone applies only to an
  empty deliverable.
- When the ledger already records run commands (tests bound before any entity
  generated), the decision reads them too: recorded commands pin the toolchain the
  same way an existing tree does.
- `produced: written` means the generated files are the deliverable. `produced: built`
  means they are the source that produces `artifact`, and [the build](#the-build) runs
  it.
- Every goal package carries the decision, and every goal's instructions state it as a
  fact rather than asking again. A session that generates under `produced: built` writes
  source, never the artifact itself and never prose about it.
- The decision is made once and reused, like the toolchain and the build. It is
  re-decided only when nothing is generated: a ledger with no entities decides again,
  so wiping the deliverable is how a project changes its mind. `--force` regenerates
  against the recorded decision.
- The decision is checked against what the sessions actually wrote. When the medium's
  `toolchain` and the ledger's programmatic run commands name different tool families
  (a Python toolchain beside `cargo test` rows), the medium diverged. The check names
  a family per known tool word (`cargo` and `rustc` are Rust, `python3` and `pytest`
  are Python, `npm` and `jest` are Node) and fires only when both sides name a family
  and share none; unknown words decide nothing. A divergence is a `mediumWarning`
  naming both sides in the `record_generation` and `record_binding` reply, in every
  goal package while it stands, and in [`jazyk status`](../frontends/cli.md#jazyk-status).
  While no entity has been generated yet, the record clears the medium instead, and
  the next session re-decides it with the recorded commands as evidence. Once an
  entity is generated the medium stands and the warning is the record.
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
- The build is per deliverable, not per entity. The first session that needs one records
  it; every later session receives it in its package and reuses it, the same way run
  commands establish one toolchain.
- What the build runs is a [support file](#file-ownership-and-conventions), never an
  entity's own file. One artifact is assembled from every entity's part, and an entry
  point owned by the entity that happened to generate first would freeze the artifact
  at that entity's part: no later session may write into it. So the entry belongs to the
  deliverable, its current content travels in every goal package, and each session
  returns it updated so the artifact includes its part too. The convention the entry
  uses to include a part (a function it calls, a list it reads) is the generator's,
  visible to every later session in the entry itself.
- A session that rewrites the entry sees what it is calling. Under a built medium the
  package carries the other entities' part files with their content, not just their
  paths and the statements they hold: an entry is a call site, and a call site needs
  the name of the thing it calls. Without it a session guesses, and the build dies on a
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
  said), and every goal package carries the failure while it stands, together with
  which of this entity's own files the failure names. Regenerating is how it gets
  fixed: the session that owns a file the failure names sees the message and writes
  source that runs. Without that, a generator repeats the same broken part every
  round, because nothing ever told it the artifact was never produced.
- A deliverable that is its own output records no build, and nothing runs.

The build is a fact the generator derives from the requirements, like every other
choice about form. Jazyk holds no list of media and no template per format.

## The entity is the unit of generation

Each [entity](../compiler/model/entity.md) is one `generate` goal. The goal's loaded set
is the entity in full: its `definition`, `stereotype`, `attributes`, its `parent` and
children, its requirements across all documents with their edges, facets, and
transitions, its derived relationships, and its state machine when one derives
([the loaded set](../compiler/context.md#the-loaded-set)). Nothing outside the loaded
set leaks in, so each goal is small, repeatable, and auditable.

The goal produces the entity's part of the deliverable. The tests already exist:
[binding](./bind.md) wrote one per requirement before the entity became generation
work, and the goal package carries them. The bound test defines the interface, and the
product conforms to it. A session that cannot make a bound test pass without changing it
reports that instead of rewriting the judge; the repair is a re-bind.

### Grouping by component

Where the graph carries containment structure, the session is wider than the goal: a
component and its subtree generate as one group, in one session, so the parts of one
component are written together under one set of conventions. The group is derived from
the graph at batch time, never stored:

- A system is a containment root with at least one child; with two or more it holds a
  level view of its own, `view:component/<system-slug>`
  ([level views](../compiler/model/view.md#level-views)). A group root is a direct
  child of a system: the «service»-like tier of the tree, whatever stereotype the
  documents gave it. A group is its root plus every descendant through `parent`.
- Every other entity generates alone: the system itself, for the requirements that name
  it directly, and a parentless entity without children.
- The `generate` goal stays per entity. The scheduler batches the ready goals of one
  group into one session under the session budget
  ([batching](../compiler/reconciler.md#batching)); the session's loaded set is the
  group. A group that does not fit one session splits in topological order over its
  members' relationships, and each later session receives the earlier parts as files
  with what they hold. Members whose facts match the ledger are not regenerated: their
  files ride in the package as context, the way other entities' parts do.
- Each member records its own manifest through `record_generation`, so ownership, fact
  hashes, and incremental regeneration stay per entity.

A flat graph (no `parent` anywhere) has no groups, and the entity is the unit
throughout. Nothing configures this; the containment the documents state, or an
`abstract-entity` goal introduced and the owner ratified, is what activates it.

## Order from relationships

[Relationships](../compiler/model/relationship.md) give structure and order. Each
contribution is directional (`a` acts on `b`), and the direction says what generates
first:

- `composition`, `aggregation` → ownership and nesting; the part before the whole.
- `association` → references; the referenced entity first.
- `dependency` → imports or injection; the dependency first.
- `realization` → an implementation of an «interface»; the interface first.
- `generalization` → inheritance or specialization; the general entity first.
- `instantiation` → fixtures and examples; the type first, the instance after.

Generation runs in topological order over the contributions: leaf entities (value
objects, interfaces) first, then the entities that compose, depend on, or realize them.
Between groups the order follows the lifted relationships between their members
([lifting](../compiler/diagrams.md#lifting-and-collapse)). A cycle breaks at its weakest
contribution, then by id. Each session can reference already generated files through the
manifest.

## File ownership and conventions

- Every deliverable file belongs to the entity whose session wrote it, recorded in
  [the ledger](#the-ledger). A session never overwrites another entity's files: the
  harness rejects a file path already recorded for a different entity and asks the
  worker for another path (one corrective retry, then the goal fails). Using another
  entity's files goes through references (imports, includes), never through rewriting
  them.
- The goal package names those files with the statements they carry, not just their
  paths. A composite deliverable is assembled from parts other sessions wrote, and a path
  alone says nothing about what is inside; the statements do, and they are what the
  graph already knows. So the entry per entity is its `files` and what each set
  `holds`, and the session composing them reads or imports those paths knowing what they
  contain.
- One toolchain per deliverable. The first session establishes it (the language, the test
  runner, the build files); every later session reuses it. The goal package carries the
  run commands already recorded in the ledger, so a worker sees the established
  conventions and never introduces a second test runner.
- A recorded run command must execute from the deliverable directory as recorded. When
  it needs a build or configuration file no session has written yet (a `package.json`, a
  `Cargo.toml`), the session returns that file as a support file. Recording a command
  that cannot run is a generation defect; verification surfaces it as a failing row.
- Support files belong to the deliverable, not to an entity. They are what makes the
  recorded commands runnable (a `package.json`, a `Cargo.toml`, the entry point a
  build runs), and every session may rewrite one: a manifest that lists more parts than
  the last session saw is exactly why the file exists. The ledger keeps them in their own
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
  The first block is the entity's part; the rest are files the session wrote too.
  Tolerating what the protocol forbade is how a `requirements.txt` header once
  swallowed five files into one.
- Which of those extra files belongs to the entity is decided by the manifest, not by
  the order they arrived in. A file the manifest lists in `supportFiles`, or names as
  the build's entry point, belongs to the deliverable; everything else the session wrote
  belongs to the entity that wrote it. Classifying by arrival would make a second test
  file deliverable-wide, unowned, and rewritable by any later session.
- A support file never lands on a file an entity owns, this goal's own product and
  tests included. Support files exist so any session may rewrite them; letting one take
  an owned path would let a manifest step quietly overwrite the module the product
  step just wrote.
- A reply in the wrong shape gets one corrective round before the goal fails: a
  product or tests reply whose `FILE:` line never appears, a manifest that is not
  valid JSON. The complaint is quoted back with the same request, and the correction
  shows the shape rather than describing it. A reply that opens with a sentence or a
  fence and then gives its `FILE:` line is not a shape failure: the preamble is dropped
  and the file starts at the line. Shape is the harness's contract, and a weak model
  drops it under a long prompt well before it gets the content wrong; failing the goal
  over a missing brace throws away work that was otherwise fine.
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
- Recording enforces the shape for every worker: `record_generation` rejects a
  programmatic row whose `artifact` or `run` is empty, naming the row and the missing
  field. An empty artifact resolves to the deliverable directory itself and fails at
  run time with a path that explains nothing; the rejection at record time says what
  to fix.

## Dense entities generate in parts

A stringent component legitimately carries 50 or more requirements, and one generation
call has an output ceiling. The generation divides:

- The first part generates the types, state, and the first group of requirements.
- Each further part receives what was generated so far and the next group of
  requirements, and returns only additional content to append.
- Parts concatenate; traceability markers per requirement are unaffected.

The group size is 20 requirements per part (the `{GROUP}` placeholder of the generation
contract). The `requirements-per-entity` limit (soft 50, hard 80) opens an
`abstract-entity` goal on an entity that grows past it
([the limits registry](../compiler/graph.md#limits),
[the abstract-entity goal](../compiler/goals/abstract-entity.md)), so splitting the
entity into a containment subtree, and proposing that structure to the documents, stays
a choice made in the graph, not an emergency at generation time.

## Tests tie requirements to the deliverable

Each [requirement](../compiler/model/requirement.md) derives a test, keyed by the
requirement id. The test is written when the requirement is [bound](./bind.md), before
generation runs. A failing test names the requirement it verifies, and a changed
requirement invalidates exactly the tests keyed to it.

The requirement's [facets](../compiler/model/requirement.md#facets) and
[transition](../compiler/model/requirement.md#transition) suggest the test shape:

- `behavior` with a trigger → a scenario: arrange, trigger the event, assert the
  response.
- `constraint` → a property or invariant check.
- `failure-mode` → a negative check: provoke the condition, assert the stated handling.
- a `transition` → a stateful check: enter `from`, fire `trigger` under `guard`, assert
  `to`.
- `quality` with a `measure` → a measured check against the stated bound.

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

Every quote-provenanced requirement carries a verbatim `quote`
([shared fields](../compiler/model.md#shared-fields)); a derived or decreed one carries
its upstream nodes or its author, and a ratification proposal toward a quote
([provenance](../compiler/model.md#provenance)). The trail from deliverable to prose has
two carriers:

- The test name embeds the requirement id and the first 8 hex characters of the hash of
  its `statement`: `req_catalog_3_a1b2c3d4`. The name is part of the artifact itself and
  of the recorded run command, so a reworded requirement mechanically breaks the
  recorded command: even a harness that has never heard of Jazyk fails to find the
  stale test.
- Anchored sites in [the ledger](#the-ledger). While writing, a worker puts a
  single-line marker comment directly above each implementing site: `req:catalog-3
  hash:a1b2c3d4` in the medium's comment syntax, nothing else on the line. The marker
  is a wire format, not part of the product: `record_generation` strips every marker
  line from the written files and records each as a site on the requirement's row: the
  file, the line, and `head`, the verbatim next significant line. A doubled prefix
  (`req:req:catalog-3`) normalizes to one on strip; a marker-like line the strip
  cannot parse (trailing text after the hash, a mangled id) stays in the file, anchors
  nothing, and the record reply names it under `markerWarnings`. The deliverable
  carries no Jazyk metadata; the binding lives in the out directory.

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
- `requirements`: the [bindings](./bind.md). How each requirement ties to the
  deliverable and how it is verified. Rows are born by `record_binding` and updated by
  `record_generation` and test runs.

Four more keys sit beside them: `support`, the deliverable-wide files any session may
rewrite; `medium`, the deliverable's
[decided form](#the-medium-is-decided-once-before-anything-is-generated), written by
the first run; `build`, present only when that medium must be produced by a tool
([the build](#the-build)); and `contradicted`, the rows recorded over an open error
diagnostic ([rows recorded over an open contradiction](#rows-recorded-over-an-open-contradiction)).

```yaml
support:                                  # deliverable-wide files any session may rewrite
  - build_deck.py                         # the build's entry point
  - requirements.txt

medium:                                   # decided once, carried by every goal package
  form: Microsoft PowerPoint deck
  produced: built                         # written | built
  toolchain: python3 with python-pptx
  artifact: jazyk.pptx                    # deliverable-relative; only when built

build:                                    # optional; absent when the files are the output
  run: python build_deck.py               # runs once, before any row is judged
  cwd: .                                  # deliverable-relative working dir
  produces:                               # deliverable-relative artifact paths
    - jazyk.pptx

contradicted:                             # rows recorded over an open error diagnostic
  req:catalog-3:                          # the diagnostic ids open at record time
    - diag:contradiction-1

entities:
  catalog:
    factHash: 9f2ab4c1d0e77a3b            # hash of name, definition, stereotype, attributes,
                                          # and every referencing statement with its edges
    requirements: [req:catalog-1, req:catalog-2, req:catalog-3]
    files:                                # deliverable-relative files this entity's
      - src/catalog.rs                    # generation produced or touched
      - tests/catalog.rs
    unattached:                           # the unattached remainder, measured at record time
      files: 0                            # owned files no requirement row names
      lines: 14                           # significant lines outside every site's run
      ratio: 0.11                         # unattached lines over significant lines

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
      requirement: <full statement hash>  # written at generation and binding, never by a run
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

1. No row → `missing` (reason `not-generated`). The test artifact is gone from disk,
   or a programmatic artifact no longer contains the declared test `name` → `missing`
   (reason `artifact-gone`): nothing judges the requirement any more, and the repair
   is a [re-bind](./bind.md#when-binding-runs). A row whose requirement id is absent
   from the graph is also `missing` (reason `requirement-gone`), but it is never
   actionable work: see [deletion](#deletion-prunes-the-ledger).
2. The live `statement` hash differs from `hashes.requirement` → `stale-requirement`.
   The test verifies a sentence the graph does not hold. The repair is a re-bind, then
   generation if the re-bind reads `unimplemented` ([the cascade](#the-cascade));
   `jazyk test` refuses to run the row and says so.
3. The test artifact bytes differ from `hashes.test` → `stale-test`. Rerun.
4. The manifest files hash differs from `hashes.files` → `stale-code`. Rerun.
5. Otherwise the last verdict: `pass` → `verified`; `fail` with an empty `files` list
   → `unimplemented` (the requirement is [bound](./bind.md) but nothing implements it:
   generation work, and the bound test is its acceptance gate); `fail` with
   implementing files → `failing` (the deliverable contradicts the statement: a
   diagnostic, never automatic regeneration); `none` → `unverified`. A run whose
   command never executed leaves the verdict at `none` and the reason at
   `runner-failed`, so a broken machine reads as unverified, not as a failing
   deliverable (see [runners](#runners)).

Hashes are written at exactly four moments: generation resolves a goal (all three),
binding records a row (`record_binding` writes all three), a test run completes
(`test` and `files` rebaseline, never `requirement`), and
[`jazyk test --audit`](#runners) rebaselines `test` and `files` against the artifacts
on disk. Every staleness flip is a deterministic hash comparison. The model owns three
judgments only: the test kind, the test itself, and the verdict of an `llm` run.

Every row a status surface lists carries its `reason` beside the status and a
`repair`: one sentence naming the command or goal that clears it (`jazyk gen <entity>`
for a re-bind and regeneration, `jazyk test <requirement>` for a rerun, nothing for a
`failing` row, which is a finding). `jazyk test --list` prints both.

Each status that says action is a goal on the board, derived from a `ledger-stale`
change record: a `bind` goal for `missing`, `stale-requirement`, and a gone artifact
([when binding runs](./bind.md#when-binding-runs)), a `generate` goal for the entity of
an `unimplemented` row or an entity whose facts moved, a `verify` goal for `stale-test`,
`stale-code`, and `unverified` ([the verify goal](../compiler/goals/verify.md)). A
`failing` row is a diagnostic, never a goal: `verification_tasks` and the board leave
it alone until its test or files change. `jazyk test` and `run_tests` with no target
rerun it anyway, because a rerun is what a person asking for one wants.

### The cascade

Rewording a requirement flips its row to `stale-requirement`. The repair order is
[bind](./bind.md) first, then generate: the re-bind rewrites the test against the new
statement and reruns it, and only an `unimplemented` outcome makes the entity
generation work. Generation then rewrites the implementing files until the bound test
passes. Hand edits to the deliverable flip exactly the rows whose `files` hash moved
to `stale-code`. Reruns update verdicts; when the test passes, the requirement is
`verified`. Each arrow is a goal the board derives with its cause on record
([edit paths](../compiler/compilation.md#edit-paths)); nothing in this loop is
remembered by a human, and `jazyk ripple` replays it
([CLI](../frontends/cli.md#jazyk-ripple)).

### Deletion prunes the ledger

Deleting a requirement ends its obligation, and its ledger row must not outlive it:

- `record_generation` prunes every row whose requirement id is absent from the graph,
  whatever entity the call records. The manifest never needs to name a dead
  requirement to bury it; absence from the graph is the signal.
- Until a record runs, such a row reads `missing` with reason `requirement-gone`, and
  no `verify` goal derives from it: it is not work, and no repair applies to it.
  `run_tests` skips it the same way. `jazyk test --audit` prunes it too.

Without pruning, a compilation that deletes a requirement leaves a row no tool can
remove: the manifest only adds and updates, reruns skip the row, and the board would
keep deriving a repair (regenerate) that provably does not clear it.

### Rows recorded over an open contradiction

A requirement that is a subject of an open `error`
[diagnostic](../compiler/model/diagnostic.md) (a `contradiction`, an unresolved
ambiguity) states something the graph itself disputes, and code written against it is
code written against one side of an open question. Generation and binding do not wait
for the answer (that is the [`answer` goal](../compiler/goals/answer.md)'s seam), but
they never run green over it silently:

- Every `generate` and `bind` package names the open error diagnostics on each of its
  requirements (`openDiagnostics`: the id, the rule, the message; suppressed ones
  excluded), so the session knows which statements are disputed before it writes a
  line.
- `record_generation` and `record_binding` write the rows that landed over an open
  error diagnostic into the ledger's `contradicted` map (requirement id to the
  diagnostic ids open at record time) and name them in the reply. A later record of
  the same row under a clean graph clears its entry.
- The flag is a record, never a verdict. A `verified` row that is `contradicted` is
  verified against one side of a dispute. Resolving the diagnostic (an answer, a docs
  edit) reworks the statement, [re-binds](./bind.md#when-binding-runs) the row, and
  the next record clears the flag.

## Criteria files for llm tests

For `kind: llm` rows, generation writes a criteria file: front matter with the
requirement id and the full statement hash; body with the `statement`, the verbatim
quote, the manifest file paths, the steps to confirm, and the verdict contract (`PASS`
or `FAIL` plus reasoning). It is the packaged setup for any harness: context, the
location of the implemented product, and what to confirm. Editing it flips
`stale-test` like any test artifact.

The built-in worker writes it to `gen/criteria/req-<slug>.md` in the out directory
(metadata, not deliverable). An external worker has no reason to know the out
directory and writes it where it writes everything else, under the deliverable. The
recorded artifact path resolves against both homes, the out directory first, so
neither reads as a gone artifact. Recording an identical manifest keeps existing
verdicts; only changed hashes reset a row.

## Runners

`jazyk test` runs [the build](#the-build) first, once, when the ledger records one. A
non-zero exit, or a path in `produces` that the command did not create, stops the run
before any row is judged and reports the build's own output. A row cannot say anything
true about an artifact that was never produced, so reporting failures per requirement
would name the wrong culprit.

- `programmatic`: `jazyk test` executes `run` in `cwd` under the deliverable. Exit 0 is
  a pass, anything else is a fail. Before running, the runner greps the artifact for the
  test `name`; if absent the row is `missing` (reason `artifact-gone`), not `failing`,
  nothing executes, and the row is bind work. The row records the exit code beside the
  output, so a verdict can be read back without rerunning it.
- `llm`: two harnesses, one contract. `jazyk test` packages the criteria file and the
  requirement's loaded set in-process and asks the configured model for a verdict
  ([the verify goal](../compiler/goals/verify.md)). An external agent connected to
  [`jazyk mcp graph`](../frontends/mcp.md) does the same through the
  [verification tools](../compiler/tools.md#verification-tools), using its own
  abilities to inspect or exercise the deliverable. Whichever harness runs, the ledger
  row comes out the same shape.

### A test that could not run says nothing

A command that never executed has not judged the requirement, so the row reads
`unverified` with reason `runner-failed` and keeps the output as evidence. The run
clears any previous verdict: it moved the row's `lastRun` and learned nothing, so the
honest state is unknown, not yesterday's answer restated with today's timestamp.
Recording it as `failing` would blame the deliverable for a broken machine, and the two
are indistinguishable in a status table.

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

`jazyk test --audit` rebuilds the ledger from the artifacts, without running anything:

- Existing rows whose artifact still carries the test name derived from the live
  statement (for an `llm` row, the criteria file's front matter with the requirement
  id and the full statement hash) refresh their `test` and `files` hashes. A refresh
  that moves either hash drops the verdict to `none`: the old verdict judged other
  bytes, and an audit never turns a hand edit into a `verified` row nothing reran. The
  row reads `unverified` and reruns.
- Rows whose requirement left the graph are pruned, the same way `record_generation`
  prunes them.
- Lost `llm` rows are recreated from their criteria files: the file carries everything
  the row needs. A lost programmatic test the scan finds is reported under `found`,
  never recreated: only the [bind goal](./bind.md#the-bind-goal) can record the command
  that runs it, so the requirement stays `missing` and binds, and the bind session
  finds the existing test instead of writing a second one.
- Sites are not rebuilt: only generation records them.
- The `requirement` hash is never rewritten from the live graph: an artifact carrying
  an outdated statement hash stays `stale-requirement` until it is re-bound.

## Incremental regeneration

A rerun skips entities whose `factHash` is unchanged and whose recorded files still
exist, so a docs edit regenerates only the entities it touched; a group session
regenerates only the members whose facts moved
([grouping by component](#grouping-by-component)). An entity with no ledger entry is
generation work only through an `unimplemented` row
([the generate goal](../compiler/goals/generate.md#created-when)), so adopted code whose
rows all read `verified` is never regenerated over. `jazyk gen` names the reason for
every entity it skips: unchanged, a bind still owed, or no row that says generate.
`--force` regenerates everything. A regeneration overwrites the entity's recorded file
set: files the previous generation recorded that the new manifest omits are removed
from the deliverable (snapshotted first, see below), so a test file under a new name
does not leave its predecessor behind. `record_generation` does the removal, whichever
worker records, and names the removed files in its reply; a file another entity also
records, a support file, or another row's test artifact is never removed. Entity ids
are stable ([identifiers](../compiler/model.md#identifiers)):

- A merged entity leaves a redirect ([mutations](../compiler/graph.md#mutations)); the
  generator follows it and folds the absorbed files into the survivor's.
- An entity whose name changes keeps its id, so its files migrate in place.
- An entity that gains a `parent` (containment the documents state, or an
  `abstract-entity` split the owner ratified) keeps its id and its files; only its group
  changes, and the next session of that group sees the files as parts.
- A deleted requirement's row is [pruned at the next record](#deletion-prunes-the-ledger);
  the journal holds the deletion, so removals are never silent.

Before a run rewrites or removes a deliverable file, the previous content is
snapshotted to `<out>/deliverable-baseline/` under the file's relative path, once per
run per file. The snapshot is the diff baseline for frontends: the
[GUI](../frontends/gui.md#deliverable-viewer) shows what the last generation changed
against it. A file the run creates fresh has no baseline.

## Command

`jazyk gen [entity...]` runs the built-in generation worker. See
[CLI](../frontends/cli.md#jazyk-gen).

- With no arguments it works every `generate` goal on the board, in topological order
  over the relationship contributions, group by group. Binding runs first: a `bind`
  goal of a targeted requirement resolves before its entity's `generate` goal.
- Named entities restrict the run to their goals.
- `--force` ignores the fact-hash skip.
- In `manual` mode the command records the generate release
  ([modes and releases](../compiler/control-plane.md#modes-and-releases)).
- `jazyk codegen` and `jazyk testgen` are aliases that print a pointer to `jazyk gen`.

`jazyk test [target...]` runs verification ([CLI](../frontends/cli.md#jazyk-test)). With
no arguments it processes every runnable row; entity ids select their requirements'
rows; requirement ids select rows directly. `--kind` filters `programmatic` or `llm`;
`--force` also reruns `verified` rows; `--list` prints the derived status table without
running anything. Exit 0 when every targeted row is `verified`, 1 otherwise.

## Pluggable workers

Generation and verification are defined by the
[generation tools](../compiler/tools.md#generation-tools) and
[verification tools](../compiler/tools.md#verification-tools), not by the built-in
commands. `jazyk gen` and `jazyk test` are workers: they consume the same goal
packages in-process, drive the configured model, and mark results. An external agent
connected to [`jazyk mcp generate`](../frontends/mcp.md#toolsets) is another worker
with the same contract: same instructions, same loaded set, same change diffs, same
ledger ([generation over MCP](../frontends/mcp.md#generation-and-verification-over-mcp)).
The executor per goal kind is a setting
([executors](../compiler/project-settings.md#executors)).

The two workers hold the same power. The external agent edits files and runs commands
with its own tools; the built-in worker runs each group as a session
([the generate goal](../compiler/goals/generate.md#tools)) whose toolset carries file
and command tools sandboxed to the deliverable, so it too writes multiple files, runs
the build it wrote, reads failures, and repairs its own work before recording. A goal
resolves when the ledger says so: the harness checks that `record_generation` landed
for the entity, not the model's word. What separates the workers is model quality,
never capability.

## Invented choices

Anything the deliverable needs that the documents do not state is an ambiguity
([edit paths](../compiler/compilation.md#edit-paths)). Generation does not stall on one:
it chooses with best judgment, records the choice, and raises it. The manifest a session
records through `record_generation` carries every choice it had to invent
(`choices: [{choice, scope, reasoning, requirements?}]`: the choice in one sentence,
its `scope` (`product`, `behavior`, or `detail`), the model's reasoning, and the
requirements it fills in when any exist), and the harness files one `invented-choice`
[diagnostic](../compiler/model/diagnostic.md) per entry, its subjects the entity and
those requirements, its `reasoning` the model's. Each choice is its own finding: the
sticky identity of `invented-choice` includes the choice sentence, so two choices over
the same subjects never merge. The severity follows the scope of the
invention:

- `error`: the invention decides what the deliverable or an entity is. A medium no
  statement names, an entity whose requirements do not say what it does, a whole feature
  the documents only allude to. "Build me a Facebook" is an error: the invention is the
  product.
- `warning`: the invention decides observable behavior a statement leaves open. An
  unspecified out-of-memory behavior, a default limit, an error response nobody stated.
- `info`: the invention has no behavioral consequence. A background color, a file name,
  an internal identifier. A human may suppress it in triage, and a suppressed one stays
  suppressed across regenerations.

Each diagnostic carries a `prompt` in the shape of a
[ratification proposal](./docsgen.md#ratification-proposals): the question, an `edit`
option with the sentence that would make the documents state the choice (targeting the
requirement's source section when the choice fills in a requirement, otherwise the
section the proposal's target rule picks for the entity), and an `answer` option to keep
the choice unstated. An unanswered prompt is a `prompt-unanswered` change, so a blocked
[`answer` goal](../compiler/goals/answer.md) rides in the verdict's `blocked` count
until a human decides; the deliverable never waits for it. Accepting the edit writes the
prose; the next build extracts the statement with quote provenance, the requirement
binds, and the choice stops being invented. Keeping it unstated records the answer and
resolves the diagnostic. Re-recording an entity overwrites its invented set: a choice the
new manifest omits resolves, so a regeneration under better documents clears its own
debt, and a choice it repeats keeps its diagnostic, so triage and answers survive
regeneration.

### The unattached remainder

The deliverable itself measures how much was invented. Generated mass attached to no
requirement is exactly the invented detail, and the ledger already computes attachment
([traceability](#traceability)). At record time the harness measures each entity's
unattached remainder over the files it owns (support files excluded):

- `files`: owned files no requirement row names in `files`, the per-entity slice of the
  [unclaimed report](./bind.md#the-unclaimed-report).
- `lines`: significant lines (non-blank, non-comment in the medium's comment syntax)
  that no site's run covers. A site's run starts at its `head` line and ends before the
  next site in the same file, or at the end of the file; the lines before the first site
  are unattached.
- `ratio`: `lines` over the entity's significant lines.

The measure lands on the entity's ledger entry (`unattached`), and the message of every
`invented-choice` diagnostic on the entity names the ratio, so the grade and the measure
read together. Three words of documentation show up as an enormous remainder; documents
written near pseudo-code leave almost none. No threshold fires on the remainder by
itself: it is evidence beside the graded diagnostics, shown as a line in
[`jazyk status`](../frontends/cli.md#jazyk-status) and returned in the
`record_generation` reply.

## Coverage as a graph query

Coverage is a query over the graph, not over the deliverable:

- requirements with no [binding](./bind.md): each is a `bind` goal on the board, counted
  in the verdict, never a silent gap,
- entities with no behavior: no requirement with a `behavior` facet or a `transition`
  names them. A purely structural entity is not a defect, so this is a report line in
  [`jazyk status`](../frontends/cli.md#jazyk-status) and the whole-build report
  ([`jazyk ripple`](../frontends/cli.md#jazyk-ripple)), not a diagnostic; an entity
  with no requirement at all is the `unused-entity` check's finding
  ([checks](../compiler/compilation.md#checks)).

The inverse query, deliverable files no binding names, is the
[unclaimed report](./bind.md#the-unclaimed-report), and its per-entity slice is the
`files` part of the [unattached remainder](#the-unattached-remainder).
