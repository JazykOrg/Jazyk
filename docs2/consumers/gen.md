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

The deliverable lives outside the out directory, in a directory named by the project:

```toml
[gen]
deliverable = "../project2"
lang = "rust"
```

- `deliverable` resolves relative to the project root. Everything generation produces
  lands under it.
- `lang` is a freeform hint passed to generation tasks (a language, a format, a genre).
  `--lang` overrides it per run.

The generator chooses the layout of the deliverable: a source tree with `src/` and
`tests/`, chapters of a book, sheets of a schematic. Task packages suggest a default
layout; the generator may override it. What binds the layout to the graph is the
manifest: every completed task records which deliverable files implement which
requirements ([the ledger](#the-ledger)).

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
  code is the verdict.
- `llm`: a test that requires judgment, or a deliverable that is not executable
  software. No programmatic definition exists; the harness gives an agent the
  requirement, its context, and the location of the implementing files, and asks it to
  confirm the behavior. The verdict is the test. See
  [criteria files](#criteria-files-for-llm-tests).

## Traceability

Every requirement carries a verbatim `quote`
([shared fields](../compiler/model.md#shared-fields)). Generated artifacts embed the
trail twice:

- The test name embeds the requirement id and the first 8 hex characters of the hash of
  its statement: `req_catalog_3_a1b2c3d4`. For tagged formats the tag carries the same:
  `@req-catalog-3 @h-a1b2c3d4`.
- A marker comment sits above the test with the id, the full-precision prefix, and the
  quote: `// req:catalog-3 hash:a1b2c3d4` followed by the verbatim source sentence.

The trail is test → requirement id → `quote` → section. Because the hash is baked into
the test name, a reworded requirement mechanically breaks the recorded run command: even
a harness that has never heard of Jazyk fails to find the stale test. Implementing files
carry the same marker at each implementing site, which is how the manifest stays
auditable.

## The ledger

`gen/ledger.yaml` in the out directory is the single generation and verification
metadata file. Two maps:

- `entities`: generation state. What was generated for each entity, against which facts.
  Drives incremental regeneration.
- `requirements`: verification state. How each requirement ties to the deliverable and
  how it is verified.

```yaml
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
   `none` → `unverified`.

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

- `programmatic`: `jazyk test` executes `run` in `cwd` under the deliverable. Exit 0 is
  a pass, anything else is a fail. Before running, the runner greps the artifact for the
  test `name`; if absent the row is `stale-test`, not `failing`, and nothing executes.
- `llm`: two harnesses, one contract. `jazyk test` packages the criteria file and the
  requirement's context in-process and asks the configured model for a verdict. An
  external agent connected to [`jazyk mcp graph`](../frontends/mcp.md) does the same
  through the [verification tools](../compiler/tools.md#verification-tools), using its
  own abilities to inspect or exercise the deliverable. Whichever harness runs, the
  ledger row comes out the same shape.

`jazyk test --audit` rebuilds the ledger from the artifact markers: it scans the
deliverable and the criteria directory for marker comments and test names, recreates
rows the ledger lost, and refreshes the `test` and `files` hashes of rows whose
artifacts still carry their statement hash. The `requirement` hash is never rewritten
from the live graph: an artifact carrying an outdated statement hash stays
`stale-requirement` until regeneration.

## Incremental regeneration

A rerun skips entities whose `factHash` is unchanged, so a docs edit regenerates only
the entities it touched. `--force` regenerates everything. Entity ids are stable
([identifiers](../compiler/model.md#identifiers)):

- A merged entity leaves a redirect ([mutations](../compiler/graph.md#mutations)); the
  generator follows it and folds the absorbed files into the survivor's.
- A renamed entity keeps its id, so its files migrate in place.
- A requirement deleted by GC leaves its row listed as `requirement-gone` in
  `verify_pending` until pruned; removals are never silent.

## Command

`jazyk gen [entity...]` runs the built-in generation worker. See
[CLI](../frontends/cli.md).

- With no arguments it generates every entity that has at least one requirement, in
  topological order over the relationship edges.
- `--lang` overrides the project's `lang` hint. `--force` ignores the fact-hash skip.
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
