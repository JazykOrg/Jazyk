# Project Settings

Jazyk project settings are stored in a file named `jazyk.toml` that marks a folder as a Jazyk
project.

## Docs

The `docs` section configures documentation.

### Glob

The set of documentation files is an ordered list of glob patterns, matched relative to the project
root (the directory holding `jazyk.toml`). A pattern starting with `!` is an exclusion. Later
patterns override earlier ones, so a file is included if the last pattern to match it is an inclusion.

```toml
[docs]
glob = [
    "docs/**/*.md",
    "docs/**/*.yaml",
    "!docs/LICENSE.md",
]
```

A file that matches the glob but has no [handler](#handlers) is surfaced as a warning during
[parse](./compilation/parse.md#parse).

### Handlers

Each file is read by a [parser](./compilation/docs-parser.md#documentation-parser). The compiler ships
with built-in handlers (e.g. [Markdown](./compilation/docs-parser/markdown.md#markdown-parser)). You
can register your own for other formats such as `docx`, `drawio`, or UML/XMI.

A handler is configured with a `matcher` (which files it applies to) and a `path` (the
implementation). Custom handlers are tried before built-in ones, and the first handler that claims a
file wins, so a custom handler can override a built-in for a subset of files.

```toml
[docs.handlers.drawio]
matcher = "docs/**/*.drawio"
path = "./handlers/drawio.wasm"
```

The handler interface follows the [parser template](./compilation/docs-parser.md#parser-template).

### Linting

Linting rules are written in plain English and grouped by the severity they produce. `warnings` allow
compilation to continue. `errors` cause it to fail.

```toml
[docs.linting.rules]
warnings = [
    "Grammatical errors and spelling mistakes",
]
errors = [
    "Deprecated terminology like 'master/slave'",
    "Unimplemented or TODO sections",
]
```

The same rule text can move between `warnings` and `errors` to change its severity. Rules are
evaluated by an LLM judge, see [Determinism](./concepts/determinism.md#determinism). Project wide
rules over the entity graph are run as part of [checks](./linking/checks.md#checks).

## LLM

LLM-backed stages call an OpenAI-compatible chat completions endpoint. Ollama exposes one, so it is
the default backend.

```toml
[llm]
base_url = "http://localhost:11434/v1"
model = "llama3.1"
api_key_env = "JAZYK_API_KEY"
```

`base_url` points at any OpenAI-compatible server. `model` is the model id and is part of the
[cache key](./concepts/determinism.md#determinism). `api_key_env` names the environment variable that
holds the API key (Ollama ignores it); a literal `api_key` may be given instead. `base_url` and
`model` can be overridden on the [CLI](../cli.md#cli) with `--llm-base-url` and `--model`.

### Global configuration

The LLM endpoint, model, and credentials describe the machine and account, not the project, so they
are better kept out of `jazyk.toml` and out of version control. A machine-level **global config** at
`~/.jazyk/config.toml` (or `~/.jazyk.toml`) holds the same `[llm]` table and applies to every project
on the machine:

```toml
# ~/.jazyk/config.toml
[llm]
base_url = "http://localhost:11434/v1"
model = "llama3.1"
api_key_env = "JAZYK_API_KEY"
```

This is the recommended home for `[llm]`. The project's own `[llm]` table is still honored as a
fallback, so a project can pin a model if it must.

The effective LLM settings are resolved per field, highest priority first:

1. CLI flags — `--llm-base-url`, `--model`, `--api-key`.
2. Environment variables — `JAZYK_LLM_BASE_URL`, `JAZYK_MODEL`, `JAZYK_API_KEY`.
3. Global config — `~/.jazyk/config.toml`.
4. Project `[llm]` — `jazyk.toml`.
5. Built-in defaults.

## Roots

Roots are the entry-point files for reachability. Every entity defined in a root file is a root
entity, and anything not reachable from a root entity is flagged as unused or unreachable, see
[checks](./linking/checks.md#checks). Roots are a list of glob patterns, matched like the
[docs glob](#glob).

```toml
[roots]
files = [
    "docs/system.md",
    "docs/api/**/*.md",
]
```

Entity [scope](./concepts/scopes.md#scopes) and entity links are not project settings. They are
expressed in the documentation itself: set an entity's scope where it is defined, and link a
reference to its definition with an ordinary in-document link. See
[Resolve entities](./linking/resolve-entities.md#resolve-entities).

## Schema

The JSON Schema for the project settings is [located here](./project-settings.schema.yaml).
