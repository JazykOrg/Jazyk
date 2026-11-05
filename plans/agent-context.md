# Plan: project and global agent context

Status: approved for implementation (2026-11-05). Owner decisions: jazyk's own
globals only, the block lists the document sets, embedded worker sessions stay
hermetic.

## Findings

Jazyk today:

- The embedded agent (`bootstrap/src/acp/agent/mod.rs`) reads no instruction
  files, no MCP configuration, and no skills. Its context is the fixed
  `embedded-agent.md` prompt plus the stdio servers named in `session/new` and
  their instructions. It refuses HTTP servers.
- `setup.rs` writes every agent's MCP file and never reads one back.
- The cloud links the engine in process over a snapshot. Nothing is discovered.
- Jazyk's MCP tools edit files: `write_document` (with `baseHash`), the dual
  write tools (`revise_requirement`, `add_requirement`, `retract_requirement`,
  `edit_fact`), `update_project_settings`, and guarded file tools in native and
  hosted chat.

Industry (researched 2026-11-05):

- `AGENTS.md` is the cross-agent standard (Agentic AI Foundation, Linux
  Foundation). Nested, root to cwd, closest wins. Codex, Cursor, Copilot,
  Gemini, Zed, goose, opencode, Factory, Kiro, Devin, Junie and Amp read it.
- Claude Code reads `AGENTS.md` only when no `CLAUDE.md` exists in that
  directory (default mode `claude-md-or-agents-md`). An existing `CLAUDE.md`
  needs an `@AGENTS.md` import.
- Global instruction files are per client: `~/.codex/AGENTS.md`,
  `~/.claude/CLAUDE.md`, `~/.gemini/GEMINI.md`, `~/.config/opencode/AGENTS.md`,
  `~/.copilot/copilot-instructions.md`, `~/.agents/AGENTS.md` (Cline, Factory).
  `~/AGENTS.md` is not a convention.
- Agent Skills (`SKILL.md`, `name` and `description`) is an open standard.
  `.agents/skills` and `~/.agents/skills` are near universal, `.claude/skills`
  is the other common path. Project skills override user skills and load only
  for trusted projects.
- MCP client configuration has no standard. SEP-2633 (`mcp.json`,
  `mcpServers`) is a draft. `.mcp.json` with `mcpServers` is converging (Claude
  Code, VS Code experimental). Cursor, Codex, Gemini and Zed read only their own
  files.
- Agents also discover slash commands, subagents, hooks, scoped rules,
  settings and permissions, ignore files, plugins, and output styles.

## Layers

One module, `agent_context.rs`, resolves a session's context from layers. Later
layers win.

| Layer | Local | Cloud |
|---|---|---|
| Built in | the jazyk contract and the session's jazyk serving | the same |
| Global | `~/.jazyk/AGENTS.md`, `~/.jazyk/config.toml` `[mcp.servers]`, `~/.agents/skills`, `~/.jazyk/skills` | account and organization settings from the Cloud database, never the host filesystem |
| Project | working tree files | the same files read from the snapshot |
| Client | servers the IDE sends in `session/new` | none |

- Other clients' globals (`~/.claude`, `~/.codex`) are not read by default.
- Chat sessions get every layer. Embedded worker goal sessions get only the
  built-in layer. External executors read `AGENTS.md` themselves, so the jazyk
  block must be safe in a worker.
- Trust (local): project MCP servers and project skills activate only after a
  per-project trust record in `~/.jazyk/trust.toml`, keyed per server by a hash
  of its configuration. Instruction text needs no trust. A non-interactive
  start skips untrusted servers and says so.

## Inventory

| Category | Project paths | Decision |
|---|---|---|
| Instructions | `AGENTS.md` nested root to cwd (also `.agents/AGENTS.md`); `CLAUDE.md` or `.claude/CLAUDE.md` when a directory has no `AGENTS.md` | Yes. `@path` imports (relative, 4 hops, not in code spans, nothing outside the project in the cloud). 32 KiB cap. The jazyk block is skipped. |
| MCP servers | `.mcp.json` (`mcpServers`), `jazyk.toml` `[mcp.servers.<name>]`; fallbacks `.vscode/mcp.json` (`servers`), `.cursor/mcp.json`, `.agents/mcp_config.json` | Yes. Stdio and streamable HTTP locally, HTTPS only in the cloud. `${VAR}` expansion. `jazyk` is reserved. |
| Skills | `.agents/skills/*/SKILL.md`, `.claude/skills/*/SKILL.md` | Yes. Names and descriptions in the prompt, `read_skill` returns the body. Scripts never run automatically. |
| Slash commands | `.claude/commands`, `.github/prompts`, `.gemini/commands` | Later. |
| Scoped rules | `.claude/rules`, `.cursor/rules`, `.github/instructions` | Later. |
| Subagents | `.claude/agents`, `.github/agents` | No. |
| Hooks | settings files | Never. Code execution from a repository. |
| Permissions | `.claude/settings*.json` and similar | No. Jazyk's policy governs. |
| Ignore files | `.gitignore`, `.aiignore`, `.geminiignore` | Later, for chat file tools. |

