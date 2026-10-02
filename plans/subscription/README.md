# Plan: shared Jazyk servers and subscriptions

Revised 2026-10-02 from the owner's direction. This is a plan, not implemented
behavior. The architecture, delivery order, and meaning of a subscription below
supersede the 2026-10-01 proposal and its UX prototype.

## Direction

Build one filesystem-independent Jazyk server that can compile multiple customers'
projects concurrently. Project files are a versioned storage interface. The server
needs no checkout, mounted project directory, shell, or project container to compile.
The existing goal-based compiler and persistent graph remain the foundation.

Separate where source lives, where compilation runs, and which agent supplies model
reasoning. ACP and LSP continue across local and remote configurations.

| Mode | Source and durable graph | Compiler | Access |
|---|---|---|---|
| Local-local | Computer folder | Local Jazyk server | Accountless, own provider or agent |
| Local-remote | Computer folder through a local connector | Shared remote Jazyk server | Jazyk account, entitlement, and credits |
| Remote-remote | Cloud storage or GitHub-backed workspace | Shared remote Jazyk server | Jazyk account, entitlement, and credits |

GitHub-backed source is a selected repository/branch with a private draft overlay.
Graph state is a durable checkpoint by default. An explicit graph-in-Git policy is
also possible. See [storage](./storage.md).

Containers enter only when a task needs execution: package installation, shell,
builds, running tests, an agent requiring native file tools, or a deployment build.
Materialize a pinned virtual workspace into a fresh container, give the agent scoped
access, and reconcile outputs back. Storing generated source or tests alone needs
no container. See [execution](./cloud-sessions.md).

## Decisions carried forward

- jazyk.dev works without an account against a local server. Browser examples remain
  a secondary discovery path. Hosted work requires login and available credits.
- Local projects can subscribe through local-remote mode. Compilation moves to the
  service while source and durable graph stay local. This replaces the previous
  model-only subscription switch as the main upgrade path.
- The local folder is authoritative for source, graph, and human decisions in
  local-remote mode. The connector must be online to save results. A save completes
  only after its local commit is acknowledged. Moving storage is a separate action.
- Keep accountless local endpoints and external ACP agents. Do not silently replace
  LocalRouter, CLI/environment settings, or an external agent's provider.
- One shared server supports many projects. Each project's builds and automated
  sessions remain sequential; different projects progress concurrently.
- AWS is acceptable. A customer database tracks identity, authorization, projects,
  subscriptions, credits, job metadata, leases, and cloud state pointers.
- Use the existing Canadian Stripe account. Currency, province, sales territories,
  tax setup, final prices, and allowances remain open.
- Production inference may use OpenRouter, OpenAI, or Anthropic. Keep the supplier
  outside the compiler and workspace contracts.
- Use the owner's Codex subscription for private local testing through ACP. Hosted
  use needs a separately supported integration. See [Codex testing](./codex-testing.md).
- Keep compiler, frontend, web, and cloud specifications separate under `docs/`.
- Source/test generation and deployment execution are optional later milestones.
  They must not delay a useful compilation-only service.

## Read in this order

- [server.md](./server.md): project contexts, concurrency, persistence, and tenant isolation.
- [storage.md](./storage.md): virtual workspaces, local connector, commits, cloud, and GitHub.
- [protocols.md](./protocols.md): ACP, LSP, tool delivery, and remote connections.
- [workflows.md](./workflows.md): accountless use, subscription upgrade, and project selection.
- [codex-testing.md](./codex-testing.md): private experiments with the owner's subscription.
- [cloud-sessions.md](./cloud-sessions.md): optional materialized execution jobs.
- [models.md](./models.md): agents, model policies, production providers, and measurement.
- [platform.md](./platform.md): account database, Stripe, credits, and AWS integration.
- [boundaries.md](./boundaries.md): package and documentation ownership.
- [tiers.md](./tiers.md): accounting and historical inference-only economics.
- [sandboxes.md](./sandboxes.md): dated provider research for execution jobs only.
- [market.md](./market.md): dated Kiro and competitor research.

