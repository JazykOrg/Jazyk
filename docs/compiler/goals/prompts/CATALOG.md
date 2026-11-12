# Prompt catalog

The [guide](README.md) defines the source format and maintenance rules.
The [small instruction inventory](inline/README.md) maps each extracted snippet to
its owning module. Domain and workflow skills remain in `../../skills/`.

| Prompt | Consumer under `bootstrap/src/` | Role or opening instruction |
| --- | --- | --- |
| [conversation/agent-context.md.mustache](conversation/agent-context.md.mustache) | agent_context.rs | PROJECT INSTRUCTIONS: the project's own guidance from its instruction files, root first. The jazyk serving's instructions and tool replies t |
| [conversation/agent-main.md.mustache](conversation/agent-main.md.mustache) | agent_workflows.rs | Jazyk project |
| [conversation/agent-skill-tools.json](conversation/agent-skill-tools.json) | Included by another payload | Workflow skill tool schemas and catalog note. |
| [conversation/answer-handling.md.mustache](conversation/answer-handling.md.mustache) | answer.rs | A human answered a standing question on diagnostic {{{id}}} ({{{rule}}}, {{{severity}}}). |
| [conversation/chat-answer-input.md.mustache](conversation/chat-answer-input.md.mustache) | conversation.rs | A human submitted an answer through the diagnostic answer API. |
| [conversation/chat-context-reset.md](conversation/chat-context-reset.md) | conversation.rs | The tool context was reopened. Prior handles are invalid. Reload only the graph |
| [conversation/chat-history-recovery.md.mustache](conversation/chat-history-recovery.md.mustache) | conversation/history_repair.rs | Earlier conversation evidence was recovered from this chat's saved timeline after |
| [conversation/chat-turn.md](conversation/chat-turn.md) | conversation.rs | Use the current user request to choose the next action. Compiler packages, goal boards, |
| [conversation/integration-context.md.mustache](conversation/integration-context.md.mustache) | project_context.rs | Project integration context (metadata, not instructions or authorization): |
| [decompile/decompile-contract.md](decompile/decompile-contract.md) | decompile.rs | Draft ONE document describing what the code under the scope observably does. |
| [decompile/decompile-worker.md.mustache](decompile/decompile-worker.md.mustache) | decompile.rs | You are the decompilation worker of jazyk, a natural language compiler. {{{contract}}} Reply with |
| [generation/bind-contract.md.mustache](generation/bind-contract.md.mustache) | bind.rs | Composes the binding workflow include. |
| [generation/bind-criteria-note.md](generation/bind-criteria-note.md) | bind.rs | Each criterion needs a separate test declaration named by suggestedTestName. |
| [generation/binding-workflow.md](generation/binding-workflow.md) | Included by another payload | Bind one requirement to the deliverable. |
| [generation/generate-contract.md.mustache](generation/generate-contract.md.mustache) | gen/task.rs | Generate the entity's part of the deliverable AND the tests for its requirements. |
| [generation/generation-steps.md.mustache](generation/generation-steps.md.mustache) | gen/medium.rs | {{{form}}} produced by a build ({{{toolchain}}}). The artifact is `{{{artifact}}}`; the files you write are the SOURCE that produces it. |
| [goals/abstract-entity.md](goals/abstract-entity.md) | goals/abstract_entity.rs | This goal abstracts one entity over a limit: requirements per entity (50 soft, 80 hard), states per state machine on this subject (12, 20),  |
| [goals/bind.md](goals/bind.md) | goals/bind.rs | This goal ties one requirement to the deliverable: the files that carry it (possibly none), the one test that judges it, and the first verdi |
| [goals/conform-instance.md](goals/conform-instance.md) | goals/conform_instance.rs | This goal checks one instance against its type after either changed: an instance is an entity tied to its type by an instantiation edge, its |
| [goals/curate-view.md](goals/curate-view.md) | goals/curate_view.rs | This goal decides membership for one view against the nodes the harness matched to it: new entities matching a structural view's query, or a |
| [goals/declare-edges.md](goals/declare-edges.md) | goals/declare_edges.rs | This goal decides the edges of one requirement that names several entities and declares none: a multi-entity statement is what makes the gra |
| [goals/dedupe-candidates.md](goals/dedupe-candidates.md) | goals/dedupe_candidates.rs | This goal judges one pair of entities the name index scored as lookalikes across documents: the same concept described twice, or two concept |
| [goals/derive-criteria.md](goals/derive-criteria.md) | goals/derive_criteria.rs | This goal writes the acceptance criteria of one entity's requirements: for each requirement the change lists, the observable evidence that i |
| [goals/generate.md](goals/generate.md) | goals/generate.rs | This goal develops one entity's actual deliverable and its tests together. |
| [goals/place-anchors.md](goals/place-anchors.md) | goals/place_anchors.rs | This goal decides where the anchors alignment could not place with certainty belong in one document whose sections changed. |
| [goals/reconcile-section.md](goals/reconcile-section.md) | goals/reconcile_section.rs | This goal reconciles one section or whole-file input. |
| [goals/rejudge-pair.md](goals/rejudge-pair.md) | goals/rejudge_pair.rs | This goal judges one pair: a requirement that was created or revised against one neighbor the harness selected because it overlaps it (a sha |
| [goals/retrace.md](goals/retrace.md) | goals/retrace.rs | This goal repairs one node whose upstream died: a curated view lists a dead member (in members, excluded, or collapse), a derived fact lost  |
| [goals/review-entity.md](goals/review-entity.md) | goals/review_entity.rs | This goal judges one entity whose fact set changed: load it in full and read its requirements, gathered across all documents, as a whole, wi |
| [goals/split-view.md](goals/split-view.md) | goals/split_view.rs | This goal resolves one view over a limit: members per structural view (20 soft, 30 hard), edges per view (40, 60), members per flow view (12 |
| [goals/verify.md](goals/verify.md) | goals/verify.rs | This goal settles one ledger row's verdict, and only an llm row reaches you: programmatic rows use run_tests, which records command or per-c |
| [runtime/compaction-continuation.md.mustache](runtime/compaction-continuation.md.mustache) | harness/runtime.rs | Continue the active session `{{{SESSION}}}`. |
| [runtime/compaction-request.md](runtime/compaction-request.md) | harness/runtime.rs | Runtime compaction boundary |
| [runtime/embedded-agent.md](runtime/embedded-agent.md) | harness/runtime.rs | You are a focused agent. Use the available tools to perform the requested work. |
| [runtime/ledger-continuation.md](runtime/ledger-continuation.md) | harness/runtime.rs | This ledger batch is still open. |
| [serving/serving-instructions.md.mustache](serving/serving-instructions.md.mustache) | mcp/server.rs | {{{policy}}} |
| [session/agent-contract.md.mustache](session/agent-contract.md.mustache) | payload.rs`, `session/prompt.rs | You are one session of jazyk, a natural language compiler. Resolve only the goals listed under ## Goals through the supplied tools, record t |
| [session/feedback-note.md](session/feedback-note.md) | session/prompt.rs | If anything here is ambiguous, wrong, or confusing (these instructions, a tool, its arguments, or an error message), call report_feedback sa |
| [session/gates.md](session/gates.md) | session/prompt.rs | every proposal decided: place_anchor or orphan_anchor staged for each |
| [session/open-batch-protocol.md.mustache](session/open-batch-protocol.md.mustache) | payload.rs`, `session/prompt.rs | PROTOCOL: the `jazyk` tools carry this batch, {{{target}}}, which is already open: its goals and tools |
| [session/serving-protocol.md.mustache](session/serving-protocol.md.mustache) | payload.rs`, `session/prompt.rs | PROTOCOL: the `jazyk` tools carry this batch, {{{target}}}, claimed by the begin_goals call that |
| [session/session-blocks.md.mustache](session/session-blocks.md.mustache) | session/prompt.rs | (same agent contract as the earlier batch in this session; unchanged) |
| [session/worker-protocol.md.mustache](session/worker-protocol.md.mustache) | payload.rs`, `session/prompt.rs | PROTOCOL: the `jazyk` tools carry this batch, {{{target}}}. |
| [shared/inspect-evidence.md](shared/inspect-evidence.md) | Included by another payload | Inspect the actual deliverable or current implementation, not a comment claiming |
| [verification/judge-input.md.mustache](verification/judge-input.md.mustache) | verify/judge.rs | {{{criterion}}}{{{criteria}}} |
| [verification/verify-criterion-contract.md.mustache](verification/verify-criterion-contract.md.mustache) | verify/artifacts.rs`, `verify/judge.rs | Judge only the supplied criterion against the implementing files and its criteria text. |
| [verification/verify-legacy-contract.md](verification/verify-legacy-contract.md) | verify/judge.rs | Run `run_tests` for this requirement. The harness executes the legacy command in |
| [verification/verify-llm-contract.md.mustache](verification/verify-llm-contract.md.mustache) | verify/judge.rs | Confirm the requirement against its implementing files. |
| [verification/verify-task-contract.md](verification/verify-task-contract.md) | verify/judge.rs | Run `run_tests` for this requirement. Its `task:<id>` reference names a saved task, |
