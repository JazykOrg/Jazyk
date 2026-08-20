# The verification call

Goal: settle one ledger row's verdict. Verification is mostly not a model call at
all: a `programmatic` row runs its recorded command in its recorded `cwd`, and the
exit code is the verdict. Only an `llm` row asks a model, because its requirement
needs judgment no command can render. See
[the ledger's verification statuses](../../consumers/gen.md#status-is-derived-never-stored).

## What the model sees (llm rows)

One one-shot completion (no tools):

- System prompt, from the row's task package: "Confirm the requirement is
  satisfied by the implementing files. Follow the criteria's confirm steps using
  the deliverable paths. Report verdict pass only if every criterion is met; state
  what you inspected or executed and what you observed."
- User message, assembled in this order:

```text
{the criteria file: front matter, statement, quote, implementing paths, confirm steps, verdict contract}

Context:
{the requirement's context pack (../context.md)}

Implementing files:
=== {path} ===
{file content, truncated at 12k characters each}

Reply with a verdict line `PASS` or `FAIL`, then your reasoning.
```

The verdict is read from the reply's first non-empty line when that line leads with
the word, as the contract asks: bare, bolded, or after a `Verdict:` label. A reply
that answers reasoning-first is read from its conclusion instead: the later of
`PASS` or `FAIL` anywhere in the reply wins, so quoting the criteria's own words on
the way to the answer never flips the verdict. A reply carrying neither word is
unparseable and the row stays pending. The trimmed reply rides into the ledger as
evidence.

The criteria file was written at generation or binding time; it is the requirement's
own test artifact. Its template lives with
[criteria files](../../consumers/gen.md#criteria-files-for-llm-tests).

## Over MCP

An external agent runs the same task through the `verify` serving (see
[task toolsets](../tools.md#task-toolsets)): `verification_tasks`, then per llm row
`begin_verification` (the package carries the same criteria, context, files, and
instructions) and `record_verdict` with evidence. `run_tests` covers the
programmatic rows. The agent reads the deliverable with its own tools.

## Finish

The verdict lands in the [ledger](../../consumers/gen.md#the-ledger) with the
evidence and the fact hash it judged. A stale fact hash keeps the row pending by
derivation: verifying an old statement never launders a changed one.
