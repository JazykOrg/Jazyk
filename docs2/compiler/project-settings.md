# Project settings

A directory containing `jazyk.toml` is a Jazyk project. The file marks the project root,
and all globs resolve relative to it. The CLI walks up from the current directory to find
it. The schema is [`project-settings.schema.yaml`](./project-settings.schema.yaml).

## Redirect

A `jazyk.toml` may contain only a redirect, pointing discovery at a nested directory:

```toml
redirect = "docs2"
```

Discovery that lands on a redirecting file continues into the named directory and loads
the project there. This lets a repository root delegate to the directory that holds the
real project, so tools launched from the root (editors, MCP clients) resolve the same
project as tools launched inside it. Redirects do not chain: the target must hold a real
`jazyk.toml`.

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
  client switches to streaming for the rest of the run.
- `model`: the model id.
- `api_key_env`: the environment variable holding the API key. A literal `api_key` may be
  given instead. Prefer `api_key_env` in tracked files.
- `temperature`: sampling temperature. Default 0. Some models only allow their own
  default and reject the parameter; on such a rejection the client retries once without
  it and stops sending it for the rest of the run.

The endpoint, model, and credentials describe the machine, not the project, so their
recommended home is a global config at `~/.jazyk/config.toml` (or `~/.jazyk.toml`) with
the same `[llm]` table. Effective values resolve per field, highest priority first:

1. CLI flag: `--llm-base-url`, `--model`, `--api-key`.
2. Environment variable: `JAZYK_LLM_BASE_URL`, `JAZYK_MODEL`, `JAZYK_API_KEY`.
3. Global config: `~/.jazyk/config.toml` (or `~/.jazyk.toml`).
4. Project `[llm]` in `jazyk.toml`.
5. Built-in default.

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
lang = "rust"
```

- `deliverable`: the directory the end product is generated into, resolved relative to
  the project root. Default `<out>/gen/deliverable` when unset, so the workflow runs
  without configuration. Generation metadata (the ledger, criteria files) always stays
  in the out directory; only the product lands here.
- `lang`: a freeform hint passed to generation tasks (a language, a format, a genre).
  Default `rust`. `--lang` overrides per run.

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
- `JAZYK_READ_TIMEOUT`: seconds to wait on one LLM response before the call fails
  (default 300). Bounds runaway calls: a stalled endpoint costs at most the timeout
  times the retries, not an open-ended wait.
- `JAZYK_VERBOSE`: when set to a non-empty value other than `0`, emit verbose
  [trace events](./turns.md#trace-events) including full context packs and raw payloads.
