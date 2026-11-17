# Hosted build 1700 review

## Evidence

Read-only PostgreSQL inspection on 2026-11-17 found the latest build in job `1700`, queued at `2026-11-17T16:25:49Z`, for conversation `chat-24`. Its six-step workflow stopped at compile. Generation, build, test, and review did not run.

- 18 sessions, 26 applied mutations, 1,685,309 reported tokens.
- Verdict: 30 open, 47 blocked, 5 optional advised, 8 parked.
- Previous workflow `1690` stopped at compile on November 14, with the same favicon reconciliation goal parked, 12 sessions and 2,709,309 reported tokens.
- The current model was `GPT-5.4 mini (OpenAI direct)`. These are reported token totals, not a cost calculation.

## Runtime failure

Session `b249-1` successfully classified `content/smotana-favicon.ico`, marked its blob covered by `req:footer-3`, and committed. The build loop then reloaded the store and synchronized the parse snapshot from the start of the build. That snapshot predates the classification and has a different blob envelope. Synchronizing it invalidates the committed coverage and reopens work.

The next worker's `begin_goals` independently scans current inputs. Its board no longer contains that stale goal. The runner treats an MCP tool error as a successful opening message and sends the board and error to the model. Sessions `b251-1`, `b253-1`, `b255-1`, and `b257-1` are labeled as favicon reconciliation but attempt pair judgments from the fresh board. A request for `b258-1` is rejected because the serving is scoped to `b257-1`. Four attempts park the stale goal. Extra prompt instructions cannot repair this contradiction.

The runner also captures its tool catalog before opening a batch. This exposes the unclaimed serving's broader catalog instead of the active goal's permitted tools.

## Prompt friction

Earlier blob sessions repeatedly try to create requirements in referring documents, or mark an input non-normative despite an existing selecting requirement. Several announce completion in prose while the batch remains open. The blob contract needs to state that an existing referring requirement is sufficient for coverage, its exact ID belongs in the note, and `done` saves the result. An already opaque input does not need classification just to acknowledge its role.

Other rejected calls involve multi-sentence quotes and stale anchors. The gates correctly reject those calls, and the model recovers in the successful extraction sessions. No evidence supports relaxing provenance or acceptance gates.

## Repair and limits

- Re-scan current inputs after sessions before synchronizing and deriving the next board.
- Reject failed opening claims before inference, preserving the actual error.
- Read tools after claim or recovery restore.
- Clarify blob coverage and completion without creating duplicate requirements.

Regression tests exercise persisted classification and coverage across reload, and error replies that include a tempting unrelated board. Local engine tests, formatting, portable checks, and release builds provide deterministic validation. No live project graph was edited and no paid compile was rerun during this review. Deployment does not itself prove that this project's remaining semantic judgments converge.

## Other friction and repair

The trace has 44 rejected or skipped tool calls and nine failed session attempts. Several asset sessions try to create a cross-document requirement instead of naming the existing referring requirement in their coverage note. Some claim completion in prose without calling `done`. Quote and multi-sentence errors are recovered in the landing-page sessions.

The repair refreshes parsed inputs after sessions, refuses erroneous opening claims before inference, and reads the tool catalog after claim or recovery restore. The asset prompt explicitly reuses existing referring requirements and finishes through `done`. Provenance, scope, coverage, and completion gates remain enforced.

The failed production workflow was inspected without modifying its graph or starting another model run. A successful software release is distinct from a new project build reaching convergence.

## Local validation

- `cargo test --locked --config 'profile.dev.package.jazyk.codegen-units=39'`: 1,297 library tests and six CLI tests pass, four library tests ignored.
- `cargo +1.99.1 check --locked --target wasm32-unknown-unknown --lib`: passes.
- `cargo fmt --check`: passes.
- `cargo build --locked --release`: passes.
- New tests preserve asset classification and coverage across reload and reject opening errors before model execution. Scheduler fixture files are now present on disk; existing retry and budget assertions remain intact.
- Added documentation text has no em dashes or new unresolved links. The unchanged `../frontends/gui.md#board` link in the reconciler documentation does not resolve.
