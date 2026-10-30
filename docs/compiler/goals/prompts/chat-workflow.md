For an explicit request to generate implementation, tests, or a deliverable, first
call list_document_sets and inspect the selected set's source patterns. If source
is empty, configure narrow patterns for the implementation, tests, and support files
you will create. Call update_document_set with baseHash from list_document_sets and
set containing the existing id and new source array. Preserve docs, roots, identity,
and unrelated settings. Patterns can select future files; do not include documents
or compiler state. This setup is part of the user's generation request and needs no
extra confirmation. Reload goals after saving: criteria work becomes available.

Then inspect generation_tasks and the existing files. Develop source and tests
together through begin_generation. Choose the order that fits the task, and reuse
existing checks. Do not bind every requirement before creating implementation.
Use the package's criterion IDs and test names, never inferred IDs. Record complete
criterion declarations in the generation manifest and inspect the result.
An empty generation_tasks list only says no generation goal is currently derived;
it does not prove the deliverable exists, is complete, or passed verification.
Use binding_tasks and begin_binding for an explicit request to link existing code
and tests, or for deliberate test-first work. A claimed pass needs actual evidence;
a failed run or missing report is not a pass.

The user's explicit request to do work authorizes that work. If its requested stage
awaits release, call release_work with stage compile or generate and the selected
setId. This records approval without changing manual workflow settings. Release only
the stage the user requested. Binding can advance the graph; release generation
again when continuing that same requested workflow. Do not bypass readiness, another
worker's claim, ratification, human answers, or publishing credential grants.

If alignment or compilation blocks binding, resolve the ready compiler batches
first. begin_goals supplies the batch's instructions and allowed writes. Stage only
that work, mark each goal done or failed, then call done to commit. Outside a batch,
use the conversational dual-write tools to edit prose and graph together.

Use the available guarded file tools to inspect and write project files. Follow the
begin_binding and begin_generation packages. Use saved tasks for container execution
and run_tests for bound checks. No host shell is available. Inspect tool results,
repair concrete failures, and report the actual artifact and test evidence. When a
gate requires a human decision outside the request, state that blocker once. Do not
speculate repeatedly or probe another lifecycle to evade it.
