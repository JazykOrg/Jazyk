# Agent context

Coding agents pick up instructions, MCP servers, and skills from conventional
places in a project and in the user's home directory. Jazyk's
[embedded agent](./acp.md#the-embedded-agent) and its in-process chat do the same,
so a project's own conventions reach every agent that works on it. Jazyk also
keeps one section of the project's `AGENTS.md` current, so every other agent
learns that the project is a jazyk project and how to work with it.

## Layers

A session's context comes from layers. A later layer overrides an earlier one by
name.

| Layer | Native | Hosted |
|---|---|---|
| Built in | the [agent prompt](../compiler/goals/prompts/embedded-agent.md) and the session's jazyk serving | the same |
| Global | `~/.jazyk/AGENTS.md`, `[mcp.servers]` in `~/.jazyk/config.toml`, `~/.agents/skills`, `~/.jazyk/skills` | the host's account and organization settings |
| Project | the files below, read from the working tree | the same files, read from the workspace snapshot |
| Client | the servers the ACP client names in `session/new` | none |

- Jazyk does not read other clients' global files (`~/.claude`, `~/.codex`).
  Their formats belong to those products.
- The hosted profile never reads the host filesystem. Its global layer is
  supplied by the host ([hosted context](#hosted-context)).
- Conversational sessions get every layer. Worker sessions
  ([worker sessions](./acp.md#worker-sessions)) get only the built-in layer: a
  goal batch's prompt, gates, and cost must not change because a project edited
  its instruction files. An external agent that reads `AGENTS.md` on its own
  still reads it in a worker session, so the
  [jazyk section](#the-jazyk-section-of-agentsmd) is written to hold there too.

## Instructions

In each directory from the project root down to the session's working
directory, the first of these files that exists is read:

1. `AGENTS.md`
2. `.agents/AGENTS.md`
3. `CLAUDE.md`
4. `.claude/CLAUDE.md`

- Files are concatenated root first, after `~/.jazyk/AGENTS.md` (native only).
- The [jazyk section](#the-jazyk-section-of-agentsmd) is left out. The jazyk
  serving's own instructions already carry it.
- `@path` imports a file in place. The token starts a line or follows
  whitespace, and it is never read inside a code fence or a code span. The path
  is relative to the importing file, or starts with `~/` (native only). Imports
  nest four deep, a file is imported once, and a hosted import never leaves the
  project. A token naming no file stays as written, e.g. `@types/node`.
- The total is capped at 32 KiB. A longer text is cut at a line boundary with a
  note naming the cap.
- The text enters the session after the agent prompt, framed as the project's
  guidance. The jazyk serving's instructions and tool replies take precedence.

## MCP servers

Servers are read from these sources, a later source overriding an earlier one by
name:

1. `[mcp.servers.<name>]` in `~/.jazyk/config.toml` (native only).
2. `.agents/mcp_config.json`, `.cursor/mcp.json` (`mcpServers`), and
   `.vscode/mcp.json` (`servers`), read when present.
3. `.mcp.json` (`mcpServers`), the canonical project file.
4. `[mcp.servers.<name>]` in `.jazyk/jazyk.toml`.

E.g.:

```toml
[mcp.servers.docs]
url = "https://mcp.example.com/mcp"
headers = { Authorization = "Bearer ${EXAMPLE_TOKEN}" }

[mcp.servers.local-db]
command = "db-mcp"
args = ["--readonly"]
env = { DB_URL = "${DB_URL:-postgres://localhost/dev}" }
```

- An entry with `command` is a stdio server. An entry with `url` (or `httpUrl`)
  is a streamable HTTP server. `type` may say `stdio`, `http`, or
  `streamable-http`. An entry with `disabled = true` or `enabled = false` is
  skipped.
- `${VAR}` and `${VAR:-default}` expand in `command`, `args`, `env`, `url`, and
  `headers`. Natively they read the process environment. Hosted, they read only
  the host's secret store. An unset variable without a default skips the server
  with a warning.
- The name `jazyk` is reserved for the session's own jazyk serving. When the
  client supplies a `jazyk` entry, a discovered one is dropped. A client entry
  wins over a discovered entry of the same name.
- A discovered server that fails to start is skipped with a warning. Session
  creation still fails for a client-supplied server, as before.
- Tool names stay unique within a session. A discovered server whose tool name
  collides with one already offered is skipped with a warning.

### Trust

A repository's configuration is untrusted until the user trusts it. Project
servers (from project files or `jazyk.toml`) and project skills activate only
when trusted:

- `jazyk setup trust` lists the project's servers and skills and records them as
  trusted in `~/.jazyk/trust.toml`, keyed by the project root.
- Each server is trusted by a hash of its entry. An edited entry is untrusted
  again until the next `jazyk setup trust`.
- `jazyk setup --remove trust` revokes the project's trust.
- An untrusted server or skill set is skipped with a warning naming
  `jazyk setup trust`. Instruction files need no trust: they are text the agent
  reads, like any other project file.
- Global entries are the user's own and need no trust.

Warnings go to the agent's standard error and enter the session as context, so
the model can tell the user what was skipped.

## Skills

Skills follow the [Agent Skills](https://agentskills.io) format: a folder holding
`SKILL.md` with `name` and `description` in its front matter.

- Global: `~/.agents/skills/*/SKILL.md`, then `~/.jazyk/skills/*/SKILL.md`
  (native only).
- Project: `.claude/skills/*/SKILL.md`, then `.agents/skills/*/SKILL.md`. A
  project skill overrides a global skill of the same name, and `.agents` wins
  over `.claude`.
- The session lists each skill's name and description. The `read_skill` tool
  returns one skill's `SKILL.md` body and the relative paths of the other files
  in its folder.
- A skill's scripts are never run by jazyk. The agent can only run them with its
  own tools, under its own permissions.
- A `SKILL.md` without `name` or `description` is skipped with a warning.

## Not picked up

- Hooks (`.claude/settings.json`, `.codex/hooks.json`, `.cursor/hooks.json`, and
  the like) are never read. Running them would execute repository commands.
- Permission and settings files of other agents are not read. Jazyk's own
  [permission policy](./acp.md#permissions) governs.
- Slash commands (`.claude/commands`, `.github/prompts`, `.gemini/commands`),
  scoped rules (`.claude/rules`, `.cursor/rules`, `.github/instructions`), and
  subagent definitions are not read yet.

## Hosted context

The hosted profile builds the same layers from the workspace snapshot and from
what the host supplies through `Execution.agent_context`:

- `instructions`: the host's global instruction texts, each with a label, placed
  before the project's.
- `skills`: global skills as name, description, and body.
- `mcp`: remote MCP servers the host already connected, each with its name,
  instructions, and tool catalog, plus one call function. The engine never opens
  a network connection for them.

The host enforces these rules for every remote server:

- HTTPS only. Stdio entries in a project file are listed as unavailable.
- A project file only proposes a server. An administrator enables it, and an
  edited entry is disabled again.
- The address is checked by `agent_context::check_remote_url` before every
  connection: loopback, private, link-local, shared, unique local, multicast, and
  metadata addresses are refused, and so is any host the service owns
  (`jazyk.dev`, `jazyk.org`, their subdomains, and the configured own hosts).
  DNS is resolved once and the connection uses that address. Redirects are not
  followed.
- Every request carries a `Jazyk-Hop` header. A jazyk-hosted MCP endpoint
  refuses any request that carries it, so no loop forms through a relay.
- Credentials come from the host's secret store and are sent only to their own
  server.
- Calls have a timeout, a response size cap, and a per-tenant rate limit, and
  are recorded in an audit log without secrets.

In the engine, remote server instructions and tool results are framed as data,
not instructions. A remote tool cannot release work: `release_work` acts only on
the person's request. A remote tool whose name collides with a jazyk tool is
dropped. Hosted skills are text: their scripts never run on the host.

## The jazyk section of AGENTS.md

Jazyk keeps one marked section in the project's `AGENTS.md`:

```markdown
<!-- JAZYK:BEGIN -->
...
<!-- JAZYK:END -->
```

The text between the markers is the
[project agents payload](../compiler/goals/prompts/project-agents.md) with the
project's document sets filled in. It tells any agent that this is a jazyk
project, which tools to use, and what jazyk manages. It holds no workflow: the
jazyk serving's instructions, each `begin_*` reply, and each tool description
carry that, and the section says they take precedence.

### When it is written

- Create: [`jazyk init`](./cli.md#jazyk-init), `init_project` over MCP, `/init`
  in the [IDE proxy](./acp.md#the-ide-proxy), `jazyk setup agents-md`, and a new
  [hosted](../compiler/hosted-runtime.md) project. A missing `AGENTS.md` is created holding the section. An
  `AGENTS.md` without the markers gets the section appended.
- Update: every command that opens a project to work on it (`compile`, `watch`,
  `gen`, `test`, `gui`, `acp`, `agent`, `mcp`, `setup`) and every jazyk write to
  `jazyk.toml`. Only the text between the markers is replaced, and only when it
  differs. Read-only commands (`status`, `explain`, `preview`, `query`) never
  write it.
- A local stamp in `.jazyk/agents-md.stamp` records the binary version and the
  configuration hash of the last check, so an unchanged project is not reread.
- Writes are atomic. Everything outside the markers stays byte for byte,
  including line endings. Each change prints one line on standard error, e.g.
  `jazyk: updated the jazyk section in AGENTS.md`.

### Opting out

- Deleting the section opts out: an update never adds it back.
- `[agents] block = false` in `jazyk.toml` stops every create and update.
- `jazyk setup --remove agents-md` deletes the section, and the file too when
  nothing else is left in it.
- Markers that are duplicated, out of order, or unclosed leave the file untouched
  with one warning.

### CLAUDE.md and GEMINI.md

Claude Code reads `AGENTS.md` only where no `CLAUDE.md` exists. When a project's
`CLAUDE.md` (or `.claude/CLAUDE.md`, or `GEMINI.md`) exists and does not already
import `AGENTS.md`, jazyk appends the import (`@../AGENTS.md` from
`.claude/CLAUDE.md`):

```markdown
<!-- JAZYK:BEGIN -->
@AGENTS.md
<!-- JAZYK:END -->
```

- An existing import (`@AGENTS.md`, `@./AGENTS.md`, or any path resolving to it)
  counts, outside code.
- A file that is a symbolic link to `AGENTS.md`, or the same file, is left alone.
- No file is created: an agent that finds no `CLAUDE.md` reads `AGENTS.md`.
- The import follows the section's lifecycle: it is appended on create and
  refreshed on update, and deleting it opts that file out.
- An `AGENTS.md` that is a link (e.g. to `CLAUDE.md`) is written through the
  link, so it stays a link.
- The same opt-outs apply.

### Not compiler input

Agent instruction files (`AGENTS.md`, `AGENTS.override.md`, `CLAUDE.md`,
`CLAUDE.local.md`, `GEMINI.md`) are matched only by a document set pattern that
names the file without a wildcard. A `**/*.md` set does not compile them
([document sets](../compiler/document-sets.md#project-configuration)).
