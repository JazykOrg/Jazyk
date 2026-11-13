# Agent workflows

One conversational policy applies to external coding agents, ACP agents, local
chat, and Cloud chat. The agent can answer questions and change code. Its code
changes stay bound to the project's documented requirements.

## Main prompt

[`agent-main.mustache`](../compiler/goals/prompts/conversation/agent-main.md.mustache) is the single
main policy template. The Mustache renderer fills in the current document sets.
The rendered policy is identical in the managed `AGENTS.md` section and MCP
`initialize.instructions` for the same project configuration. Serving capabilities
and integration metadata follow it as separate context. They describe available
operations and do not introduce a second policy.

The main policy contains rules needed on every turn:

- Follow the current request and preserve its scope and authorization.
- A request to build a deliverable includes the necessary documentation
  reconciliation, implementation, and local verification within that scope.
  Carry the request through those stages. Report concrete blockers and continue
  independent work. Honor separate cost, human decision, and publication gates.
- Treat prose as intended behavior and implementation as observed behavior.
- Keep code within requirements, or update the docs first for an authorized
  behavior change. Do not invent requirements to justify an implementation.
- Inspect current evidence, preserve unrelated work, and use current hashes.
- Resolve document links from the containing file, then pass project-relative
  paths to project file tools. Check the path before declaring a file missing.
- Surface ambiguity and contradictions. Never edit compiler state by hand.
- Load a workflow skill before its procedure. Follow active packages and gates.
- Report saved changes, evidence, remaining synchronization, and publication
  separately. A test pass does not establish coverage or deployment.

Task execution instructions live in the `execute-task` skill. The task runtime
and worker packages embed that same skill body when it is needed.
Runtime mechanics remain in the embedded agent contract. Worker contracts and
`begin_*` replies retain batch-specific scope, gates, and evidence. They do not
replace the conversational policy with a separate copy.

## File paths

Initialization context names the project filesystem root used by Jazyk. It can
be a local directory or a virtual root in a hosted or browser workspace. It does
not establish an external agent's shell working directory.

Project file tools such as `read_document` and `inspect_input` take paths from
that root. Markdown links resolve from the directory containing the document.
Resolve `.` and `..`, remove URL fragments and queries for a file read, and pass
the resulting project-relative path. A fragment-only link stays in the same file.
External URLs and site routes require their own URL context. Do not treat them
as project file paths. A tool that documents another base, such as a deliverable
directory, keeps that base.

E.g.: in `content/menu-top.md`, `docs/index.md#intro` names
`content/docs/index.md`, and `../docs/index.md` names `docs/index.md`.
A missing read establishes only that the requested path is missing. Check the
containing document and the available file inventory before proposing a new file.

## Deferred skills

Built-in workflow skills live in [`skills/agent/`](../compiler/skills/agent/).
They are embedded into the compiler. Each skill's YAML front matter holds its
name, description, and required capabilities. The main template contains the
reviewable routing table. Tool descriptions live in
[`agent-skill-tools.json`](../compiler/goals/prompts/conversation/agent-skill-tools.json). `list_jazyk_skills` lists their stable names,
descriptions, required capabilities, and source paths. `read_jazyk_skill` returns
one exact Markdown body. Both tools are read-only, require no document set or
graph, and remain available during a batch and in restricted servings.

A skill being readable does not make its actions available or authorized. The
serving's actual tool catalog and tool gates decide availability. User, project,
and account skills continue to use `read_skill`. They cannot override built-in
workflow skill names or replace the shared policy.

## Skill-gated tools

A writable chat serving starts with a small tool catalog and adds a workflow's tools
when the agent reads its skill. Every tool definition travels with every model
request, and a catalog of about 90 tools costs a model both context and accuracy.

