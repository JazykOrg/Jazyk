# Project settings

A directory containing `jazyk.toml` is a Jazyk project. The file marks the project root,
and all globs resolve relative to it. The CLI walks up from the current directory to find
it. The schema is [`project-settings.schema.yaml`](./project-settings.schema.yaml).

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

- `base_url`: any OpenAI-compatible server.
- `model`: the model id.
- `api_key_env`: the environment variable holding the API key. A literal `api_key` may be
  given instead. Prefer `api_key_env` in tracked files.
- `temperature`: sampling temperature. Default 0.

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

## Limits

[Turn and build budgets](./turns.md#budgets). All optional.

```toml
[limits]
turn_rounds = 12
turn_mutations = 64
context_budget = 24000
build_turn_factor = 3
```

- `turn_rounds`: maximum message rounds per turn. Default 12.
- `turn_mutations`: maximum staged mutations per turn. Default 64.
- `context_budget`: maximum context pack size in characters. Default 24000.
- `build_turn_factor`: sets the per-build turn cap as
  `build_turn_factor × (dirty documents + touched entities)`. Default 3. See
  [convergence](./reconciler.md#convergence).

## Environment tuning

Run-level knobs are environment variables only, since they tune one run, not the project:

- `JAZYK_MAX_CONCURRENCY`: cap on parallel turns within a level (default 6).
- `JAZYK_MAX_RETRIES`: retries, in addition to the first attempt, for a failed LLM call
  (default 2).
- `JAZYK_TEMPERATURE`: overrides `temperature` (default 0). A negative value omits the
  field for models that only accept their default.
- `JAZYK_VERBOSE`: when set to a non-empty value other than `0`, emit verbose
  [trace events](./turns.md#trace-events) including full context packs and raw payloads.
