# Tools

The tool registry is the graph's only interface for models. One registry, one set of
schemas, served two ways:

- in-process, to the [turn harness](./turns.md) during compilation,
- over stdio as an MCP server (`jazyk mcp graph`), to external agents. See
  [MCP](../frontends/mcp.md).

Both servings dispatch the same implementations, so the tools an external agent uses are
exactly the tools the compiler uses. Read tools are public. Write tools require the
server's `--write` flag and are otherwise reserved for compilation turns.

The catalog is deliberately small. Weak models handle few, simple tools better than many
clever ones.

## Read tools

- `context({target, focus?, budget?})`: the [context engine](./context.md). Returns a
  rendered pack plus [expansion handles](./context.md#expansion-handles).
- `expand({handle})`: load the frontier behind a handle, under the same budget rules.
- `search({query, kind?})`: deterministic lookup over names and aliases: normalized exact
  match, then alias, then substring, then token overlap. Returns up to 8 results as
  `{id, name, definition}` lines. No embeddings, no LLM.
- `read_section({ref})`: one section's raw body and its child titles.
- `get_entity({id})`: one entity with its definition, mentions, requirements, and
  relationships.

## Write tools

- `upsert_entity({name, definition?, aliases?, scope?, mention: {section, quote}, note?})`
  → `{id, created}`. Keys on `name` plus `scope`; a match updates instead of duplicating.
  Omit `scope` unless the documents explicitly name a bounded context. An invented scope
  splits one concept into two.
- `update_entity({id, name?, definition?, add_aliases?})`: a rename keeps the id.
- `delete_entity({id, reason})`: rejected while requirements reference the entity.
- `merge_entities({keep, absorb, reason})`: the store rewires references and leaves a
  redirect. See [mutations](./graph.md#mutations).
- `upsert_requirement({ears, entities, section, quote, edges?})`: the store mints the
  id; any id supplied is ignored. Idempotency comes from the natural key (the source
  section plus the punctuation-insensitive statement text), so a retried or lightly
  reworded statement updates in place, refreshing its `ears` and `quote`, never minting
  a duplicate. `edges` name entity pairs the statement ties together, with an optional
  [relationship type](./model/relationship.md). Real revisions go through
  `update_requirement`.
- `update_requirement({id, ears?, entities?, edges?})`.
- `delete_requirement({id, reason})`.
- `report_diagnostic({rule, severity, subjects, message, reasoning})`. `rule` is one of
  the review rules: `contradiction`, `duplicate-entity`, `missing-link`, `ambiguity`, or
  `lint` for violations of the project's own
  [lint rules](./project-settings.md). Free-form rule names are rejected, so
  findings stay comparable across builds.
- `resolve_diagnostic({id, reason})`.
- `set_coverage({section, state, note?})`: `state` is `covered` or `non-normative`.
  `non-normative` requires the `note`. A `covered` claim on a section containing `shall`
  requires a requirement sourced from that section; the `done` gate enforces it.
- `done({summary})`: end the turn and request commit.

There is no write tool for relationships. Edges exist only as a
[derived product of requirements](./graph.md#derived-data).

## Generation tools

[Generation](../consumers/gen.md) is a workflow over the graph, so its steps are tools
too. Any agent that speaks MCP can be the generation worker; `jazyk gen` is one such
worker and consumes the same task packages in-process. These tools read the graph and
the [ledger](../consumers/gen.md#the-ledger) (`gen/ledger.yaml`); they never mutate the
graph.

- `gen_instructions({lang?})`: the generation contract as text: one bounded task per
  entity producing both the entity's part of the deliverable and the tests for its
  requirements, the traceability markers, the two test kinds, and the parts protocol
  for dense entities.
- `gen_pending({lang?})`: entities whose facts differ from the ledger:
  `{entity, reason, changed}` where `changed` lists the requirement ids added, removed,
  or reworded since the entity was last generated.
- `gen_task({entity, lang?})`: the full package for one task: the instructions, the
  entity's context pack, its requirements in generation groups, the change diff, the
  deliverable directory, a suggested default layout the worker may override, the
  `factHash`, and the manifest of already generated files.
- `gen_mark({entity, factHash, manifest})`: record the task done. The worker writes the
  deliverable files itself; the `manifest` binds them to the graph:
  `{files: [...], tests: [{requirement, kind, label, artifact, name, run, cwd}]}`.
  Marking updates both ledger maps and the entity leaves `gen_pending`. A `factHash`
  that no longer matches the live graph is recorded but leaves the entity pending, so a
  graph that moved mid-task is never masked.

## Verification tools

Verification runs the tests the ledger records and feeds verdicts back. Same worker
model: `jazyk test` is the built-in worker; any MCP agent is another. These tools write
only the ledger, never the graph.

- `verify_pending({filter?, entity?})`: rows needing action, with their derived
  [status](../consumers/gen.md#status-is-derived-never-stored) and a `reason`
  (`not-generated`, `artifact-gone`, `never-run`, `requirement-changed`,
  `test-changed`, `code-changed`, `failed`, `requirement-gone`). Requirements the
  graph holds but the ledger does not appear as `missing`, so ungenerated work is
  never silent. Deterministic; no model involved.
- `verify_task({requirement})`: the package for one row: the statement, quote, and
  hash; the context pack; the manifest files; and either the run command
  (`programmatic`) or the criteria and confirm-steps (`llm`).
- `verify_mark({requirement, verdict, factHash?, evidence?})`: record a `pass` or
  `fail` verdict with its evidence, rebaselining the test and files hashes. A stale
  `factHash` is recorded but the row stays pending, the same protection `gen_mark` has.

## Validation and errors

Every call is validated by the [gates](./graph.md#validation-gates). An error names the
violated rule and how to repair the call. E.g.:

```
quote not found in docs/cli.md#/cli/commands; copy the sentence verbatim from the section
```

Errors are part of the tool design. They are written for a model that will read them and
try again.

## Task toolsets

Turns see subsets, not the whole catalog:

- `reconcile-doc`: `context`, `expand`, `search`, `read_section`, `upsert_entity`,
  `update_entity`, `delete_entity`, `upsert_requirement`, `update_requirement`,
  `delete_requirement`, `set_coverage`, `done`.
- `review-entity`: `context`, `expand`, `search`, `get_entity`, `update_entity`,
  `merge_entities`, `update_requirement` (a review adds missing `edges` when
  requirements tie entities structurally), `delete_requirement`, `report_diagnostic`,
  `resolve_diagnostic`, `done`.
- `jazyk mcp graph` (default): the read tools.
- `jazyk mcp graph --write`: everything.

Tool input and output shapes are specified in [`tools.schema.yaml`](./tools.schema.yaml).
