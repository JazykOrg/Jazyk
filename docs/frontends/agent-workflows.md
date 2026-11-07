# Agent workflows

One conversational policy applies to external coding agents, ACP agents, local
chat, and Cloud chat. The agent can answer questions and change code. Its code
changes stay bound to the project's documented requirements.

## Main prompt

[`agent-main.mustache`](../compiler/goals/prompts/agent-main.mustache) is the single
main policy template. The Mustache renderer fills in the current document sets.
The rendered policy is identical in the managed `AGENTS.md` section and MCP
`initialize.instructions` for the same project configuration. Serving capabilities
and integration metadata follow it as separate context. They describe available
operations and do not introduce a second policy.

The main policy contains rules needed on every turn:

- Follow the current request and preserve its scope and authorization.
- Treat prose as intended behavior and implementation as observed behavior.
- Keep code within requirements, or update the docs first for an authorized
  behavior change. Do not invent requirements to justify an implementation.
- Inspect current evidence, preserve unrelated work, and use current hashes.
- Surface ambiguity and contradictions. Never edit compiler state by hand.
- Load a workflow skill before its procedure. Follow active packages and gates.
- Report saved changes, evidence, remaining synchronization, and publication
  separately. A test pass does not establish coverage or deployment.

Task execution instructions live in the `execute-task` skill. The task runtime
and worker packages embed that same skill body when it is needed.
Runtime mechanics remain in the embedded agent contract. Worker contracts and
`begin_*` replies retain batch-specific scope, gates, and evidence. They do not
replace the conversational policy with a separate copy.

## Deferred skills

Built-in workflow skills live in [`skills/agent/`](../compiler/skills/agent/).
They are embedded into the compiler. `list_jazyk_skills` lists their stable names,
descriptions, required capabilities, and source paths. `read_jazyk_skill` returns
one exact Markdown body. Both tools are read-only, require no document set or
graph, and remain available during a batch and in restricted servings.

A skill being readable does not make its actions available or authorized. The
serving's actual tool catalog and tool gates decide availability. User, project,
and account skills continue to use `read_skill`. They cannot override built-in
workflow skill names or replace the shared policy.

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
