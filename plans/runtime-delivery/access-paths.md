# Cloud, local MCP, and ACP access

Status: required access paths, explicitly confirmed by the owner. The transport
and the location of execution are separate choices. All paths use the same
[finite-run contract](runtime.md) and [tool definitions](agent-contract.md).

## Access matrix

| Entry point | Model/tools | Run host | Result presentation |
| --- | --- | --- | --- |
| Cloud editor and hosted agent | Shared registry invoked through the hosted engine | Authenticated Cloud execution service | Runs beside Files, logs and retained changes in existing tabs. |
| Local MCP inside a coding agent | Jazyk MCP execution tools | Local adapter by default; explicitly configured hosted service optionally | Structured tool results, log/search tools, retained diff/artifact handles. |
| ACP inside an IDE | `jazyk acp` proxy injects the shared tools into the downstream agent | Local adapter by default; explicitly configured hosted service optionally | ACP tool-call progress and results, paths/diffs/links supported by that IDE. |

Using MCP or ACP does not imply running on the IDE machine. Choosing Cloud execution
does not require using the Cloud editor. Host discovery reports actual execution
location, capabilities, grants, limits, and whether Cloud usage will be charged.
Accountless local execution remains supported. Hosted execution needs explicit
Cloud authentication and allowance regardless of the client.

## Common request and evidence contract

A submitted action resolves project/set identity, staged or committed revision,
script/configuration/image hashes, selection, retention rules, resource size, timeout,
actor, grant scope, and operation ID. All transports return the same durable run ID.
Observe, log/search, read changes, and cancel address that run rather than a live
process handle owned by the transport.

The same engine validates settings, file ownership, verification, and imports.
Adapters handle transport and policy, not another definition of a passing test.
Artifacts are binary-capable handles with content hashes, not huge encoded tool
messages. Local paths and hosted download handles differ in presentation, while
the evidence manifest and access checks remain the same.

## Local MCP

Extend the existing MCP registry/toolsets to expose finite-run operations to
external agents. The current generation MCP contract often relies on the agent's
own file/shell tools; this is an explicit change to that contract. Execution tools
must not depend on the `serve_files` fallback being enabled. Existing local file
tools can still edit the project between runs.

Read-only connections can inspect permitted history but cannot mutate definitions,
submit, or cancel runs. Add an explicit execution capability/toolset in the accepted
design. Do not treat a read-only invocation as permission to spend runtime resources.
Normal generation/verification sessions receive the relevant execution capability.

Freeze a local snapshot before dispatch. Concurrent native agent writes affect the
next candidate, not the running one. An agent's own shell tools remain outside
Jazyk's runtime contract; prompts direct evidence-producing builds/tests through
Jazyk runs. Never silently accept a claimed host-shell result as an isolated receipt.

## ACP inside IDEs

Use the existing [ACP proxy and tool injection](../../docs/frontends/acp.md#roles).
`jazyk acp` acts as the IDE-facing agent and drives the downstream agent with Jazyk
tools, commonly injected through MCP. A second ACP-specific runtime or prompt set
is unnecessary. The IDE does not itself need MCP support.

Report run submission, queued/running steps, and completion using existing
`tool_call`/`tool_call_update` handling. Bounded `wait_run` responses avoid current
tool transport idle limits. Reconnect reads durable state by run ID. IDE support
for rich diffs or resource links is optional; provide a bounded text diff and stable
file/artifact reference as the baseline.

Preserve existing ACP permission decisions and goal/session boundaries. A publication
grant is checked by the run host even if the IDE approved a tool invocation. Neither
the IDE nor a downstream agent receives provider or publication credentials merely
to observe a run. No ACP terminal capability is required or used to attach to it.

Local input normally means saved disk files. If an IDE integrates unsaved buffers,
it must pass an explicit staged revision; otherwise prompt for save or clearly state
that the run tests saved content. Imports check the same revision/ownership rules
and must not overwrite newer disk edits or dirty editor buffers. The IDE adapter
surfaces a reviewable diff on conflict instead.

## Lifecycle across connections

Submitting a run gives its full fixed instructions to a supervisor independent
of UI/model transport. Closing a browser tab or dropping MCP/ACP connectivity does
not wait for a future model command and cannot leave an idle environment. The run
finishes or reaches its deadline, collects available results, and shuts down.

Explicit cancellation is different from a lost connection. `cancel_run` always
targets one authorized run. An ACP `session/cancel` or compiler-job stop cancels
active child runs owned by that operation. An independently submitted user delivery
is not cancelled merely because an observing chat/IDE session stops. Record parent
ownership so the choice is deterministic. Historical state survives either case.

The local implementation also needs durable records and supervised lifetime outside
the MCP stdio session. Use a bounded local worker plus container deadline/watchdog,
or an equivalent host-owned job supervisor; do not rely on the agent process to
issue the final stop. The Cloud service owns its corresponding durable supervisor.
All paths reconcile unknown shutdowns and expose incomplete results honestly.

## Contract tests

Run the same fixtures through Cloud API, local MCP, and ACP proxy paths. Compare
resolved inputs, outcomes, logs, file diffs, and verification receipts. Include
cancellation, disconnect/reconnect, duplicate submissions, concurrent edits, missing
runtime capability, and timeout teardown. Test ACP with a minimal IDE client that
has no terminal or rich-diff support. Verify that a local client selecting hosted
execution uses the same account admission and billing as the Cloud editor.
