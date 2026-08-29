# The verify goal

`verify` settles one ledger row's verdict. Most of the work is no model call at all:
a `programmatic` row runs its recorded command in its recorded `cwd`, and the exit
code is the verdict. Only an `llm` row asks a model, because its requirement needs
judgment no command can render. See
[the ledger's verification statuses](../../consumers/gen.md#status-is-derived-never-stored).

- Class: compile. Mandatory. Readiness tier 3.
- Unit: one ledger row. Goal id `g:verify:req:<doc-stem>-<n>`.
- Skill: none.

## Created when

The goal derives from a `ledger-stale` [change record](../graph.md#change-records)
whose `detail.goal` is `verify`, written whenever a row's derived status says action:

- `never-run`: the row has a current test and files and no verdict (`unverified`).
- `test-changed`: the test artifact bytes differ from `hashes.test` (`stale-test`)
  and the artifact still contains the declared test name.
- `code-changed`: the manifest files hash differs from `hashes.files` (`stale-code`).
- `runner-failed`: the last run could not execute, so the row is `unverified` with
  its output as evidence.

Rows the other ledger goals own derive no `verify` goal: `missing` and
`stale-requirement` rows, and artifacts that lost their declared test name, are
[`bind`](./bind.md) work; `unimplemented` rows are [`generate`](./generate.md) work;
a `failing` row (a `fail` verdict on implementing files) is a finding for the author,
re-verified only when its test or files change. Rows whose requirement left the graph
are not work.

E.g.:

```yaml
- id: c416-0
  generation: 416
  mutation: 0
  kind: ledger-stale
  subject: req:orders-6
  via: ledger
  detail: {goal: verify, reason: code-changed, kind: programmatic}
```

`cause` is the commit that rebaselined the row, or the `edit` journal entry the build
writes when it notices the deliverable or a test changed
([edit paths](../compilation.md#edit-paths)).

## Readiness

- Tier 3, after binding and generation: ready when no goal of tiers 0 to 2 is open in
  the requirement's cone and no `bind` or `generate` goal is open on the requirement
  or its entity ([readiness](../reconciler.md#readiness)).
- Never gated by a release: verification writes nothing into the deliverable.
- Batches group by run: every ready programmatic row in a burst runs as one
  `run_tests` pass, the build first, once. `llm` rows batch by entity into sessions
  ([batching](../reconciler.md#batching)).

## Gate

A verdict (`pass` or `fail`) recorded on the row with a `factHash` equal to the live
statement hash, and the `test` and `files` hashes rebaselined. A stale `factHash` is
recorded but leaves the row pending: verifying an old statement never launders a
changed one, and the goal derives again.

- A programmatic row resolves without a session: the harness records the exit code
  and the output tail, and the journal entry names the goal with the command as its
  justification. Zero LLM calls.
- A run that could not execute (exit `127` or `126`, or every executed row failing
  with identical evidence once the command line is stripped) records `runner-failed`,
  leaves the verdict at `none`, and fails the goal with that reason: a broken machine
  never reads as a failing deliverable, and a build whose tests cannot run does not
  converge ([a test that could not run says nothing](../../consumers/gen.md#a-test-that-could-not-run-says-nothing)).
  The next build derives the goal again and retries.
- An `llm` row resolves when `record_verdict` lands. `mark_goal_done` is validated
  against the row; a claim without a verdict is rejected naming the gate. A session
  that recorded and ended without marking still resolves the goal at the next
  derivation. `mark_goal_failed({goal, reason})` when the implementing files cannot
  be read or the criteria cannot be followed.

## Hints

- The reason and the previous verdict with its evidence.
- For a programmatic row: the run command and `cwd`. For an `llm` row: the criteria
  file path and the implementing files.
- The build when one exists.
- The call that resolves the kind: `begin_verification` with this requirement, then
  `record_verdict`.

## What the model sees

Only an `llm` row's goal reaches a model
([generation sessions](../sessions.md#generation-sessions)). The session prompt is
[assembled](../sessions.md#the-prompt): the goal block carries the contract paragraph
from [`prompts/verify.md`](./prompts/verify.md) (confirm the requirement is satisfied
by the implementing files, follow the criteria's confirm steps, report `pass` only
when every criterion is met, state what was inspected or executed and what was
observed), the change in one line, the gate in one line, the hints. The loaded set
holds the requirement in full and its entity as a stub.

`begin_verification({requirement})` answers with the package: the statement, quote,
and hash; the requirement's neighborhood ([rendering](../context.md#rendering)); the
manifest files; the criteria and confirm steps. The model reads the deliverable with
its own tools or the served [file tools](./generate.md#file-and-command-tools), and
records with `record_verdict({requirement, verdict, factHash, evidence})`. The
evidence is short: what was inspected or executed and what was observed.

The criteria file was written at generation or binding time; it is the requirement's
own test artifact ([criteria files](../../consumers/gen.md#criteria-files-for-llm-tests)).

### The one-shot form

`jazyk test` outside a build holds no session. It judges an `llm` row with one
completion and no tools:

- System prompt, from the row's package: "Confirm the requirement is satisfied by the
  implementing files. Follow the criteria's confirm steps using the deliverable
  paths. Report verdict pass only if every criterion is met; state what you inspected
  or executed and what you observed."
- User message, assembled in this order:

```text
{the criteria file: front matter, statement, quote, implementing paths, confirm steps, verdict contract}

Context:
{the requirement's neighborhood (../context.md#rendering)}

Implementing files:
=== {path} ===
{file content, truncated at 12k characters each}

Reply with a verdict line `PASS` or `FAIL`, then your reasoning.
```

The verdict is read from the reply's first non-empty line when that line leads with
the word: bare, bolded, or after a `Verdict:` label. A reply that answers
reasoning-first is read from its conclusion instead: the later of `PASS` or `FAIL`
anywhere in the reply wins, so quoting the criteria's own words on the way to the
answer never flips the verdict. A reply carrying neither word is unparseable and the
row stays pending. The trimmed reply rides into the ledger as evidence. Both forms
land the same row.

## Tools

An `llm` session's serving runs in `verify` mode ([toolsets](../tools.md#toolsets)):

- The [read tools](../tools.md#read-tools), including the loaded-set tools.
- The [goal tools](../tools.md#goal-tools).
- The [verification tools](../tools.md#verification-tools): `verification_tasks`,
  `begin_verification`, `run_tests`, `record_verdict`.
- [`report_feedback`](../tools.md#feedback-tool).
- The read-only [file tools](./generate.md#file-and-command-tools)
  (`read_text_file`, `list_files`, `run_command`) when the profile sets
  [`serve_files`](../project-settings.md#acp). A verify session writes no file.

An external agent on the same serving runs the goal through `verification_tasks`,
`begin_verification`, and `record_verdict`, with `run_tests` covering the
programmatic rows ([over MCP](../../frontends/mcp.md#generation-and-verification-over-mcp)).
Whichever harness runs, the row comes out the same shape. The trace carries the
worker's own events per row (`verifyRowStart`, `verifyRowDone`, `verifyRowStale`,
`verifyRowError`, [trace events](../sessions.md#trace-events)).
