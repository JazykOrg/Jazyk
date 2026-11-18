# Build 1716 review

## Evidence and outcome

Read-only production PostgreSQL inspection on 2026-11-18 identifies job `1716`,
conversation `chat-29`, in matusfaro/jazyk-org. It ran from 20:26:38 to 20:43:23 UTC.
The requested workflow was compile → generate → build → test → review. Only compile ran.

Compilation returned incomplete, and the workflow recorded failure because mandatory
graph work remained. Four sessions ended their turns with the batch still open.
The scheduler reached its four-attempt limit and parked goals. There was no recorded
model transport crash and no explicitly failed goal in the final report.

- 49 derived goals, 34 open, 47 blocked, 31 parked, 15 optional advised.
- Four sessions, zero applied mutations, 100 percent input coverage.
- 6,808,419 reported tokens, not unique input text and not a dollar calculation.
- 309 tool calls: 173 mark_goal_done, 30 load, 28 load_skill, 17 diagnostics.
- 67 tool-error trace events: 25 actual refusals and 42 subsequent skipped calls.

## What survives

All four attempts remain saved in the host record store. Their staged mutation counts
are 7, 14, 6, and 8; their outcome counts are 24, 15, 11, and 11 respectively.
These are overlapping attempts, not 35 independent edits to apply together.
They are not committed graph changes. Recovery inspects open_work/read_open_work,
adopts a suitable attempt, repairs the remaining gates, and calls done. Preserve the
other attempts until useful non-overlapping judgments have been reconciled. Starting a
fresh Build did not automatically adopt them.

## Failure mechanism

Each worker held 10 pair judgments, 12 entity reviews and two view reference repairs.
One review's completion gate checks whether diagnostic subjects exist using only
entity and requirement maps. A diagnostic naming both ent:diagram and a live section
therefore fails as though the section were deleted. Both complained-about landing
sections are present in all four persisted snapshots. This is a compiler defect.
Criterion subjects suffer the same defect.

The model repeatedly repairs one claim and meets another. Some refusals are legitimate:
missing lookalike/diagnostic judgments, claiming a definition current after staging a
replacement, and marking a view repaired while it retains dead references. The all-goal
completion requirement makes the latter block otherwise useful pair judgments.

One failed tool suppresses later writes in that model reply. A 14-claim reply can
therefore produce one refusal and 13 skipped actions. The model did batch some calls;
112 tool-bearing request intervals contained one call, while others contained as many
as 15. Nine judgment skill loads and nine flow-view skill loads show repeated retrieval.
Diagnostics is a read of recorded project findings, not another compiler pass.

## Repairs

- Accept live section and criterion subjects in diagnostic review evidence, retaining
  rejection of genuinely missing subjects.
- Keep graph kinds in separate commit batches while preserving same-kind batching and
  existing component-local ledger grouping. Unfinished view repair no longer shares a
  commit boundary with completed pair judgments.
- Support batched skill lookup with independent per-skill results. Remove conflicting
  hardcoded skill limits from the documentation.
- Continue independently validated goal outcome calls after another tool fails. Keep
  ordinary writes skipped until the agent has inspected the preceding failure.
- Add explicit done checkpoints that save valid partial progress while unclaimed
  goals remain open, including commit-time revalidation.
- Add saved unfinished-attempt metadata and a recovery explanation to incomplete
  workflow results. Distinguish staged persistence from graph commit.
- Use plain project-finding labels. Hide skipped calls in normal chat and build
  activity; retain them in Debug while showing the original failure.

These changes do not mark unfinished goals successful or discard graph-integrity,
provenance, or evidence checks. Ordinary completion still requires all goals; explicit
checkpoints retain unclaimed goals without asserting completion. Automatic adoption of overlapping attempts is not implemented.

## Feedback inventory

All 17 stored project feedback entries were read, including the two submitted during
this build. Older entries describe earlier contracts; the disposition below separates
current defects from already changed behavior. Local repair is not live proof of a
successful semantic compilation.

### 1. quote-multi-sentence

Recorded 2026-11-11T18:55:31Z.

The validator treats a trailing '(via [MCP](https://en.wikipedia.org/wiki/Model_Context_Protocol))' fragment as a second sentence, although it qualifies the preceding configuration sentence. I will record the configuration and protocol separately with their respective verbatim quotes.

Disposition: Current multi-source requirement contract permits a quote containing several sentences. The old single-sentence rejection belongs to the earlier contract.

### 2. read_section

Recorded 2026-11-11T19:03:51Z.

