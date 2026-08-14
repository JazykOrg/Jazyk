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
  match, then alias, then substring, then token overlap. Returns `{hits}`, up to 8
  `{id, name, definition}` entries. No embeddings, no LLM.
  A miss is an answer, not a dead end: `hits` is empty and the result carries
  `entityCount`, the graph's entities by id and name (up to 25), and a `next` line
  saying the search will keep returning this and to create the entity instead. A bare
  empty list reads as "ask again", and models loop on it; naming the whole graph lets
  the caller decide without another call.
- `read_section({ref})`: one section's raw body and its child titles.
- `get_entity({id})`: one entity with its definition, mentions, requirements, and
  relationships.
- `diagnostics({lifecycle?, rule?, subject?})`: the diagnostics, `open` by default
  (`lifecycle` takes `open`, `resolved`, or `all`; `rule` and `subject` narrow
  further). Each entry carries id, rule, severity, lifecycle, triage, subjects, and
  message. This is the read surface for document health; without it an agent can
  only learn diagnostic state by reading the out directory off disk.

## Write tools

- `upsert_entity({name, definition?, aliases?, scope?, mention: {section, quote}, note?})`
  → `{id, created}`. Keys on `name` plus `scope`; a match updates instead of duplicating.
  A name variant of an existing entity is rejected toward reusing that entity and
  recording the wording as an alias; a `note` overrides. Omit `scope` unless the
  documents explicitly name a bounded context. An invented scope splits one concept in
  two.
