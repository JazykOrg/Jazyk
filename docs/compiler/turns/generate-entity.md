# The generation turn

Goal: produce ONE entity's part of the deliverable AND the tests for its
requirements, then record the manifest. The medium is decided once per deliverable
before any task runs (below); the turn implements, it never re-decides. See
[generation](../../consumers/gen.md).

## What the model sees

Two worker modes, selected by [`gen.worker`](../project-settings.md#generation):

### Agentic (the default)

1. The session prompt is the [generate pointer](./prompts/generate-pointer.md),
   `{target}` replaced by the entity id: call `begin_generation`, follow the
   package, write real files, record, run.
2. The `begin_generation` reply carries the
   [generation contract](./prompts/generate-contract.md) as `instructions`
   (`{GROUP}` replaced by the part size, 20 requirements), plus the package:

```text
entity, name, deliverable          the target and where its files go
factHash                           passed back on record_generation
medium                             already decided; never re-decided
build                              the recorded build (reuse and extend, never a second)
runCommands                        the established toolchain; reuse it
changed                            requirement ids added, removed, or reworded
generatedFiles                     other entities' files; never write to them
boundTests                         tests binding already wrote; make them pass
requirementGroups                  each requirement with its required testName, ears, quote
context                            the entity's context pack (../context.md)
```

The packaged form (the [system prompt](./prompts/generate-entity.md) plus the same
package rendered as text) serves the [benchmark](../../benchmark/benchmark.md).

### Pipeline (`gen.worker = "pipeline"`)

A fixed sequence of one-shot completions, each with the
[generation contract](./prompts/generate-contract.md) as the system prompt: one ask
per product part (dense entities split into groups of `{GROUP}` requirements), one
for the tests, one for the manifest. Format rejections retry once with the error
quoted. No tools; the harness writes the returned files itself.

## The medium decision

Once per deliverable, before any task: a one-shot completion whose system prompt is
one line ("You decide what a deliverable is made of. Answer with one JSON object
and nothing else.") and whose user message carries the graph's statements and asks
for `{"form", "produced": "written"|"built", "toolchain", "artifact"}`. The answer
is recorded in the [ledger](../../consumers/gen.md#the-ledger); every later package
restates it as settled. See
[gen.md](../../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated).

## Tools

Agentic mode: the serving runs in `generate` mode (see
[task toolsets](../tools.md#task-toolsets)): read tools, binding and generation
lifecycles, `run_tests`, plus `report_feedback`. File and shell tools are the
agent's own; the embedded agent gets them served (`--serve-files`).

## Finish

`record_generation` with the manifest (every file written, one test row per
requirement, the build when the medium is built), then `run_tests`. The harness
checks the ledger, not the model's word.
