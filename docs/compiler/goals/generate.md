# The generate goal

`generate` develops one entity's part of the deliverable and its tests, reuses
existing checks, and records the manifest. The agent chooses when to write source
and tests. The medium is decided once per
deliverable before any generation runs (below); the session implements, it never
re-decides. See [generation](../../consumers/gen.md).

- Class: compile. Mandatory. Readiness tier 3.
- Unit: one entity. Goal id `g:generate:ent:<slug>`.
- Skill: none. The contract rides the opening prompt, or the package on a standalone serving (below).

## Created when

The goal derives from a `ledger-stale` change, derived by the ledger comparison at
derivation and never stored ([change records](../graph.md#change-records)),
whose `detail.goal` is `generate`, present whenever the entity's facts differ from
the [ledger](../../consumers/gen.md#the-ledger):

- `facts-changed`: the entity has a ledger entry whose `factHash` (name, definition,
  stereotype, attributes, every referencing statement with its edges, and
  [referenced input paths and byte digests](../../consumers/gen.md#referenced-assets)) differs from
  the live graph. `detail.changed` lists
  the requirement ids added, removed, or reworded since the last generation.
- `files-gone`: the entity has a ledger entry whose facts match, but a file it records
  is gone from the deliverable (deleted by hand). `detail.changed` lists the missing
  files. The part has to be written again before its rows can be judged.
- `source-selection-changed`: a recorded implementing file left current Source,
  including a file assigned to Documents in any set. Re-bind first, then write
  the entity's part inside current Source. Test artifacts and build outputs do
  not count as implementing files. Reclassification preserves every file.
- `unimplemented`: a requirement of the entity has a bound row whose derived status
  is `unimplemented` or `partial` (a test fails and nothing, or only part, implements
  it). The bound test is the acceptance gate; generation is the work that clears it. A
  row still red after a generation over the current facts reads `failing` and derives
  nothing ([status](../../consumers/gen.md#status-is-derived-never-stored)): a
  contradictory criterion once regenerated its entity on every build.
- `unbound-requirements`: a requirement needs a current binding or criterion test.
  Generation can inspect existing source or create implementation before selecting
  and recording its tests. A failing test against absent source is not required.
- `referenced-inputs`: an adopted entity names local input assets but has no entity
  fingerprint. Generate once to establish the input baseline, even when its bindings
  already pass. Later input path or byte changes use `facts-changed`.

The reasons are checked in that order and the first that holds names the goal. An
entity with no ledger entry becomes generation work through missing bindings or
`unimplemented` rows. An adopted entity whose bindings are current and whose rows
all read `verified` or `failing` derives nothing unless it needs that referenced-input
baseline. An entity with no requirement derives nothing.
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

- Tier 3: ready when no tier 0, 1, or 2 goal is open or parked and the entity owes
  no `derive-criteria` ([readiness](../reconciler.md#readiness)). Missing bindings
  do not delay implementation. The requirements and criteria define the outcome;
  the coding agent chooses source, tests, and their development order. Over MCP,
  `begin_generation` refuses an entity whose goal is not ready with the reason
  (`not-ready`). Existing bindings remain available for reuse.
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
  record, or a stale `factHash`, is rejected naming the gate. A record whose files are
  all gone is rejected naming them; a record with no implementing file of its own
  stands when every requirement of the entity has a row naming an implementing file
  that exists (an actor whose statements the tool it uses implements writes nothing of
  its own), and is rejected naming the requirements that have none. Test artifacts and
  criteria files are not implementation. A `factHash` that moved
  mid-session is recorded but leaves the entity pending: the goal derives again with
  the new facts.
- `record_generation` applies the shape gates, all before any side effect: a row
  with no `requirement` is rejected naming its artifact or test name (a local model
  once recorded three tests without it, read `tests: 0` in the reply, and spent the
  rest of its rounds binding each requirement by hand); a
  programmatic row with an empty `artifact` or `run` is rejected naming the row; a
  row whose artifact does not exist, or a programmatic row whose artifact does not
  contain the declared test name (its last path segment: `tests::req_x`, the name a
  Rust runner addresses inside `mod tests`, is found by `req_x`), is rejected naming
  the row. A JUnit selector such as `Suite::req_x` follows the same rule during
  generation, binding, and later binding checks. A selector ending in `::` is invalid.
  Missing artifacts are rejected naming
  the row and the file (the same
  gate `record_binding` applies; the pipeline worker gets one corrective retry before
  it records, and a row still wrong after it falls back to `llm`); a path that
  escapes the deliverable is rejected naming it, and one that restates the deliverable
  directory's name is normalized under the root with a `note`; a path an entity
  outside this entity's component group owns is rejected with the owner named
  ([file ownership](../../consumers/gen.md#file-ownership-and-conventions)); a
  manifest whose `files` name no implementing file while a requirement of the entity
  has no row naming one is rejected naming those requirements; a
  `built` medium with no build recorded anywhere is rejected
  ([the build](../../consumers/gen.md#the-build)). A record that lands removes the
  files the entity's previous record listed and this manifest omits
  ([incremental regeneration](../../consumers/gen.md#incremental-regeneration)) and
  names them in its reply. A file outside current Source is preserved.
- A session that recorded and ended without marking still resolves the goal at the
  next derivation. The run compares the entity's monotonic `recordSequence`, so a
  second record within the same timestamp second still proves completion. An older
  saved session without sequence receipts keeps its `recordedAt` comparison. A
  session that never recorded has not resolved it. A compiler build gives it up to
  four attempts with fresh sessions, then parks it
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
- The other entities' files and what each holds: a group member's are shared, any
  other entity's are referenced, never written.
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
the change in one line, the gate in one line, and the hints; the last hint names
the resolving calls: `begin_generation`, then `record_generation`, then
`run_tests`. The loaded set holds the entity in full with
its requirements, and its parent and children as stubs.

A session whose batch holds a `generate` goal opens with one generation contract,
[`prompts/generate-contract.md`](./prompts/generate-contract.md) (`{GROUP}` replaced
by the part size, 20 requirements), after the assembled prompt. Every session form
assembles it the same way: an owned worker batch, a benchmark case, a snippet, a
replay. Its `begin_generation` replies then omit `instructions`. A standalone serving
holds no batch, so its reply carries the contract.

1. `begin_generation({entity})` answers with the package (and the contract as
   `instructions` on a standalone serving):

```text
entity, name, deliverable          the target and where its files go
factHash                           passed back on record_generation
medium                             already decided; never re-decided
build                              the recorded build (reuse and extend, never a second)
runCommands                        the established toolchain; reuse it
changed                            requirement ids added, removed, or reworded
generatedFiles                     other entities' files with what each holds; never write to them outside sharedWith
sharedWith                         the entity's component group: members whose files it may write and record too
supportFiles                       the deliverable-wide files any session may rewrite
buildEntry, buildError             the build's entry with its content; the standing failure, if any
boundTests                         existing tests, one per criterion under criteriaTests; reuse valid checks, correct undocumented implementation assumptions, record fresh evidence
ownershipNote                      a row already passing under another entity's files is carried there: list it unchanged, record no file for it; a requirement another group's file carries names that file on its row's `files`; an entity whose rows are all carried records files: []; a group member's file is shared
requirementGroups                  each requirement with its statement, quote, openDiagnostics, criteria (id, statement, suggestedTestName) and testName (for a requirement with no criteria)
contradicted, mediumWarning        the disputed requirements; the medium's standing divergence
context                            the entity's neighborhood (../context.md#rendering)
```

2. The model writes its files, extends and runs the build when there is one, reads
   failures, repairs its own work, records the manifest, runs the tests.

The [benchmark](../../benchmark/benchmark.md) grades this same opening prompt and
these same replies: a case session assembles its prompt through the build's path.
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

- The [read tools](../tools.md#read-tools): the loaded-set tools `load`, `expand`,
  `unload`, `graph_status`, and the lookups `search`, `read_section`, `get_entity`,
  `get_view`, `diagnostics` ([context](../context.md#tools)).
- The [goal tools](../tools.md#goal-tools): `mark_goal_done`, `mark_goal_failed`,
  `load_skill`, `done`.
- The [generation tools](../tools.md#generation-tools): `generation_tasks`,
  `begin_generation`, `record_medium`,
  `record_generation({entity, factHash, manifest, choices?})`. A session whose package
  reports an undecided medium can record that decision before writing files.
- The [binding tools](../tools.md#binding-tools) `binding_tasks`, `begin_binding`,
  `record_binding`, and `run_tests`.
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
  deliverable. `limit` defaults to 400 lines and the content is capped at 64 KiB.
  Content preserves the original UTF-8 bytes, including LF or CRLF line endings
  and a final newline. Delimiters count toward the byte cap. Concatenating complete
  pages reproduces the file exactly. The reply includes `hash`, the SHA-256 of the
  whole file, even when only part of its content is returned.
  When more lines remain the reply carries `truncated: true` and `nextLine`, so a
  bundle or a lockfile never floods the history the session resends every round.
  A single longer line returns a UTF-8-safe prefix within the same byte cap and
  `truncatedLine` names that line. The reply states that the rest of the line was
  omitted. `nextLine` advances to a following line only when one exists.
  Reads reject hidden paths, credential paths, Git internals and private compiler
  state and nested projects, even under a broad Source selection. The same rule
  checks the resolved symlink target and its owning project. Ordinary documents,
  build evidence and explicit `.github` files
  remain readable without requiring them to be Source.
- `write_text_file({path, content, append?, old_text?, expectedHash?})`: write one
  file, or append when `append` is true. Use `old_text` for a small literal edit:
  it must match exactly once, and `content` replaces that occurrence. Empty,
  missing or repeated matches are rejected without changing the file. Replacement
  requires `expectedHash` from the last read or write, and cannot combine with
  `append: true`. Quotes, backslashes and line endings are literal, not a regex.
  `expectedHash` also guards ordinary overwrites and appends when supplied.
  A mismatch returns `file-conflict` with the current hash. Read the affected
  region again and reconsider the edit. Do not retry the stale edit blindly.
  Hash comparison and commit run synchronously in the workspace's serialized
  execution. This does not lock out writes by unrelated external programs.
  Each accepted write commits complete bytes through a sibling temporary file
  renamed into place. Native writes preserve existing file permissions. Replies
  include `hash` for the complete resulting file, `fileBytes`, and `replaced`
  for a literal edit. Prefer a localized edit over rewriting unchanged sections.
  New files and complete rewrites still use `content`; append is available when
  a file genuinely needs several complete calls.
  A path recorded for an entity outside the session's component group is rejected
  with the owner named. The same Source, protected-path and ownership gates apply
  to every write mode. During a `bind` goal, a path the ledger records as any
  entity's implementing file is rejected the same way. A path that restates the
  deliverable directory's name is normalized under the root, here and in
  `read_text_file` and `list_files`, and the reply's `note` says so
  ([file ownership](../../consumers/gen.md#file-ownership-and-conventions)).
- `list_files({path?})`: the deliverable tree, walked breadth first in name order.
  A directory reached again through a symlink is walked once. The reply names at most
  500 files and carries `truncated: true` when the walk stopped there; list a
  subdirectory with `path` to see more of it.
- The [task tools](../tools.md#task-runtime-tools) execute saved commands in finite
  isolated runs. The session inspects status, logs, retained changes, and test reports,
  then repairs its source and submits another run when needed.
- `run_tests({requirements?})` and `record_generation({...})`: the same tools the
  generation toolset serves over MCP.

The names and shapes track the Agent Client Protocol's file-system and terminal
methods; the tools ride the injected MCP serving like every other jazyk tool. Paths
that escape the deliverable are rejected. Command execution during generation is the
same trust decision as `jazyk test` running recorded commands, made at generation
time. A [`verify`](./verify.md) session is served the read-only subset
(`read_text_file`, `list_files`) and the task tools, with no `write_text_file`.

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
