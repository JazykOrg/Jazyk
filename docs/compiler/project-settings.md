# Project settings

A directory containing `jazyk.toml` is a Jazyk project. The file marks the project root,
and all globs resolve relative to it. The CLI walks up from the current directory to find
it. The schema is [`project-settings.schema.yaml`](./project-settings.schema.yaml).

## Redirect

A `jazyk.toml` may contain only a redirect, pointing discovery at a nested directory:

```toml
redirect = "docs"
```

Discovery that lands on a redirecting file continues into the named directory and loads
the project there. This lets a repository root delegate to the directory that holds the
real project, so tools launched from the root (editors, MCP clients) resolve the same
project as tools launched inside it. Redirects do not chain: the target must hold a real
`jazyk.toml`.

A redirect applies where it stands. Discovery starting in the redirecting directory
follows it. Discovery walking up from a subdirectory stops at the redirecting file
instead of following it: the redirect declares that the tree's project lives in the
target, so a subdirectory outside the target is its own place, and commands run there
fall back to an ad hoc project at the working directory.

## Docs

### Glob

`docs.glob` is an ordered list of glob patterns selecting the documentation files. A
pattern starting with `!` excludes. Later patterns override earlier ones: a file is
included when the last pattern to match it is an inclusion. A matched file with no
[format handler](./parsing.md#format-handlers) yields an `unsupported-format` diagnostic.

```toml
[docs]
glob = ["docs/**/*.md", "!docs/LICENSE.md"]
```

Some paths are never doc input, regardless of the glob:

- The resolved [output directory](../frontends/cli.md) (default `jazyk-out`, or the
  `--out` override). The compiler never reads its own output as source.
- Any directory whose name starts with `jazyk-out` (e.g. a local backup of generated
  output).
- Hidden directories (name starting with `.`), `target`, and `node_modules`.

The [deliverable directory](#generation) is excluded too, but through the glob rather
than unconditionally: an implicit `!<deliverable>/**` pattern runs before the
configured patterns, so a later inclusion whitelists paths back in. With the defaults
(deliverable `.`, glob `docs/**/*.md`) the whole project is excluded as generated
product and the `docs/` tree is included again as source.

### Handlers

Custom [format handlers](./parsing.md#format-handlers) are registered per project. A
handler has a `matcher` (which files it claims) and a `path` (the implementation). Custom
handlers are tried before built-in ones; the first handler to claim a file wins.

```toml
[docs.handlers.drawio]
matcher = "docs/**/*.drawio"
path = "./handlers/drawio.wasm"
```

### Linting

Linting rules are plain English, grouped by the severity they produce. Rules are
evaluated during [review turns](./turns.md#task-types) and the
[checks wave](./reconciler.md#waves). Findings become
[diagnostics](./model/diagnostic.md): `warnings` let `jazyk check` pass, `errors` fail it.

```toml
[docs.linting.rules]
warnings = ["Grammatical errors and spelling mistakes"]
errors = ["Unimplemented or TODO sections"]
```

## LLM

[Turns](./turns.md) call an OpenAI-compatible chat completions endpoint.

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
  same fields as a non-streaming reply. Some providers reject reasoning fields echoed
  back in the message history; on such a rejection the client strips them from outgoing
  messages for the rest of the run. The turn transcript and trace keep the text (see
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
3. Project `[llm]` in `jazyk.toml`.
4. Global config: `~/.jazyk/config.toml` (or `~/.jazyk.toml`).
5. Built-in default.

These settings feed the [embedded agent](../frontends/acp.md#the-embedded-agent).
An external [ACP agent](../frontends/acp.md#agents) brings its own model and ignores
them.

## ACP

The downstream [ACP agent](../frontends/acp.md#agents) that performs AI work. All
optional; the default agent is `embedded`.

```toml
[acp]
agent = "opencode"

[acp.agents.opencode]
command = "opencode"
args = ["acp"]
env = { }
serve_files = false

[acp.agents.codex]
command = "codex-acp"
```

- `agent`: the name of the profile to use. `embedded` is built in: it runs
  `jazyk agent` with `serve_files = true` and needs no profile.
- `[acp.agents.<name>]`: one profile per agent. `command` and `args` launch it;
  `env` adds environment variables; `serve_files` (default `false`) makes jazyk
  serve file and command tools into the agent's sessions, for agents that bring no
  editor of their own.

Like the LLM settings, agent profiles describe the machine as much as the project:
the same `[acp]` table in the global config is the machine default, and values
resolve per field, highest priority first:

1. CLI flag: `--agent`.
2. Environment variable: `JAZYK_ACP_AGENT` (a profile name).
3. Project `[acp]` in `jazyk.toml`.
4. Global config: `~/.jazyk/config.toml` (or `~/.jazyk.toml`).
5. Built-in default: `embedded`.

## Roots

`roots.files` is a glob list (matched like [`docs.glob`](#glob)) naming the root
documents. Roots seed [reconciler scheduling](./reconciler.md#scheduling): the root
document reconciles first, so the core vocabulary exists before other documents need it.
Roots also anchor reachability [checks](./reconciler.md#waves): an entity unreachable
from a root is flagged.

```toml
[roots]
files = ["docs/main.md"]
```

## Generation

Settings for the [generation workflow](../consumers/gen.md). All optional.

```toml
[gen]
deliverable = "../project2"
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

- `worker`: the built-in generation worker. `agentic` (default) runs each entity as a
  [generation turn](./turns.md#generation-turns) with file and command tools;
  `pipeline` keeps the fixed file-reply sequence, for models too weak to drive tools.

- `code`: a glob list (matched like [`docs.glob`](#glob), relative to the
  deliverable) scoping which deliverable files count as implementation for the
  [unclaimed report](../consumers/bind.md#the-unclaimed-report) and for
  [decompilation](../consumers/decompile.md). Default: every file under the
  deliverable minus the standard exclusions (the out directory, hidden directories,
  `target`, `node_modules`, and the docs the glob claims as source).

The project file never says what the deliverable is. The medium is a fact the documents
state, reaching workers through the graph; see
[the deliverable](../consumers/gen.md#the-deliverable).

## Workflow

Defaults for the [control plane](./reconciler.md#the-control-plane). All optional.

```toml
[workflow]
compile = "manual"
generate = "manual"
worker = "agent"
```

- `compile`, `generate`: `manual` (default) or `auto`. `manual` gates the work
  behind a [release](./reconciler.md#modes-and-releases): changes queue, nothing
  acts until approved. `auto` lets a watcher act on changes as they land; it spends
  LLM budget, so it is opt-in. Explicit commands (`jazyk compile`, `jazyk gen`,
  `jazyk watch`) are their own approval and run under either mode.
- `worker`: who acts on a GUI release. `internal`, `agent`, or `any` (default).
  See [dispatch](./reconciler.md#dispatch).

These are defaults; the live values sit in `control.yaml` in the out directory,
where a GUI toggle or CLI flag changes them at runtime without editing the project
file. Deleting `control.yaml` returns to the defaults.

## Limits

[Turn and build budgets](./turns.md#budgets). All optional.

```toml
[limits]
turn_rounds = 24
turn_mutations = 64
context_budget = 24000
build_turn_factor = 3
max_section_chars = 6000
max_doc_sections = 40
max_entity_requirements = 50
```

- `turn_rounds`: maximum message rounds per turn. Default 24.
- `turn_mutations`: maximum staged mutations per turn. Default 64.
- `context_budget`: maximum context pack size in characters. Default 24000.
- `build_turn_factor`: sets the per-build turn cap as
  `build_turn_factor × (dirty documents + touched entities)`. Default 3. See
  [convergence](./reconciler.md#convergence).
- `max_section_chars`: a section body over this size draws `section-too-large`.
  Default 6000.
- `max_doc_sections`: a document with more sections draws `doc-too-large`. Default 40.
- `max_entity_requirements`: an entity with more requirements draws `entity-too-dense`,
  the signal to split the topic into subsections. Default 50. Code generation divides
  dense entities into parts regardless
  ([dense entities](../consumers/gen.md#dense-entities-generate-in-parts)).

## Environment tuning

Run-level knobs are environment variables only, since they tune one run, not the project:

- `JAZYK_MAX_CONCURRENCY`: cap on parallel turns within a level (default 6).
- `JAZYK_MAX_RETRIES`: retries, in addition to the first attempt, for a failed LLM call
  (default 2). A transient transport failure retries after a 5 second pause; a
  rate-limited call waits 20 seconds. Hammering a struggling endpoint only makes it
  worse.
- `JAZYK_MIN_INTERVAL_MS`: minimum gap between request starts to the endpoint
  (default 500). Bounds the request rate even when calls fail fast in a tight loop.
- `JAZYK_TEMPERATURE`: overrides `temperature` (default 0). A negative value omits the
  field for models that only accept their default.
- `JAZYK_READ_TIMEOUT`: seconds to wait for the next byte of one LLM response before
  the call fails (default 300). Bounds stalls: a dead endpoint costs at most the
  timeout times the retries, not an open-ended wait. It does not bound a response
  that keeps streaming; the two knobs below do.
- `JAZYK_MAX_COMPLETION_TOKENS`: cap on one response's completion, sent as
  `max_tokens` on every request (default 4096). This is the loop detector: a small
  model stuck repeating itself hits the cap and the call fails as
  `runaway completion` instead of generating forever. The stream reader enforces the
  same cap on accumulated content, so a server that ignores the field is still
  bounded. An endpoint that rejects the field gets one retry without it, sticky for
  the run, the same fallback contract as `temperature`.
- `JAZYK_CALL_TIMEOUT`: seconds for one whole LLM call (default 600).
  `JAZYK_READ_TIMEOUT` waits for the next byte; this bounds the call even when bytes
  keep arriving.
- `JAZYK_VERBOSE`: when set to a non-empty value other than `0`, emit verbose
  [trace events](./turns.md#trace-events) including full context packs and raw payloads.
- `JAZYK_ACP_IDLE_TIMEOUT`: seconds a [worker session](../frontends/acp.md#worker-sessions)
  may go without an update before jazyk cancels its turn (default 600).
