# Compilation

A build brings the [graph](./model.md) in line with the documents and the rest of the
system in line with the graph: docs, graph, diagrams, deliverable, and tests as one fixed
point. The [reconciler](./reconciler.md) computes what is stale and orders the work;
this file describes what a build does with it, from dirty set to verdict. The
[control plane](./control-plane.md) decides whether anyone may act on the work at all.

## A build

```
parse all documents → align section trees → dirty set → edit generation (change records)
  → derive the board: "compile: N goals (k reconcile-section, ...), b blocked"
  → sessions, one batch at a time, highest ready tier first:
      tier 0  place-anchors        (a document's alignment proposals)
      tier 1  reconcile-section    (link levels, roots first)
      tier 2  rejudge-pair, review-entity, retrace, conform-instance
      tier 3  bind, generate, verify   (ratify and answer wait on a human)
    every commit recomputes derived data, writes change records, re-derives the board
    GC bursts fire as cones quiet: declare-edges, dedupe-candidates, curate-view,
      split-view, abstract-entity
  → checks (deterministic)
  → render every view to diagrams, docsgen
  → verdict: converged with counts, or incomplete with parked work
```

The first build and every rebuild run the same lifecycle. The first build starts from an
empty graph, so every section is dirty. A rebuild with no changes derives zero goals and
makes zero LLM calls.

