# ACP, LSP, and the project connection

The same project can be used from jazyk.dev, the CLI, or an IDE in all three modes.
Compiler location and storage are connection settings, not different language-server
or agent semantics. New commands and flags below are illustrative, not implemented.

## Protocol responsibilities

| Interface | Responsibility | Does not replace |
|---|---|---|
| Project API and event stream | Project binding, file revisions, graph/artifact reads, jobs, changesets, reconnect | ACP conversations or LSP language features |
| ACP, Jazyk as agent | IDE/chat actions, tool progress, questions, configured reasoning sessions | Storage consistency, tenancy, or billing |
| ACP, Jazyk as client | Drive embedded or approved downstream agents for compiler batches | Compiler goal derivation, gates, scheduling |
| MCP project tools | Scoped graph/workspace reads, gated edits, optional execution tools | Authorization in the host |
| LSP | Diagnostics, navigation, references, hovers, explicit code actions | Whole-project file synchronization or general model transport |

The account/project binding is established before accepting protocol requests. ACP
or LSP message ids alone confer no authority. Do not overload LSP document changes
as the entire workspace protocol: unopened files, binary artifacts, graph state,
deletions, and durable commit receipts also need representation.

## Local-local

Keep existing `jazyk acp` and `jazyk lsp` stdio entry points. They attach to the
selected local project context or start one when appropriate. The local server
owns workspace reads and graph operations. Accountless use remains possible.

Avoid one independent hidden build engine per frontend. A CLI build and an IDE use
the same project identity, persistent state, and single-writer rules. File URIs map
to the authorized folder, and local policy controls agents and command execution.

## Local-remote

A local connector exposes a standard stdio ACP agent and LSP server to the IDE. It
multiplexes authorized sessions over an outbound authenticated transport to the
remote Jazyk server. jazyk.dev attaches to the same logical project/connection.

- The IDE can keep local `file://` URIs. The connector maps them to project-relative
  logical paths. Server paths and other tenants' ids never become client file paths.
- Forward ACP prompts, progress, cancellation, permission questions, and session ids
  through a scoped protocol adapter. Handle protocol versions and unsupported features.
- Fetch LSP results from the matching remote graph revision, then map locations to
  the client's URIs. Keep passive reads and cached diagnostics usable without credits.
- Scope unsaved buffers to the client session and document version. The first release
  compiles acknowledged saved workspace revisions. Require save or explicit inclusion
  before a remote job reads a dirty buffer; do not silently compile another version.
- Return edits with expected document/workspace versions. A stale quick fix fails
  safely. Local graph/dual-write actions go through the connector commit coordinator.
- Store device credentials in the OS keychain or documented headless fallback. Do
  not embed them in command arguments, project files, or browser localStorage.

The local process remains necessary for filesystem access even when the compiler
runs remotely. A hosted web page cannot automatically read an arbitrary local folder.
Browser filesystem support can be an additional adapter later, not a beta prerequisite.

## Remote-remote

jazyk.dev speaks to the shared project service for the selected cloud/GitHub workspace.
ACP sessions and LSP adapters use virtual file identities and authenticated artifacts.

For desktop IDE access, retain a small local protocol proxy with a selected remote
project binding. A capable IDE extension can expose a virtual workspace/filesystem
provider. Otherwise offer an explicit checkout/export as a local-backed project.
Bare ACP and LSP do not make every IDE a remote file explorer.

Support remote ACP/LSP endpoints as part of the server foundation. Full remote folder
browsing in every IDE remains a client integration task. State capability differences
explicitly; do not promise universal cloud IDE attachment from stdio forwarding alone.

LSP image/card links need authenticated artifact URLs or a connector-managed cache
with client-local URIs. Do not emit `file://` links to the server's filesystem.
Use short-lived artifact access where a client cannot attach authorization headers.

## Downstream agents and virtual tools

Every automated compiler batch remains an ACP session. The embedded agent can call
an approved API provider, while private Codex testing uses its own ACP profile.
The harness still assembles payloads, derives goals, and validates writes.

File tools resolve through the injected workspace. Graph tools resolve through the
project's staged changeset. File and graph authority do not come from an agent's cwd.
If the protocol requires an absolute-looking working directory, provide a documented
virtual alias and translate it at the adapter; it must not select a real shared folder.

Current repository evidence needs reconciliation before implementation:

- The [ACP spec](../../docs/frontends/acp.md#protocol-versions) says file/terminal tools
  should use MCP rather than relying on protocol callbacks.
- [host.rs](../../bootstrap/src/acp/host.rs) currently advertises and serves v1 file
  callbacks. Route these through the same authorized workspace for compatible agents.
- The public [ACP v1 file interface](https://agentclientprotocol.com/protocol/v1/file-system)
  makes file access a negotiated client capability. Its existence does not prove an
  agent has disabled its own native shell/filesystem tools.

Probe each agent for MCP/virtual-file support, cancellation, tool errors, and isolation.
An agent requiring native file or terminal access is an execution-mode agent. It must
run against a materialized isolated workspace for hosted customer work. Do not launch
an unrestricted customer-selected process inside the shared compiler service.

## Execution switch

When a task explicitly starts an execution environment, issue a new environment id
bound to the project, run, and base revision. Agent file/shell tools then operate on
that container. Display the active environment. Graph tools remain bound to the
original project and its gated commit path.

Finish by importing approved changes and evidence through the workspace contract.
Switch tools back to the virtual workspace and revoke container handles. Do not let
some file tools silently read virtual files while others read stale container files.

## Compatibility checks

- Existing local ACP/LSP clients keep working without login.
- Local and remote runs yield equivalent graph changes and diagnostics on a fixture.
- A dirty buffer, stale edit, disconnected connector, or expired login is explicit.
- Cancellation and progress survive proxying. Reconnect cannot attach another tenant's
  conversation or replay another tenant's events.
- Remote diagnostics and artifact links open correctly in the supported IDE client.
- Model work initiated by an explicit action uses the selected route and allowance;
  ordinary navigation and graph reading make no model calls.
