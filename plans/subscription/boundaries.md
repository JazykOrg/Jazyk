# Compiler, server, web, and cloud boundaries

Revised 2026-10-02. The compiler may become a separate open-source project. Its
project/storage/runtime contracts must remain useful without an account, Stripe,
AWS, or any particular inference provider.

## Documentation ownership

| Tree | Owns | Excludes |
|---|---|---|
| `docs/compiler/` | Graph, goals, sessions, project contexts, storage transactions, scheduling, generic provider policies and limits | Account tiers, Stripe, vendor credentials |
| `docs/consumers/` | Artifact generation, verification evidence, execution requirements | Sandbox vendor billing |
| `docs/frontends/` | CLI, portable server API, ACP, LSP, MCP, logical paths and capability contracts | Hosted commercial policy |
| `docs/web/` | Accountless/signed-in navigation, files/compiler/AI selection, save and usage states | Compiler semantics and monetary enforcement |
| `docs/cloud/` | Tenants, accounts, credits, managed providers, durable cloud adapters, GitHub integration, execution broker, operations | Duplicated compiler goal logic |
| `plans/subscription/` | Alternatives, proposed architecture, estimates and dated research | Claims of implemented behavior |

Specify these additions before code:

- `docs/compiler/workspaces.md`: per-instance logical files, source revisions, state
  commits, authority epochs, change feeds and local/remote consistency.
- `docs/frontends/server.md`: multi-project service, job/events API, transport binding,
  no-execution profile, proxy connections and authentication hooks.
- Existing graph/control/session/settings pages: injected context, per-project writer,
  per-run state, provider selection, traces and resource stops.
- Existing ACP/LSP pages: storage-independent tools, URI mapping, virtual artifacts,
  dirty-buffer semantics and agent compatibility. Reconcile v1 file callbacks with
  the documented MCP-first direction.
- `docs/web/web.md`, `projects.md`, `accounts.md`: common entry, three mode transitions,
  credits and accountless behavior. Move page-only material from the current GUI spec.
- `docs/cloud/cloud.md`, `accounts.md`, `billing.md`, `storage.md`, `github.md`,
  `models.md`, `execution.md`, `operations.md`: managed adapters and commercial service.
- Consumer/tool specs: distinguish artifact-only generation, execution required,
  executed verification, stale evidence, and optional deployment handoff.

Keep relative links and link new roots from `docs/main.md` when they exist. Preserve
prompt/skill exclusions. A compiler-only dogfood configuration may test extraction
boundaries; do not implicitly split or reset the existing persistent dogfood graph.

## Packages and dependencies

- Expose a portable compiler/project-runtime library from `bootstrap/` first. It owns
  reconciliation and logical state commits through interfaces, not concrete cloud SDKs.
- Keep the `jazyk` binary and local server/connector entry points around that library.
  The native local and memory adapters need no cloud identity or payment dependency.
- A separate `cloud/` host depends on the public compiler/service interfaces and
  implements authorization, SQL/object storage, credits, provider routing, GitHub,
  and optional container execution. It hosts many project contexts in one service.
- Reuse the same project runtime in local and hosted servers. Do not shell out to one
  full Jazyk process/container per customer merely to avoid fixing global state.
- Use mature crates for HTTP, streaming, databases, object storage, OAuth, filesystem
  confinement, archives and standard protocols. Hand-write Jazyk domain logic only.
- Keep `bootstrap/app/` in place initially. A later move to `web/` can extract the
  web product without delaying the server contract. Its backend interface targets
  project capabilities, not a particular sandbox provider.

This reverses the earlier rule that cloud must only launch a project binary inside
a sandbox. The cloud host may call the compiler's public library interface; it must
not duplicate internal graph or goal semantics. Exact crate splitting is an
implementation choice after mapping dependency cycles and current feature gates.

## Runtime boundaries

Compilation has workspace, graph, model-session and artifact capabilities. It has
no implicit shell or process-launch capability. A separate execution interface can
materialize a workspace into an isolated container for tasks that require one.

The local server may offer native execution under local user policy. The hosted
server never grants that host privilege to a customer project. Do not use a native
build target as evidence that shell access is available to a hosted request.

A local Codex adapter is for the owner's private experiments. Do not import personal
Codex authentication into cloud credential management. Production model/provider
integration remains outside the compiler. See [Codex testing](./codex-testing.md).

## Compatibility

Preserve local YAML graph serialization, immutable ids, provenance, gates, journal,
and one-writer semantics. Cloud manifests may store those serialized values as blobs.
A physical project directory is one adapter, not an assumption of every graph tool.

The compiler's no-migration policy remains. Check compatibility before restore and
retain the pre-upgrade checkpoint. Cloud account/database schemas use normal SQL
migrations; that policy does not require discarding customer accounts.

## Acceptance

- The portable compiler builds and compiles through local or memory storage without
  cloud credentials, a customer database, or a project shell.
- Accountless local CLI, GUI, ACP, and LSP remain usable during cloud outages.
- A shared native server compiles multiple project contexts concurrently with no
  ambient cwd, filesystem, provider state, or token counter selecting a tenant.
- Cloud providers and payment systems can be changed without rewriting graph tools.
- Execution containers are replaceable adapters and never own the canonical graph.
- Accepted behavior is documented before code. Local links/anchors resolve and docs
  retain the repository's short declarative style with no em dashes.
