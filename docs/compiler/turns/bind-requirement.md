# The bind turn

Goal: tie ONE requirement to the deliverable. The turn observes and judges, it
never changes implementation files: it searches for an implementation, finds or
writes the one test that judges the statement, runs it, and records the row. The
verdict classifies the requirement: verified, unimplemented (the failing test is
the acceptance gate generation must clear), or failing (a contradiction for the
author). See [binding](../../consumers/bind.md).

## What the model sees

The live path differs from the compilation turns: the contract rides the tool
reply, not the session prompt, because the agent must interleave jazyk's package
with its own file and shell tools.

1. The session prompt is the [bind pointer](./prompts/bind-pointer.md), `{target}`
   replaced by the requirement id: call `begin_binding`, follow the package, do
   exactly this one requirement.
2. The `begin_binding` reply carries the [bind contract](./prompts/bind-contract.md)
   as `instructions`, plus the package:

```text
requirement, entity, reason        unbound | requirement-changed | artifact-gone
ears, quote                        the statement and its verbatim source
suggestedTestName                  req id plus statement hash prefix
medium                             already decided; never re-decided
build                              the recorded build, when one exists
testConventions                    recorded rows to imitate (runner, command style)
entityFiles                        the entity's recorded files: start the search here
context                            the requirement's context pack (../context.md)
```

The packaged form (the [system prompt](./prompts/bind-requirement.md) plus the same
package rendered as text) serves the [benchmark](../../benchmark/benchmark.md),
which grades the turn without a live deliverable agent loop.

## Tools

The session's jazyk serving runs in `generate` mode (see
[task toolsets](../tools.md#task-toolsets)): the read tools, the binding lifecycle
(`binding_tasks`, `begin_binding`, `record_binding`), the generation lifecycle, and
`run_tests`, plus `report_feedback`. File and shell tools are the agent's own; for
an agent that brings none (the embedded agent), the serving adds them
(`--serve-files`). Test and criteria files are the only files a bind turn may
write.

## Finish

`record_binding` is the finish line: the files that carry the statement (an empty
list is a finding, not a failure), the test row, the verdict, the evidence. The
harness checks the [ledger](../../consumers/gen.md#the-ledger), not the model's
word: a turn that never recorded has failed the task.
