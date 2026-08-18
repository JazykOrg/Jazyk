# Binding

A binding ties one requirement to the deliverable: the files that carry it (possibly
none), the test that judges it, and the last verdict. The binding is the requirement's
row in the [ledger](./gen.md#the-ledger); this page defines how rows are born.

Binding runs before generation, not inside it. A requirement enters the graph knowing
nothing about the deliverable. The bind task answers three questions in one bounded
step: is it already implemented, is it already tested, and does the deliverable satisfy
it. The verdict sorts the requirement into existing work, new work, or a contradiction,
and the same step serves every arrival: a requirement extracted from a fresh document,
a whole project [adopted](#adoption) at once, or a statement reworded after its binding
existed.

## The bind task

One requirement per task. The steps:

- Search the deliverable for an implementation. Record the carrying files, or record
  none. Absence is a finding, not a failure.
- Search for an existing test that judges the statement. Bind to it when found; never
  write a duplicate beside it.
- When no test exists, write one. Both directions get one:
  - Implementation found: the test pins the observed behavior against the statement.
    It should pass.
  - Implementation absent: the test encodes the statement and fails by design. The
    failing test is the spec, and it is the acceptance gate
    [generation](./gen.md) later has to clear.
- Run the test and record the binding: the files, the test row, the verdict, the
  evidence.

The test follows the same contract generation used to own: the
[EARS shape suggests the test shape](./gen.md#tests-tie-requirements-to-the-deliverable),
the two kinds are `programmatic` and `llm`, the test must be
[falsifiable](./gen.md#tests-tie-requirements-to-the-deliverable), and the name embeds
the requirement id and statement hash
([traceability](./gen.md#traceability)). A deliverable with no
[decided medium](./gen.md#the-medium-is-decided-once-before-anything-is-generated) yet
gets the decision from the first bind task, the same way the first generation task
decides it: a test is written in the medium's toolchain, so the decision cannot wait.

## What the verdict means

The bound row's [derived status](./gen.md#status-is-derived-never-stored) classifies
the requirement:

- Test passes → `verified`. The documentation described behavior the deliverable
  already has.
- Test fails and the binding names no implementing files → `unimplemented`. New
  functionality: the requirement joins the
  [generation worklist](./gen.md#incremental-regeneration), and the bound test is
  what generation must make pass.
- Test fails and the binding names implementing files → `failing`. The deliverable
  contradicts the statement. That is a
  [diagnostic](../compiler/model/diagnostic.md) for the author (the docs are wrong,
  or the code is), never a silent regeneration: rewriting existing code because a
  sentence disagrees with it is a decision, not a default.

## When binding runs

The [task queue](../compiler/reconciler.md#the-task-queue) emits a `bind-requirement`
task for any requirement whose binding is absent or invalid. Nothing invokes binding
by name; the queue notices the gap:

- Absent: the requirement is new (just extracted), or the whole ledger is (an adopted
  project).
- Invalid: the statement hash moved (the requirement was reworded, so the test judges
  a sentence that no longer exists), or the test artifact is gone from disk.

A bind task is ready when the compile queue is empty: the statement must be final
before a test encodes it. It writes test files into the deliverable, so in `manual`
mode it gates under the generate
[release](../compiler/control-plane.md#modes-and-releases) beside generation. It takes a
per-requirement lease and defaults to the `agent`
[worker](../compiler/control-plane.md#dispatch): searching a codebase is what a coding
agent's own tools are best at. The built-in worker performs the same task in-process
with read tools over the deliverable; what separates the workers is model quality,
never capability.

## Generation makes bound tests pass

Binding redraws the boundary between the two workflows:

- Binding owns the tests. Generation owns the product.
- The generation worklist is derived from bindings: an entity is generation work when
  its facts moved, or when any of its requirements' bindings is `unimplemented`.
- The bound test defines the interface, and the product conforms to it. A generation
  task that cannot make a bound test pass without changing it reports that; the
  repair is a re-bind, never a quiet rewrite of the judge.

The cascade after a docs edit becomes: compile → re-bind the reworded requirements →
generate the `unimplemented` ones → the tests go green → `verified`. Each arrow is a
derived worklist; nothing in the loop is remembered by a human.

## Adoption

Pointing Jazyk at an existing codebase is not a mode. The code is the
[deliverable](./gen.md#the-deliverable), the docs (written or
[decompiled](./decompile.md)) compile into the graph, and every requirement arrives
unbound. The resulting wave of bind tasks classifies the whole document against the
whole codebase: `verified` where the doc described what exists, `unimplemented` where
it described what is missing, `failing` where the two disagree. A document mixing
existing and new functionality needs no annotation saying which is which; the code is
the arbiter, requirement by requirement.

## The unclaimed report

The inverse of coverage. Every binding names its files; the deliverable files named
by no binding are unclaimed: behavior the code carries that no requirement describes.

- The scope is the [`gen.code`](../compiler/project-settings.md#generation) glob when
  set, otherwise every file under the deliverable minus the standard exclusions.
- The report surfaces in `jazyk status`, in the [GUI](../frontends/gui.md#workers),
  and in `monitor` and `await_changes` payloads.
- It is the worklist [decompilation](./decompile.md) consumes: unclaimed territory is
  exactly what has no docs yet.
