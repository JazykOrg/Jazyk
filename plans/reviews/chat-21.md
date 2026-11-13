# Chat 21 review

Source: uploaded `chat-21.json`, version 1, chat ID `chat-21`, 100 updates.
Reviewed against engine master `51a83a7d`. Update numbers below refer to the
export's `n` field, not separate user turns. There is one completed user turn
and a second user request with no response in this export.

## Findings

1. The agent stops on an untested capability claim. Neither `classify_input`
   on the introduction or setup index nor `delete_requirement` is attempted.
   The store already prunes requirement-derived entity mentions before checking
   classification in the same changeset. Use `edit_graph` to delete mistaken
   extractions and classify their source atomically, without editing prose.
   Independent mentions, entity provenance, attributes, or criterion sources can
   still block classification. The export does not establish that every remaining
   quote is requirement-derived. Inspect the actual references before deciding.
2. The task response points exclusively to CLI or GUI release approval.
   Writable native and hosted chat already exposes `release_work`. The agent
   should use that tool for a requested build stage, subject to separate cost and
   human decision rules. Release generation after graph repairs because approval
   names the current generation. Do not change the manual workflow mode.
3. The agent treats asset metadata as the endpoint of a deliverable request.
   It creates no symlinks, claims no generation package, writes no implementation,
   records no generation, and runs no checks. A blocked article does not cancel
   independent website work. Build through reconciliation, implementation, and
   evidence. Do not call a metadata edit a completed build.
4. Article prose has been extracted as requirements for Jazyk, the compiler,
   IDEs, and Cloud. The website needs requirements to present those supplied
   articles, not implement the products they describe. Preserve the layout index
   as specification and reclassify the articles. Do not drain the unrelated goal
   board or manufacture examples to satisfy those mistaken extractions.
5. The agent assumes a Markdown section at `/`, causing an avoidable error and
   two skipped calls. Use returned section IDs. Inspect each result in a batch.
6. Five byte inspections are repeated without intervening changes. Two section
   loads inject large extraction skills, and `goals(full: true)` fetches unrelated
   human questions. Reuse current evidence and request focused or compact data.
7. Several raw outputs end near 8 KiB inside JSON or text. This limits what this
   export proves about complete tool responses. It does not prove provider input
   was similarly truncated. Treat export truncation separately from tool failure.
8. The final response correctly admits that nothing was built or tested. Its
   categorical missing-capability diagnosis and requirement for fresh generation
   approval are not established by the attempted tools. The feedback repeats that
   unsupported diagnosis. A local feedback entry neither repairs nor completes work.

## Turn-by-turn inspection

