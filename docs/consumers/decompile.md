# Decompilation

Decompilation goes from code to prose: a project that has documentation for none or
part of its code gains documents describing what the code does. It produces
documentation files, never graph mutations. Every fact in the graph carries one
provenance: a verbatim quote from a document, or a derived or decreed fact carrying a
ratification proposal toward one ([provenance](../compiler/model.md#provenance)), so
extracting requirements from code directly into the graph would create a second source
of truth. Instead, decompilation writes prose, and the normal
[compile](../compiler/compiler.md) runs on it. Code is evidence; the docs remain the
source.

Decompilation stays outside the goal board: its work derives from the unclaimed report
and a release, never from the dirty set, and it never counts toward convergence.

## Inventory

A deterministic pass, no model. It maps the scope before any drafting: the file tree,
the module boundaries, the public surface, the entry points, and the test files. Tests
are listed per module and marked as the primary evidence: an existing test is already
an executable requirement, and a test name with its assertions decompiles into a
statement far more reliably than implementation code, where intent and accident look
alike.

## Draft goals

One `draft-document` goal per scope (a module, a directory, a subsystem). The goal's
package carries the inventory slice, the test list with their assertions, the project's
[lint rules](../compiler/project-settings.md#docs), and the drafting contract
(`decompile-contract.md` under `compiler/goals/prompts/`, embedded into the binary):

- Write prose sections stating the obligations the code observably carries, in the
  project's documentation voice. Statements must be extractable: the draft is compiler
  input like any other document ([statements](../compiler/concepts/statements.md)).
- Every statement carries its evidence class:
  - observed: a test pins the behavior. The statement cites the test name.
  - inferred: the code does it, nothing asserts it. The statement cites the file.
- Behavior is not requirement. A draft states what the code does; whether that is what
  the code should do is the author's call at [ratification](#ratification). A bug
  described faithfully is a correct draft.

Drafting is code-reading work, so the executor resolves per goal kind
([executors](../compiler/project-settings.md#executors),
[dispatch](../compiler/control-plane.md#dispatch)). The embedded agent runs the goal as a
session with read-only file tools over the deliverable; an attached coding agent with its
own tools is the better executor and the default dispatch target. One session per scope,
one draft per session.

## Drafts land in the docs tree

There is no shadow tree and no second pipeline. `submit_draft` validates the draft
(exactly one H1, no em dash, a project-relative path with no `..` that the docs glob
includes) and writes it as a normal file under the docs glob. The compiler picks it
up like any hand-written document: sections, dirty set, `reconcile-section` sessions,
the graph.

A draft never overwrites a document a person wrote or edited. A path that already
exists is accepted only when the file on disk is an unedited draft (its content hash
still equals the hash recorded at submission), so a scope drafted again replaces its
own earlier draft and nothing else; any other existing file is rejected naming the
path, and the session picks another.

The out directory records each draft in `decompile/drafts.yaml`: the document path and
the content hash as submitted. That record is what ratification reads.

## Ratification

A decompiled statement is provisional until a human accepts it. The compiler attaches
an `unratified` [diagnostic](../compiler/model/diagnostic.md) (info severity) to every
document whose current content hash still equals its drafted hash: nobody has touched
it since the machine wrote it. Editing the document, even to accept it with a
one-line change, moves the hash and clears the diagnostic. The document is the review
surface, and reviewing it is editing it, the same loop as everything else. This is
distinct from the ratification of graph facts: a drafted document is prose already, so
its facts enter the graph with quote provenance and need no
[ratification proposal](./docsgen.md#ratification-proposals).

## The self-check

A draft that compiles produces requirements, and every one arrives unbound, so the
[bind goals](./bind.md#when-binding-runs) run against the very code the draft
describes. The expected outcome is `verified` across the board: the code satisfies
statements extracted from the code. Any `failing` binding on a decompiled requirement
means the draft misdescribed the code (or found flaky behavior), and it surfaces as a
diagnostic on the draft, not as generation work. Decompilation ships with its own lie
detector.

## Scope and iteration

The [unclaimed report](./bind.md#the-unclaimed-report) drives decompilation: files no
binding names are the territory without docs. Decompiling a subset is not a mode;
a draft goal simply takes a scope, its statements bind to their files, and the report
shrinks. Repeating until the report is empty documents the whole project; stopping
early documents a subsystem. Progress is visible as a number either way.

## Triggering

Decompilation spends model budget on every draft, so it never runs on save and has no
auto mode. `draft-document` goals derive from the unclaimed report but are always
gated until a decompile release names their scope:

- `jazyk decompile [path...]` records the release for the named scope (default: the
  whole unclaimed set) and runs or dispatches the goals
  ([CLI](../frontends/cli.md#jazyk-decompile)).
- The GUI's decompile action records the same release and dispatches by the executor
  preference, like compile and generate
  ([workflow modes](../frontends/gui.md#workflow-modes)).
- `released.decompile` in `control.yaml` holds the approved scopes; a submitted draft
  covering a scope consumes it. See
  [the control plane](../compiler/control-plane.md).

An attached agent works the goals over `jazyk mcp decompile`
([toolsets](../frontends/mcp.md#toolsets)): `decompile_tasks` lists the released scopes,
`begin_decompile` hands over the package, `submit_draft` lands the document
([decompilation tools](../compiler/tools.md#decompilation-tools)).

## Round-trip fidelity

Decompilation closes a measurable loop: decompile a project, compile the drafts,
generate from the graph, and run both test suites. The distance between the original
code and the regenerated code, judged by the tests, is a fidelity score for the whole
chain. A codebase whose docs already exist by hand is ground truth: decompile it into
a scratch project and diff the drafts against the human documents.
