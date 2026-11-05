# Project settings

A document set with an open diagnostic answer in `handling` state needs compile.
It remains dirty even when its documents match the graph and no compile goal is
ready. All-dirty compilation includes it so the build can process the answer.

A project is a folder with `.jazyk/jazyk.toml`. Paths resolve from that folder,
not from `.jazyk`. The CLI discovers the nearest config while walking upward.
The schema is [`project-settings.schema.yaml`](./project-settings.schema.yaml).

The [document-set contract](./document-sets.md) defines discovery, independent
stores, matching, management, and execution. The runtime has no legacy config
conversion. A folder without config remains browsable without creating files.

Limits and budgets stay in the binary's [limits registry](./graph.md#limits).

## Document sets

```toml
version = 1

[[sets]]
id = "ds_checkout"
name = "checkout"
docs = ["docs/checkout.md", "docs-checkout/**/*.md"]
roots = ["docs/checkout.md"]
source = ["src/checkout/**", "src/shared/**"]

[[sets]]
id = "ds_search"
name = "search"
docs = ["docs/search.md"]
source = ["src/search/**", "src/shared/**"]
```

Each set has an immutable `ds_` ID and an editable name. Its store is
`.jazyk/<id>/`. The project may contain zero sets. `docs`, `roots`, and `source`
are ordered pattern lists. `source` is optional. Empty means no source mapping.
Project settings are defaults; `[sets.llm]`, `[sets.acp]`, `[sets.executors]`,
`[sets.workflow]`, and `[sets.gen]` override them for one set.

Generation needs a nonempty `source` selection for its implementation and test
files. An explicit generation request authorizes the agent to configure that
selection with the document-set tools. It preserves the set ID, document selection,
and unrelated settings. Patterns may select files that do not exist yet. They must
not include the input documents. Binding discovery names missing source mapping
before criteria prerequisites. After saving the selection, the agent reloads the
board and resolves criteria work before binding.

## Redirect

Root `jazyk.toml` redirects are no longer read. Define the document paths relative
to the containing project in `.jazyk/jazyk.toml`.

## Docs

### Glob