- Scope: every writable chat serving without an open batch. That is the embedded
  agent, the hosted and browser conversation, and an external agent connected
  with `jazyk mcp chat` or `jazyk mcp all` (what [agent setup](./mcp.md#agent-setup)
  installs). A reader serving, a worker serving and an open batch keep their own
  catalogs.
- `--all-tools` lists every served tool from the start, for an MCP client that does
  not refresh its tool list on `notifications/tools/list_changed`
  ([`jazyk mcp`](./cli.md#jazyk-mcp-graph), [flags](./mcp.md#mcp-into-acp-sessions)).
- The core catalog is always listed: `project_context`, `list_jazyk_skills`,
  `read_jazyk_skill`, the [read tools](../compiler/tools.md#read-tools), the
  [file tools](../compiler/goals/generate.md#file-and-command-tools)
  (`read_text_file`, `write_text_file`, `list_files`, `search_files`),
  `read_document`, `write_document`, `list_document_sets`, `recent_edits`, and
  `report_feedback`.
- Each skill's front matter lists the tools its procedure uses under `tools`.
  `read_jazyk_skill` adds them to the serving's catalog for the rest of the
  conversation. Its reply names the newly listed tools under `unlocked`, with a
  note for a client that does not refresh. `list_jazyk_skills` shows each skill's
  tools. The serving announces the change with `notifications/tools/list_changed`;
  the embedded agent refreshes its catalog before every model request.
- Related tools travel together: the task tools as one group, `run_tests` with the
  verification tools, the graph write tools with `edit_graph` and `edit_fact`.
- A text names only tools its reader can see. The main policy, the serving
  instructions, the skill catalog and the core tool descriptions name core tools.
  They reach a locked workflow through its skill. A skill body, and the description
  of every tool listed after reading only that skill, name core tools and that
  skill's tools.
- A tool result or error that names a locked tool carries `lockedTools`, each such
  tool with the skills that list it, and `lockedToolsNote`, the instruction to
  read one. A one-word tool name such as `goals` counts only as code: in backticks
  or before `(`.
- Gating changes what is listed, not what is authorized. A served tool that is
  not listed still answers a direct call under its usual gates. The embedded agent
  refuses a call to an unlisted tool before dispatch and names the skill tools as
  the way to list it.
- The read skills are part of the serving's checkpoint, so a restored execution
  keeps its catalog. A hosted conversation also reapplies the skills its history
  read when it reopens its serving.
- Every tool of `jazyk mcp all` is in the core catalog or in at least one skill.

E.g.: a chat turn asked to run the tests reads `execute-task`, and its next model
request lists the task and verification tools.

## Requests and responses

| User request | Skill | Response |
|---|---|---|
| General question | None | Answer directly. No project discovery or write. |
| Explain this project, code, or document | `inspect-project` | Read relevant evidence. Separate intended and observed behavior. |
| Review a change or assess status | `inspect-project` | Report findings without editing or running a build. |
| Edit documentation | `edit-documentation` | Make a targeted, version-checked edit. Do not compile implicitly. |
| Repair code within existing requirements | `change-code` | Find requirements and ownership. Repair, record, and verify the affected scope. |
| Add or change behavior | `change-code`, `edit-documentation` | Update intended behavior in prose first. Reconcile, implement, and verify within authorization. |
| Import or explain undocumented code | `reverse-compile` | Describe observed behavior and tests. Do not turn observation into invented intent. |
| Compile or reconcile docs | `compile` | Use the serving's build path or goal loop. Report convergence and blockers. |
| Generate implementation | `generate` | Follow requirements and criteria. Record the actual deliverable and tests. |
| Bind existing implementation | `bind` | Inspect the code and evidence. Record observed bindings. |
| Run tests or judge criteria | `verify` | Run authorized checks. Separate execution from judgment and record evidence. |
| Run a saved task | `execute-task` | Read current task context. Preserve execution and approval gates. |
| Diagnose a failed tool or workflow | `recover-workflow` | Repair the named failure within scope. Preserve completed evidence. |
| Push, publish, or deploy | `publish` | Use current integration metadata and explicit authority. Verify the actual remote result. |
| Mixed request | Relevant skills | Order docs → graph → code → evidence where those changes are requested and authorized. |
| Missing tools, configuration, or credentials | Relevant skill | Explain the concrete missing prerequisite once. Do not bypass it. |
| Ambiguous or contradictory requirements | `edit-documentation` | Surface the conflict and request a concrete decision before choosing behavior. |

An implementation repair that preserves behavior does not require rewriting the
docs or recompiling an unchanged graph. A behavior change must update the prose.
Reverse compilation can propose documentation for an authorized implementation
change, but cannot silently ratify a deviation from existing requirements.

## Transport parity

- `agents_md` renders the shared policy and adds a Markdown maintenance comment
  immediately after `<!-- JAZYK:BEGIN -->`.
- MCP initializes with the same policy. Its capability context identifies served
  toolsets and restrictions. A reader or browser serving does not gain writes,
  shell access, or generation by reading a skill.
- ACP agents receive the policy and skill tools through their injected MCP
  serving. User instruction files are additional context.
- Local and Cloud chat obtain the policy and skill tools from the same MCP
  initialization path. Cloud supplies the project snapshot, account context,
  integration metadata, and runtime facilities to the shared compiler.
- The compiler strips the managed section when reading the project's instruction
  files, because that policy is already injected. Human rules remain intact.
- Cloud must use a compiler revision containing these changes. A local rebuild
  does not update a deployed Cloud service.

The project settings file is `.jazyk/jazyk.toml`. `[agents] block = false` disables
managed-file writes. It does not disable the compiler's built-in policy or gates.

## Validation

- Compare the rendered policy in `AGENTS.md`, MCP initialization, ACP-facing
  serving initialization, and hosted conversation instructions.
- Check maintenance-comment placement, config-only opt-out, restoration of
  missing files and imports, and preservation of human text and line endings.
- Exercise skill discovery and exact-body retrieval without a graph, across
  multiple sets, in readers, and inside an active batch.
- Check every main-prompt skill reference against the built-in registry.
- Check procedural deferral and capability limits, including document edits
  without implicit compilation and requirements-bound code changes.
- Run local engine tests, the portable check, and a release build. Smoke-test
  actual MCP initialization and skill calls. Report unrelated failures separately.
