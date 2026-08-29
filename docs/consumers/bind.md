# Binding

A binding ties one requirement to the deliverable: the files that carry it (possibly
none), the test that judges it, and the last verdict. The binding is the requirement's
row in the [ledger](./gen.md#the-ledger); this page defines how rows are born.

Binding runs before generation, not inside it. A requirement enters the graph knowing
nothing about the deliverable. The bind goal answers three questions in one bounded
session: is it already implemented, is it already tested, and does the deliverable satisfy
it. The verdict sorts the requirement into existing work, new work, or a contradiction,
and the same goal serves every arrival: a requirement extracted from a fresh document,
a whole project [adopted](#adoption) at once, or a statement reworded after its binding
existed.

## The bind goal

One requirement per goal, `g:bind:<requirement id>`
([the bind goal](../compiler/goals/bind.md)). The goal's loaded set is the requirement in
full: its `statement`, facets, edges, and transition, the verbatim quote, and its entity
as a stub with its definition. The contract rides the `begin_binding` reply as
`instructions`, with the package: the statement, the quote, the reason, the suggested
test name, the decided medium and build when they exist, the entity's recorded files, and
the ledger's recorded conventions (the run commands, the files other rows name)
([binding tools](../compiler/tools.md#binding-tools)). The steps:

- Search the deliverable for an implementation, starting at the entity's recorded files.
  Record the carrying files, or record none. Absence is a finding, not a failure.
- Search for an existing test that judges the statement. Bind to it when found; never
  write a duplicate beside it.
- When no test exists, write one. Both directions get one:
  - Implementation found: the test pins the observed behavior against the statement.
    It should pass.
  - Implementation absent: the test encodes the statement and fails by design. The
    failing test is the spec, and it is the acceptance gate
    [generation](./gen.md) later has to clear.
- Run the test and record the binding with `record_binding`: the files, the test row,
  the verdict, the evidence.

The session observes and judges; it never changes implementation files. Test and
criteria files are the only files it writes: a write to a path the ledger records as an
entity's implementing file is rejected with the owner named
([file and command tools](../compiler/goals/generate.md#file-and-command-tools)).

`record_binding` is the gate. The goal resolves when a current row is recorded (its
`hashes.requirement` equals the live statement hash, and the test artifact exists and
contains the declared name; for an `llm` row, the criteria file exists), never on the
model's word: `mark_goal_done` is validated against the row and rejected naming the gate
otherwise, and a session that recorded the row and ended without marking still resolves
the goal at the next derivation. The verdict is not part of the gate: a failing test on
an unimplemented requirement is the intended outcome. The reply to `record_binding`
previews what the row opens ([bubbling](../compiler/reconciler.md#bubbling)): an
`unimplemented` row opens `generate` on the owning entity.

The test follows the same contract as generation's tests: the
[facets and transition suggest the test shape](./gen.md#tests-tie-requirements-to-the-deliverable),
the two kinds are `programmatic` and `llm`, the test must be
[falsifiable](./gen.md#tests-tie-requirements-to-the-deliverable), and the name embeds
the requirement id and the hash of its `statement`
([traceability](./gen.md#traceability)). When no falsifiable programmatic assertion
exists, the kind is `llm` and the artifact is a
[criteria file](./gen.md#criteria-files-for-llm-tests). A deliverable with no
[decided medium](./gen.md#the-medium-is-decided-once-before-anything-is-generated) yet
gets the decision from the first bind goal, the same way the first generation session
decides it: a test is written in the medium's toolchain, so the decision cannot wait.

## What the verdict means

The bound row's [derived status](./gen.md#status-is-derived-never-stored) classifies
the requirement:

- Test passes → `verified`. The documentation described behavior the deliverable
  already has.
- Test fails and the binding names no implementing files → `unimplemented`. New
  functionality: the requirement's entity gains a `generate` goal
  ([incremental regeneration](./gen.md#incremental-regeneration)), and the bound test is
  what generation must make pass.
- Test fails and the binding names implementing files → `failing`. The deliverable
  contradicts the statement. That is a
  [diagnostic](../compiler/model/diagnostic.md) for the author (the docs are wrong,
  or the code is), never a silent regeneration: rewriting existing code because a
  sentence disagrees with it is a decision, not a default.

## When binding runs

The board derives a `bind` goal from a `ledger-stale` change record whose `detail.goal`
is `bind`, written whenever the ledger and the graph disagree about a requirement
([goal derivation](../compiler/reconciler.md#goal-derivation)). Nothing invokes binding
by name; the board notices the gap, with the reason in the record:

- `unbound`: the requirement has no row. It is new (just extracted), or the whole ledger
  is (an adopted project).
- `requirement-changed`: the `statement` hash moved. The requirement was reworded, so
  the recorded test judges a sentence the graph does not hold.
- `artifact-gone`: the test artifact is gone from disk.

The goal exists exactly while the row is absent or stale and disappears the moment a
current row lands. A requirement that left the graph derives nothing: its row is
[pruned](./gen.md#deletion-prunes-the-ledger), never bound.

A `bind` goal sits in the ledger tier (tier 3): it becomes ready when no goal of the
earlier tiers is open in the requirement's cone, so the statement is final before a test
encodes it ([readiness](../compiler/reconciler.md#readiness)). Batches group by locality:
the requirements of one entity, and under containment the entities of one component
subtree, so one session searches one part of the deliverable
([batching](../compiler/reconciler.md#batching)). The goal writes test files into the
deliverable, so in `manual` mode it is blocked on the generate
[release](../compiler/control-plane.md#modes-and-releases) beside generation; a blocked
goal counts in the verdict. The executor resolves per goal kind
([executors](../compiler/project-settings.md#executors),
[dispatch](../compiler/control-plane.md#dispatch)); a coding agent is the natural choice,
because searching a codebase is what its own tools are best at. The embedded agent
performs the same goal in-process with file tools served over the deliverable; what
separates the executors is model quality, never capability. `jazyk gen` resolves the owed
`bind` goals of its targets before their `generate` goals ([command](./gen.md#command)).
Over MCP, `binding_tasks` lists the open goals and `begin_binding` claims the row under a
lease ([workers and leases](../compiler/control-plane.md#workers-and-leases)).

## Generation makes bound tests pass

Binding draws the boundary between the two workflows:

- Binding owns the tests. Generation owns the product.
- The `generate` goals derive from bindings: an entity is generation work when its
  facts moved, or when any of its requirements' rows is `unimplemented`.
- The bound test defines the interface, and the product conforms to it. A generation
  session that cannot make a bound test pass without changing it reports that; the
  repair is a re-bind, never a quiet rewrite of the judge.

The cascade after a docs edit is: compile → re-bind the reworded requirements →
generate the `unimplemented` ones → the tests go green → `verified`. Each arrow is a
goal the board derives with its cause on record; nothing in the loop is remembered by a
human, and `jazyk ripple` replays it ([edit paths](../compiler/compilation.md#edit-paths),
[the cascade](./gen.md#the-cascade)).

## Adoption

Pointing Jazyk at an existing codebase is not a mode. The code is the
[deliverable](./gen.md#the-deliverable), the docs (written or
[decompiled](./decompile.md)) compile into the graph, and every requirement arrives
unbound. The resulting burst of `bind` goals classifies the whole document against the
whole codebase: `verified` where the doc described what exists, `unimplemented` where
it described what is missing, `failing` where the two disagree. A document mixing
existing and new functionality needs no annotation saying which is which; the code is
the arbiter, requirement by requirement. An adopted entity whose rows all read `verified`
derives no `generate` goal.

## The unclaimed report

The inverse of coverage. Every binding names its files; the deliverable files named
by no binding are unclaimed: behavior the code carries that no requirement describes.

- The scope is the [`gen.code`](../compiler/project-settings.md#generation) glob when
  set, otherwise every file under the deliverable minus the standard exclusions.
- The report surfaces in `jazyk status`, in the [GUI](../frontends/gui.md), in the
  whole-build report, and in `monitor` and `await_changes` payloads.
- It is the worklist [decompilation](./decompile.md) consumes: unclaimed territory is
  exactly what has no docs yet.
- Its per-entity slice is the `files` part of the
  [unattached remainder](./gen.md#the-unattached-remainder) generation measures.
