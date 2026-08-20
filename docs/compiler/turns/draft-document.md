# The decompile call

Goal: draft ONE markdown document stating what the code under a released scope
observably does, tests first, every statement tagged with its evidence class
(observed or inferred). The draft is compiler input, held unratified until a human
accepts it. See [decompilation](../../consumers/decompile.md).

## What the model sees

One one-shot completion per scope (no tools on the internal path):

- System prompt: a role line ("You are the decompilation worker of jazyk, a natural
  language compiler."), then the
  [decompile contract](./prompts/decompile-contract.md), then the reply contract:
  "Reply with exactly one file: a line `FILE: <docs-relative path>` (use the
  suggested path unless a better name exists), then the full markdown content."
- User message:

```text
# Scope: {scope}
suggested path: {suggested docs-relative path}
lint rules: {the project's lint rules, as JSON}

## Inventory (tests first)
{per file: path, then its test names and assertions, then signatures}
```

A rejected draft (bad `FILE:` line, empty content) retries once with the rejection
quoted: "Your previous draft was rejected: {error}. Fix exactly that and resubmit."

## Over MCP

An external agent runs the same task through the `decompile` serving:
`decompile_tasks`, `begin_decompile` (the package carries the inventory and the
same contract), reading the code with its own tools, then `submit_draft`. See
[task toolsets](../tools.md#task-toolsets).

## Finish

The draft lands beside the docs, unratified. Binding self-checks it against the
code after the next compile; ratification is the author's call. A bug described
faithfully is a correct draft.
