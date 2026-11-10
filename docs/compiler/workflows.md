# Selected workflows

A workflow runs an ordered selection of actions for one document set. The engine
owns execution, cancellation, traces, and results. The editor selects actions.
A workflow is a job, so closing its page does not stop its engine actions.

## Actions

- `compile`: reconcile documents, graph, diagrams, and diagnostics. Ledger goals
  stay open. A workflow compile never releases generation as a side effect.
- `bind`: inspect existing source and create or update requirement bindings. This
  is an explicit action, not a prerequisite for generation.
- `generate`: develop product source and its tests together. The coding agent
  chooses their order, reuses existing checks, and runs checks when they can guide
  implementation. Missing bindings are completed in this action.
- `build`: run the ledger's artifact build once. A written product needs no build.
- `test`: run existing programmatic bindings. Do not generate or repair files.
- `review`: judge existing files against model criteria. Do not generate files.
- `task`: execute one saved project task by its stable ID. Multiple tasks are
  allowed. Their selection order is their execution order.

The default product workflow is `compile` → `generate` → `build` → `test` →
`review`. It does not create and run every requirement test before source exists.
The coding agent can start from source, a test, or a small working slice. Its
completion gate still requires implementation and current requirement evidence.
Related checks share a saved suite task and container run where practical.

Binding and generation may run focused checks while constructing their output.
They do not imply the final artifact build or a full test run. The editor states
this execution permission beside generation. `jazyk gen` uses the same adaptive
development contract. Workflow actions use the explicit boundaries above.
Tests and reviews do not implicitly rebuild artifacts. Select `build` first when
required. Missing artifacts block a check. A standalone test run uses existing
files and retains the requirement freshness rules of the verification ledger.

## Execution

A request includes an ordered, nonempty `steps` array, with at most 100 actions.
Unknown actions and duplicate actions or task IDs are refused. Actions run
sequentially. Each action records its name, result, and state in the job result.
The execution checkpoint retains the frozen task definitions, current action and
completed results. Restoration resumes the active action before selecting another
one. Completed actions retain their results and do not execute again. Deployment
suspension retains the current action and does not record a failed result.
Failure or cancellation stops subsequent actions. A failed check never counts as
successful publication evidence. Unselected mandatory goals remain visible on the
board. Finishing a selected workflow does not imply whole-project convergence.
A test skipped because its binding left current Source
(`source-selection-changed`) fails the test action and stops subsequent actions.
Re-bind before testing again. The skipped row and its files remain unchanged.

Compile completion and generation readiness are separate checks. A compile action
finishes when no mandatory graph work remains open, parked, or failed. Ledger work
not selected by the action does not prevent compile completion. An incomplete
compile keeps its report, including its verdict and counts, beside the reason the
workflow stopped. The error names up to five blocking goals, their states, and
stored parking or failure reasons, followed by the count of further blockers.
It directs the user to those blockers instead of asking them to run the compile
that just failed.

A ledger action checks what was committed before reporting a session error. A
completed binding or valid generation manifest remains successful after a later
model error. When required work did not land, a model or runtime error stops the
action before the next target. The result retains that reason beside the completed
and failed counts. Unattempted targets are not counted as failed. Review stops on a
model transport error without inventing judgments for the remaining rows.
An unfinished ledger target after a normal turn end or a round limit keeps its
target, stop reason, and unmet gate in the action's `error`. Other targets may still
run. The result's `stopped` field is reserved for an early action halt.

Document changes must be compiled before binding or generating. Unfinished mandatory
graph work blocks these actions. Goals waiting on a human answer or ratification
do not block compile completion, binding, or generation. Their questions and
proposals remain visible. Binding and generation carry open error diagnostics into
their packages and record affected rows as
[`contradicted`](../consumers/gen.md#rows-recorded-over-an-open-contradiction).

A workflow can carry a [selection](./compilation.md#selected-builds). It narrows
every action that runs goals or ledger rows:

- `compile` runs only selected goals. It finishes when no mandatory graph goal in the
  selection remains open, parked, or failed. Work outside the selection does not hold
  it.
- `bind`, `generate`, `test`, and `review` take the entities and requirements anchored
  in the selected sections as their targets. An action with no selected target
  reports `Nothing in the selection` and succeeds without a session.
- Before `bind` and `generate`, only the selected documents must be compiled, and
  only selected mandatory graph work must be finished.
- `build` and `task` actions ignore the selection. They run as usual.

A changed task definition after submission blocks its run; it never substitutes a
new command. Every task uses the existing snapshot, idempotency, cancellation, and
retained-product contract.

Publication remains a managed delivery operation. A workflow can prepare the
candidate, then the editor offers the selected deliveries with their destination
and existing confirmation. Publication is never selected by default. Reloading
must not silently repeat a publication. Downloads are explicit artifact actions.
