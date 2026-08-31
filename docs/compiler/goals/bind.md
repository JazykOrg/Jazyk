# The bind goal

`bind` ties one requirement to the deliverable: the files that carry it (possibly
none), the test that judges it, and the first verdict. The session observes and
judges; it never changes implementation files. What it records is the requirement's
row in the [ledger](../../consumers/gen.md#the-ledger), and the row's
[derived status](../../consumers/gen.md#status-is-derived-never-stored) classifies
the requirement: `verified` (the deliverable already behaves as stated),
`unimplemented` (the failing test is the acceptance gate [`generate`](./generate.md)
must clear), or `failing` (the deliverable contradicts the statement, a finding for
the author). See [binding](../../consumers/bind.md).

- Class: compile. Mandatory. Readiness tier 3.
- Unit: one requirement. Goal id `g:bind:req:<doc-stem>-<n>`.
- Skill: none. The contract rides the binding package (below).

## Created when

The goal derives from a `ledger-stale` [change record](../graph.md#change-records)
whose `detail.goal` is `bind`. The record is written whenever the ledger and the graph
disagree about a requirement, with the reason in `detail.reason`:

- `unbound`: the requirement has no row. It was just extracted, or the whole ledger is
  new (an [adopted](../../consumers/bind.md#adoption) project).
- `requirement-changed`: the row's `hashes.requirement` differs from the live
  statement hash. The recorded test judges a sentence the documents have dropped.
- `artifact-gone`: the row's test artifact is missing from disk.

E.g.:

```yaml
- id: c413-2
  generation: 413
  mutation: 2
  kind: ledger-stale
  subject: req:orders-6
  via: ledger
  detail: {goal: bind, reason: requirement-changed, entity: ent:order}
```

`cause` is the commit that moved the statement (a `reconcile-section` session's
`update_requirement`) or, for a test file deleted by hand, the `edit` journal entry
the build writes when it notices the deliverable changed
([edit paths](../compilation.md#edit-paths)). The row comparison is recomputed on
every derivation, so the goal exists exactly while the row is absent or stale and
disappears the moment a current row lands. A requirement that left the graph derives
nothing: its row is [pruned](../../consumers/gen.md#deletion-prunes-the-ledger),
never bound.

## Readiness

- Tier 3: ready when no tier 0, 1, or 2 goal is open or parked anywhere on the board
  ([readiness](../reconciler.md#readiness)). Tiers are global, not cone-scoped: a
  cone gates GC goals only. The statement must be final before a test encodes it, so
  every `reconcile-section` and `rejudge-pair` goal runs first.
- The session writes test files into the deliverable, so in `manual` mode the goal is
  `blocked {on: release}` until the generate
  [release](../control-plane.md#modes-and-releases) lands. A blocked goal counts in
  the verdict and renders on every status surface.
- Batches group by locality ([batching](../reconciler.md#batching)): the requirements
  of one entity, and under [containment](../model/entity.md#containment) the entities
  of one component subtree, so one session searches one part of the deliverable.
- The executor resolves through [`[executors]`](../project-settings.md#executors).
  Searching a codebase is what a coding agent's own tools do best; the
  [embedded agent](../../frontends/acp.md#the-embedded-agent) gets file tools served.

## Gate

A ledger row recorded for the requirement by `record_binding` and current: the row's
`hashes.requirement` equals the live statement hash, and the test artifact exists and
contains the declared test name (for an `llm` row, the criteria file exists). The
harness checks the ledger, not the model's word:

- `mark_goal_done({goal, justification})` is validated against the row. A claim with
  no row, or a stale one, is rejected naming the gate.
- A session that recorded the row and ended without marking the goal still resolves
  it: the next derivation finds the row current, and the journal records the
  `record_binding` as the resolution.
- A session that ends without a row has not resolved the goal. It retries once with a
  fresh session, then parks
  ([resolving, failing, parking](../sessions.md#resolving-failing-parking)).
- `mark_goal_failed({goal, reason})` when the deliverable cannot be searched or no
  test can be written in the medium's toolchain. A failed mandatory goal blocks
  convergence and surfaces on the requirement.

The verdict is not part of the gate. A failing test on an unimplemented requirement
is the intended outcome, and a failing test on implemented files is a finding, never
a reason to withhold the row.

## Hints

Computed by the harness and rendered under the goal block:

- The reason (`unbound`, `requirement-changed`, `artifact-gone`) and, for a changed
  requirement, the test name the previous row recorded.
- `load ent:<owner>` when the owning entity is not in the loaded set.
- The entity's recorded files (`entityFiles`): start the search there.
- The test conventions already recorded (runner, command style) and the suggested
  test name, the requirement id plus the first 8 hex characters of the statement
  hash ([traceability](../../consumers/gen.md#traceability)).
- Whether the medium is decided. When it is not, this session decides it
  ([the medium](../../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)),
  because a test is written in the medium's toolchain.
- The call that resolves the kind: `begin_binding` with this requirement, then
  `record_binding`.

## What the model sees

The session prompt is [assembled](../sessions.md#the-prompt) like every session's:
the agent contract, the project block, the goals block, the loaded set. The goal
block for a `bind` goal carries the contract paragraph from
[`prompts/bind.md`](./prompts/bind.md) (claim the requirement with `begin_binding`,
follow the package, record the row, never touch implementation files), the change in
one line, the gate in one line, and the hints. The last hint names the resolving
calls: `begin_binding`, then `record_binding`. The loaded set holds the requirement
in full and its entity as a stub.

The binding contract rides the tool reply, not the prompt, because the model must
interleave jazyk's package with its own file and shell work
([generation sessions](../sessions.md#generation-sessions)):

1. `begin_binding({requirement})` answers with
   [`prompts/bind-contract.md`](./prompts/bind-contract.md) as `instructions`
   (search before write, both directions get a test, the two test kinds,
   falsifiability, the naming scheme) and the package:

```text
requirement, entity, reason        unbound | requirement-changed | artifact-gone
statement, quote                   the statement and its verbatim source
suggestedTestName                  req id plus statement hash prefix
medium                             already decided; never re-decided
build                              the recorded build, when one exists
testConventions                    recorded rows to imitate (runner, command style)
entityFiles                        the entity's recorded files: start the search here
context                            the requirement's neighborhood (../context.md#rendering)
```

2. The model searches the deliverable, binds to an existing test or writes the missing
   one, runs it, and records with `record_binding`. The statement's shape suggests the
   test's shape (an event → a scenario, an invariant → a property check, an unwanted
   behavior → a negative check, a state condition → a stateful check), and the test
   must be [falsifiable](../../consumers/gen.md#tests-tie-requirements-to-the-deliverable):
   when no falsifiable programmatic assertion exists, the kind is `llm` and the
   artifact is a [criteria file](../../consumers/gen.md#criteria-files-for-llm-tests).

The packaged form (the contract plus the package rendered as text) is what the
[benchmark](../../benchmark/benchmark.md) grades. [`jazyk gen`](../../consumers/gen.md#command)
runs the same goal outside a full build for the named entities' requirements, and an
external agent on the `generate` serving claims the same package with `binding_tasks`
and `begin_binding`
([generation over MCP](../../frontends/mcp.md#generation-and-verification-over-mcp)).
`jazyk preview <goal>` shows the assembled prompt before it is spent.

## Tools

The session's serving runs in `generate` mode ([toolsets](../tools.md#toolsets)):

- The [read tools](../tools.md#read-tools): the loaded-set tools `load`, `expand`,
  `unload`, `graph_status`, and the lookups `search`, `read_section`, `get_entity`,
  `get_view`, `diagnostics` ([context](../context.md#tools)).
- The [goal tools](../tools.md#goal-tools): `mark_goal_done`, `mark_goal_failed`,
  `load_skill`, `done`.
- The [binding tools](../tools.md#binding-tools): `binding_tasks`, `begin_binding`,
  `record_binding({requirement, files, test, verdict, evidence?})`.
- The [generation tools](../tools.md#generation-tools) `generation_tasks`,
  `begin_generation`, `record_generation`, and `run_tests`: binding and
  generation share the serving because they share the worker persona.
- [`report_feedback`](../tools.md#feedback-tool).
- The [file and command tools](./generate.md#file-and-command-tools) when the agent's
  profile sets [`serve_files`](../project-settings.md#acp); a coding agent brings its
  own.

Test and criteria files are the only files a bind session writes. A `write_text_file`
to a path the ledger records as an entity's implementing file is rejected with the
owner named: binding observes, generation changes.

`record_binding` is the finish line: the files that carry the statement (an empty
list is a finding, not a failure), the test row `{kind, label, artifact, name, run,
cwd}`, the verdict, the evidence. It rejects a test whose artifact does not exist or
does not contain the declared name, the same shape gate `record_generation` applies.
Its reply previews what the row opens ([bubbling](../reconciler.md#bubbling)): an
`unimplemented` row opens `generate` on the owning entity. The session then marks the
goal done with a one-line justification and ends with `done`.