The setup documentation links to web.md, but read_section rejected content/docs/setup/web.md#/ as unknown-section. There is no documented tool for discovering a linked document's valid root section reference; returning valid references with this error would help. I will continue from the loaded source sections.

Disposition: Section discovery is available through document and section reads. Keep the distinction between a Markdown heading reference and the whole-file asset `/` reference.

### 3. generation diagnostics

Recorded 2026-11-11T19:09:05Z.

The loaded requirement needs actual Cloud subscription and hosted-project access, but the recorded medium is a static HTML/CSS website and the existing deliverable contains only a landing page. No Cloud service endpoint, authentication contract, subscription provisioning, or hosted-project interface is supplied. The serving provides diagnostic inspection but no tool to file an author-facing diagnostic about these missing product decisions. I cannot implement this behavior honestly by adding descriptive copy or a simulated connection.

Disposition: A static website cannot satisfy hosted provisioning by descriptive text. The feedback identifies missing product decisions, not permission to invent a service.

### 4. classify_input requires an open compiler batch

Recorded 2026-11-12T15:48:12Z.

The user requests that input classification be available anytime. classify_input previously failed with no-open-batch, forcing a compile release and reconciliation batch merely to classify reader-facing documentation as assets. Classification should work directly in chat without begin_goals or a compile release, committing immediately outside a batch and staging within one, while retaining byte-hash validation and provenance safety checks. Its tool description should clearly describe this behavior.

Disposition: Incidental classification is now available through the edit-documentation workflow. Do not start paid compilation for a conversational classification edit.

### 5. asset classification batch scope

Recorded 2026-11-12T15:52:24Z.

The user authorized classifying five reader-facing files as assets, but the opened batch authorizes only three sections of content/docs/setup/index.md and says to resolve only those goals. Whole-file classification requires retracting or relocating provenance from other sections of that file (including five requirements loaded from its ancestors), outside this batch's scope. Please provide a supported way to open an asset-transition changeset covering the entire file and its provenance, rather than requiring unrelated section reconciliation first. I have not classified or retracted anything.

Disposition: Classification scope and input inventory repairs landed before this review. A new model run is still needed to prove project convergence.

### 6. classify_input: retract existing entity mentions without editing prose

Recorded 2026-11-13T16:04:29Z.

Asset reclassification of content/docs/intro.md and content/docs/setup/index.md requires existing quotes to be relocated or retracted. delete_requirement supports graph-only requirement removal, but no available tool removes individual entity mentions while preserving the entities and unchanged article prose. retract_requirement would delete prose and is unsuitable. Please provide a graph-only mention retraction or atomic asset-reclassification migration that preserves source bytes. Three other inputs were successfully classified; remaining classification and reconciliation are blocked on this capability.

Disposition: Graph-only mention retraction and atomic edit_graph are present. Preserve source bytes when classifying supplied assets.

### 7. read_section asset reference context

Recorded 2026-11-13T16:13:54Z.

The JPEG envelope has no incoming references, while its classification says it is referenced by the Web IDE article. That article is itself classified as an asset, so read_section returns only its envelope, not the prose needed to establish the JPEG's quoted reference. No supplied tool exposes that asset's textual contents or nested references.

Disposition: read_document/read_text_file expose asset text. Section envelopes remain structural metadata, not the supplied prose.

### 8. begin_goals batch scoping

Recorded 2026-11-14T22:31:56Z.

A serving scoped to batch b206-1 persists after the board advanced to generation 207, where b206-1 no longer exists and only b207-1 is offered. begin_goals rejects both the batch form and the goals-list form with wrong-target ('this serving is scoped to b206-1; batch b207-1 belongs to another session'), and begin_goals(b206-1) fails with no-ready-batch. The session is stuck in a catch-22: it cannot claim the only ready batch, and no tool re-scopes the serving. A way to re-scope or refresh the serving from within the session (or automatic re-scoping when the claimed batch disappears from the board) would have unblocked the four judgment goals.

Disposition: The previous build-1700 repair refreshes input inventory and refuses stale opening claims before inference.

### 9. begin_goals serving scope

Recorded 2026-11-14T22:32:23Z.

The serving is scoped to batch b208-1, but that batch no longer exists on the board; the only ready batch is b209-1, which begin_goals refuses with wrong-target ('belongs to another session'). There is no tool to switch the serving scope or re-target, so the four ready goals (three rejudge-pairs and review-entity for ent:footer) cannot be claimed by this agent. A way to re-scope a serving to a current batch, or allowing begin_goals to claim a batch when its scoped batch is gone, would unblock this.

Disposition: Same stale-scope issue as entry 8.

### 10. begin_goals / serving scoping

Recorded 2026-11-14T22:32:54Z.