- Parse and align. [Parsing](./parsing.md) produces section trees;
  [alignment](./alignment.md) applies exact moves mechanically and persists proposals
  for the rest. The [dirty set](./reconciler.md#dirty-set) lands as change records under
  an `edit` generation.
- Derive the board. The terminal prints the summary first
  ([`jazyk compile`](../frontends/cli.md#jazyk-compile)). E.g.:
  `compile: 27 goals (12 reconcile-section, 9 rejudge-pair, 6 retrace), 3 blocked`.
- Sessions. The [scheduler](./reconciler.md#scheduling) takes the highest ready tier,
  groups goals by locality, fills one batch under the context budget, and runs one
  [session](./sessions.md) for it. The session sees exactly the batch's goals, the
  loaded graph for their locality, and one summary line for the rest. Write tools stage
  mutations; `done` runs the batch gates; the changeset commits atomically
  ([changesets](./graph.md#changesets)).
- Commit. Derived data recomputes (relationships, state machines, default views), change
  records land, the journal entry records resolved goals with their justifications and
  opened goals with their causes ([journal](./graph.md#journal)), the deterministic sweep
  runs ([garbage collection](./graph.md#garbage-collection)), and every view whose
  `.puml` changed renders again ([diagrams](./diagrams.md#rendering)). The board
  re-derives; the live trace and the [GUI board](../frontends/gui.md#board) show counts
  ticking down.
- The tail. When no goal is ready or a budget is spent, the [checks](#checks) run, the
  [requirements documents](../consumers/docsgen.md) regenerate, and the
  [verdict](#convergence) lands in `status.yaml`. The tail needs no model.

Sessions are sequential: one build at a time under the build lease, one session at a
time within it ([sequential builds](./control-plane.md#sequential-builds)).

## Compile and garbage collection

Goals come in two classes, and a build interleaves them in bursts.

- Compile goals bring the graph in line with the documents: dirty sections, changed
  statements needing re-judgment, dangling references, instance conformance, stale
  ledger rows. They are mandatory.
- Garbage collection (GC) goals restructure and tidy: decoupling, splitting, combining.
  An entity over its requirement cap, a level over its fan-out
  ([fan-out](./reconciler.md#fan-out)), a view over its member cap, lookalike
  duplicates, missing edges, view curation. They are optional until a hard threshold
  makes them mandatory ([escalation](./reconciler.md#escalation)).

GC also names the store's deterministic sweep at commit (orphaned facts deleted,
tombstone redirects left, stranded diagnostics settled): the sweep is the mechanical
half, the GC goals the judgment half ([garbage collection](./graph.md#garbage-collection)).

One rule ties the classes together: a GC goal becomes ready only when no compile goal is
open in its target's [cone](./reconciler.md#cones). Nothing waits for a global phase. As
each cone's compile goals settle, its GC goals become ready and the scheduler runs them
right there, often in the session that just finished the cone
([bubbling](./reconciler.md#bubbling)). The trace prints one line per burst. E.g.:

```
gc burst: abstract-entity ent:order (54 > 50)
```

A GC session sees the cone's final counts, so an entity that doubled its requirements
this build is abstracted once, holistically. GC mutations can reopen compile goals: a
split entity writes `entity-changed` records for the new parents and children and
`requirement-revised` records for the statements it moved, and the loop runs compile for
that cone and returns. [Flip detection](./reconciler.md#flip-detection) and the budgets
bound the alternation; compile outranks GC when the build budget is tight.

## Edit paths

Every human edit enters as one of four paths, and all four end the same way: change
records written, goals derived, sessions scheduled, fixed point restored. Every path
journals its own generation, so the root of every ripple is itself a generation
([journal](./graph.md#journal)).

- Edit prose. Parse, align, dirty sections, `reconcile-section`, cascade. The journal
  entry is `edit`.
- Edit a quote-provenanced fact through a diagram, the GUI inspector, or chat. The
  fact's provenance names the sentence, so the edit is a dual write
  ([dual-write tools](../frontends/acp.md#dual-write-tools): `revise_requirement`,
  `add_requirement`, `retract_requirement`, `edit_fact`). The model proposes the
  sentence rewrite, the human accepts it, and the prose replacement commits with the
  graph mutation in one changeset (journal entry `dual-write`). Changing a
  class-diagram arrow's cardinality rewrites the sentence behind the requirement that
  declared it. The commit absorbs the new section hashes, so the edit does not dirty the
  document it just changed; downstream goals derive from the graph change. When no
  proposed rewrite is accepted, the edit falls through to the decree path: the compiler
  never rewrites a source document without an accepted proposal.
- Edit a derived fact, or add a fact with no prose behind it: a decree. The changeset
  lands graph-only with `decree` provenance (journal entry `decree`). Downstream goals
  derive normally. Upward, the commit writes a `provenance-pending` record and a
  [`ratify`](./goals/ratify.md) goal derives, blocked on a human: docsgen renders the
  sentence the docs should gain as a `ratification-pending` diagnostic whose prompt's
  `edit` option inserts it
  ([ratification proposals](./model/diagnostic.md#ratification-proposals)). Accepting
  the proposal is one changeset (journal entry `ratify`): the sentence lands in the
  document, the fact's provenance flips to `quote` on that sentence, the diagnostic
  resolves, and the `ratify` goal is gone. The commit absorbs the new section hashes
  like a dual write, so the inserted sentence dirties no section and no
  `reconcile-section` follows it: the flip is the ratify changeset's own work, never a
  later session's. Retracting deletes the decree. The compiler never overwrites a
  decree; it files diagnostics against it. Derived facts a GC session creates
  (`abstract-entity`'s new parents) take the same path.
- Edit the deliverable or tests. Ledger statuses flip (`stale-code`, `stale-test`;
  [status is derived](../consumers/gen.md#status-is-derived-never-stored)) and
  `verify` goals derive. The rerun's verdict decides what follows: an `unimplemented`
  outcome (a `fail` on a row with no implementing files) derives `generate`; a `fail`
  on a row with implementing files is a `failing` row, a finding, never a goal; a gone
  artifact derives `bind` ([the cascade](../consumers/gen.md#the-cascade)). The
  [unclaimed report](../consumers/bind.md#the-unclaimed-report) feeds decompile drafts.

Deletion runs the same paths in reverse. A removed section leaves its anchors homeless:
`place-anchors` and `reconcile-section` decide what the facts anchored there become
(re-recorded elsewhere, revised, or deleted). The sweep at commit deletes what is left
without provenance, leaves tombstone redirects, and writes `node-deleted` and
`view-member-gone` records for the live nodes that referenced the dead ones; those derive
[`retrace`](./goals/retrace.md) goals on the views, instances, and derived facts that
depended on them. Derived data (relationships, state machines, default views) simply
recomputes.

Anything the deliverable needs that the documents do not state is an ambiguity.
Generation does not stall on one: it chooses with best judgment, records the choice as an
`invented-choice` diagnostic graded by the scope of the invention (error, warning,
suppressible info; [invented choices](../consumers/gen.md#invented-choices)), and the
unattached remainder of the deliverable measures the debt
([the unattached remainder](../consumers/gen.md#the-unattached-remainder)).
Ratification is how the debt is repaid: every derived and decreed fact carries a
proposal for the sentence the docs should gain, and the graph converges toward fully
quoted.

## Checks

The checks are deterministic lint over the whole graph. They run at the end of every
build and on [`jazyk check`](../frontends/cli.md#jazyk-check), file their findings as
sticky diagnostics ([rules catalog](./model/diagnostic.md#rules-catalog)), and never
re-ask a question a person already answered. Sessions never see them: an empty file has
no section to schedule, and a link only feeds levels, so both are invisible to the model.
Every check run journals a `checks` entry under its own generation.

- Coverage: `uncovered-section` for a section with a body of its own left
  `unprocessed`; `suspicious-non-normative` for a `non-normative` section whose text
  still looks normative ([coverage](#coverage)).
- `stale-provenance`: an anchor no session addressed, a quote that fails to locate.
- `unused-entity`: an entity with no requirements. `unreachable-entity`: an entity not
  reachable from the declared [roots](./project-settings.md#roots) over relationships
  and shared requirements.
- `unstable-extraction`: a natural key deleted and recreated across recent builds
  ([flip detection](./reconciler.md#flip-detection)).
- `duplicate-requirement`, the mechanical part: near-identical statements on one
  entity, both quote-anchored. The same sentence extracted twice in one section is a
  warning (keep one); the same fact restated across documents is an info (both kept).
  Rephrased duplicates are [`rejudge-pair`](./goals/rejudge-pair.md)'s judgment.
- Document quality, prose problems a human can fix, surfaced where the human writes
  ([LSP](../frontends/lsp.md) shows them inline): `section-too-large` (a body over 6000
  chars), `doc-too-large` (over 40 sections), `empty-file` (a matched file with no
  content), `broken-link` (a relative link to a `.md` file whose target does not exist).
  The thresholds are [registry constants](./graph.md#limits). The docs absorb detail by
  dividing, not bloating: these two size diagnostics tell the human where to split, and
  incoming links keep the parts bound to the whole.
- `pinned-fact-drift`, when the [ledger](../consumers/gen.md#the-ledger) exists: a
  code-span literal in a requirement's statement that looks pinned (a path, an
  identifier, a value: it carries a digit, dot, slash, dash, colon, or underscore) and
  appears in none of the requirement's bound files. The docs say `us-east-1` and the
  code never mentions it: one of them is wrong, and no model is needed to notice. The
  diagnostic carries a [prompt](./model/diagnostic.md#prompts) (the docs are right and
  the code must change, or the value is stale here, or a freeform reply), and a human
  answer sticks.
- Justification closure: `unjustified-fact` for a fact or rendered element whose
  provenance walk does not end in a verbatim quote in a live section, or in `derived`
  or `decree` provenance with live upstream nodes and an open ratification proposal
  ([provenance](./model.md#provenance)).
- Flow placement, wherever flow views exist: `unplaced-behavior` for a behavior
  requirement in no flow view and not excluded from one with a note;
  `unrepresented-failure-mode` for a failure-mode requirement represented in no branch.
  Each finding also writes a `flow-unplaced` record, feeding
  [`curate-view`](./goals/curate-view.md).
- Containment consistency: `containment-mismatch` when a `composition` edge and the
  `parent` field disagree ([containment](./model/entity.md#containment)).
- `level-shape`, wherever a node has children ([levels](./concepts/levels.md#levels)):
  a node with two or more children (the scope root included) that has no structural
  [level view](./diagrams.md#level-views); a node whose direct children exceed the hard
  `children-per-entity` threshold ([limits](./graph.md#limits), or the node's own
  [bump](./graph.md#per-node-bumps)); a derived
  [grouping](./concepts/levels.md#groupings) with fewer than two children. The first
  and the third name a store the commit did not settle: level views recompute on every
  commit and the sweep dissolves an under-membered grouping
  ([the sweep](./graph.md#the-sweep)). The second is the level the mandatory
  [fan-out](./reconciler.md#fan-out) goal must regroup before the build converges; the
  finding stands beside the goal and clears with it.
- Conformance, the mechanical part: `nonconformant-instance` when an instance carries an
  attribute name its type does not declare. Value and link conformance is
  [`conform-instance`](./goals/conform-instance.md)'s judgment.
- State machine checks, wherever transition facets exist: `unreachable-state`,
  `dead-end-state`, `nondeterministic-transition`, `unhandled-event`
  ([state machine](./model/state-machine.md#checks)).
- Provider check, wherever «interface»-like entities exist: `provider-missing` for an
  «interface» some entity depends on with no `realization` toward it,
  `provider-ambiguous` for one with more than one.
- `quality-unmeasured`: a `quality` facet without a `measure`
  ([facets](./model/requirement.md#facets)).
- Cross-class flip detection: `unstable-derivation`, parking the oscillating pair
  ([flip detection](./reconciler.md#flip-detection)).
- `incomplete-build`: goals parked because a budget ran out.

The checks also settle stranded diagnostics: every open judged diagnostic whose subjects
are all missing from the graph is resolved and journaled (`settle-diagnostics`), so a
store deleted into a stranded state heals at the next build or board poll instead of
staying wedged.

## Convergence

The build is done when both goal classes derive empty of open or failed mandatory goals
and the checks are clean: every finding is recorded as a diagnostic or a goal, and no
section with a body of its own is left `unprocessed`. Idempotence is what makes this a
fixed point rather than a loop: a session that re-derives an unchanged conclusion stages
a no-op upsert, no mutation lands, and that branch of the cascade dies.

The verdict in `status.yaml` is `verdict: {state, open, failed, blocked, optional}`:

- `converged` when no open or failed mandatory goal of either class remains and the
  checks are clean. The counts ride with it: `blocked` (goals waiting on a human:
  unanswered prompts, ratification proposals, gated releases) and `optional` (standing
  advice). Printed as `converged, 2 blocked, 1 optional advised`.
- `incomplete` otherwise, with `open` (open and parked mandatory goals), `failed`
  (failed mandatory goals), `blocked`, and `optional`. Printed as
  `incomplete: 3 open, 1 failed, 2 blocked, 5 optional advised`.

A mandatory [fan-out](./reconciler.md#fan-out) goal
([`abstract-entity`](./goals/abstract-entity.md) on a node, or on `scope:<scope>` for
the top level) open or failed blocks `converged`: it is a mandatory GC goal, and the rule
above already counts it. An optional fan-out goal rides under `optional`. A level a
session declared genuinely flat stays a failed goal until a human bumps the node's limit
([per-node bumps](./graph.md#per-node-bumps)), retries the goal, or the level changes
again ([parked and failed](./reconciler.md#parked-and-failed)), so a verdict never hides
an over-fan-out level.

A build with open blocked goals is `converged, 2 blocked`, never silently done.
A hard per-build cap (3 × derived goals + 8 sessions) backstops the loop; work still open
when it runs out is parked in `status.yaml` and reported as an `incomplete-build`
diagnostic, and the next build resumes parked goals first
([parked and failed](./reconciler.md#parked-and-failed)). A session that exhausts its
round budget without meeting its gate leaves its goals open, so the verdict counts them;
a build that stopped halfway is never reported as converged.

The verdict speaks to work completion, never to document health: a graph can converge
with open `error` diagnostics standing. So the verdict never travels alone. Open
diagnostic counts by severity (suppressed excluded) ride beside it: in `status.yaml`
(`diagnostics`), in the empty-board `goals` reply, in the final `done` reply, and in
`await_changes` (`openDiagnostics`). An agent deciding "done" sees the open errors in
the same breath as `converged`. Costs ride there too: `costs {sessions, tokens,
by_kind, by_class}`, and [`jazyk ripple <generation>`](../frontends/cli.md#jazyk-ripple)
renders the whole build's causality with cost beside it.

The shape of the containment tree rides beside the verdict as well.
[`jazyk status`](../frontends/cli.md#jazyk-status) prints one shape line: the nodes per
depth (the scope root's parentless entities at depth 1, their children at depth 2, and so
on) and the fan-out histogram (how many nodes hold how many direct children, bucketed
against the `children-per-entity` soft and hard values). The line is derived from the
graph like the board counts. It says at a glance whether the graph is a navigable pyramid
or a flat list, before anyone opens a diagram. E.g.:

```
shape: 3 / 9 / 31 / 118 nodes per depth; fan-out 2-9: 14, 10-15: 2, over 15: 0
```

## Coverage

Every section carries a coverage state in the store:

- `unprocessed`: not yet reconciled.
- `covered`: the model claimed it; its content is reflected in the graph.
- `non-normative`: the model marked it as carrying no requirements (examples, prose,
  navigation). A `note` is required.

Coverage is the completeness meter of a build and part of its termination criterion.
Checks flag sections that stay `unprocessed`, and `non-normative` sections whose text
still looks normative (`suspicious-non-normative`). "Looks normative" is a cheap
deterministic signal: the body says `shall`, uses obligation verbs (supports, manages,
handles, provides, requires, allows, stores, can be performed, is responsible), or
holds definition-list bullets (`` - `name` - description ``).

## Incremental builds

There is no separate incremental mode:

- Nothing changed → empty dirty set → zero goals, zero LLM calls.
- A cosmetic edit → one `reconcile-section` session that stages no mutations → graph
  unchanged, no records written, nothing downstream.
- A section moved, split, or merged → a `place-anchors` session decides its anchors,
  then `reconcile-section` re-judges only the ones marked for re-evaluation.
- A real edit → sessions for that section, then for the goals its commit opens. The rest
  of the graph is not visited: a change reaches exactly its cone, never the pyramid.

Idempotence and convergence replace per-stage caching. The graph plus the change records
is the cache. The trace a one-sentence edit leaves (`orders.md`: "held orders expire
after 21 days" becomes "30 days"), as [`jazyk ripple`](../frontends/cli.md#jazyk-ripple)
renders it:

```
edit g87 docs/orders.md#/orders/holds (human)
└─ reconcile-section docs/orders.md#/orders/holds g88: req:orders-6 revised (quote, statement, transition guard)
   │  recomputed at commit: sm:order (held→expired guard), view:sequence/holds
   ├─ rejudge-pair req:orders-6~req:payment-9 g89: consistent
   └─ bind req:orders-6 g90: row stale (requirement-changed), test rewritten, no file implements 30 days → unimplemented
      └─ generate ent:order g91: files rewritten
         └─ verify req:orders-6 g92: pass
gc: no goals derived
converged: 4 sessions, 2 recomputes, 29k tokens
```

Every line is a journal entry; every indent is a goal with its cause and justification
on record. The recompute line is the payoff of derived data: the state machine and the
sequence view followed the requirement without a session. The `bind` line shows the
cascade's rule: the re-bind reruns the test, and only an `unimplemented` row (a `fail`
with no implementing file) opens `generate`. The verify line is a journal entry without
a session: a programmatic row runs its command and records the verdict, and only the
four goals that needed a model cost tokens.
