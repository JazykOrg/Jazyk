# Compiler

The compiler reads natural language documentation using [Large Language Models (LLMs)
](https://en.wikipedia.org/wiki/Large_language_model). 
It surfaces documentation ambiguity, open-endedness, and contradictions within the documentation.
And finally it produces machine readable [project requirements (artifacts)](./artifacts.md).
These artifacts may be used to generate, continuously update and test software.

## High-level

The compiler is a Rust library. It is embedded in a frontend such as the [CLI](../cli.md), the
[Language Server](../lsp/lsp.md), or the [MCP server](../mcp.md).

Several stages call a large language model through an OpenAI-compatible endpoint (e.g. local LLM),
configured in [project settings](./project-settings.md#llm) and overridable with CLI flags.

The design mimics similar compilation and linking processes as programming languages. Each file is
compiled on its own, exposing entities and relationships between those entities. And later, entities
across compiled files are linked together and their structure merged.

## Phases

- [Compilation](./compilation.md) runs per file, may run in parallel. Splits a file
  into sections and extracts entities, requirements, and relationships. It produces one object
  artifact per file.
- [Linking](./linking.md) runs over all object artifacts. It resolves entities across
  files, then validates them together. It produces the linked and reviewed artifacts.

## Model

The semantic model is a graph. The node types are sections, entities, requirements, relationships,
and diagnostics.

[See more](./model.md)

## Build artifacts

The output of each phase is a build artifact: the object artifact (per file), the linked artifact,
and the reviewed artifact.

[See more](./artifacts.md)

## Concepts

Cross cutting concepts used across the phases:

- [Determinism](./concepts/determinism.md)
- [Stable diagnostics](./concepts/stable-diagnostics.md)
- [Stable identity](./concepts/stable-identity.md)
- [Scopes](./concepts/scopes.md)
- [Incrementality](./concepts/incremental.md)
- [Reasoning](./concepts/reasoning.md)
- [EARS](./concepts/ears.md)

## Project settings

[See more](./project-settings.md)
