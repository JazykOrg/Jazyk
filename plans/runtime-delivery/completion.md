# Runtime completion

Status: implementation and review complete; final deployment verification in progress, 2026-10-15.

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


## Completion evidence

The release now includes Docker/Podman selection, authenticated native Cloud
execution, ACP-owned cancellation, definition-pinned publishing/private-registry
grants, environment editing, explicit legacy conversion, paged history, log search,
before/after file previews, and usage split between models and containers.

- Final compiler suite: 659 passed, 3 optional tests ignored.
- Cloud suite: 95 passed, 3 optional tests ignored; storage integration passed.
- Editor unit suite: 417 passed.
- Cloud browser suite: 92 passed. Its four runtime cases passed again after fixes.
- Final general browser suite: 136 passed, 7 optional cases skipped.
- Real Docker and Podman roundtrips passed. The final Docker test also changed
  settings into invalid TOML during execution and successfully cancelled the run.
- Real ACP proxy with no IDE terminal passed Docker submission, ownership
  cancellation, subsequent execution, and product import.
- Native MCP to production Fargate passed two PDF/EPUB builds, cached dependency
  reuse, fresh JUnit reports, product import, and command cancellation. Run IDs:
  `28dc7b2c-b9cf-4711-a700-4d4a8ca6bda1`,
  `4a143da3-3823-4825-a922-3bc480f89bf0`, and
  `20befd09-86da-43a0-bfc1-579cf241cabf`.
- The first remote submission response was deliberately dropped. Retrying the
  same operation returned the same run. An API restart during that allocation
  preserved the provider task and the successful result.
- Direct native HTTPS passed. A changed command was refused the old grant.
  Revoking the synthetic grant cancelled run
  `6f7129ba-995b-48a3-ba2a-0a1963a259cc` and withheld new products.
- Secret injection and split-write log masking passed with a synthetic secret.
  The PDF was rendered and visually checked. EPUB structure was inspected.

## Evidence that remains external

The configured `qwen3.8:27b-mlx` model through the owner's LocalRouter failed
`gen-basic` in both native and text codecs. Neither run recorded generation,
scoring 0/4 checks. This is not a passed live-model generation/repair benchmark.
The prompt and tool protocol checks above passed with a deterministic ACP agent.

The AWS runtime cost tag was inactive and is now active. The read-only cost report
returns `pending_usage_data`, with an estimated empty AWS period. Actual invoice
comparison awaits provider data. No zero-cost conclusion is drawn.

Private-registry credential wiring is implemented. An authenticated pull from an
external private registry was not exercised with a real customer credential.
Native Apple/Windows execution, interactive shells, parallel project runs, and
additional providers remain outside this finite-container release.
