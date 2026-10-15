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

## Redirect

Root `jazyk.toml` redirects are no longer read. Define the document paths relative
to the containing project in `.jazyk/jazyk.toml`.

## Docs

### Glob

a set's `docs` is an ordered list of glob patterns selecting the documentation files. A
pattern starting with `!` excludes. Later patterns override earlier ones: a file is
included when the last pattern to match it is an inclusion. A matched file with no
[format handler](./parsing.md#format-handlers) yields an `unsupported-format` diagnostic.

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
chat completions endpoint.

```toml
[llm]
base_url = "http://localhost:11434/v1"
model = "llama3.1"
api_key_env = "JAZYK_API_KEY"
temperature = 0
```

- `base_url`: any OpenAI-compatible server. Endpoints that only answer streaming
  responses are handled transparently: on a "stream must be set to true" rejection the
  client switches to streaming for the rest of the run. A transient HTTP failure on a
  non-streaming request also retries the same request over streaming (some proxies fail
  to relay non-streaming tool call responses); if the streaming retry succeeds,
  streaming stays on for the rest of the run. A streamed response is assembled from its
  deltas, including any `reasoning_content` or `reasoning` deltas, so it carries the
  same fields as a non-streaming reply. A tool call delta that carries a new call `id`
  starts a new call even when its `index` repeats: an Ollama endpoint behind a router
  streams each parallel call whole under index 0, and joining their arguments made
  every call of the reply unparseable. Some providers reject reasoning fields echoed
  back in the message history; on such a rejection the client strips them from outgoing
  messages for the rest of the run. The session transcript and trace keep the text (see
  [the embedded agent](../frontends/acp.md#the-embedded-agent)).
- `model`: the model id.
- `api_key_env`: the environment variable holding the API key. A literal `api_key` may be
  given instead. Prefer `api_key_env` in tracked files.
- `temperature`: sampling temperature. Default 0. Some models only allow their own
  default and reject the parameter; on such a rejection the client retries once without
  it and stops sending it for the rest of the run.

The endpoint, model, and credentials describe the machine, not the project, so their
recommended home is a global config at `~/.jazyk/config.toml` (or `~/.jazyk.toml`) with
the same `[llm]` table. The global config is the machine default; a project `[llm]`
overrides it, so a project that names its own model wins. Effective values resolve per
field, highest priority first:

1. CLI flag: `--llm-base-url`, `--model`, `--api-key`.
2. Environment variable: `JAZYK_LLM_BASE_URL`, `JAZYK_MODEL`, `JAZYK_API_KEY`.
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
args = ["--yes", "@zed-industries/codex-acp"]
```

- `agent`: the name of the profile to use. `embedded` is built in: it runs
  `jazyk agent` with `serve_files = true` and needs no profile.
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
per-kind and per-class token costs in `status.yaml` (`costs`) are what make the choice
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
  deliverable minus the standard exclusions (the out directory, hidden directories,
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

## Environment tuning

Run-level knobs are environment variables only, since they tune one run, not the project.
They bound the endpoint and the process; session and build budgets are
[registry constants](./sessions.md#budgets), and builds are
[sequential](./control-plane.md#sequential-builds), so there is no concurrency knob.

- `JAZYK_MAX_RETRIES`: retries, in addition to the first attempt, for a failed LLM call
  (default 2). A transient failure (a transport error, HTTP 429, 502, 503, or 504)
  retries after a 5 second pause; a rate-limited call (HTTP 429) waits 20 seconds.
  Hammering a struggling endpoint only makes it worse.
- `JAZYK_MIN_INTERVAL_MS`: minimum gap between request starts to the endpoint
  (default 500). Bounds the request rate even when calls fail fast in a tight loop.
- `JAZYK_TEMPERATURE`: overrides `temperature` (default 0). A negative value omits the
  field for models that only accept their default.
- `JAZYK_READ_TIMEOUT`: seconds to wait for the next byte of one LLM response before
  the call fails (default 300). Bounds stalls: a dead endpoint costs at most the
  timeout times the retries, not an open-ended wait. It does not bound a response
  that keeps streaming; the two knobs below do.
- `JAZYK_MAX_COMPLETION_TOKENS`: cap on one response's completion, sent as
  `max_tokens` on every request (default 4096). After a reply the cap cut in its
  reasoning with no call made, the embedded agent's next request alone carries twice
  the cap, since a local reasoning model planning a two-section extraction ran past
  4096 tokens on every round of a session and never reached its first write. This is the loop detector: a small
  model stuck repeating itself hits the cap and the call fails as
  `runaway completion` instead of generating forever. The stream reader enforces the
  same cap on accumulated content, so a server that ignores the field is still
  bounded. An endpoint that rejects the field gets one retry without it, sticky for
  the run, the same fallback contract as `temperature`.
- `JAZYK_CALL_TIMEOUT`: seconds for one whole LLM call (default 600).
  `JAZYK_READ_TIMEOUT` waits for the next byte; this bounds the call even when bytes
  keep arriving, streaming or not. Connecting is bounded separately at 15 seconds.
  Every layer above adds its own bound: session round budgets, the ACP idle watchdog
  (`JAZYK_ACP_IDLE_TIMEOUT`), and lease TTLs, so no single stall can hold a build.
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