- `update_entity({id, name?, definition?, add_aliases?})`: a rename keeps the id.
- `delete_entity({id, reason})`: rejected while requirements reference the entity.
- `merge_entities({keep, absorb, reason})`: the store rewires references and leaves a
  redirect. See [mutations](./graph.md#mutations).
- `upsert_requirement({ears, entities, section, quote, edges?})`: the store mints the
  id; any id supplied is ignored. Idempotency comes from the natural key (the source
  section plus the punctuation-insensitive statement text), resolved the moment the
  call is staged, against the store and the turn's own staged statements alike. A match
  returns the existing id with `updated: true` and refreshes its `ears` and `quote` in
  place, never minting a duplicate; the model sees the resolution, not a misleading
  fresh id. A statement re-extracted from the same source sentence whose content
  subsumes or is subsumed by the existing statement is the same fact reworded: it also
  updates in place. A stale anchor in the same section whose statement subsumes or is
  subsumed by the new statement resolves the same way, so re-recording a reworded
  statement lands on the anchor's id. A sentence carrying several atomic facts is
  unaffected, since those statements are not subsets of each other. Parallel turns can
  still stage the same key concurrently; the store reconciles survivors at commit.
  `edges` name entity pairs the statement ties together, with an optional
  [relationship type](./model/relationship.md). Real revisions go through
  `update_requirement`.
- `update_requirement({id, ears?, entities?, edges?, section?, quote?})`: a revision
  keeps the id. `section` plus `quote` re-anchor the provenance; the quote must locate
  verbatim in the section. This is the path for a stale anchor whose statement changed
  meaning: new `ears`, new `quote`, same id. Omitting both leaves the anchor untouched,
  which is what a call that only adds an entity reference wants. Passing the `ears`
  statement as the `quote` is the common miscall, so every rejection on `section` or
  `quote` names the requirement's existing anchor and says to drop the two fields when
  only `entities` or `edges` were meant to change.
- `delete_requirement({id, reason})`.
- `report_diagnostic({rule, severity, subjects, message, reasoning})`. `rule` is one of
  the review rules: `contradiction`, `duplicate-entity`, `duplicate-requirement`,
  `missing-link`, `ambiguity`, or `lint` for violations of the project's own
  [lint rules](./project-settings.md). Free-form rule names are rejected, so
  findings stay comparable across builds.
- `resolve_diagnostic({id, reason})`.
- `set_coverage({section, state, note?})`: `state` is `covered` or `non-normative`.
  `non-normative` requires the `note`; a placeholder note (`<nil>`, `none`, `n/a`)
  counts as absent. `non-normative` is rejected for a section that already yielded a
  requirement, in the store or in this turn's own staged work: the mark and the
  statement contradict each other, and the statement is the evidence. The rejection
  names the statements and asks for `covered`. A repeated call for the same section within one turn replaces the
  earlier mark: a changeset carries at most one coverage mark per section. A `covered`
  claim requires a requirement sourced from that section; the `done` gate enforces it.
- `done({summary})`: end the turn and request commit. Rejected while a stale anchor
  from the work item is untouched: the anchor's quote must locate again, or a staged
  mutation must re-record, revise, or delete it. An explicit `done` is also rejected
  while one of the turn's dirty sections has no coverage mark, staged or already
  recorded: extracting requirements from a section and walking away without its mark
  leaves the section unprocessed. The implicit path commits without the mark
  ([budgets](./turns.md#budgets)).

There is no write tool for relationships. Edges exist only as a
[derived product of requirements](./graph.md#derived-data).

## Chat tools

The [chat serving](../frontends/mcp.md#toolsets) carries tools built for a
conversation about the project. They are not in any turn's toolset.

Dual-write tools (see [ACP](../frontends/acp.md#dual-write-tools)): a requirement
lives in the prose, so a chat edit moves the prose and the graph together:

- `revise_requirement({id, new_text, ears?})`: locate the requirement's verbatim
  quote in its source section, stage an `edit_doc_prose` replacing it with
  `new_text`, and stage the requirement update (`quote: new_text`, plus `ears` when
  given) in the same changeset. Commits atomically; the document's stored hashes
  absorb the edit ([mutations](./graph.md#mutations)). A quote that no longer
  locates is rejected with the section's current text as repair guidance.
- `add_requirement({doc, section, after_quote?, text, ears, entities})`: insert
  `text` into the section (after the located `after_quote`, or at the section's
  end) and stage the requirement sourced from it.
- `retract_requirement({id, reason})`: remove the requirement's sentence from the
  prose and delete the requirement, one changeset.

Project tools (see [ACP](../frontends/acp.md#project-tools)):

- `init_project({dir?})`: scaffold `jazyk.toml` and the starter layout.
- `update_project_settings({...})`: typed edits to `jazyk.toml`, rendered as minimal
  edits.

## Feedback tool

`report_feedback({kind, subject?, message})` is the model's channel back to jazyk's own
developers. It reports that a prompt, a tool, a schema, or an error message is
ambiguous, wrong, confusing, or missing something, not that the project's documents are.
Findings about the documents are [diagnostics](./model/diagnostic.md); findings about
jazyk are feedback.

- `kind`: one of `ambiguous`, `wrong`, `confusing`, `missing`, `other`. An unknown value
  is recorded as `other` rather than rejected.
- `subject`: what the feedback is about, e.g. a tool name, an argument, an instruction.
- `message`: what was unclear and what would have helped. Trimmed to 4000 characters.

The tool never touches the graph. It stages no mutation, counts against no mutation
budget, and passes no gate beyond a non-empty `message`, so a confused model is never
bounced while asking for help. It is in every [toolset](#task-toolsets), including the
read-only MCP serving.

Each call appends one JSON line to `<out>/feedback.jsonl`, with the payload plus the
references that identify the caller: the timestamp, the source (`turn` or `mcp`), the
task, the work item's target, the model, the codec, the store generation, the run's
transcript name, and the MCP client name when one is known. The log is append-only and is never
read back by the compiler. The [GUI](../frontends/gui.md#feedback) renders its history.

A turn records at most 5 feedback entries. Beyond that the call is acknowledged without
a record, so a confused model cannot flood the log.

The reply is `{recorded, note}`. The note tells the model to continue with its best
judgment: feedback is not an escape from the work item.

## Compilation tools

Compilation over MCP is task-based: the agent performs work items from
[the task queue](./reconciler.md#the-task-queue), staging graph writes into an open
changeset exactly as an in-process turn does. One task is open at a time per serving.

- `compilation_tasks({})`: the queue: kind, target, dirty sections, stale anchor
  count, ready or blocked with the reason. Zero tasks returns the build verdict
  instead; nothing to do is an answer. The verdict carries `openDiagnostics`, the
  open diagnostic counts by severity, so a converged build with standing errors
  says so ([convergence](./reconciler.md#convergence)).
- `begin_compilation({task?})`: claim the named task, or the first ready one. Reloads
  the store, syncs section trees in memory, opens a changeset, and returns the work
  package: the task's `instructions` (the same extraction or review contract an
  in-process turn gets as its system prompt) and the same pack (dirty section bodies,
  statements already extracted per section, known entities, incoming links, stale
  anchors). The [write tools](#write-tools) stage into the open changeset; outside an
  open task they are rejected toward `begin_compilation`.
- `done({summary})`: run the gates, commit atomically, update the
  queue, and return `{committed, next}` with the next ready task. A gate failure
  leaves the changeset open and names the repair. The finish that empties the queue
  also runs the deterministic tail (checks, docsgen, verdict) and reports the verdict
  and any generation tasks that became ready. `finish_compilation` is an unlisted
  compatibility alias.
- `abandon_compilation({reason})`: drop the staged changeset. Leaves no trace.

## Generation tools

[Generation](../consumers/gen.md) is a workflow over the graph, so its steps are tools
too. Any agent that speaks MCP can be the generation worker; `jazyk gen` is one such
worker in-process ([generation turns](./turns.md#generation-turns)). These tools read
the graph and the [ledger](../consumers/gen.md#the-ledger) (`gen/ledger.yaml`); they
never mutate the graph. The agent edits deliverable files and runs commands with its
own tools; jazyk serves no file editing over MCP.

- `generation_tasks({})`: entities whose facts differ from the ledger:
  `{entity, reason, changed}` where `changed` lists the requirement ids added, removed,
  or reworded since the entity was last generated.
- `begin_generation({entity})`: the full package for one task: the instructions (the
  generation contract: both halves per entity, traceability markers, the two test
  kinds, the parts protocol), the entity's context pack, its requirements in
  generation groups, the change diff, the deliverable directory, the `factHash`, the
  manifest of already generated files with what each holds, and the deliverable's
  [medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)
  and build. Layout, file names, and run commands are the worker's choices, recorded
  in the manifest; the medium is not, it is already decided. Stateless on the server:
  beginning claims nothing and holds nothing.
- `record_generation({entity, factHash, manifest})`: record the task done. The worker
  writes the deliverable files itself; the `manifest` binds them to the graph:
  `{files: [...], tests: [{requirement, kind, label, artifact, name, run, cwd}],
  build?}`. Marking strips every single-line marker comment from the manifest files
  and records each as an anchored site on its requirement's row
  ([traceability](../consumers/gen.md#traceability)), then updates both ledger maps
  and [prunes rows](../consumers/gen.md#deletion-prunes-the-ledger) whose requirement
  left the graph; the entity leaves `generation_tasks`. A `factHash` that no longer
  matches the live
  graph is recorded but leaves the entity pending, so a graph that moved mid-task is
  never masked.

## Binding tools

[Binding](../consumers/bind.md) creates the ledger rows generation and verification
work from. Same worker model as generation: the agent searches and edits with its own
tools; jazyk holds the ledger. These tools never mutate the graph.

- `binding_tasks({})`: requirements owing a binding, with a `reason` (`unbound`,
  `requirement-changed`, `artifact-gone`). Deterministic; no model involved.
- `begin_binding({requirement})`: the package for one task: the instructions (the
  [bind contract](../consumers/bind.md#the-bind-task): search before write, both
  directions get a test, the two test kinds, falsifiability, the naming scheme), the
  statement, quote, and hash, the context pack, the deliverable directory, the
  decided [medium](../consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)
  and build when they exist, and the test conventions already recorded in the ledger.
  Stateless on the server; the lease is the claim.
- `record_binding({requirement, files, test, verdict, evidence?})`: record the row:
  the implementing files (an empty list is a finding), the test row
  (`{kind, label, artifact, name, run, cwd}`), and the first verdict. Rejects a test
  whose artifact does not exist or does not contain the declared name, the same shape
  gate `record_generation` applies. The row's
  [derived status](../consumers/gen.md#status-is-derived-never-stored) classifies the
  requirement (`verified`, `unimplemented`, `failing`).

## Decompilation tools

[Decompilation](../consumers/decompile.md) produces documents, never graph writes.

- `decompile_tasks({})`: the released scopes with their unclaimed files and inventory
  summaries.
- `begin_decompile({scope})`: the package for one draft: the inventory slice, the test
  files with their assertions, the lint rules, and the
  [drafting contract](../consumers/decompile.md#draft-tasks).
- `submit_draft({path, content})`: validate the draft (lint rules, extractable
  statements, evidence anchors) and write it into the docs tree. Records the draft
  hash for [ratification](../consumers/decompile.md#ratification) and consumes the
  scope's release.

## Verification tools

Verification runs the tests the ledger records and feeds verdicts back. Same worker
model: `jazyk test` is the built-in worker; any MCP agent is another. These tools write
only the ledger, never the graph.

- `verification_tasks({filter?, entity?})`: rows needing action, with their derived
  [status](../consumers/gen.md#status-is-derived-never-stored) and a `reason`
  (`not-generated`, `artifact-gone`, `never-run`, `requirement-changed`,
  `test-changed`, `code-changed`, `failed`). Requirements the
  graph holds but the ledger does not appear as `missing`, so ungenerated work is
  never silent. Rows whose requirement left the graph are not listed: they are not
  work, and the next `record_generation`
  [prunes them](../consumers/gen.md#deletion-prunes-the-ledger). Deterministic; no
  model involved.
- `begin_verification({requirement})`: the package for one row: the statement, quote,
  and hash; the context pack; the manifest files; and either the run command
  (`programmatic`) or the criteria and confirm-steps (`llm`).
- `run_tests({requirements?})`: execute the recorded programmatic tests: the
  [build](../consumers/gen.md#the-build) first, once, then each selected row's
  command. Verdicts and evidence are recorded as a side effect and returned. `llm`
  rows are skipped here; they go through `record_verdict`. This is how a worker
  verifies without hand-rolling the harness, and it serves in the generation toolset
  too.
- `record_verdict({requirement, verdict, factHash?, evidence?})`: record a `pass` or
  `fail` verdict with its evidence, rebaselining the test and files hashes. A stale
  `factHash` is recorded but the row stays pending, the same protection
  `record_generation` has.

## Validation and errors

Every call is validated by the [gates](./graph.md#validation-gates). An error names the
violated rule and how to repair the call. E.g.:

```
quote not found in docs/cli.md#/cli/commands; copy the sentence verbatim from the section
```

Errors are part of the tool design. They are written for a model that will read them and
try again.

## Task toolsets

Turns see subsets, not the whole catalog. Every subset carries
[`report_feedback`](#feedback-tool); it is listed once here, not per task:

- `reconcile-doc`: `context`, `expand`, `search`, `read_section`, `upsert_entity`,
  `update_entity`, `delete_entity`, `upsert_requirement`, `update_requirement`,
  `delete_requirement`, `set_coverage`, `done`.
- `review-requirement`: `context`, `expand`, `search`, `get_entity`, `read_section`,
  `diagnostics`, `update_requirement`, `delete_requirement`, `report_diagnostic`,
  `resolve_diagnostic`, `done`.
- `review-entity`: `context`, `expand`, `search`, `get_entity`, `diagnostics`,
  `update_entity`, `merge_entities`, `update_requirement` (a review adds missing
  `edges` when requirements tie entities structurally), `delete_requirement`,
  `report_diagnostic`, `resolve_diagnostic`, `done`.
- `generate-entity`: the read tools, `record_generation`, `run_tests`, `done`. The
  [file and command tools](./turns.md#generation-turns) join only when the agent's
  profile sets [`serve_files`](./project-settings.md#acp); coding agents bring
  their own.
- `jazyk mcp compile`: the read tools, the [compilation tools](#compilation-tools),
  and the write tools (gated behind an open task), plus `await_changes`.
- `jazyk mcp generate`: the read tools, the [binding tools](#binding-tools), the
  [generation tools](#generation-tools), `run_tests`, plus `await_changes`. Binding
  and generation share the serving because they share the worker persona: search and
  edit the deliverable, record into the ledger.
- `jazyk mcp verify`: the read tools and the
  [verification tools](#verification-tools), plus `await_changes`.
- `jazyk mcp decompile`: the read tools and the
  [decompilation tools](#decompilation-tools), plus `await_changes`.
- `jazyk mcp graph`: the read tools, plus `await_changes`; `--write` adds the raw
  write tools, each call committing as its own changeset. See
  [MCP](../frontends/mcp.md#toolsets).
- `jazyk mcp chat`: the read tools, the compilation, binding, generation, and
  verification lifecycles, the [chat tools](#chat-tools), plus `await_changes`. No
  raw write tools.

Tool input and output shapes are specified in [`tools.schema.yaml`](./tools.schema.yaml).