a set's `docs` is an ordered list of glob patterns selecting the documentation files. A
pattern starting with `!` excludes. Later patterns override earlier ones: a file is
included when the last pattern to match it is an inclusion. A matched file with no
[format handler](./parsing.md#format-handlers) compiles as an opaque whole-file input.

```toml
[[sets]]
id = "ds_main"
name = "Main"
docs = ["docs/**/*.md", "!docs/LICENSE.md"]
```

Some paths are never doc input, regardless of the glob:

- The resolved [output directory](../frontends/cli.md) (default `.jazyk`, or the
  `--out` override). The compiler never reads its own output as source.
- Any directory whose name starts with `.jazyk` (e.g. the `.jazyk.bak` archive
  the [store version](./graph.md#store-version) check leaves behind) or with
  `jazyk-out` (the out directory's old name, left where it is).
- Hidden directories (name starting with `.`), `target`, and `node_modules`.
- A file the [ledger](../consumers/gen.md#the-ledger) records as a test artifact under
  the deliverable (an `llm` row's criteria file): it is generation metadata, never a
  document, whatever the glob says. An erp deliverable's `tests/crit_*.md` criteria
  files, pulled in by a `**/*.md` glob, once reconciled as documents and dropped the
  project's coverage to 40 percent.

The [deliverable directory](#generation) is excluded too, but through the glob rather
than unconditionally: an implicit `!<deliverable>/**` pattern runs before the
configured patterns, so a later inclusion whitelists paths back in. With the defaults
(deliverable `.`, glob `docs/**/*.md`) the whole project is excluded as generated
product and the `docs/` tree is included again as source.

Prompt and skill payloads are excluded from the glob by the project that hosts them:
they are instructions to a model, not prose about the subject
([the prompt](./sessions.md#the-prompt)).

### Handlers

Custom [format handlers](./parsing.md#format-handlers) are registered per project. A
handler has a `matcher` (which files it claims) and a `path` (the implementation). Custom
handlers are tried before built-in ones; the first handler to claim a file wins.

```toml
[handlers.drawio]
matcher = "docs/**/*.drawio"
path = "./handlers/drawio.wasm"
```

### Linting

Linting rules are plain English, grouped by the severity they produce. Rules are
evaluated by [`review-entity`](./goals/review-entity.md) sessions and by the
[checks](./compilation.md#checks). Findings become
[diagnostics](./model/diagnostic.md) under the `lint` rule: `warnings` let `jazyk check`
pass, `errors` fail it.

```toml
[linting.rules]
warnings = ["Grammatical errors and spelling mistakes"]
errors = ["Unimplemented or TODO sections"]
```

## LLM

The [embedded agent](../frontends/acp.md#the-embedded-agent) calls an OpenAI-compatible
Chat Completions or Responses endpoint.

```toml
[llm]
base_url = "http://localhost:11434/v1"
model = "llama3.1"
api_key_env = "JAZYK_API_KEY"
temperature = 0
```

- `base_url`: the base URL of the selected API. Native requests use streaming.
  Answer, reasoning, and summary deltas remain separate. Only a complete response
  supplies executable tool calls. The client never changes APIs after submission.
- `model`: the model id.
- `api_key_env`: the environment variable holding the API key. A literal `api_key` may be
  given instead. Prefer `api_key_env` in tracked files.
- `temperature`: sampling temperature. Default 0. A negative value omits the field.
  Some models require their default temperature.

The endpoint, model, and credentials describe the machine, not the project, so their
recommended home is a global config at `~/.jazyk/config.toml` (or `~/.jazyk.toml`) with
the same `[llm]` table. The global config is the machine default; a project `[llm]`
overrides it, so a project that names its own model wins. Effective values resolve per
field, highest priority first:

1. CLI flag: `--llm-base-url`, `--model`, `--api-key`, `--llm-api`, `--reasoning-summary`.
2. Environment variable: `JAZYK_LLM_BASE_URL`, `JAZYK_MODEL`, `JAZYK_API_KEY`,
   `JAZYK_LLM_API`, `JAZYK_REASONING_SUMMARY`.
3. Set `[sets.llm]`, then project `[llm]` in `.jazyk/jazyk.toml`.
4. Global config: `~/.jazyk/config.toml` (or `~/.jazyk.toml`).
5. Built-in default.

These settings feed the [embedded agent](../frontends/acp.md#the-embedded-agent).
An external [ACP agent](../frontends/acp.md#agents) brings its own model and ignores
them.

## ACP

The downstream [ACP agent](../frontends/acp.md#agents) that runs
[sessions](./sessions.md#execution). All optional; the default agent is `embedded`.

```toml
[acp]
agent = "opencode"

[acp.agents.opencode]
command = "opencode"
args = ["acp"]
env = { }
serve_files = false

[acp.agents.codex]
command = "npx"
args = ["--yes", "@agentclientprotocol/codex-acp@2.4.0"]
```

- `agent`: the name of the profile to use. `embedded` is built in: it runs
  `jazyk agent` with `serve_files = true` and needs no profile. `claude`, `codex`,
  and `opencode` are built in too ([agents](../frontends/acp.md#agents)).
- `[acp.agents.<name>]`: one profile per agent. `command` and `args` launch it;
  `env` adds environment variables; `serve_files` (default `false`) makes jazyk
  serve the [file and command tools](./goals/generate.md#file-and-command-tools) into
  the agent's `generate` sessions, for agents that bring no editor of their own.

Like the LLM settings, agent profiles describe the machine as much as the project:
the same `[acp]` table in the global config is the machine default, and values
resolve per field, highest priority first:

1. CLI flag: `--agent`.
2. Environment variable: `JAZYK_ACP_AGENT` (a profile name).
3. Project `[acp]` in `jazyk.toml`.
4. Global config: `~/.jazyk/config.toml` (or `~/.jazyk.toml`).
5. Built-in default: `embedded`.

The `[acp]` agent runs every session unless an [executor](#executors) override names
another profile for the session's goal kind or class.

## Executors

The `[executors]` table overrides the `[acp]` agent per goal kind or per goal class, so
extraction can run on a cheap agent while GC judgment runs on the strongest one
available ([executors](./control-plane.md#executors)). All optional.

```toml
[acp]
agent = "embedded"

[executors]
gc = "claude-code"               # every GC goal kind
reconcile-section = "embedded"   # one compile goal kind
```

- A key is a goal kind that runs in a session (`place-anchors`, `reconcile-section`,
  `rejudge-pair`, `review-entity`, `retrace`, `conform-instance`, `bind`, `generate`,
  `verify`, `declare-edges`, `dedupe-candidates`, `curate-view`, `split-view`,
  `abstract-entity`; see [goal derivation](./reconciler.md#goal-derivation)) or a goal class
  (`compile`, `gc`). Any other key is a settings error naming the accepted keys.
- A value is a profile name: `embedded`, or a `[acp.agents.<name>]` profile. A value
  naming no profile is a settings error naming the profiles that exist.
- `ratify` and `answer` run no session and take no executor.

The executor for one batch resolves in this order, first match wins:

1. CLI flag: `--agent`, one profile for every session of the run.
2. Environment variable: `JAZYK_ACP_AGENT`.
3. `[executors].<kind>` for each goal kind in the batch, the project table over the
   global config's.
4. `[executors].<class>` for the batch's goal class, the project table over the global
   config's.
5. The `[acp]` agent, resolved as [above](#acp).

The same `[executors]` table in `~/.jazyk/config.toml` is the machine default; a project
key overrides a global key of the same name. A batch holds goals of one class and one
tier whose kinds all resolve to the same executor; several kinds may share it. The
choice is unambiguous because the scheduler resolves the executor per kind before it
batches ([executors](./control-plane.md#executors),
[batching](./reconciler.md#batching)). Chat sessions, answer sessions, and follow
sessions always use the `[acp]` agent. The resolved profile is recorded on the session's trace and worker file, and
per-kind and per-class token costs in `build.yaml` (`costs`) are what make the choice
informed ([storage layout](./graph.md#storage-layout)). Editing the table is a
[project tool](../frontends/acp.md#project-tools) in chat too.

## Roots

A set's `roots` is a glob list (matched like [a set's `docs`](#glob)) naming the root
documents. Roots seed [readiness](./reconciler.md#readiness): `reconcile-section` goals
order by document link level from the roots, so the core vocabulary exists before other
documents need it. Roots also anchor the reachability [check](./compilation.md#checks):
an entity unreachable from a root is flagged `unreachable-entity`.

```toml
roots = ["docs/main.md"]
```

## Generation

Settings for the [generation workflow](../consumers/gen.md). All optional.

```toml
source = ["project2/**"]
worker = "agentic"
code = ["src/**", "tests/**"]
```

- `deliverable`: the directory the end product is generated into, resolved relative to
  the project root. Default `.`, the project root itself, so the generated product
  lands beside `jazyk.toml` and the workflow runs without configuration. The directory
  is excluded from doc input except where the docs glob whitelists it (see
  [glob](#glob)); the default glob keeps `docs/` as source. Generation metadata (the
  ledger, criteria files) always stays in the out directory; only the product lands
  here.

- `worker`: the built-in generation worker. `agentic` (default) runs each
  [`generate`](./goals/generate.md) goal as a session with file and command tools;
  `pipeline` keeps the fixed file-reply sequence, for models too weak to drive tools.

- `code`: a glob list (matched like [a set's `docs`](#glob), relative to the
  deliverable) scoping which deliverable files count as implementation for the
  [unclaimed report](../consumers/bind.md#the-unclaimed-report) and for
  [decompilation](../consumers/decompile.md). Default: every file under the
  deliverable minus the standard exclusions (the out directory, hidden directories
  except the [allowed project scaffolding](./document-sets.md#matching-and-file-roles),
  `target`, `node_modules`, and the docs the glob claims as source).

The project file never says what the deliverable is. The medium is a fact the documents
state, reaching workers through the graph; see
[the deliverable](../consumers/gen.md#the-deliverable).

## Workflow

Defaults for the [control plane](./control-plane.md). All optional.

```toml
[workflow]
compile = "manual"
generate = "manual"
worker = "agent"
```

- `compile`, `generate`: `manual` (default) or `auto`. `manual` gates the work
  behind a [release](./control-plane.md#modes-and-releases): goals wait on the board,
  nothing acts until approved. `auto` lets a watcher act on changes as they land; it
  spends LLM budget, so it is opt-in. Explicit commands (`jazyk compile`, `jazyk gen`,
  `jazyk watch`) are their own approval and run under either mode.
- `worker`: who acts on a GUI release. `internal`, `agent`, or `any` (default).
  See [dispatch](./control-plane.md#dispatch).

These are defaults; the live values sit in `control.yaml` in the out directory,
where a GUI toggle or CLI flag changes them at runtime without editing the project
file. Deleting `control.yaml` returns to the defaults.

## Agents

How jazyk treats the project's agent instruction files
([agent context](../frontends/agent-context.md)).

```toml
[agents]
block = false
```

- `block` (default `true`): keep the
  [jazyk section of `AGENTS.md`](../frontends/agent-context.md#the-jazyk-section-of-agentsmd)
  and the `CLAUDE.md` and `GEMINI.md` imports. `false` stops every create and
  update. Nothing already written is removed.

## MCP servers

Extra MCP servers for the embedded agent's conversational sessions, alongside the
project's `.mcp.json` ([MCP servers](../frontends/agent-context.md#mcp-servers)).
The same table in `~/.jazyk/config.toml` is the user's global list.

```toml
[mcp.servers.docs]
url = "https://mcp.example.com/mcp"
headers = { Authorization = "Bearer ${EXAMPLE_TOKEN}" }

[mcp.servers.local-db]
command = "db-mcp"
args = ["--readonly"]
```

- `command`, `args`, `env`: a stdio server.
- `url`, `headers`: a streamable HTTP server.
- `enabled = false` skips the entry.
- Secrets belong in environment variables, referenced as `${VAR}`.
- A project entry is used only once the project is
  [trusted](../frontends/agent-context.md#trust).

## Environment tuning

Run-level knobs are environment variables only, since they tune one run, not the project.
They bound the endpoint and the process; session and build budgets are
[registry constants](./sessions.md#budgets), and builds are
[sequential](./control-plane.md#sequential-builds), so there is no concurrency knob.

- `JAZYK_TEMPERATURE`: overrides `temperature` (default 0). A negative value omits the
  field for models that only accept their default.
- `JAZYK_MAX_COMPLETION_TOKENS`: cap on one response's completion, sent as
  `max_tokens` on every request (default 4096). The runtime does not switch codecs,
  remove the cap, or boost a later response. Native responses are bounded to 16 MiB.
- `JAZYK_CALL_TIMEOUT`: seconds for one whole native LLM call (default 600).
  Hosted and browser transports enforce their host's deadline. Session round
  budgets, the ACP idle watchdog and lease TTLs provide additional bounds.
- `JAZYK_COMMAND_TIMEOUT`: seconds one shell command may run (default 600): a recorded
  build or test under `jazyk test`, a session's `run_command`. Past it the harness kills
  the command and records exit `124`
  ([a test that could not run says nothing](../consumers/gen.md#a-test-that-could-not-run-says-nothing)).
- `JAZYK_VERBOSE`: when set to a non-empty value other than `0`, emit verbose
  [trace events](./sessions.md#trace-events) including full loaded sets and raw
  payloads.
- `JAZYK_ACP_IDLE_TIMEOUT`: seconds a [worker session](../frontends/acp.md#worker-sessions)
  may go without an update before jazyk cancels it (default 600).
- `JAZYK_ACP_MCP`: the transport of the serving injected into
  [worker sessions](../frontends/acp.md#worker-sessions), `stdio` or `http`. Unset
  (or `auto`), the choice follows the agent's `initialize` reply: HTTP when it
  advertises `mcpCapabilities.http`, stdio otherwise
  ([MCP over HTTP](../frontends/mcp.md#mcp-over-http)).
- `JAZYK_PLANTUML`: path to the official PlantUML native binary, selecting it as the
  renderer behind the render seam for the process
  ([the renderer](./diagrams.md#the-renderer)). Unset, the in-process renderer draws
  every view.

## Tasks

The project-level `[[tasks]]` list defines finite command execution. See
[task execution](task-runtime.md) for fields, file globs, and host limits.

## Native tool capability

The embedded runtime requires native tool calling. `JAZYK_CODEC` is no longer
read. No run switches to a text tool protocol. An unsupported model receives an
explicit capability error. Older
benchmark and transcript codec labels remain historical data.

## Model API selection

`[llm].api` selects `chat_completions` (default) or `responses`. `--llm-api`
and `JAZYK_LLM_API` use the existing per-field configuration precedence.
`[llm].reasoning_summary`, `--reasoning-summary`, and `JAZYK_REASONING_SUMMARY`
select `auto`, `concise`, or `detailed`. An absent summary setting requests no
summary. Summary support belongs to the exact provider and model. Responses
support does not establish summary support. Invalid settings fail before dispatch.

The transport is selected before submission. A protocol failure does not submit
a second request through another API. Chat Completions remains supported.
Responses uses locally owned history and `store: false`. Complete provider output
items, including encrypted reasoning, survive tool rounds and recovery. They stay
bound to the endpoint and model that produced them. The harness scopes tool ids
locally and restores the original call ids on provider replay.

The shared GDK formats requests and parses complete responses. The harness retains
provider output separately from visible text because the GDK projection can omit
opaque fields. Text, reasoning, and provider summaries have separate event channels.
Summaries are display data, never a substitute for continuation items.
