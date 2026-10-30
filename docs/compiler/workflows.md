# Selected workflows

A workflow runs an ordered selection of actions for one document set. The engine
owns execution, cancellation, traces, and results. The editor selects actions.
A workflow is a job, so closing its page does not stop its engine actions.

## Actions

- `compile`: reconcile documents, graph, diagrams, and diagnostics. Ledger goals
  stay open. A workflow compile never releases generation as a side effect.
- `bind`: create or update requirement tests and bindings.
- `generate`: generate product source against existing bindings. Missing bindings
  block this action. Select `bind` first to create them.
- `build`: run the ledger's artifact build once. A written product needs no build.
- `test`: run existing programmatic bindings. Do not generate or repair files.
- `review`: judge existing files against model criteria. Do not generate files.
- `task`: execute one saved project task by its stable ID. Multiple tasks are
  allowed. Their selection order is their execution order.

Binding and generation may run focused checks while constructing their output.
They do not imply the final artifact build or a full test run. The editor states
this execution permission beside generation. The legacy CLI commands retain their
existing combined behavior. Workflow actions use the explicit boundaries above.
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

Document changes must be compiled before binding or generating. Mandatory graph
work blocks generation. A changed task definition after submission blocks its run;
it never substitutes a new command. Every task uses the existing snapshot,
idempotency, cancellation, and retained-product contract.

Publication remains a managed delivery operation. A workflow can prepare the
candidate, then the editor offers the selected deliveries with their destination
and existing confirmation. Publication is never selected by default. Reloading
must not silently repeat a publication. Downloads are explicit artifact actions.