| Updates | Observed action and assessment |
| --- | --- |
| 1 | User explicitly requests asset classification, symlinks, and a built deliverable. This is an action request covering all three. |
| 2 | Agent's opening correctly reflects the full requested outcome. |
| 3-4 | Lists workflow skills. Appropriate discovery. |
| 5-6 | Reads inspection skill. Appropriate. |
| 7-8 | Reads generation skill. Appropriate, but never follows its generation lifecycle. |
| 9-10 | Reads compile skill. Appropriate prerequisite; compilation is never attempted. |
| 11-12 | Lists files. Useful inventory, including existing deliverable and tests. |
| 13-14 | Reads project front door. Establishes website scope. |
| 15-16 | Reads document sets. Establishes source mapping, so missing mapping is not the blocker. |
| 17-18 | Reads project context. Correctly establishes Cloud, repository, branch, and unsynced work. No publication authorized. |
| 19-20 | Reads project instructions. Appropriate. |
| 21-22 | Reads building rules: static site, no JavaScript, deliverable directory. These must constrain implementation. |
| 23-24 | Reads menu specification. Correctly resolves the relative docs link. |
| 25-26 | Reads documentation layout index. Correctly keeps this as specification. Many target articles are absent; preserve this limitation and do not invent their contents. |
| 27-28 | Reads existing HTML. Necessary, but the exported output is cut inside serialized content. No later source inspection or implementation follows. |
| 29-30 | Inspects layout index bytes. No classification follows for this file; not needed to ship the linked articles. |
| 31 | Correctly distinguishes layout specification from supplied article content. |
| 32-33 | Reads introduction. Useful content evidence; it describes Jazyk, not obligations to implement a compiler in the website. |
| 34-35 | Reads setup article. Useful content evidence, including a missing `ide.md` link. |
| 36-37 | Reads blank coding-agent article. Correct to preserve blank content rather than invent it. |
| 38-39 | Reads web article. Identifies its referenced image. |
| 40-41 | Inspects introduction hash. Appropriate before classification. |
| 42-43 | Inspects setup index hash. Appropriate. |
| 44-45 | Inspects coding-agent hash. Appropriate. |
| 46-47 | Inspects web article hash. Appropriate. |
| 48-49 | Inspects image hash without parsing. Appropriate. |
| 50-51 | Loads nonexistent `content/docs/index.md#/`. Avoidable guessed section ID. Tool reports `bad-target`. |
| 52-53 | Generation task query is skipped after the earlier failure. Not evidence of a generation query failure. |
| 54-55 | Skill read is also skipped. Must inspect batch outcomes. |
| 56-57 | Successfully retries the needed skill read. Appropriate recovery. |
| 58-59 | Lists 16 generation tasks, all gated; also reports 61 binding items and 29 stale rows. The task guidance omits the existing chat release route. Stale evidence needs verification, not an assertion of completion. |
| 60-61 | Requests entire goal board with hints. Expands unrelated human questions derived from article content. Compact or targeted inspection would suffice. Export is truncated. |
| 62 | Announces another inspection rather than acting on already gathered evidence. |
| 63-64 | Loads documentation entity. Confirms mixed article and layout references. Does not establish missing mutation capability. |
| 65-66 | Repeats unchanged introduction inspection. Tool explicitly says to act on existing result. |
| 67-68 | Repeats unchanged setup inspection. Same waste. |
| 69-70 | Repeats unchanged coding-agent inspection. Same waste. |
| 71-72 | Repeats unchanged web article inspection. Same waste. |
| 73-74 | Repeats unchanged image inspection. Same waste. |
| 75 | Correctly distinguishes files with existing quotes. Assumes manual mention retraction is needed without checking the deletion cascade. |
| 76-77 | Successfully classifies blank article as asset, committed generation 175. Actual metadata progress; no deliverable change. |
| 78-79 | Successfully classifies web article, generation 176. Same limitation. |
| 80-81 | Successfully classifies image, generation 178. Do not infer the reason for the generation jump from this export. |
| 82-83 | Loads introduction section and extraction skill. Lists mistaken requirements but performs no repair. Export is cut. |
| 84-85 | Loads setup section and extraction skill. Same issue; asset migration does not require another extraction pass. |
| 86 | Announces graph reference inspection again. No attempt at atomic repair. |
| 87-88 | Reads Jazyk entity. Many article-derived references; output is cut. Does not prove every mention is independent. |
| 89-90 | Reads documentation entity. Confirms layout and article contributions coexist. Preserve layout requirements during repair. |
| 91 | Announces recovery investigation. Useful only if followed by checking actual supported mutations. |
| 92-93 | Reads recovery skill. Reasonable, but recovery instructions do not replace attempting a supported repair. |
| 94 | Declares missing mention capability. Premature: no requirement deletion or classification attempt on the two articles. |
| 95-96 | Logs local feedback with the unproven missing-capability claim. No remote transmission; no repair. |
| 97 | Final admits no symlinks, implementation, builds, or tests. Honest outcome, incomplete execution, unsupported diagnosis. |
| 98 | Turn ends normally. This is agent abandonment of the requested outcome, not an engine crash in the export. |
| 99 | Workflow marker for job 1685. No evidence of its execution or conclusion. |
| 100 | User adds Footer build to scope. Export ends here. Its handling cannot be reviewed. |

## Changes and verification

- `inspect_input` returns `quoteReferences`: requirement IDs, entity mentions with
  supplying requirement IDs, quoted entity fields, and criterion IDs. The
  classification gate uses the same enumeration to avoid divergent diagnostics.
- Tool descriptions and documentation explain the graph-only atomic repair path.
  Independent provenance remains protected; classification deletes nothing implicitly.
- Shared conversational policy and deferred skills carry a requested deliverable
  build through necessary reconciliation, implementation, and local verification.
  Guidance covers the existing release tool, source links, and independent work.
- Generation's gated response names the available chat release path conditionally.
  Read-only and non-chat release permissions remain enforced by the existing tool.
- Inspection guidance covers real section IDs, skipped calls, repeated reads,
  and compact board inspection.
- Regression coverage exercises classification with cached entity mentions,
  retention of an independent layout requirement, unchanged article bytes, and
  an actual deliverable symlink. Separate coverage enumerates independent quotes
  without mutating them.

This review changes the engine and its guidance. It does not replay the uploaded
Cloud project, edit its deliverable, run paid model sessions, deploy Cloud, or
prove a future model will follow the repaired workflow. The Cloud service must
adopt the new engine revision before hosted chats receive these changes.

## Continued build screenshot