## Cloud

- Global layer: organization, workspace and user settings in the Cloud
  database, passed through `Execution.agent_context`. The engine merges them
  with the project layer from the snapshot.
- MCP: HTTPS only. Stdio entries are listed as unavailable. Each server is
  enabled by an admin; a project file only proposes it, and a changed hash
  disables it again.
- SSRF: resolve and pin DNS, re-check on connect; deny loopback, private,
  link-local, CGNAT, ULA, multicast and metadata addresses; no redirects;
  standard ports; one egress proxy with no route into the VPC.
- No self links: deny `jazyk.dev`, `*.jazyk.dev`, `jazyk.org`, `*.jazyk.org`,
  the configured own hosts, and addresses of Cloud's own load balancers. Every
  outbound MCP request carries `Jazyk-Hop`; any jazyk-hosted MCP endpoint
  refuses it.
- Credentials: encrypted per-organization or per-user secrets, OAuth where
  offered. `${VAR}` resolves only from that store. No cross-host forwarding.
- Untrusted content: server instructions and tool results are framed as data.
  Write or unannotated tools need per-call approval. A tool result never
  authorizes `release_work`.
- Limits: per-call timeout, response and catalog caps, per-tenant rate limits,
  owner-fenced cancellation, an audit log without secrets.
- Snapshot skills and instructions are text only. Scripts never run on the host.

## Server assembly

A project `.mcp.json` entry cannot replace the injected serving. Workers need
per-session flags a static file cannot carry (`--ephemeral`, `--only`,
`--build-token`, `--packaged`, `--cancel-file`, `--edit-sink`, the toolset per
kind, the transport chosen from `initialize`). Chat needs `--ephemeral`,
`--cancel-file`, `--edit-sink` and the `chat` toolset. Cloud and fresh GUI
sessions have no file.

Discovery lives in the embedded agent, not in the clients that open sessions.
External agents (Claude Code, Codex) already read their own configuration, so a
client injecting discovered servers would hand them duplicates. The embedded
agent connects the client's servers (all must connect), then adds discovered
servers outside worker sessions: a client name wins, a discovered `jazyk` is
dropped when the client supplied one, and a discovered server that fails,
collides, or is untrusted is skipped with a warning. The embedded agent accepts
HTTP entries but does not advertise MCP over HTTP, so jazyk's own sessions keep
the stdio serving.

## Setup

- `.mcp.json` is written always, as the canonical project entry.
- Per-agent files are written only for agents that do not read `.mcp.json`.
- Each setup row carries `reads_mcp_json`. When an agent adopts the standard,
  its private file is no longer written.

## The AGENTS.md section

Lifecycle:

- Markers `<!-- JAZYK:BEGIN -->` and `<!-- JAZYK:END -->`, no version. The text
  between them is compared, so the file changes only when the text changes.
- Create: `jazyk init`, MCP and ACP `/init`, `jazyk setup`, Cloud project
  creation. No `AGENTS.md`: create it. One without markers: append.
- Update: commands that open a writable project (`compile`, `watch`, `gui`,
  `acp`, `mcp`, `gen`, `setup`) and every jazyk configuration write. Read-only
  commands never write. A local stamp skips the comparison per binary version
  and configuration hash.
- Opt out: delete the block (updates never re-add it), or `[agents] block =
  false`. `jazyk setup agents-md --remove` strips it.
- Broken markers: leave the file alone and warn.
- Atomic writes, line endings and everything outside the markers preserved,
  one stderr line.
- `CLAUDE.md`, `.claude/CLAUDE.md` and `GEMINI.md` shim: when the file exists
  and does not import `AGENTS.md`, append a marked block holding the relative
  import. Same lifecycle as the section. Skip a link to `AGENTS.md`. An
  `AGENTS.md` that is itself a link is written through the link.
- Never compiler input: agent instruction files are matched only by a literal
  pattern, so `**/*.md` sets skip them.

Content: orientation and pointers only, no workflow. It defers to the MCP
server instructions and tool replies, never asks the user, and never tells an
agent to compile. Payload `docs/compiler/goals/prompts/project-agents.md` with
a `{SETS}` placeholder rendered from `jazyk.toml`. Tool names it mentions are
checked against the registry in tests. The MCP instructions state the same
precedence.

## Work order

1. Docs: `docs/frontends/agent-context.md`, `acp.md`, `mcp.md#agent-setup`,
   `cli.md`, `project-settings.md`, `hosted-runtime.md`, `parsing.md`, the
   payload.
2. The block lifecycle, the shim, parse stripping, the stamp.
3. Server assembly and HTTP MCP in the embedded agent.
4. Local discovery of instructions, MCP servers and skills, the trust store.
5. Setup with canonical `.mcp.json`.
6. Engine support for the cloud: `Execution.agent_context`, snapshot discovery,
   the remote MCP URL policy. Cloud storage, admin enabling, the egress proxy and
   the secret store land in jazyk-cloud.
7. Later: slash commands, scoped rules, ignore files.
