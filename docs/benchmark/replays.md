# Replays

A [case](./cases.md) grades a model: one session, one goal, a fixture, checks over what
the model staged. A replay grades the harness instead: the same fixture and goal, but
the tool calls are scripted, so the session runs without a model, in seconds, and the
assertions cover what the harness answered at each step. A fix to a gate, a hint, a
reply note, or a contract is verified by replaying the exact scenario that exposed it,
never by compiling a project again.

## Where replays come from

Every session leaves a [trace](../compiler/sessions.md#trace-events) whose `toolCall`
and `toolResult` events carry each call's full arguments and reply.
`jazyk replay extract <trace.jsonl> <label>` turns one session of a trace (its batch
label, `b24-1`, or its goal id for a ledger session) into a replay file on stdout: the
goal, the project's documents as the fixture, the calls in order, and each call's
recorded outcome as its expectation (`ok`, or the `rule` the harness refused with).
The graph the session saw comes from the batch's
[snapshot](../compiler/sessions.md#snapshots) when the run left one, else from the
project's current graph (the file says which): the extractor seeds under `given.graph`
the goal's targets, every node the calls name, the requirements that reference a
goal's entity, the entities of those requirements, and their parent chains, so the
goal derives as it did. The author trims the fixture to what
the scenario needs, keeps the steps that matter, and edits the expectations to what the
harness should have answered. A replay is the finding written down as a test.

A replay can also be written by hand from a finding, the way a case is: a small
fixture, the few calls that reproduce the scenario, the expected replies.

## Replay format

A replay file is a markdown file under [`replays/`](./replays) with an H1, a paragraph
naming the finding it holds, and one or more fenced `yaml` blocks per
[`case.schema.yaml`](./case.schema.yaml). A replay is a case with `steps`:

- `name`, `description`, `goal`, `given`: as a [case](./cases.md#case-format). The
  fixture seeds the same sandbox, the goal derives or assembles the same way, and the
  session opens with the same prompt and the same loaded set a build's session would.
- `goals` (optional): the batch's other goals, each `{kind, target}`, derived from the
  same fixture and held in the same session, since a recorded batch holds several (a
  review batch on f2 held four entities) and a session marks each of them.
- `steps`: the calls, in order. Each is `call` (a tool name from the goal's
  [toolset](../compiler/tools.md#toolsets)), `args` (the arguments object), and an
  optional `expect`:
  - `ok: true` (the default): the reply is not an error.
  - `rule: <name>`: the reply is an error with this rule.
  - `contains: <pattern>`: the reply (the error message, or the reply JSON) matches
    the pattern, case-insensitive.
  - `absent: <pattern>`: the reply does not match the pattern.
  A `done` step ends the session the way the model's `done` does: the batch gates
  run, and the staged work commits when they hold.
- `assert`: the [checks](./cases.md#checks) over the sandbox after the commit, as in
  a case. A replay whose steps never reach `done` still runs its checks over the
  seeded fixture, so a replay about a refusal asserts on the refusal alone.

E.g.:

```yaml
name: heading-quote
description: A quote that is the section's heading is refused as a title, with the reason
goal: {kind: reconcile-section, target: docs/plot.md}
given:
  docs:
    docs/plot.md: |
      # Plot

      ## Chapter 1: Arrival

      Theo arrives on the afternoon ferry.
steps:
  - call: upsert_entity
    args:
      name: Chapter 1
      definition: The first chapter.
      mention: {section: docs/plot.md#/plot/chapter-1-arrival, quote: "## Chapter 1: Arrival"}
    expect: {rule: quote-not-found, contains: heading}
assert:
  - entityAbsent: {namePattern: "^Chapter 1$"}
```

## Running replays

`jazyk replay <file|dir>...` runs every replay in the named files and directories and
prints one line per step (`ok`, or the mismatch: what was expected and what the
harness answered) and one per check, then a summary; the exit code is non-zero when
any step or check fails. With no argument it runs [`replays/`](./replays). A named file holding no replay is an
error, never a silent pass: an unclosed fence once graded nothing and exited 0. The
replays under `replays/` also run under `cargo test`, so a harness change that breaks
a recorded scenario fails the build.

A replay never calls a model. The single-call variant (run the recorded steps up to a
point, then ask the configured model for its next action once and grade that action)
is the [snippet](./benchmark.md#snippets-from-a-real-project) path's next step and is
not implemented yet.