The follow-up screenshot shows a different, concrete engine defect. The agent
loads `crit:footer-3`; `LoadedSet::render_full` and `stub_line` had no criterion
branch. The loader therefore reports an unknown target even if the criterion
exists. Diagnostic and document reads in that reply are then skipped by the
batch's stop-after-error behavior. They are not independent read failures.

The screenshot alone cannot establish the outcome of the entire continued build.
It establishes a failed criterion lookup and two skipped calls, not a user
approval request. The UI combines a success phrase (`Project context loaded`)
with `needs attention`, leaves the responsible party unnamed, and displays
agent-facing identifier grammar to the user.

- Criterion nodes now load and expand through their stub handles. Their evidence
  fields, provenance, and requirement reference remain budgeted context. Depth two
  can load the requirement neighbor. A regression uses `crit:footer-3`.
- The editor change is in the sibling Cloud repository. Failed context reads say
  `Agent could not load project context`. Skipped calls have a neutral status and
  explain that the agent needs to recover from the earlier error and retry.
- Ordinary lookup explanations do not ask the user for a decision. Raw errors,
  internal IDs, and lookup syntax stay in `Technical details`. A failed action
  never retains its success title. Actual permission or decision requests still
  need a concrete explanation.
- Logic tests and rendered-card tests cover the screenshot's failure and skipped
  document read. The original tool receipts remain available for debugging.

## Database continuation

Read-only PostgreSQL inspection finds the same conversation in `record_streams`
(`chat-updates`, key `chat-21`, stream 5), through update 301. The latest activity
is 2026-11-13 16:38:33 UTC. No database records or project files were changed.
The persisted chat outputs retain the same roughly 8 KiB truncation as the export.
The job completion events provide additional authoritative build outcomes.

### Failed workflow 1685

The job ran from 16:10:45 to 16:17:58 UTC. Its final event is sequence 389,
`job.finished`, state `failed`. Its compile step stopped before generation:

- One mandatory goal failed: reconciliation of the JPEG at
  `content/docs/setup/jazyk-web-gui.jpg#/`.
- The failed justification says the image envelope has no incoming references,
  and its referring Web IDE article is an asset, so no quoted selection of the
  JPEG is available. This is a dependency-inventory gap: asset classification
  hides the article's image links from the scanner.
- The report records 12 sessions, 19 applied mutations, 1,067,980 tokens,
  97 percent coverage, 26 open goals, one failed goal, and 95 blocked goals.
  These are graph mutations and compile progress, not deliverable generation.
- Stopping the workflow's generation step after incomplete compile preserves
  the stated lifecycle. The lost nested input reference causes the failure.

Text asset envelopes now inventory their links without projecting article
sections or extracting article requirements. Incoming prose references propagate
through asset dependencies. Generation uses the same links transitively under
its existing safety and Source checks. Cycles terminate. A requirement selecting
an article can therefore select its supplied JPEG without quoting the article.
Regression coverage includes a JPEG, cyclic article links, a blocked escaping
path, coverage against the originating requirement, and byte fingerprint changes.

### Continued agent job 1686

The job ran from 16:26:38 to 16:38:34 UTC and ended `completed`. That describes
execution of the chat turn, not completion of the user's entire requested build.
Unlike the first turn, it does create deliverable changes. Updates 243, 252, and
254-258 save HTML, create the Smotana symlink, and save browser test changes.
Update 263 accepts a five-file manifest with six tests. Updates 268 and 271
report six verified Footer checks, 25 passing suite cases, no failures, a
terminated successful task, and exit code zero. These are persisted tool results,
not merely the agent's claims. They do not establish the docs deliverable exists.

