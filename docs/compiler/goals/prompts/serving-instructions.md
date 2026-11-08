<!-- initialize -->
{policy}

## Serving capabilities

{capabilities}

<!-- project-unloadable -->
The project file `{config}` does not load: {error}. Tools that need the project refuse until it is repaired; edit the file with your own tools.

<!-- toolsets -->
Served toolsets: {modes}. The actual tools/list catalog is authoritative.

<!-- initialized -->
The project is already initialized. There is nothing to scaffold.

<!-- uninitialized -->
NO PROJECT HERE: there is no jazyk.toml or graph yet. For an authorized setup, use init_project if served, then reopen the conversation. Inspection and workflow skill tools need no graph.

<!-- tab -->
This serving runs in a browser tab without a shell. Binding, generation, and verification are not served here.

<!-- writer-build-note -->
Use compile_document_sets to compile named set IDs, or omit setIds to compile all dirty sets.

<!-- graph-write -->
Write tools are enabled for manual graph surgery; each call commits as its own changeset.

<!-- workflow -->
Read list_jazyk_skills and read_jazyk_skill for the requested workflow. Active begin_* packages and tool gates supply specific scope, instructions, and evidence. Use report_feedback for wrong or confusing Jazyk instructions or tools.

<!-- reader -->
This serving can only read the project. No tool here changes documents or the graph. Binding, generation, verification, shell execution, and publication are unavailable.

<!-- tab-build-note -->
You cannot start a build: the Build button at the top of the page builds the documents into the graph. When asked to build, say so.

<!-- begin-goals-next -->
load what the goals name, stage findings, then done with a one-line summary and final goals claims ({goal, justification, evidence?}) in the same call

<!-- compile-incomplete -->
compilation incomplete: {failed} failed goal(s) stand; a change on a goal's subject or a human retry from the board reopens it, and jazyk explain <goal> shows the reason

<!-- compile-converged -->
compilation converged

<!-- optional-ready -->
{count} optional goal(s) stay claimable; beginNext stops at convergence, so begin_goals takes the next batch when you want it, and stopping here is right when your instruction was to converge (the verdict does not wait for them)

<!-- generation-ready -->
{count} generation goal(s) ready (generation_tasks lists them)

<!-- nothing-pending -->
nothing pending

<!-- next-batch -->
begin_goals claims the next batch ({batch})

<!-- no-batch-ready -->
no goal is ready; goals lists the board with blockedBy per goal

<!-- began-note -->
the next batch is claimed; if this commit was your last, abandon_goals releases it untouched and leaves no trace

<!-- ledger-nothing-owed -->
nothing owed

<!-- ledger-gated -->
every row is gated on the generate release: `jazyk release generate` (or the GUI) approves them, then {begin} claims one

<!-- ledger-ready -->
{begin} claims one ready row (blockedBy says what a row that is not ready waits for); recording it resolves its goal

<!-- ledger-stale -->
{count} row(s) went stale since their last run (a test or implementation file changed); run_tests re-baselines them, and any generation work they hold returns to this list afterwards

<!-- source-mapping -->
The document set has no source mapping. For the requested generation, call list_document_sets, then update_document_set with its hash as baseHash and set {id, source: [narrow implementation, test, and support-file patterns]}. Preserve documents and existing settings. Reload goals to resolve criteria, then generation_tasks. An empty generation list is not a completed deliverable.

<!-- status-next-goals -->
goals lists the board

<!-- status-next-binding -->
binding_tasks lists the work

<!-- status-next-generation -->
generation_tasks lists the work

<!-- status-next-verification -->
verification_tasks lists the work

<!-- status-next-gated -->
work is gated; a release (`jazyk release` or the GUI) opens it, and this poll returns when it lands

<!-- status-next-none -->
nothing to do

<!-- released-generate -->
generation_tasks, then verification_tasks; binding_tasks is available for explicit inspection

<!-- released-compile -->
goals

<!-- decompile-none -->
no unclaimed files; every deliverable file is named by a binding

<!-- decompile-next -->
begin_decompile on a released scope; gated scopes await `jazyk decompile` or the GUI

<!-- init-next -->
edit docs/README.md, then compile

<!-- benchmark-scored -->
all cases scored; call benchmark_report with an honest model name

<!-- benchmark-begin -->
begin_case {case: <name>} claims that case; with no name, the first pending one

<!-- benchmark-pending -->
{count} case(s) pending; begin_case claims the next

<!-- benchmark-case -->
do the work, then finish_case

<!-- benchmark-verify -->
Judge whether the implementing files satisfy the statement. Read them with your own tools. finish_case with verdict pass or fail and one-line evidence.

<!-- benchmark-generate -->
write real files into deliverableDir with your own tools; record_generation and run_tests act on this sandbox while the case is open