The [old prototype](./ux/index.html) remains a historical design artifact. It couples
compiler location to file location, which this revision explicitly separates. Use
the new [project panel](./workflows.md#project-panel) for the current UX proposal.

## Existing implementation and gaps

- [vfs.rs](../../bootstrap/src/vfs.rs) redirects shared compiler filesystem calls to
  native disk or browser memory. It uses compile-target selection and thread-local
  browser trees, not per-project injected storage instances.
- [llm.rs](../../bootstrap/src/llm.rs) keeps provider fallbacks, token counters, pacing,
  and overrides in process-wide state. Move these into run/provider contexts before
  unrelated customers share a process.
- [store.rs](../../bootstrap/src/store.rs) assumes filesystem locks and commits. Add
  atomic versioned commits over storage adapters while retaining gates, journals,
  graph identity, and recovery.
- Native trace and ACP session files still have direct disk paths. Audit all I/O,
  not only document reads. Never discover the current tenant from cwd.
- The [ACP spec](../../docs/frontends/acp.md#protocol-versions) prefers MCP file tools,
  but [host.rs](../../bootstrap/src/acp/host.rs) also implements v1 file callbacks.
  Route both through the project context and reconcile this specification gap.
- [LSP](../../docs/frontends/lsp.md) assumes local file URIs and output directories.
  Remote graph/artifact reads and client URI mapping need an explicit contract.
- [sys.rs](../../bootstrap/src/sys.rs) runs a shell natively. Hosted compilation must
  receive a no-execution capability even in a native binary.

## Delivery sequence

Estimates are engineering days for one maintainer, including docs-first changes and
focused tests. They replace the older 19–33 day model-gateway estimate. State isolation
and distributed commits are the difficult work, not adding an HTTP endpoint.

| Phase | Work | Exit condition | Estimate |
|---|---|---|---|
| 0. Contracts and Codex spike | Project contexts, workspace/state APIs, protocol mapping, private Codex constraints | An existing fixture resolves a goal through Codex; virtual-tool compatibility or the exact execution dependency is established | 3–6 days |
| 1. Portable compiler contexts | Instance VFS, run-local state, traces, cancellation, no-shell profile; local and memory adapters | Two projects with identical paths/ids compile concurrently without filesystem access or state mixing | 8–14 days |
| 2. Shared project service | Admission, writer epochs, account/project capabilities, events, quotas, persistence contract | Restart/retry and competing writers preserve one graph history; deterministic isolation tests pass | 8–14 days |
| 3. Local-remote connector | Pinned revisions, commit receipts, reconnect, ACP/LSP proxy and URI mapping | A local folder compiles remotely; edits are never overwritten; disconnect preserves graph state | 8–15 days |
| 4. Remote-remote and web | Object-backed source/state, accountless and signed-in project panels, remote protocol access | Cloud compilation needs no project directory or sandbox; exports preserve ids and decisions | 8–12 days |
| 5. Public paid service | Production provider, measured rates, Stripe, device auth, reservations, retention, operational hardening | Credits, cancellation, authorization, failures, restore, and cross-tenant tests pass | 12–20 days |
| 6. GitHub adapter | Tree/blob reads, private overlay, explicit publish/PR, conflicts and revocation | GitHub-backed compilation needs no checkout; graph restore and branch movement are explicit | 6–10 days |
| 7. Optional execution | Fresh containers, materialization, agent tools, evidence, diff import, limits and cleanup | Test jobs cannot access another project; stale output never overwrites newer source | 8–15 days |
| 8. Optional generation/deployment | Artifact generation, execution/verification, deployment handoff | Generated tests stay visibly unverified until run against the recorded revision | Rescope after phase 7 |

Phases 0–4 total roughly 35–61 days for the three-mode private foundation. A first
vertical result should arrive during phases 0–1. Public billing adds roughly 12–20
days before contingency, independent review, and elapsed provider/account setup.
GitHub and execution are separate increments. These ranges are not delivery dates.

Write accepted behavior into [the separated specs](./boundaries.md) before code.
This task plans the work. It does not run models, change credentials, provision AWS,
implement the server, or modify the production UI.

## Remaining decisions

- Decide whether GitHub is needed for the first external beta or phase 6.
- Choose a production inference route after benchmarks. Confirm Sign in with ChatGPT
  integration eligibility before relying on it for hosted access.
- Decide whether source/test generation ships before execution. Proposed default:
  compilation first, artifact-only generation next, isolated execution afterward.
- Set credit rates, compiler resource limits, storage retention, execution pricing,
  and currency from measurements.
- Select a sandbox provider when execution is in scope. No sandbox selection is
  required for a compilation-only beta.

The shared server, three storage/compiler modes, ACP/LSP, accountless local access,
and isolated execution when running code are settled by this direction.
