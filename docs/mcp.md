# MCP

The MCP server exposes Jazyk to LLMs and their harnesses over the Model Context Protocol. It exposes
the [compiler](./compiler/compiler.md#compiler) and efficient access to the
[build artifacts](./compiler/artifacts.md#build-artifacts), so an agent can compile a project and then
navigate the entity graph without re-reading the documentation.

## Definition

The server is started as `jazyk mcp` ([CLI](./cli.md#cli)) and speaks MCP over stdio, the same way
the [language server](./lsp/lsp.md#language-server) is started as `jazyk lsp`. It honors the same
`--llm-base-url` and `--model` overrides as the rest of the CLI.

### Welcome message

The server operates on a Jazyk project (a directory with a `jazyk.toml`), found by walking up from the
working directory. It can compile the project and answer questions about the resulting graph. The
welcome message points the client at the project root in use and summarizes the tools below.

### Tools

Compilation:

- `compile` runs [compilation](./compiler/compilation.md#compilation) and
  [linking](./compiler/linking.md#linking) and returns warnings and errors.
- `diagnostics` lists the [diagnostics](./compiler/model/diagnostic.md#diagnostic) from the latest
  build, optionally filtered to a file or entity.

Graph exploration:

- `get_entity` fetches an [entity](./compiler/model/entity.md#entity): its definition, requirements,
  and relationships.
- `requirements_for` lists the [requirements](./compiler/model/requirement.md#requirement) for an
  entity, or between two entities.
- `relationships_for` traverses the [relationships](./compiler/model/relationship.md#relationship)
  from an entity.
- `search` finds entities by content.

## Usages

The MCP server is also the entry point through which agents drive the [usages](./main.md#usages):
[project management](./pm.md#project-management), [code generation](./codegen.md#code-generation),
[test generation](./testgen.md#test-generation), and
[documentation generation](./docsgen.md#documentation-generation).
