# Runtime completion

Status: implementation in progress, 2026-10-15.

The owner authorized completing the remaining first-release gaps, review, testing,
and the existing merge/deployment scope. Saved per-task credential grants are the
selected publication policy. A person saves the grant for the task definition;
agent changes cannot extend it.

## Work sequence

- Complete Docker and Podman adapters, availability, cancellation ownership, and
  real local acceptance with cache, binary products, reports, and teardown.
- Add authenticated Cloud execution for local MCP and ACP through the same run
  protocol. Keep credentials outside project settings.
- Add saved publication grants and private image credentials with revocation.
- Finish run history, before/after previews, environment settings, legacy task
  conversion, and runtime usage presentation.
- Exercise agent and IDE paths, restart/failure recovery, and representative
  document generation. Review isolation, authorization, accounting, and races.
- Build releases, merge, deploy, and verify production. Record measured results
  and any evidence dependent on an external billing cycle.

## Validation record

- Docker Desktop and rootless Podman 6.1.4 passed actual MCP execution with
  Python 3.12 Alpine, exact project inputs, two fresh allocations, cache reuse,
  binary product import, reports, logs, cancellation, and cleanup. Podman uses a
  dedicated local VM, stopped after validation.
- The ACP proxy passed an IDE fixture without terminal/filesystem capabilities.
  The injected MCP can execute tasks before compilation. Session cancellation
  stops its child, and a subsequent prompt can create a retained product.
- Full compiler suite: 657 passed, 3 optional tests ignored before the last
  remote cancellation regression test. Release binary rebuilt.
- Cloud API/PostgreSQL: 95 passed, 3 optional tests ignored, plus the separate
  storage integration test passed. Scoped tokens, revocation, definition grants,
  cross-account denial, and 65-entry history paging are covered.
- Editor: 417 unit tests passed. Run history, paging, cost, diff previews, and log
  search passed in Chrome. Expanded settings/credential UI tests are in progress.
- Added durable pending remote-submission ownership after review found a
  cancel/new-prompt race. Revoked-secret cleanup treats already missing secrets
  as complete. Cloud deployment and live credential checks are still pending.

These are protocol and runtime checks. They do not establish live model judgment
or AWS invoice reconciliation. Those are recorded separately.
