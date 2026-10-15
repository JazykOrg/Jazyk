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

## Completion evidence

The release now includes Docker/Podman selection, authenticated native Cloud
execution, ACP-owned cancellation, definition-pinned publishing/private-registry
grants, environment editing, explicit legacy conversion, paged history, log search,
before/after file previews, and usage split between models and containers. The
shared prompt covers task selection, finite execution, logs, repairs, test evidence,
budgets, and credentials in compilation and generation sessions.

The owner delegated the autonomy choice. A person saves a credential grant for the
exact task definition. An agent can execute it within the account limits. It cannot
create a grant or carry one across a definition change. Revoking a grant cancels its
active runs. Project source changes do not invalidate the grant.

- Final compiler suite: 661 passed, 3 optional tests ignored.
- Cloud suite: 95 passed, 3 optional tests ignored; storage integration passed.
- Editor unit suite: 417 passed.
- Cloud browser suite: 92 passed. Its four runtime cases passed again after fixes.
- Final general browser suite: 136 passed, 7 optional cases skipped.
- Real Docker and Podman roundtrips passed. Both include declared products
  under `target/`. Both also changed
  settings into invalid TOML during execution and successfully cancelled the run.
- Real ACP proxy with no IDE terminal passed Docker and Fargate submission, ownership
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
- The final Files regression covers native and virtual filesystems plus Chrome.
  Explicit products in build directories are visible and readable. Ordinary cache,
  secrets, and symlinks to secret paths remain hidden.
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