| Updates | Observed action and assessment |
| --- | --- |
| 101 | User explicitly asks for retry and a completed deliverable, including feedback for genuine tool gaps. |
| 102 | Agent accurately admits the prior attempt did not generate the deliverable. |
| 103-108 | Reads compile, generate, and verify skills. Appropriate. |
| 109-114 | Loads integration, set mapping, and files. Useful refresh for the new turn. |
| 115 | Announces the correct scope: Footer and remaining documentation. |
| 116-119 | Full goal board and generation tasks. Useful blockers, but still unnecessarily broad. |
| 120-123 | Reads updated Footer and instructions. Correct before editing. |
| 124-125 | Releases compilation under explicit user authority. Appropriate. |
| 126-128 | Releases generation before graph repairs. Later repairs invalidate it, causing extra gate round trips. |
| 129-134 | Loads Footer, HTML, and logo. Useful evidence. |
| 135-137 | Reads recovery skill and announces generation package. No implementation yet. |
| 138-139 | Claims Footer generation. Appropriate ownership package. |
| 140-145 | Reads styling, Footer suite, and task context. Reuses the existing browser toolchain. |
| 146-150 | Repeats generation skill and unchanged HTML read. Additional inspection without a changed source. |
| 151-158 | Reads actual Footer tests and browser suite. Necessary before changing assertions. |
| 159 | Correctly identifies the stale criterion and supplied assets. |
| 160-161 | Existing criterion load fails due to the loader defect fixed in this review. |
| 162-171 | Five later reads are skipped, not separate underlying failures. UI must explain this distinction. |
| 172-183 | Successfully retries requirement, diagnostic, document, logo, asset, and file reads. Good recovery. |
| 184 | Resolves colour from existing styling prose and corroborating logo. Good evidence, no arbitrary new choice. |
| 185-186 | Attempts to re-source a quoted criterion through `update_criterion`; receives a specific refusal. |
| 187-188 | Correct atomic recovery: re-source with `upsert_criterion`, update its statement, resolve the now-answered colour warning. |
| 189-190 | Generation begin fails because graph mutation advanced the generation beyond the earlier release. |
| 191-193 | Correctly reuses the user's existing authorization and releases current generation. |
| 194-195 | Generation now waits for criteria derivation. This is a prerequisite, not another permission request. |
| 196-202 | Reads compile skill, tasks, and board. Identifies Footer's actual prerequisite. |
| 203-205 | Begins the specific criteria batch. Appropriate limited scope. |
| 206-213 | Reads styling and relevant diagnostic and requirement. Some redundant reads, but evidence is relevant. |
| 214-215 | Creates a colour criterion anchored to the existing styling value. |
| 216-217 | Commits criteria. The remaining failed JPEG goal still makes compilation incomplete. Independent Footer generation can proceed. |
| 218-224 | Refreshes tasks, rereads skill, and releases the current generation again. Correct release authority, avoidable repeated skill load. |
| 225-226 | Claims the current Footer package successfully. |
| 227-241 | Announces implementation, rereads skill, HTML, tests, and inventory again. Work remains correct but inefficient. |
| 242-243 | Saves Footer HTML with center branding and updated attribution. First actual deliverable write in this conversation. |
| 244-249 | Inspects supplied asset and logo files. Reuses existing artwork. |
| 250-252 | Creates the supplied Smotana symlink in the deliverable. Concrete authorized progress. |
| 253-254 | Repairs stale attribution text assertion and retains navigation assertions while adding icon checks. Consistent with the changed prose. |
| 255-258 | Adds center checks and extends JUnit reporting. Uses existing test runner rather than inventing another runtime. |
| 259-263 | Reads verification skill and records five files and six tests. Manifest accepted. |
| 264-268 | Reads execution skill and runs the Footer requirements through the existing browser suite. Six selected checks pass; suite reports 25 passes. |
| 269-271 | Confirms task termination and exit code zero. Good distinction between passed checks and a finished run. |
| 272-274 | Reads recovery skill again and resumes the outstanding docs scope. Correctly preserves completed Footer work. |
| 275-276 | No diagnostic is returned for the introduction subject. This does not prove the input needs no reconciliation. |
| 277-280 | Guesses pending alignment targets; first is absent and second is skipped. Use the actual board rather than assuming alignment proposals exist. |
| 281-284 | Refreshes the two remaining article inputs. Appropriate after intervening build activity. |
| 285-286 | Repeats the guessed Markdown `/` section error seen in the original turn. Fixed inspection guidance covers it. |
| 287-293 | Reads article text and searches graph references. Appropriate recovery, though repeated work remains costly. |
| 294-299 | Reads three entities. Still does not attempt graph-only requirement deletion and asset classification. |
| 300 | Accurately reports Footer verification, but repeats the unproven missing entity-mention capability claim and stops with docs unfinished. |
| 301 | Chat turn ends normally. Job completion does not mean the complete requested build succeeded. |

### Additional improvements

- Explain three distinct outcomes: a compile prerequisite failed, a tool call was
  rejected and needs agent recovery, or a concrete user decision is required.
- Prefer actual board prerequisites over invented alignment targets. Use returned
  section IDs instead of assuming every Markdown document has `/`.
- Repair graph facts before releasing generation and claiming current packages.
- Re-source quoted criteria with the supported atomic two-operation path.
- After scoped verification succeeds, preserve it and continue independent work.
- Do not describe a completed chat job as a completed product build. In this
  continuation the Footer is implemented and verified, while documentation remains
  incomplete. The earlier blanket impression that nothing is ever generated is
  no longer accurate for job 1686.
