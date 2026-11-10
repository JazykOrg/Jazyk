# Build review, 2026-11-10

## Evidence

Read-only PostgreSQL queries over the existing SSH operator connection identify one new workflow job today, job `822`, queued at `21:31:39 UTC`. It is still running at the review snapshot. Earlier completed and failed workflows are dated November 9 or earlier. No build was started or retried by this review.

The retained snapshot contains 523 events, 25 `report_feedback` calls, and 20 `done` calls. Counts describe a running job, not its final outcome. The reviewed source starts at engine revision `9f95233`.

## What worked

- The main document navigation section resolves in two model rounds without inventing requirements.
- The landing title section resolves in two rounds by reusing `req:landing-1`, preserving existing extraction.
- The schema section resolves in six rounds. One requirement records the availability obligation and four filenames. Graph mutations open entity review and criterion derivation work.
- Provenance and coverage gates refuse an unsupported blob coverage claim. The agent inspects the stored requirement, identifies the missing link in its quote, reports actionable feedback, and refuses to invent envelope provenance or dismiss a normative input.

## What failed

- Enumeration extraction quotes only the lead-in for `req:schemas-wbnz`. The statement lists four schema files, but its quote selects none. Blob coverage uses quoted references, so the requirement ID in a coverage note is insufficient.
- `update_requirement` refuses the repair in the referring prose section as `wrong-document` while the session owns the blob. This conflicts with the blob contract, which requires referring prose provenance and forbids quoting the envelope.
- Invalid blob coverage is accepted into the changeset and rejected only by `done`. Failing the goal leaves that invalid mark staged. Abandoning loses the outcome and repeats work.
- A clean reclaimed batch also cannot save the failure: `done` requires a coverage mark even for an explicitly failed reconciliation goal.
- The case schema parks after four attempts. The graph schema then enters the same cycle. Most reviewed tool traffic after initial reconciliation is feedback and recovery rather than graph progress. Multiple sessions share generation-based label `b135-1`, so labels alone do not identify a unique attempt.

## Repairs

- Reference-bearing enumeration quotes include the linked items needed to select opaque inputs.
- A blob session may re-anchor an existing requirement within its original prose section when the quote selects that blob. Other cross-document writes retain their restriction. Normal quote location and sentence validation still apply.
- Blob coverage validation runs before staging. Errors explain missing quoted references instead of displaying an empty set as the repair instruction.
- Explicitly failed reconciliation goals may finish with unmarked sections. Failure still blocks convergence. Staged coverage remains validated.
- Session instructions describe the allowed repair and failure paths.

## Verification limits

Deterministic local tests cover the tool paths. The active hosted job uses its existing binary and is not changed by this source repair. No model replay, hosted retry, graph mutation, or deployment is performed. Documentation graph reconciliation remains pending.

## Local validation

- `cargo test --locked` passes: 1,214 library tests and 6 CLI tests, 4 ignored library tests. The new regression exercises refused staging, a clean failure finish, restricted cross-document writes, and successful reference repair.
- `cargo fmt --check` and `git diff --check` pass. Docs edits preserve existing link targets and contain no em dashes.
- The portable library check passes for `wasm32-unknown-unknown` with the pinned rustup compiler. The shell Homebrew compiler lacks that target despite the rustup target being installed.
- `cargo build --locked --release` passes. The shared release output refreshes the owner's local binary.
- The initial native dependency download fails certificate validation. Retrying with the system CA bundle succeeds without weakening TLS.