A serving stayed scoped to batch b210-1 after that batch disappeared from the goal board (re-formed as b211-1). begin_goals with no args, with the new batch id, and with the underlying goal ids all fail with wrong-target, and no tool can re-scope or release the stale serving binding, so the session is deadlocked. A way to re-scope a serving to the current board, or an automatic re-scope when the claimed batch no longer exists, would have helped.

Disposition: Same stale-scope issue as entry 8.

### 11. begin_goals / serving scoping

Recorded 2026-11-14T22:33:32Z.

Serving is scoped to batch b212-1, but that batch no longer exists on the board (generation 213 only lists b213-1). begin_goals for b213-1 (by batch id and by goal ids) is refused with wrong-target: 'this serving is scoped to b212-1; batch b213-1 belongs to another session'. There is no way to claim any work or abandon the stale scope from this serving; a way to re-scope or release a stale serving would help.

Disposition: Same stale-scope issue as entry 8.

### 12. extraction skill quote rules

Recorded 2026-11-17T16:26:33Z.

The section-level quote for the landing diagram contains two sentences, and the parallel write batch was rejected as quote-multi-sentence. I need clearer guidance on whether to split the section-level requirement into separate single-sentence requirements before retrying, since the goal text suggests one coverage mark after extraction.

Disposition: The current extraction/requirement model supports multiple sources and quotes. Do not retain instructions requiring deletion to represent same-document duplicates.

### 13. set_coverage

Recorded 2026-11-17T16:27:31Z.

The coverage gate rejected my note because it requires naming the linked requirement id, but the section is a whole-file asset envelope and I only had a derived requirement staged for its role. A clearer rule for whether derived statements can satisfy the required named-reference coverage note would help.

Disposition: Asset coverage uses an existing selecting requirement ID. The earlier blob-contract repair documents this path.

### 14. mark_goal_done / goal gate for g:reconcile-section:content/docs/intro.md#/

Recorded 2026-11-17T16:28:13Z.

The goal gate refused with unmarked-section even though the section is a whole-file asset envelope and the instructions say a coverage note can name linked requirement IDs. I need the exact acceptable coverage path for an asset envelope so I can satisfy the gate without inventing prose extraction.

Disposition: The earlier build-1700 inventory refresh preserves committed asset coverage. This run contains no new asset-reconciliation failure.

### 15. mark_goal_done for same-document duplicate rejudge-pair

Recorded 2026-11-18T01:27:26Z.

The rejudge-pair goal gate says same-document duplicates should be settled by delete_requirement plus mark_goal_done with evidence {verdict: duplicate, carried_by: <deleted id>}, but mark_goal_done refused that exact evidence because it said neither side is deleted in this session. The tool behavior conflicts with the goal contract and blocked completion.

Disposition: Current duplicate handling uses merge_requirements with a survivor redirect. The latest run successfully stages merges; it does not exhibit this old delete-evidence failure.

### 16. batch instructions / goal state

Recorded 2026-11-18T20:30:28Z.

The session is already in an open batch, but the instruction text also says to finish with done while the goal board shows the batch is still open and multiple goals are parked. It would help if the runtime clearly distinguished which goals are actionable in this batch versus only available in a future batch.

Disposition: Confirmed current friction: batches mix 24 goals across three contracts, and warnings from prior attempts obscure the current attempt. Separate graph kinds into distinct commit batches; retain saved attempts.

### 17. done / goal evidence handling

Recorded 2026-11-18T20:42:14Z.

The batch instructions mixed stale historical warnings with the current goal set, and the completion gate wording for rejudge-pair goals was easy to misread. It would help to clearly separate current open work from prior-session warnings and to spell out whether `settle_pairs` or `mark_goal_done` should be preferred for each pair.

Disposition: Confirmed current friction: claims and diagnostics are repeatedly validated, and stale-warning text confuses the model. Correct the live-section gate, allow independent outcome calls to proceed, and expose saved work on incomplete workflow results.


## Local validation

- 1,370 library tests pass, four ignored.
- Pinned Rust 1.99.1 WASM library check and clippy with warnings denied pass.
- Formatting and added documentation link checks pass.
- Editor unit suite passes before the final display changes; the focused transcript,
  action and failure suite passes all 59 tests after them.
- Ten browser chat-reading tests pass. Their endpoint setup helper uses the current
  coding-agent chooser; all existing assertions remain.
- The checkpoint integration test commits a completed section through real commit-time
  revalidation while another section stays open. Stale-anchor protection still rejects
  a checkpoint around a real unaddressed source.
- No paid project compilation is rerun during this review.
