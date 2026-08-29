# The generate goal

`generate` produces one entity's part of the deliverable, makes the tests bound to
its requirements pass, and records the manifest. The medium is decided once per
deliverable before any generation runs (below); the session implements, it never
re-decides. See [generation](../../consumers/gen.md).

- Class: compile. Mandatory. Readiness tier 3.
- Unit: one entity. Goal id `g:generate:ent:<slug>`.
- Skill: none. The contract rides the generation package (below).

## Created when

The goal derives from a `ledger-stale` [change record](../graph.md#change-records)
whose `detail.goal` is `generate`, written whenever the entity's facts differ from
the [ledger](../../consumers/gen.md#the-ledger):

- `facts-changed`: the entity has a ledger entry whose `factHash` (name, definition,
  every statement referencing it) differs from the live graph. `detail.changed` lists
  the requirement ids added, removed, or reworded since the last generation.
- `unimplemented`: a requirement of the entity has a bound row whose derived status
  is `unimplemented` (the test fails and nothing implements it). The bound test is
  the acceptance gate; generation is the work that clears it.

An entity with no ledger entry becomes generation work through `unimplemented` rows
only: [`bind`](./bind.md) classifies first, and an adopted entity whose rows all read
`verified` derives nothing. An entity with no requirement derives nothing.
`jazyk gen --force` regenerates the named entities against the recorded decision
regardless of the ledger
([incremental regeneration](../../consumers/gen.md#incremental-regeneration)).

E.g.:

```yaml
- id: c414-0
  generation: 414
  mutation: 0
  kind: ledger-stale
  subject: ent:order
  via: ledger
  detail: {goal: generate, reason: unimplemented, changed: [req:orders-6]}
```

## Readiness

- Tier 3, after binding: ready when no goal of tiers 0 to 2 is open in the entity's
  cone and no `bind` goal is open on any of its requirements
  ([readiness](../reconciler.md#readiness)). The bound tests define the interface
  the product conforms to, so they exist before the product does.
- Within the tier, goals order topologically over the derived relationships
  ([order from relationships](../../consumers/gen.md#order-from-relationships)):
  leaf entities first, then the entities that compose or depend on them.
- In `manual` mode the goal is `blocked {on: generate release}` until the generate
  [release](../control-plane.md#modes-and-releases) lands.
- Batches group by locality ([batching](../reconciler.md#batching)): one entity per
  session by default; where [containment](../model/entity.md#containment) exists, a
  component subtree (a parent and the children whose goals are ready) batches into
  one session when the budget allows, so one session composes the whole part. A
  dense entity generates in parts of `{GROUP}` (20) requirements
  ([dense entities](../../consumers/gen.md#dense-entities-generate-in-parts)).
- The executor resolves through [`[executors]`](../project-settings.md#executors).

## Gate

`record_generation` landed for the entity with a `factHash` equal to the live
graph's, and `run_tests` ran afterwards. The manifest names every file the session
wrote, one test row per requirement (`{requirement, kind, label, artifact, name,
run, cwd}`), and the build when the medium is `built`. The harness checks the
ledger, not the model's word:

- `mark_goal_done({goal, justification})` is validated against the ledger entry. No
  record, or a stale `factHash`, is rejected naming the gate. A `factHash` that moved
  mid-session is recorded but leaves the entity pending: the goal derives again with
  the new facts.
- `record_generation` applies the shape gates: a programmatic row with an empty
  `artifact` or `run` is rejected naming the row; a declared test name absent from
  the artifact, or a present test left undeclared, gets one corrective retry; a path
  another entity owns is rejected with the owner named
  ([file ownership](../../consumers/gen.md#file-ownership-and-conventions)); a
  `built` medium with no build recorded anywhere is rejected
  ([the build](../../consumers/gen.md#the-build)).
- A session that recorded and ended without marking still resolves the goal at the
  next derivation. A session that never recorded has not resolved it: retry once with
  a fresh session, then park
  ([resolving, failing, parking](../sessions.md#resolving-failing-parking)).
- A bound test the session cannot make pass without changing it is left untouched.
  The session records what it wrote and says so in its justification; the row reads
  `failing`, and the repair is a human decision (re-bind, or fix the docs), never a
  quiet rewrite of the judge
  ([generation makes bound tests pass](../../consumers/bind.md#generation-makes-bound-tests-pass)).
- `mark_goal_failed({goal, reason})` when the medium cannot be produced (no
  toolchain available, a build that cannot run). A failed mandatory goal blocks
  convergence and surfaces on the entity.

## Hints

Computed by the harness and rendered under the goal block:

- The reason and `changed`: the requirement ids added, removed, or reworded.
- The bound tests the product must make pass and the run commands already recorded:
  one toolchain per deliverable.
- The other entities' files and what each holds: reference them, never write to them.
- The build when one exists, and its last failure while it stands, with the files of
  this entity the failure names (`build.lastRun`).
- The part count for a dense entity.
- `load ent:<parent>` when the entity has a parent not in the loaded set.
- The call that resolves the kind: `begin_generation` with this entity, then
  `record_generation`, then `run_tests`.

## What the model sees

Two worker modes, selected by [`gen.worker`](../project-settings.md#generation).

### Agentic (the default)

The session prompt is [assembled](../sessions.md#the-prompt): the agent contract,
the project block, the goals block, the loaded set. The goal block carries the
contract paragraph from [`prompts/generate.md`](./prompts/generate.md) (claim the
entity with `begin_generation`, follow the package, write real files, record, run),
the change in one line, the gate in one line, and the hints; the last hint is the
pointer line from [`prompts/generate-pointer.md`](./prompts/generate-pointer.md),
`{target}` replaced by the entity id. The loaded set holds the entity in full with
its requirements, and its parent and children as stubs.

1. `begin_generation({entity})` answers with
   [`prompts/generate-contract.md`](./prompts/generate-contract.md) as
   `instructions` (`{GROUP}` replaced by the part size, 20 requirements) and the
   package:

```text
entity, name, deliverable          the target and where its files go
factHash                           passed back on record_generation
medium                             already decided; never re-decided
build                              the recorded build (reuse and extend, never a second)
runCommands                        the established toolchain; reuse it
changed                            requirement ids added, removed, or reworded
generatedFiles                     other entities' files with what each holds; never write to them
boundTests                         tests binding wrote; make them pass
requirementGroups                  each requirement with its testName, statement, quote
context                            the entity's neighborhood (../context.md#rendering)
```

2. The model writes its files, extends and runs the build when there is one, reads
   failures, repairs its own work, records the manifest, runs the tests.

The packaged form (the contract plus the package rendered as text) is what the
[benchmark](../../benchmark/benchmark.md) grades.
[`jazyk gen`](../../consumers/gen.md#command) runs the same goal outside a full
build; an external agent on the `generate` serving claims the same package with
`generation_tasks` and `begin_generation`
([generation over MCP](../../frontends/mcp.md#generation-and-verification-over-mcp)).
`jazyk preview <goal>` shows the assembled prompt before it is spent.

### Pipeline (`gen.worker = "pipeline"`)

A fixed sequence of one-shot completions, each with the generation contract as the
system prompt: one ask per product part (dense entities split into groups of
`{GROUP}` requirements), one for the tests, one for the manifest. No tools; the
harness writes the returned files itself, applies the same manifest gates with one
corrective retry per complaint, and records. The pipeline resolves the same goal
through the same ledger.

## The medium decision

Once per deliverable, before any generation: a one-shot completion whose system
prompt is one line ("You decide what a deliverable is made of. Answer with one JSON
object and nothing else.") and whose user message carries the graph's statements and
asks for `{"form", "produced": "written"|"built", "toolchain", "artifact"}`. The
answer is recorded in the ledger, every later package restates it as settled, and a
[bind](./bind.md) session that runs first makes the same decision the same way. See
[the medium](../../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).

## Invented choices

Anything the deliverable needs that the documents do not state is an ambiguity. The
session does not stall on one: it chooses with best judgment and names the choice in
the `choices` argument of `record_generation`, beside the manifest
(`choices: [{choice, scope, reasoning, requirements?}]`: the choice in one sentence,
`scope` one of `product`, `behavior`, `detail`, the model's reasoning, and the
requirements the choice fills in). `record_generation` files one `invented-choice`
diagnostic per choice, its
subjects the entity and the requirements the choice fills in when any exist, graded
by the scope of the invention: `product` grades `error` (the invention is the product
itself), `behavior` grades `warning` (an unspecified behavior), `detail` grades a
suppressible `info` (cosmetic detail). The diagnostic
carries a prompt in the shape of a ratification proposal (an `edit` option with the
sentence the docs should gain, an `answer` option to keep the choice unstated), so it
surfaces as an [`answer`](./answer.md) goal; accepting the edit writes the choice into
the documents, and the next build extracts it as a requirement. Re-recording the
entity overwrites its invented set: a choice the new record omits resolves.
Generated mass attached to no requirement, the unattached remainder, measures the
same debt and lands on the entity's ledger entry. See
[invented choices](../../consumers/gen.md#invented-choices).

## Tools

The session's serving runs in `generate` mode ([toolsets](../tools.md#toolsets)):

- The [read tools](../tools.md#read-tools), including the loaded-set tools `load`,
  `expand`, `unload`, `graph_status` ([context](../context.md#tools)).
- The [goal tools](../tools.md#goal-tools): `mark_goal_done`, `mark_goal_failed`,
  `load_skill`, `done`.
- The [generation tools](../tools.md#generation-tools): `generation_tasks`,
  `begin_generation`, `record_generation({entity, factHash, manifest, choices?})`.
- The [binding tools](../tools.md#binding-tools) and `run_tests`.
- [`report_feedback`](../tools.md#feedback-tool).
- The file and command tools below when the agent's profile sets
  [`serve_files`](../project-settings.md#acp).

### File and command tools

Generation, binding, and verification sessions edit and inspect the deliverable
([generation sessions](../sessions.md#generation-sessions)), so their toolsets add
file and command tools sandboxed to the deliverable directory. They are served into a
session only when the agent's profile sets `serve_files`: a coding agent brings its
own editor and shell
([MCP into sessions](../../frontends/mcp.md#mcp-into-acp-sessions)); the
[embedded agent](../../frontends/acp.md#the-embedded-agent) has none, so jazyk
serves these:

- `read_text_file({path, line?, limit?})`: one file's content, path relative to the
  deliverable.
- `write_text_file({path, content})`: write one file. A path recorded for another
  entity is rejected with the owner named; during a `bind` goal, a path the ledger
  records as any entity's implementing file is rejected the same way.
- `list_files({path?})`: the deliverable tree.
- `run_command({command, cwd?})`: execute a shell command under the deliverable,
  bounded by a timeout; the exit code and output tail come back. This is how the
  session runs the build it wrote, reads the traceback, and fixes its own work.
- `run_tests({requirements?})` and `record_generation({...})`: the same tools the
  generation toolset serves over MCP.

The names and shapes track the Agent Client Protocol's file-system and terminal
methods; the tools ride the injected MCP serving like every other jazyk tool. Paths
that escape the deliverable are rejected. Command execution during generation is the
same trust decision as `jazyk test` running recorded commands, made at generation
time. A [`verify`](./verify.md) session is served the read-only subset
(`read_text_file`, `list_files`, `run_command`) and no `write_text_file`.

### The finish contract

`record_generation` records the manifest (every file written, one test row per
requirement, the build when the medium is built) and the invented choices. Recording
strips every single-line marker comment (`req:<id> hash:<hash8>`) from the manifest
files and records each as an anchored site on its requirement's row
([traceability](../../consumers/gen.md#traceability)), updates both ledger maps, and
[prunes rows](../../consumers/gen.md#deletion-prunes-the-ledger) whose requirement
left the graph. `run_tests` then verifies the programmatic rows: the build first,
once, then each row's command; verdicts land as a side effect. The session marks the
goal done with a one-line justification and ends with `done`. A session that ends
without recording has failed the goal.
