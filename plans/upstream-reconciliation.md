# Plan: upstream reconciliation

Status: proposal for iteration. Nothing here is built. Read with
[ripple](./ripple.md) (edit paths, causality, termination) and
[acceptance-criteria](./acceptance-criteria.md) (criteria as the evidence tests
encode). Every behavior below lands in `docs/` first, then in `bootstrap/`.

## Start here

- The repo root `AGENTS.md` (docs first, the writing style, the division of labor).
- Provenance and ratification: [provenance](../docs/compiler/model.md#provenance),
  [ratification proposals](../docs/compiler/model/diagnostic.md#ratification-proposals),
  [the ratify goal](../docs/compiler/goals/ratify.md),
  [the answer goal](../docs/compiler/goals/answer.md),
  [prompts](../docs/compiler/model/diagnostic.md#prompts) and
  [answers](../docs/compiler/model/diagnostic.md#answers).
- The edit paths and the dual write:
  [edit paths](../docs/compiler/compilation.md#edit-paths),
  [dual-write tools](../docs/frontends/acp.md#dual-write-tools), the GUI
  [inspector](../docs/frontends/gui.md#inspector), `bootstrap/src/gui/facts.rs`
  (`edit_fact_core`, `mechanical_sentence`), `bootstrap/src/answer.rs` (`ratify_ops`,
  `retract_ops`).
- The ledger and its change detection:
  [the ledger](../docs/consumers/gen.md#the-ledger),
  [status is derived](../docs/consumers/gen.md#status-is-derived-never-stored),
  [traceability](../docs/consumers/gen.md#traceability),
  [the unattached remainder](../docs/consumers/gen.md#the-unattached-remainder),
  [the unclaimed report](../docs/consumers/bind.md#the-unclaimed-report),
  `bootstrap/src/verify.rs` (`status_of`), `bootstrap/src/gen.rs` (`strip_markers`,
  `measure_unattached`).
- Decompilation: [decompile](../docs/consumers/decompile.md),
  `docs/compiler/goals/prompts/decompile-contract.md`, `bootstrap/src/decompile.rs`.
- Goal derivation: [change records](../docs/compiler/reconciler.md#change-records),
  [tiers](../docs/compiler/reconciler.md#tiers),
  [flip detection](../docs/compiler/reconciler.md#flip-detection),
  [journal](../docs/compiler/graph.md#journal).

## Why

The flow today is downward. The docs compile into the graph, the graph binds to tests,
generation makes the tests pass. Four [edit paths](../docs/compiler/compilation.md#edit-paths)
exist, and three of them already reach the prose: a prose edit is the source, a dual
write moves prose and graph together, and a decree carries a ratification proposal back
toward the prose. The fourth path, an edit to the deliverable or its tests, stops short.
It flips ledger statuses and reruns tests, and that is all:

- An edited test silently becomes the judge. `stale-test` derives `verify`, the rerun
  rebaselines `hashes.test`, and a test that now checks 45 days reads `verified` under a
  criterion that still says 30. Nothing asks whether the criterion changed.
- An edited implementation that breaks a test ends as a `failing` row, a finding. The
  `failing-binding` diagnostic has no option that moves the docs to what the code does.
- An edited implementation that adds behavior and breaks nothing only grows the
  [unattached remainder](../docs/consumers/gen.md#the-unattached-remainder). No
  requirement ever states it.
- Generation can overwrite a hand edit. A `facts-changed` goal on the entity rewrites
  its files; the before-image lands in `deliverable-baseline/`, and nothing reads it.
- A decree on a field with no mechanical sentence (`edges`, `transition`, `facets`)
  gets a composed statement, never a minimal rewrite of the sentence that states it.
- A fact restated in two documents gets one proposal edit; the other sentence keeps the
  old value.
- A hand edit to a graph shard (`graph/requirements.yaml`) bypasses changesets,
  provenance, and the journal.

The owner's request is to close the loop: an edit anywhere (docs, graph, tests, code)
reconciles upward into the documents, and edits in several places at once reconcile
without silent overwrites. The machinery for most of this exists. The plan adds one
baseline, two change record kinds, three goal kinds, two diagnostic rules, and a small
extension to the prompt shape, and reuses everything else.

## The design in one page

- The documents stay the source. Code and tests are evidence about the documents, never
  a second source. A quoted fact moves only when a human accepts a sentence.
- Every write jazyk makes to the deliverable moves an origin baseline: the bytes as
  jazyk last wrote or accepted them. Any divergence from origin is an outside edit, and
  only outside edits open upward work. This is what keeps the loop from cycling.
- Outside edits open goals by the layer they touched: `lift-test` on a requirement
  whose test region diverged, `lift-code` on an entity whose owned files diverged. New
  files no binding names stay the [unclaimed report](../docs/consumers/bind.md#the-unclaimed-report)
  and feed decompilation, as today.
- A lift session classifies each changed hunk (`refactor`, `implements`, `revises`,
  `adds`, `regression`, `detail`). The harness gates the classification with facts it
  can check (a `refactor` claim requires the entity's tests green).
- Additions land as `derived` facts with evidence and a ratification proposal, exactly
  as `abstract-entity` and `derive-criteria` land theirs. The graph reflects what the
  product does now, marked invented until ratified.
- Revisions of a quoted or decreed fact never edit the fact. They land as an
  `upstream-change` diagnostic whose prompt carries the minimal sentence edit (and the
  criteria sentences that change with it) as one `edit` option, plus a `restore`
  option. Accepting is a dual write, the same as any prompt `edit`.
- An upward accept absorbs the downstream baseline it came from, as a dual write absorbs
  the section hashes of the document it rewrites. The accepted fact does not reopen
  `generate` over the code that already carries it.
- Same fact edited in two layers in one window: consistent edits resolve without a
  question; inconsistent edits become one `upstream-conflict` question naming both
  sides. The docs never lose to code silently, and code never loses to docs silently.
- `generate` never writes over an unread outside edit: it waits while a lift goal is
  open on its entity, and stays blocked while a revision question on the entity is
  unanswered.
- Decrees gain the missing pieces: `phrase-proposal` words a proposal the harness
  cannot compose mechanically, a proposal edits every sentence that restates the fact,
  and a hand-edited graph shard is read as a decree.
- Convergence: lift goals are mandatory compile goals; the questions they file are
  blocked goals (`answer`, `ratify`) and ride as counts. `converged, 3 blocked` is a
  normal outcome.

## Sources of truth and precedence

### Fact classes

Each fact class has one authoritative layer. The others are evidence about it.

| fact class | authoritative | evidence | never lifted |
|---|---|---|---|
| obligation (requirement `statement`, `edges`, `transition`, `facets`) | the documents (`quote`) | a decree, a test, the code | |
| evidence (criterion) | the documents (`quote`) | a decree, a test | |
| concept (entity `name`, `definition`, `stereotype`, `parent`, `attributes`) | the documents | a decree | module and type names in code |
| behavior no document states | the code, until the owner decides | the tests | |
| deliverable form (medium, layout, file names, toolchain, build) | the code and the ledger's `medium` | | yes, unless a statement pins it |
| presentation (views) | the graph | | yes, views are never ratified |

- An obligation the documents state is changed only by the documents. A test or a code
  edit that disagrees with it is a question. A decree that disagrees with it is a
  decree with a ratification proposal (the existing rule: a decree outranks derivation,
  not the documents).
- Behavior the documents never state has no authority above the code. The code is what
  exists, so a lift records it as a `derived` fact: honest about where it came from,
  and pushed toward the documents by its proposal.
- Form is the generator's choice today ([the medium](../docs/consumers/gen.md#the-medium-is-decided-once-before-anything-is-generated)).
  A hand edit to a support file, a build file, or a file name is not a fact about the
  subject and lifts nothing. When a statement pins a literal the edit changes (a path,
  a port, a region in a code span), the `pinned-fact-drift` check already catches it.

### Intent by edit site

Where the human edited says what the edit is about. The harness infers the class of
intent from the site deterministically; the model judges the content.

| edit site | intent about | default reading |
|---|---|---|
| a document sentence | the obligation it states | the source; reconcile the graph to it |
| a graph field through the inspector, chat, or a shard | the fact | a decree; propose the sentence |
| a recorded test's region (a criterion's test) | that criterion | the criterion changed; propose its sentence |
| a new test beside recorded ones | a criterion not yet stated | a new criterion of the nearest requirement |
| a deleted test | the criterion it encoded | the criterion is retracted, or the test was lost |
| code inside a site's run | the behavior of that requirement | a revision, a regression, or a refactor |
| code outside every site's run in an owned file | behavior no requirement states | an addition, or a detail |
| a new file no binding names | territory without docs | the unclaimed report, decompilation |
| a support or build file | the deliverable's form | nothing upstream |

A test edit is the most explicit intent a developer expresses below the prose: a test
is a criterion in executable form ([criterion](../docs/compiler/model/criterion.md)).
Its default option is to adopt. A code edit is the least explicit: the same diff is a
spec change, a bug fix toward the spec, a new bug, or a refactor. Its default is to ask.

### Precedence rules

1. A human act outranks a derivation. A test or code edit by a person outranks a
   `derived` fact a session invented: a lift may update a derived fact in place
   (it stays `derived`, the evidence joins its `from`, its proposal refreshes).
2. The documents outrank every other human act on an obligation. A code or test edit
   never changes a quoted fact. It files a question whose accept option is the prose
   edit.
3. The compiler never overwrites a decree (the existing rule). A code or test edit that
   disagrees with a decreed fact files an `upstream-conflict` naming the decree.
4. Within one window, the later layer does not win by being later. Two human edits to
   the same fact in two layers either agree (nothing to ask) or become one question.
5. Nothing jazyk writes counts as intent. Generation, binding, and every accepted
   proposal move the origin baseline, so their writes are never lifted.

### Ties and conflicts become questions

Every disagreement the rules do not settle is a diagnostic with a
[prompt](../docs/compiler/model/diagnostic.md#prompts), never an overwrite. The
unanswered prompt is the existing `prompt-unanswered` record and a blocked
[`answer`](../docs/compiler/goals/answer.md) goal. Two new rules join the
[catalog](../docs/compiler/model/diagnostic.md#rules-catalog):

- `upstream-change` (warning): the deliverable or a test diverged from a stated fact.
  Options: the prose edit (accept), `restore` (put the deliverable back), freeform.
- `upstream-conflict` (warning, error when a decree is one side): the same fact was
  edited in two layers in one window, and the edits disagree. Options: one per side,
  each an edit or a restore, plus freeform.

Both carry `subjects` (the fact, and the criterion when one is involved) and an
`evidence` block in `detail`: the file, the recorded `head`, the before and after lines.
A re-detected condition is not re-asked: the answer is a human record, as today.

## Detection per layer

### The origin baseline

The ledger's hashes cannot tell an outside edit from a jazyk write, and they move on the
wrong events. `hashes.test` and `hashes.files` rebaseline on every test run and on
`jazyk test --audit` ([status is derived](../docs/consumers/gen.md#status-is-derived-never-stored)):
after one rerun, the evidence that a human edited the test is gone. And a hash says that
something changed, never what. A lift needs the bytes.

The origin baseline is new:

- At every jazyk write to the deliverable, the harness records the written bytes as
  the origin of that file: `record_generation` (every owned file and test artifact),
  `record_binding` (the test artifact, and the implementing files it names as found),
  an accepted `restore`, and an accepted upstream proposal (below). Adoption is the
  first `record_binding`, so an adopted codebase gets its origin at bind time.
- The ledger keeps `origin: {<path>: <content hash>}` per entity (owned files) and per
  test entry (the artifact). The bytes live content-addressed under
  `<out>/gen/origin/<hash>`, pruned when no ledger entry names the hash.
- Test runs and `--audit` never move origin. They rebaseline verification, not intent.
- The existing `deliverable-baseline/` stays what it is: the before-image of a
  generation, for the GUI diff. Origin is the after-image of the last accepted write.
- A file under a `built` medium's `produces` has no origin: it is output. Support files
  keep an origin so a divergence can be reported, but it opens no goal.

Divergence is computed at derivation, level-triggered like everything else: the current
bytes against origin, diffed in memory, whitespace-normalized over significant lines
(the same notion the unattached measure uses: non-blank, non-comment in the medium's
comment syntax). A formatting-only change diverges in bytes and not in significant
lines, and opens nothing. The diff comes from a crate (`similar`), per the dependency
policy; only attribution is hand-rolled.

### Attribution

Attribution maps each hunk to what it touches, without parsing the medium:

- A hunk in a code file falls in a site's run when its lines lie between a site's
  located `head` and the next site in the same file (the run rule the unattached
  measure already uses). It is attributed to that site's requirement.
- A hunk before the first site, or in an owned file with no site, is unattached.
- A hunk in a test artifact is attributed to the recorded test whose declared `name`
  line precedes it most closely; the entry's criterion is the attributed criterion. A
  hunk that removes a recorded name line entirely is a deleted test.
- The literal pass: the concrete values of the entity's statements and criteria
  (numbers, code spans, quoted strings) are matched against removed and added lines.
  A value removed in a hunk and a same-shaped value added at the same position is a
  moved literal (`30` → `45`). A criterion value present in a test region before and
  absent after is a weakened test.

### Docs

Unchanged: the `edit` entry, the dirty set, `section-dirty` and `anchor-stale`
([dirty set](../docs/compiler/reconciler.md#dirty-set)).

### IR

- Inspector, chat, and GUI edits: unchanged. `edit_fact` alone on a quoted fact is a
  decree; a `provenance-pending` record opens `ratify`.
- New: a decree on a field whose sentence the harness cannot compose (`edges`,
  `transition`, `facets`, a criterion's `given`/`when`/`then`) writes a
  `phrase-needed` record beside `provenance-pending`. The proposal files with the
  mechanical fallback at once; `phrase-proposal` replaces it with a worded edit.
- New: hand-edited shards. The store records each shard's content hash in
  `status.yaml` at commit. A load that finds a shard changed without a commit diffs it
  against the journal's replayed state (the replay `gui/diff.rs` already does), and the
  build journals each changed field as a `decree` entry authored by the local user,
  note `hand edit of graph/<shard>.yaml`. Each staged decree passes the
  [validation gates](../docs/compiler/graph.md#validation-gates); a change a gate
  refuses is not landed, the shard is rewritten from the committed state, and an
  `invalid-graph-edit` error names the field and the gate. Ids, provenance,
  `created`, and `updated` are never editable by hand, the same as `edit_fact`.

### Tests

A `test-diverged` record on the requirement, written at derivation when any of its
test entries' artifacts diverge from origin in significant lines:

```yaml
- id: c501-0
  generation: 501
  mutation: 0
  kind: test-diverged
  subject: req:orders-6
  via: ledger
  detail:
    artifact: tests/orders.rs
    hunks:
      - {id: h1, criterion: crit:orders-5, kind: changed, weakened: false,
         literals: [{from: "31", to: "46"}]}
      - {id: h2, criterion: null, kind: added, after: crit_orders_5_c3d4e5f6}
```

`llm` rows' criteria files are test artifacts and diverge the same way.

### Code

A `code-diverged` record on the entity, written at derivation when any owned file
diverges from origin in significant lines:

```yaml
- id: c501-1
  generation: 501
  mutation: 0
  kind: code-diverged
  subject: ent:order
  via: ledger
  detail:
    files: [src/orders.rs]
    hunks:
      - {id: h1, file: src/orders.rs, requirement: null, attached: false,
         literals: [{from: "30", to: "45", matches: [req:orders-6, crit:orders-4]}]}
    rows: {req:orders-6: stale-code}
```

New files are not records. A file under the `gen.code` scope that no binding names is
the unclaimed report, as today. One exception keeps a small edit out of decompilation:
a new file whose directory holds only files one entity owns is attributed to that
entity and joins its `code-diverged` record as an unattached addition.

### The deliverable-edit entry

The build that first notices a divergence journals a `deliverable-edit` entry, one
mutation per diverged file, the way a docs save journals an `edit` entry
([journal](../docs/compiler/graph.md#journal)). The two records above carry its
generation as their cause. Every upward ripple is rooted in a generation, so
`jazyk ripple src/orders.rs` has a root.

### Records summary

| record kind | subject | written by | derives |
|---|---|---|---|
| `test-diverged` | requirement | the origin comparison at derivation | `lift-test` |
| `code-diverged` | entity | the origin comparison at derivation | `lift-code` |
| `phrase-needed` | the decreed node | a decree commit on a non-mechanical field | `phrase-proposal` |

Both divergence records clear when origin catches up (a lift resolves, a restore lands,
generation rewrites the file) or when the file returns to its origin bytes.

## New goal kinds

All three are compile class, mandatory, tier 3. They sit in the ledger tier because
they need a settled statement to compare against, and ahead of `bind` and `generate` on
the same target because both would rewrite the evidence before it is read.

### lift-test

`lift-test` reads a human edit to the tests of one requirement and says what it means
for the criteria.

- Unit: one requirement. Goal id `g:lift-test:req:<doc-stem>-<n>`.
- Skill: `upstream` (new, `docs/compiler/skills/upstream.md`).

Created when. A `test-diverged` record on the requirement.

Readiness. No tier 0 to 2 goal open or parked in the requirement's cone. `bind` on the
requirement waits for it: a `requirement-changed` or `artifact-gone` bind would rewrite
the test the human just edited. The pack reruns the diverged rows' programmatic tests
before the session starts, deterministically, as `verify` would, and records the
verdicts (verification baseline only).

Gate. `mark_goal_done` carries one entry per hunk:
`{hunk, verdict, criterion?, proposal?}`.

- `revises`: the test now checks a different outcome for an existing criterion. A
  quoted or decreed criterion needs a staged `upstream-change` proposal naming it; a
  derived criterion needs a staged `update_criterion` (stays derived, evidence added).
- `adds`: the hunk is a new check. A staged `upsert_criterion` with `derived`
  provenance and `evidence: {test: {artifact, name}}` on the requirement, or on
  another requirement the session names, and the ledger entry for it (below).
- `retracts`: a deleted test whose criterion the human dropped. A staged
  `upstream-change` proposing the criterion sentence's removal, or a staged
  `delete_criterion` on a derived one.
- `regression`: the edit lost a check by accident. Allowed only when the test fails or
  the hunk is `weakened`; files an `upstream-change` whose first option is `restore`.
- `refactor`: the edit changed nothing a criterion observes. Allowed only when the hunk
  is not `weakened` and every test entry of the requirement passes on the rerun.
- `handover`: a new check for behavior no requirement states. Allowed only when a
  `lift-code` goal is open on the entity; the hunk joins that goal's change, and
  `lift-code` stages the requirement with the criterion.

A weakened hunk is never `refactor`. A proposal on a weakened hunk is filed at warning
and says which value the test stopped checking.

Hints: the hunks with attribution, the rerun verdicts, the weakened flags, the moved
literals, the requirement's criteria with their provenance, and the target sections the
harness computed for new criteria.

What the model sees. The requirement in full with its criteria and quotes, the
criteria's source sections once each, the test artifact's diverged regions before and
after, and the verdicts. The contract paragraph is `goals/prompts/lift-test.md`: read
each hunk as a statement about observable evidence, classify it, stage the fact or the
proposal, word any sentence as a minimal edit of the sentence it changes.

Tools: the read tools, the goal tools, `upsert_criterion`, `update_criterion`,
`delete_criterion`, `propose_upstream` (new, below), `classify_hunks` (new, below),
and the read-only file tools when `serve_files` is set.

### lift-code

`lift-code` reads a human edit to one entity's implementation and says what it means
for the requirements.

- Unit: one entity. Goal id `g:lift-code:ent:<slug>`.
- Skill: `upstream`.
- Executor: code reading, so an attached coding agent is the natural executor, as for
  `bind` and decompilation ([executors](../docs/compiler/project-settings.md#executors)).

Created when. A `code-diverged` record on the entity.

Readiness. No tier 0 to 2 goal open or parked in the entity's cone, and no
`lift-test` open on its requirements (handed-over hunks arrive first; the two batch
together by ledger locality when the budget allows). `generate` on the entity waits for
it, and `bind` on the entity's requirements waits for it. The pack reruns the entity's
programmatic rows first, as in `lift-test`.

Gate. One entry per hunk, `{hunk, verdict, requirement?, proposal?}`:

- `implements`: the edit brings the code in line with a current statement. Allowed
  when the rows of the named requirement pass on the rerun.
- `revises`: the code now does something a stated fact says otherwise. Needs a staged
  `upstream-change` proposal naming the fact, with every criterion sentence that
  changes with it in the same option.
- `adds`: behavior no requirement states. A staged `upsert_requirement` with
  `derived` provenance (`from` the entity, `evidence` the file and head) and at least
  one criterion. The proposal follows at commit, as for every derived fact.
- `regression`: the edit breaks a stated fact by accident. Allowed only when a row of
  the named requirement fails on the rerun; files an `upstream-change` whose first
  option is `restore`. This replaces the bare `failing-binding` finding for rows whose
  files diverged: the check skips a failing row that a lift covers, the way it skips a
  pair a session already judged.
- `detail`: an unattached change with no observable obligation (a log line, an internal
  name). Graded like an invented choice: the session may file an `invented-choice`
  at `info`, never a proposal.
- `refactor`: no behavior change. Allowed only when every row of the entity passes on
  the rerun.

Hints: the hunks with attribution (site, unattached, new file), moved literals with the
statements and criteria each matches, rerun verdicts per row, the entity's unattached
ratio before and after, and target sections for additions.

What the model sees. The entity with its requirements and criteria in full (sites
listed per requirement), the source sections once each, the hunks before and after with
a few lines of context, and the verdicts. The contract paragraph is
`goals/prompts/lift-code.md`. It reuses the decompile contract's two evidence classes:
a hunk a test pins is `observed`, a hunk nothing asserts is `inferred`, and the
proposal says which. Behavior is not requirement: the session states what the code does
now; whether that is what it should do is the owner's answer.

Tools: as `lift-test`, plus `upsert_requirement` and `update_requirement` (derived
facts only; a quoted fact is refused toward `propose_upstream`).

The literal fast path. When every hunk of the goal carries a moved literal that matches
exactly one statement (and its criteria) and nothing else, the harness classifies the
hunks as `revises` itself and files the proposal mechanically: the located sentences
with the literal replaced. No session runs, as a programmatic `verify` runs no session.
Anything ambiguous goes to the model.

### phrase-proposal

`phrase-proposal` words the prose edit for a decree the harness could not phrase.

- Unit: one proposal diagnostic. Goal id `g:phrase-proposal:diag:ratification-pending-<n>`.
- Skill: `upstream`.

Created when. A `phrase-needed` record: a decree on `edges`, `transition`, `facets`,
or a criterion's structured fields, where `mechanical_sentence` has no template.

Readiness. Tier 3, no compile goal open in the fact's cone. The `ratify` goal it serves
stays blocked on a human meanwhile; it only gets a better option.

Gate. The proposal's `edit` option was rewritten by `propose_upstream` in this session:
`old_text` is the fact's former quote sentence, `new_text` passes the prose gates below,
and the decreed value is stated in it. Or `mark_goal_failed` with the reason (the
mechanical fallback then stands).

What the model sees: the decree (field, prior value, new value, author, note), the
quoted sentence and its section, and the other sentences that restate the fact.

### Extensions to existing kinds

- `generate`: a new readiness rule. Blocked while a `lift-code` goal is open on the
  entity, and `blocked {on: upstream answer}` while an unanswered `upstream-change` or
  `upstream-conflict` of rule `revises` names one of its requirements. Regenerating
  would answer the question by overwriting the code. The package gains
  `outsideEdits`: the hunks a lift read and classified `refactor` or `detail`, with the
  instruction to keep them unless a fact requires otherwise.
- `bind`: waits for `lift-test` on its requirement and `lift-code` on its entity.
- `verify`: rows whose files diverged from origin are rerun by the lift's pack; no
  separate `verify` derives for them while the lift goal is open.
- `ratify`: accepting an option with `edits` (plural) lands every edit. Proposals on
  lifted facts name the evidence on the card.
- Checks: `failing-binding` skips rows a lift covers. `pinned-fact-drift` gains an
  `edit` option when the drifted literal has a unique moved counterpart in the code
  (the literal pass): today it only offers answers.

### New tools

- `classify_hunks({goal, entries: [{hunk, verdict, requirement?, criterion?, reasoning}]})`:
  a batch verdict list, the same shape as `settle_pairs`
  ([goal tools](../docs/compiler/tools.md#goal-tools)). Each entry is gated as above
  when staged, so a false `refactor` bounces at once with the failing row named.
- `propose_upstream({subject, criteria?, kind: revise | retract | conflict, hunks,
  edits: [{doc, section, old_text, new_text, after_quote?}], statement?, reasoning})`:
  stages an `upstream-change` or `upstream-conflict` diagnostic with its prompt built
  by the harness (accept option from `edits`, `restore` option from the hunks,
  freeform). The model supplies the wording; the harness supplies the shape, so no
  session writes a prompt by hand.

### Prompt option extensions

- `edits`: a list of `{doc, section, old_text, new_text}` under one option, applied as
  one changeset, all or nothing: each `old_text` must locate at apply time, or the
  option is refused naming the one that did not. `edit` stays the one-edit form.
- `after_quote` on an insertion: the new sentence lands after the located sentence
  instead of at the section's end, so a new criterion lands next to its siblings.
  `add_requirement` already takes `after_quote`; the proposal shape catches up.
- `restore: {hunks}`: a deterministic option. Choosing it writes the origin bytes back
  over the named hunks, provided the current file still holds the hunk's after-lines
  (the same rule as `edit_doc_prose`: an edit made underneath skips with a report).
  It is journaled as `answer` and moves origin.

## The upward path

The path is the same for every layer:

```
outside edit → deliverable-edit entry → test-diverged / code-diverged record
  → lift goal (literal fast path, or a session)
      → derived fact + ratification-pending proposal   (additions)
      → upstream-change / upstream-conflict question   (revisions)
  → human: accept / restore / reword / retract
      → accept: dual write, provenance quote, downstream baselines absorbed
```

### Absorbing baselines

The dual write works because it absorbs the upstream baseline: the section hashes of
the document it rewrote move in the same changeset, so the edit does not dirty what it
came from. The upward path needs the mirror rule: an upward change absorbs the
downstream baseline it came from.

- A lift resolution moves origin for every hunk it classified. The edit has been read;
  the proposal carries the evidence from here on.
- A derived requirement or criterion lifted from existing code records its ledger row
  in the same commit: the files and head it came from, the test the human wrote (for a
  lifted criterion), the rerun verdict. It does not wait for a `bind` session to find
  what the lift already found.
- An accepted revision moves the entity's `factHash` in the same changeset when the lift
  judged the code to carry the new fact (`revises` with passing rows under the new
  value, or the code edit alone). The cascade still re-binds the test when the test
  asserts the old value, and `generate` does not reopen over code that already does
  what the accepted sentence says.
- An accepted restore moves origin to the restored bytes.

The ledger is not in the graph store, so the absorb is written under the same `.lock`
before the journal entry, and the journal entry names it (`absorbed: {entity,
factHash}`). A crash between the two leaves a ledger ahead of its journal; the worst
outcome is one extra `generate` over code whose edits were already read, which the
`outsideEdits` package keeps.

### Worked example (a): a constant, 30 to 45 days

The docs, `docs/orders.md#/orders/holds`: "Held orders expire after 30 days." The
examples section states two criteria: "An Order held on day 0 is still held on day 30."
and "An Order held on day 0 is expired on day 31." A developer changes
`const HOLD_EXPIRY_DAYS: u32 = 30;` to `45` in `src/orders.rs`. The line sits above the
first site, so it is unattached.

- `jazyk compile`: the origin comparison finds `src/orders.rs` diverged. The build
  journals `deliverable-edit g501` and writes `code-diverged` on `ent:order`.
- The literal pass: `30` → `45` on one line, matching `req:orders-6` and
  `crit:orders-4`. `crit:orders-5` states `31`, the day past the bound: its new value
  (`46`) follows from the edit but appears nowhere in it, so the fast path cannot
  complete the option. The goal goes to a session.
- The pack reruns the rows: `crit:orders-4` passes (day 30, still held),
  `crit:orders-5` fails (day 31, now still held).
- The `lift-code` session classifies `h1` as `revises req:orders-6` and stages one
  proposal with three edits: the statement's sentence and both criteria's sentences.

```yaml
diag:upstream-change-1:
  rule: upstream-change
  severity: warning
  subjects: [req:orders-6, crit:orders-5]
  message: "src/orders.rs now expires held orders after 45 days; docs/orders.md says 30."
  reasoning: "lift-code g502: HOLD_EXPIRY_DAYS moved 30 → 45; crit:orders-5 fails on the rerun. Evidence inferred (no test pins 45)."
  prompt:
    question: "The code now expires held orders after 45 days. Should docs/orders.md say 45?"
    options:
      - label: "Yes: the docs say 45 days"
        edits:
          - {doc: docs/orders.md, section: /orders/holds,
             old_text: "Held orders expire after 30 days.",
             new_text: "Held orders expire after 45 days."}
          - {doc: docs/orders.md, section: /orders/holds/examples,
             old_text: "An Order held on day 0 is still held on day 30.",
             new_text: "An Order held on day 0 is still held on day 45."}
          - {doc: docs/orders.md, section: /orders/holds/examples,
             old_text: "An Order held on day 0 is expired on day 31.",
             new_text: "An Order held on day 0 is expired on day 46."}
      - label: "No: restore 30 days in src/orders.rs"
        restore: {hunks: [h1]}
    freeform: true
```

- `generate` on `ent:order` is `blocked {on: upstream answer}`. The verdict reads
  `converged, 1 blocked`.
- Accept: one `answer` changeset. The three sentences land; `req:orders-6` and both
  criteria re-anchor with statements updated mechanically (the replaced text appears
  verbatim in them); `factHash` is absorbed. The cascade: `criterion-revised` stales
  the two tests, `bind` rewrites them to 45 and 46, both pass, the row reads
  `verified`. No `generate`, no `reconcile-section`.
- Restore: `45` goes back to `30`, origin moves, the row reruns green. Nothing in the
  docs moved.

```
deliverable-edit g501 src/orders.rs (outside)
└─ lift-code ent:order g502: h1 revises req:orders-6 (45 days) → diag:upstream-change-1
   └─ answer diag:upstream-change-1 g503 (human): accepted, 3 sentences, factHash absorbed
      ├─ bind req:orders-6 g504: criteria revised, tests rewritten → verified
      └─ rejudge-pair req:orders-6~req:payment-9 g505: consistent
converged: 2 sessions, 0 generate
```

### Worked example (b): a new negative test

The developer adds a test below `crit_orders_5_c3d4e5f6` in `tests/orders.rs`: a hold
of `-1` days is refused with `invalid-duration`. The code already refuses it.

- `test-diverged` on `req:orders-6`, one `added` hunk attributed after the last
  recorded test.
- The pack runs the new test through the artifact's recorded runner style: it passes.
- `lift-test` classifies `h2` as `adds` and stages
  `crit:orders-6: "A hold of -1 days is refused with invalid-duration."`, kind
  `negative`, `derived: {from: [req:orders-6], evidence: {test: {artifact:
  tests/orders.rs, name: hold_rejects_negative_days}}}`. The ledger entry for the new
  criterion is recorded in the same commit with the developer's test name and the
  passing verdict: the name suggestion is a suggestion
  ([file ownership](../docs/consumers/gen.md#file-ownership-and-conventions)).
- The commit files the ratification proposal. Its target is the section the other
  criteria of `req:orders-6` quote, inserted `after_quote` the last of them, in their
  shape (a list item when they are list items).
- Nothing downstream opens: the criterion is bound and green. `converged, 1 blocked`.

When the new test fails (the developer wrote the test first), the same lift lands the
derived criterion, the row reads `partial`, and `generate` makes it pass. Additions do
not block generation; only open revisions do. Test-first development works from the
test upward and the product downward in one build.

### Worked example (c): a new endpoint with no requirement

The developer adds `GET /orders/{id}/history` to `src/api.rs`, owned by `ent:order-api`,
with a test in `tests/api.rs`.

- `code-diverged` on `ent:order-api`: one unattached hunk. `test-diverged` on the
  requirement whose test region the new test landed in.
- `lift-test` runs first (same tier, the test goal targets a requirement of the entity)
  and classifies the new test as `handover`: it checks behavior no requirement
  states, so it stages nothing and the hunk joins the `lift-code` goal's change.
- `lift-code` classifies the hunk `adds` and stages `req:x-7`: "The Order API returns
  the status history of an Order by its id.", derived from `ent:order-api` with the
  file and head as evidence, and the criterion from the developer's test. The ledger
  row lands with the sites and the test. The unclaimed and unattached counts drop.
- The proposal targets the section that sources most of the requirements whose sites
  live in `src/api.rs` (code locality maps to document locality), after the sentence
  of the neighboring endpoint.
- A new file `src/history.rs` in a directory `ent:order-api` owns alone would join the
  same record. A new file in a new directory stays unclaimed and goes to decompilation.

### Worked example (d): an architect edits a requirement in the graph

In the GUI inspector, the architect changes the cardinality of the edge behind
"An Order holds one or more Order Items." from `1..*` to `*`.

- `edit_fact` on `edges`: no mechanical sentence, so the inspector offers no rewrite
  and the edit lands as a decree: `decree` entry, `provenance-pending`, the
  `ratification-pending` proposal with the fallback sentence, and `phrase-needed`.
- Downstream as today: the relationship recomputes and the class view redraws. The
  statement hash did not move, so no `bind` derives; the entity's facts did, so
  `generate` derives.
- `phrase-proposal` rewrites the proposal: `old_text` "An Order holds one or more
  Order Items.", `new_text` "An Order holds any number of Order Items." Accept flips
  the requirement back to `quote` on the new sentence.
- When `docs/cart.md` restates the same fact ("Each order has at least one item."),
  the `duplicate-requirement` info pair links them, and the proposal carries both
  sentences in one `edits` option. The same holds for a statement edit that the
  inspector phrased mechanically: the restating sentences join the option.
- A hand edit of `graph/requirements.yaml` doing the same thing takes the same path
  after the shard check reads it as a decree.

### Worked example (e): the same fact in docs and code in one change set

Consistent. The developer's branch changes "30 days" to "45 days" in `docs/orders.md`
and the constant to 45 in one commit, then runs `jazyk compile`.

- The `edit` entry dirties the section; the `deliverable-edit` entry records the file.
- Tiers 1 and 2 run first: `reconcile-section` revises `req:orders-6` and the
  criteria to 45 and 46.
- `lift-code` compares the hunk to the revised facts: `30` → `45` is the new
  statement. Verdict `implements`, gated on the rows passing, but the tests still
  assert 30 and 31, so the rows fail. The gate allows `implements` when the only
  failing criteria are `criterion-revised` in this window (their tests are stale by
  construction). No question is filed; origin moves.
- `bind` re-binds the stale tests to 45 and 46, they pass. `generate` does not open:
  the lift absorbed the `factHash` because the code carries the new facts.

Inconsistent. The docs say 45, the code says 60.

- Tiers 1 and 2 revise the graph to 45: the docs are the source.
- `lift-code` finds `30` → `60` against a statement revised to 45 in the same window.
  A revision against a fact whose quote changed after the file's origin is a conflict,
  not a proposal. The session stages `propose_upstream` with `kind: conflict`:

```yaml
prompt:
  question: "docs/orders.md now says 45 days and src/orders.rs now says 60. Which holds?"
  options:
    - label: "45 days: restore the code and regenerate"
      restore: {hunks: [h1]}
    - label: "60 days: the docs say 60"
      edits: [ ... the statement and criteria sentences, 45 → 60 ... ]
  freeform: true
```

- `generate` on `ent:order` is blocked on the answer. Choosing 45 restores `30` and
  the open `generate` (facts changed) writes 45; choosing 60 is a dual write and the
  code already agrees.

Docs and a decree. A decree on the statement plus a docs edit to the same sentence in
one window: the docs edit reconciles first and the requirement's quote moves; the
decree already made the fact `decree`, and the compiler never overwrites a decree, so
`reconcile-section` files the existing contradiction path (a quoted statement against a
decreed one, `rejudge-pair`). No new machinery.

## Mixed changes

### Order within one build

A build that starts with edits in every layer journals its roots in a fixed order, each
its own generation:

1. Shard hand edits: `decree` entries. The graph must be valid before anything reads it.
2. Document saves: the `edit` and `align` entries, as today.
3. Deliverable divergence: the `deliverable-edit` entry.

The tiers then order the work, unchanged at the top: alignment, ingest, judgment, and
in tier 3 the lifts before the ledger goals that would rewrite their evidence:

| tier 3 kind | waits for (same target or entity) |
|---|---|
| `lift-test`, `phrase-proposal` | tiers 0 to 2 in the cone |
| `lift-code` | tiers 0 to 2 in the cone, `lift-test` on its requirements |
| `derive-criteria` | as today |
| `bind` | `lift-test` on the requirement, `lift-code` on its entity |
| `generate` | `bind` (as today), `lift-code`, unanswered revision questions |
| `verify` | as today; diverged rows are rerun by the lift pack |

The lifts compare the code against the graph after the documents reconciled, which is
what makes "consistent" and "conflict" decidable.

### Atomicity

- Each root is one generation and one changeset.
- A lift session commits one changeset: the derived facts, the questions, the ledger
  rows it lands, and the origin moves. All or nothing, with the retry-then-park rule of
  every session.
- An accept is one changeset: the prose edits, the graph mutation, the provenance flip,
  the diagnostic resolution, and the absorb. `edits` apply all or nothing.
- The origin bytes are content-addressed, so writing them first and committing the
  ledger reference after is safe: an orphan blob is pruned.

### No loops

The upward path cannot cycle without a person in it:

- Every automated write to the deliverable (generation, binding, restore) moves origin,
  so no automated write ever opens a lift.
- Every lift moves origin for what it read, so the same bytes never open a second lift.
- No lift moves a quoted fact. Only an accept moves the documents, and an accept is a
  human act.
- An addition lands as a derived fact bound to the code it came from; its row is
  current at commit, so it opens neither `bind` nor `generate`.
- The remaining automated cycle candidate is a derived fact a lift updates and a later
  GC session undoes (`abstract-entity` moving a lifted requirement, `review-entity`
  merging a lifted entity). Flip detection covers it by extension: lift sessions are
  a third class in the replay (`upstream`), and two flips between upstream and GC on
  one key park the pair with an `unstable-derivation` question, as today.

Human-driven oscillation (a developer restoring 45 after an owner chose 30) is not a
loop: each round is a new edit with a new root, and each asks once. Answers stick to
the hunk's normalized content: re-applying the exact edit a person already rejected
re-asks nothing and leaves the row `failing` with the answer cited in the finding.

### Convergence with pending proposals

- Lift goals are mandatory: an unread outside edit keeps the build `incomplete`. This
  is deliberate. A build that converges over code nobody reconciled would report a
  fixed point that is not one.
- Their outcomes are blocked goals: `answer` for `upstream-change` and
  `upstream-conflict`, `ratify` for lifted derived facts. They ride as counts:
  `converged, 3 blocked`.
- A `generate` blocked on an upstream answer counts as blocked, not open.
- A no-op rebuild derives zero lift goals: nothing diverges from origin.
- The verdict line gains nothing new. `jazyk status` gains one line (below).

## Prose editing quality

Upward edits are only useful if an owner would have written them. The rules:

- One sentence per edit. `old_text` is the whole sentence that holds the fact's located
  quote, expanded to sentence boundaries by the splitter the `quote-multi-sentence`
  gate uses. `new_text` is one sentence (the same gate).
- Minimal. The harness diffs `old_text` and `new_text` by tokens. A revision changes
  only the tokens the fact changes: the gate refuses an edit whose changed tokens
  exceed a bound (start at four tokens or a third of the sentence, whichever is larger;
  measure on the cases) unless the call carries `rewrite: true` with a reason. Untouched
  sentences are untouched by construction: `edit_doc_prose` replaces only the located
  run.
- The documents' voice. The session sees the whole section and its neighbors. The
  sentence uses the entity names and aliases the documents use, never a code
  identifier the documents do not use (`HOLD_EXPIRY_DAYS` never reaches the prose; a
  value in a code span stays in a code span). The gate warns on an identifier-shaped
  token (snake case, a path, camel case) absent from the section and the name index.
  The project's lint rules run on `new_text` (the same rules
  [plain-English lint](../docs/consumers/docsgen.md#plain-english-lint) applies), and a
  violation is a refusal with the rule named.
- Extractable. The accepted sentence becomes the quote; the fact's `statement` follows
  mechanically when the replaced text appears in it verbatim, and otherwise the
  proposal carries `statement`, so no `reconcile-section` is owed. The same
  [wording](../docs/compiler/concepts/statements.md#wording) rules the extractor follows
  apply: specific, testable, entity-anchored, one obligation.
- Placement of a new sentence. The harness ranks at most three candidate sections and
  the model picks one, plus an `after_quote` anchor inside it:
  1. The section that sources most requirements whose sites share a file with the
     hunk (for a test: the section the requirement's other criteria quote).
  2. The section quoted by most of the fact's `from` nodes (the existing rule).
  3. The section of the entity's first mention.
  The target is always an existing section in an existing document; a proposal never
  creates a document. An oversized section keeps its split advice. An entity no
  document mentions has no target: its behavior is decompilation's territory.
- Test-case sections. A new criterion whose siblings quote a test-case or examples
  section lands there, in the siblings' form (a list item beside list items, the
  lead-in and values form of [test cases](../docs/compiler/concepts/statements.md#test-cases-state-obligations)).
- Multi-document facts. Every sentence that restates the fact joins one option: the
  requirement's own quote, the other side of every `duplicate-requirement` info pair
  on it, and the quotes of its criteria that carry the changed value. A document that
  only mentions the entity is not edited. The option is all or nothing.

## Division of labor

| concern | harness | model | human |
|---|---|---|---|
| detect outside edits | origin comparison, `deliverable-edit` entry, records | | |
| what a hunk touches | attribution to sites, test regions, literals, weakened flags | | |
| rerun evidence | the pack's test runs, verdicts | | |
| goals | derivation, readiness, blocking `generate` and `bind` | never creates, routes, or orders them | |
| what a hunk means | gates each verdict against verdicts and flags | `refactor`, `implements`, `revises`, `adds`, `regression`, `detail` | |
| obvious literal moves | the fast path, mechanical proposal | | |
| wording | minimality, one sentence, lint, identifier warning | the sentence, in the documents' voice | rewords freely |
| placement | candidate sections, existence of the target | picks one and the `after_quote` | may write it anywhere |
| the prompt | builds the options from `edits` and hunks | | |
| the decision | | | accept, restore, reword, retract |
| applying | dual write, restore, absorb, origin moves, journal | | |

The model never edits a document and never edits the deliverable in a lift session. It
stages facts and proposals; the human's answer is the only thing that writes upward.

## Frontends

### CLI

- `jazyk status` gains one line: `upstream: 2 diverged (1 test, 1 code), 3 questions,
  1 conflict`, with the entities named under `--verbose`.
- `jazyk explain g:lift-code:ent:order`: the record, the hunks with their attribution
  and literals, the rerun verdicts, what the goal blocks (`generate ent:order`).
- `jazyk ripple <deliverable path>`: the cascade rooted at the last `deliverable-edit`
  entry that recorded the file, like a document path today.
- `jazyk answer` unchanged in form. An option with `edits` prints every edit; a
  `restore` option prints the hunks it writes back.
- `jazyk lift [entity|path...]`: run only the lift goals of the named targets (and
  their fast paths), the same way `jazyk gen` narrows to generation. Useful from a
  pre-commit hook: the developer sees the questions their edit raises before pushing.

### GUI

- The Inbox's [Proposals](../docs/frontends/gui.md#proposals) section lists
  `upstream-change` and `upstream-conflict` beside ratification proposals, grouped by
  target document. Each item shows the code hunk and the prose diff side by side:
  "this change in `src/orders.rs`" over "this change in `docs/orders.md`", with
  Accept, Restore, and a reword box. Conflicts show the three versions.
- The [product file viewer](../docs/frontends/gui.md#product-file-viewer) marks lines
  that diverge from origin with a distinct gutter color from the generation diff, and a
  code lens on each hunk names the question it raised.
- The [trace matrix](../docs/frontends/gui.md#trace-matrix) shows `diverged` as a
  chip beside the status, and the repair sentence names the lift goal.
- The [inspector](../docs/frontends/gui.md#inspector) shows a pending revision on a
  quoted fact as "Proposed from code: 45 days" under the statement, linking the
  question.

### LSP

- Documents: `upstream-change` and `upstream-conflict` publish at the target sentence
  with `Apply:` and `Answer:` code actions, as every prompted diagnostic does
  ([capabilities](../docs/frontends/lsp.md#capabilities)).
- New: the LSP publishes the same diagnostics on deliverable files the ledger owns, at
  the hunk's located `head`: "this edit proposes: Held orders expire after 45 days."
  with the same code actions. The developer answers in the file they just edited.
  The LSP stays read-only on the graph: code actions call the same
  `jazyk.answerDiagnostic`.

### MCP and coding agents

The agent that made an edit knows why. The best lift is the author's own.

- `upstream_preview({files?})`, a read tool on every serving: the origin comparison for
  the named files or all, computed now, no session: the hunks, their attribution, the
  moved literals, the requirements and criteria they touch, and the proposal the fast
  path would file. An agent editing code sees which documented facts its edit changes
  before it finishes.
- `lift_tasks`, `begin_lift`, `record_lift` in the `generate` and `all` servings: the
  lift goals as a lifecycle, like `binding_tasks`, `begin_binding`, `record_binding`.
  `record_lift` takes the same entries `classify_hunks` and `propose_upstream` take and
  applies the same gates. An agent that just changed the timeout records
  `revises req:orders-6` with its own reasoning ("the ticket asks for 45") while the
  intent is fresh.
- `await_changes` carries `upstream: {diverged, questions}`.
- Chat: the dual-write tools stay the chat's way to move prose. A chat agent that edited
  code through its own tools calls `upstream_preview` and offers the resulting
  proposal in conversation, then `answer_diagnostic` applies it.

## Risks and failure modes

- Refactor noise. A rename or an extract-function touches many lines. Mitigations: the
  significant-line normalization drops formatting; the `refactor` verdict is cheap and
  gated on green tests; one goal per entity collects every hunk since origin, so a
  sequence of commits is read once; lifts derive at build time, never on save.
- Proposals no one wants. A developer's internal change surfaced as a question is a
  cost. `detail` exists for that, graded like an invented choice and suppressible. The
  acceptance rate per rule is measured (below); a rule that is mostly rejected is a
  contract problem, not a human problem.
- Generated code edited by hand, then regenerated. The `generate` block and the
  `outsideEdits` package are the guard. A `refactor` or `detail` hunk can still be
  rewritten by a later generation when a fact requires it; the package says so and the
  journal records it.
- Weakened tests. A test edited to pass (a boundary removed, an assertion loosened) is
  the classic failure. The weakened flag is deterministic, blocks `refactor`, and puts
  `restore` first. It cannot catch every weakening (an assertion rewritten to a
  tautology keeps the values); `bind`'s falsifiability rule still applies when the
  test is re-bound.
- Wrong attribution. A hunk attributed to the preceding site may belong to the next
  requirement. The model sees the attribution as a hint, the gate checks the named
  requirement's rows, and a wrong `revises` target shows up as an edit whose
  `old_text` holds no changed value (refused).
- Stale origin after upgrade. A project without origin baselines starts with origin
  equal to the current bytes at the first build: edits made before the upgrade are
  never lifted. Acceptable, and stated in the release note; no migration code.
- Origin storage growth. Content-addressed, pruned by reference, source files only.
  Built artifacts and large binaries are excluded by the `produces` rule.
- A lift that misdescribes. The self-check from decompilation applies: a lifted fact
  binds to the code it came from and must read `verified`. A lifted fact that reads
  `failing` at commit is refused (the session repairs it or fails the goal).
- Concurrency with a running build. The origin comparison runs at derivation; an edit
  landing mid-build is picked up by the next derivation. The build lease already
  serializes writers of the out directory.
- Hand-edited shards that break invariants. Every field passes the write gates; a
  refused field is restored and reported, never half-applied.

## Measuring success

The project's doctrine: replays and benchmark cases per scenario, never whole-project
reruns ([benchmark](../docs/benchmark/benchmark.md),
[replays](../docs/benchmark/replays.md)).

- Replays (harness only, scripted calls): each gate of `classify_hunks` and
  `propose_upstream` (a false `refactor` with a red row, an `old_text` that does not
  locate, a two-sentence `new_text`, an edit over the token bound, a lint violation);
  the `edits` all-or-nothing apply; `restore` over a file edited underneath; the
  absorb (an accepted revision opens no `generate`); the no-loop property (accept →
  rebuild derives zero lift goals).
- Unit tests (`cargo test`, no model): origin recording at each write, divergence with
  whitespace-only edits, attribution to site runs and test regions, the literal pass,
  weakened detection, readiness (`generate` blocked, `bind` waits), the shard decree.
- Benchmark cases, one per scenario, each a fixture with an origin snapshot, an edited
  deliverable, and a planted ground truth, graded deterministically:
  - `lift-constant`: the 30 → 45 edit; the verdict is `revises req:orders-6`, the
    option carries the statement and both criteria, and every byte outside the three
    sentences is unchanged.
  - `lift-negative-test`: one `adds` criterion on the right requirement, kind
    `negative`, placed after the last sibling.
  - `lift-endpoint`: one derived requirement on the right entity with a criterion;
    zero proposals on the existing requirements.
  - `lift-refactor`: an extract-function over two sites; zero proposals, zero facts.
  - `lift-weakened-test`: a boundary assertion removed; `regression` or `revises`,
    never `refactor`, `restore` first.
  - `lift-mixed-consistent` and `lift-mixed-conflict`: zero questions and one conflict.
  - `phrase-edge-decree`: a cardinality decree reworded within the token bound, the
    restating sentence in a second document included.
- Snippets from real projects: `jazyk benchmark --project <dir> --goal
  g:lift-code:<entity>` on a copy of `example-sort` and `example-erp` with a planted
  edit ([snippets](../docs/benchmark/benchmark.md#snippets-from-a-real-project)).
- Measures reported beside the verdicts: proposals per behavior-changing hunk
  (precision against the planted truth), refactor false positives (target zero), the
  token distance between the proposal and the planted sentence, and, on the dogfood
  once it runs, the acceptance rate per rule.

## Implementation order

Each step names the docs pages first, then the modules. Each step ships alone and is
committed and pushed on its own.

1. Doctrine. Docs only.
   - `docs/compiler/concepts/upstream.md` (new): the fact classes, intent by edit site,
     the precedence rules, the absorb rule. `docs/compiler/compilation.md#edit-paths`:
     the fourth path continues upward and links it. `docs/compiler/model.md#provenance`:
     `derived` may carry `evidence`. `docs/consumers/decompile.md`: where lifting ends
     and decompilation begins.
2. The origin baseline. No model.
   - Docs: `docs/consumers/gen.md#the-ledger` (`origin`, `gen/origin/`),
     `docs/compiler/graph.md#journal` (`deliverable-edit`),
     `docs/frontends/cli.md#jazyk-status` (the upstream line),
     `docs/frontends/mcp.md` (`upstream_preview`).
   - Code: `gen.rs` (record origin at `record_generation` and `record_binding`, prune),
     `verify.rs` (divergence and attribution beside `status_of`; test runs leave origin
     alone), `model.rs` (journal kind), `reconcile.rs` (the entry at build start),
     `cli.rs`, `mcp.rs`, `Cargo.toml` (`similar`). Unit tests for every rule.
3. The literal pass and the prompt extensions. No model.
   - Docs: `docs/compiler/model/diagnostic.md#prompts` (`edits`, `after_quote`,
     `restore`), the two new rules in the catalog, `docs/compiler/compilation.md#checks`
     (`pinned-fact-drift` gains an edit, `failing-binding` defers to lifts).
   - Code: `model.rs` (prompt option shapes), `answer.rs` (apply `edits` all or nothing,
     `restore`, absorb), `reconcile.rs` (the checks), `store.rs` (the gates). Replays for
     the apply paths.
4. `lift-code` with its fast path.
   - Docs: `docs/compiler/goals/lift-code.md`, `docs/compiler/goals/prompts/lift-code.md`,
     `docs/compiler/skills/upstream.md`, `docs/compiler/reconciler.md#the-catalog` and
     `#tiers`, `docs/compiler/goals/generate.md#readiness` (the block and
     `outsideEdits`), `docs/compiler/graph.md#change-records` (`code-diverged`),
     `docs/compiler/tools.md` (`classify_hunks`, `propose_upstream`).
   - Code: `goals.rs` (the kind, its gate, its hints, `record_kinds`), `board.rs`
     (readiness, the `generate` block), `tools.rs` (the two tools and their gates),
     `session.rs` (payloads), `gen.rs` (`outsideEdits` in the package), `reconcile.rs`
     (flip detection's third class). Benchmark cases `lift-constant`, `lift-endpoint`,
     `lift-refactor`, `lift-mixed-*`.
5. `lift-test`.
   - Docs: `docs/compiler/goals/lift-test.md`, its contract paragraph,
     `docs/compiler/goals/bind.md#readiness`, `docs/compiler/graph.md#change-records`
     (`test-diverged`), `docs/compiler/model/criterion.md#lifecycle` (a criterion lifted
     from a test).
   - Code: `goals.rs`, `board.rs`, `verify.rs` (weakened detection), `bind.rs` (the
     wait). Cases `lift-negative-test`, `lift-weakened-test`.
6. The MCP lift lifecycle for coding agents.
   - Docs: `docs/compiler/tools.md` (`lift_tasks`, `begin_lift`, `record_lift`),
     `docs/frontends/mcp.md#toolsets`, `docs/compiler/control-plane.md` (the lease).
   - Code: `mcp.rs`, `control.rs`. An agent-run case through `jazyk mcp benchmark`.
7. Decree completeness.
   - Docs: `docs/compiler/goals/phrase-proposal.md`,
     `docs/compiler/goals/ratify.md#the-proposal` (restating sentences join the option),
     `docs/compiler/graph.md#storage-layout` (shard hashes, hand edits as decrees).
   - Code: `goals.rs`, `gui/facts.rs` (`phrase-needed` beside the fallback), `docsgen.rs`
     (the option composition), `store.rs` (the shard check). Case `phrase-edge-decree`.
8. Frontends.
   - Docs: `docs/frontends/gui.md#proposals`, `#product-file-viewer`, `#trace-matrix`,
     `#inspector`; `docs/frontends/lsp.md#capabilities` (deliverable files);
     `docs/frontends/cli.md` (`jazyk lift`, `ripple` on a deliverable path).
   - Code: `gui/api.rs`, `gui/deliverable.rs`, `gui/product.rs`, `bootstrap/app`,
     `lsp.rs`, `cli.rs`. Finish with `npm run build` and `cargo build --release`.
9. Measure. `example-sort` (code beside docs) and `example-erp` with planted edits as
   snippets; record results in `bootstrap/VALIDATION.md`; move open items to
   `docs/TODO.md`.

## Open questions for the owner

- Auto-accept. Should a project be able to adopt test lifts without asking
  (`[workflow] upstream = "adopt-tests"`), since a test edit is explicit intent? The
  plan asks always; an opt-in mode is one setting away.
- Lift goals mandatory. The plan makes an unread outside edit block `converged`. The
  alternative, optional lifts that only advise, keeps the verdict quiet in projects
  where people edit code freely, at the price of a fixed point that ignores them.
- `restore` in the working tree. Restoring bytes in files a developer is editing is
  explicit (a chosen option), but some teams will prefer the `restore` option to open
  `generate` instead and let the worker write it. Both are deterministic to offer.
- The new-file rule. Attributing a new file to the entity that owns its directory
  alone is a heuristic. The alternative is to send every new file to decompilation.
- Evidence on derived provenance. `evidence` points into the deliverable, which is not
  a node; justification closure ignores it. Should a lifted fact whose evidence file is
  gone draw a finding of its own?
