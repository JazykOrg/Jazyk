# Cloud sessions

A cloud session is one project's sandbox running jazyk. The page at jazyk.dev connects
to it as it connects to a local `jazyk gui` today. In it the graph is compiled, the
product generated and built, its tests run, and a web product previewed.

The research behind this design is in [sandboxes.md](./sandboxes.md).

## What the wasm demo cannot do

The demo runs the store, the reconciler, and the tools in a tab, but
[wasm.md](../../docs/frontends/wasm.md) lists what a tab does not do: no `gen`, `bind`,
`verify`, or `decompile` (there is no shell), no LSP, no ACP, no rendering, no
benchmark. A cloud session is the full binary on a real machine, so all of it works.

## Threat model

Treat the whole sandbox as hostile, the jazyk binary in it included. The model writes
code and the harness runs it; a generated build script, a dependency's install hook,
or a prompt injection in a fetched page can do anything the VM allows.

So nothing valuable lives in the sandbox:

- No provider keys. The model is reached through the gateway with a short-lived token
  scoped to one project and one cloud session.
- No long-lived git token. Git goes through the credential proxy.
- No other tenant's data. One microVM per project, never shared.
- Usage is metered at the gateway, never reported by the sandbox.

## Architecture

```
jazyk.dev (page) ──HTTPS/SSE/WS──▶ api.jazyk.dev (auth, relay)
                                        │ per-session token
                                        ▼
                       ┌──────── microVM per project ────────┐
                       │ jazyk gui --cloud                     │
                       │ /workspace: docs, deliverable, .jazyk │
                       │ toolchains: base image + setup        │
                       └──────┬──────────────┬────────────────┘
                              │              │
                   egress proxy (allowlist)  gateway (models)
                              │
                      git proxy (injects token)
```

- The page gains a fourth backend beside `server`, `demo`, and `wasm`: `cloud`. The
  capabilities descriptor already describes a backend in one place
  ([gui.md](../../docs/frontends/gui.md)), so a cloud backend reports `jobs` with
  `gen`, `verify`, `audit`, `persistence: disk`, and `model: configured`.
- The relay authenticates the user, checks they own the project, resumes the sandbox
  if paused, and proxies `/api/*`, the events stream, and `/lsp` to it. The page never
  learns the sandbox's address.
- In the sandbox, `jazyk gui --cloud` takes its session token from the environment
  instead of minting one, admits only the relay, and skips opening a browser. The
  rest of the server is unchanged: one server, one project.

## Isolation

- A Firecracker microVM per project, from a managed provider (E2B first, behind a
  `Sandbox` interface: create, resume, pause, exec, expose a port, snapshot). Every
  major cloud agent uses a VM boundary per tenant: Claude Code, Cursor, Copilot,
  Jules, Devin, Vercel, Manus ([sandboxes.md](./sandboxes.md#how-others-isolate)).
- Resource limits per tier: vCPU, memory, disk, and a maximum running time per cloud
  session (24 hours, then a forced pause).
- Inside the VM, jazyk adds its own limits to every command it runs (phase 2, useful
  locally too):
  - a timeout on `run_command`, `run_build`, and the test runners, with the process
    group killed on expiry (the docs already promise a timeout the code lacks);
  - output caps (the tool already truncates; the build and the runners do not);
  - an environment scrubbed of the gateway token before a command runs, so a
    generated script cannot read it from `env`. The token sits in a file the command
    user cannot read, with jazyk and the commands running as different users.

## Network egress

Default deny. All traffic leaves through a proxy outside the VM, filtered by host:

- Always reachable: the gateway, the git proxy, and package registries (npm, PyPI,
  crates.io and its index and static hosts, proxy.golang.org, RubyGems, Docker Hub,
  GHCR, the base image's OS repositories). Claude's "Trusted" list is the template.
- A per-project allowlist the user edits (Pro and up).
- Full internet for Max and Ultra, and only after the account is a week old with a
  paid invoice.
- SMTP ports blocked always, everywhere.

## Credentials

- Models: the gateway token, minted per cloud session, expiring with it.
- Git: the sandbox's git remote points at the git proxy. The proxy holds the GitHub
  App installation token, checks the operation (push only to the project's branches),
  and swaps in the real token on the way out, as Claude Code's GitHub proxy does.
- The product's own secrets (a database password for the generated app, a test API
  key): stored encrypted per project, injected only into the build and test commands
  that name them, never into the model's tools. Codex removes secrets before its agent
  runs; jazyk can do better because the harness, not the model, runs the build.

## Persistence

The graph lives in `.jazyk/`, which is gitignored, so git alone loses it. A lost graph
is recoverable (the next build reconciles from empty) but costs the user's quota.

- Pause: when no page is connected, no build lease is held, and no job runs for 15
  minutes, the orchestrator pauses the VM with memory and filesystem (E2B keeps a
  paused sandbox indefinitely; resume takes about a second). A running dev server or
  database comes back as it was.
- Snapshot: at every committed generation (the journal's generations are natural
  checkpoints) and on pause, the workspace (`docs`, the deliverable, `.jazyk/`, minus
  build caches) is written as a tarball to object storage. A reclaimed or lost VM is
  rebuilt from the latest snapshot.
- Environment cache: the base image plus the project's setup command (`[cloud]
  setup` in `jazyk.toml`, run once) are snapshotted and reused for a week, as Claude
  Code and Jules do.
- Git, optional: the docs and the deliverable push to the user's GitHub through the
  proxy. The docs are the source code, so this is the user's real backup.
- Local and cloud stay one project: `jazyk pull` and `jazyk push` move the workspace
  snapshot between a local checkout and the cloud.

## Lifecycle

1. Create: from an example, an empty project, a GitHub repository, or an upload.
2. Resume: the page opens the project; the relay resumes or rebuilds the VM.
3. Work: compile, generate, test, chat, exactly as locally. Compute units count
   running time, at the sandbox size's multiple.
4. Unattended builds: a build started in `auto` mode keeps running when the page
   closes, until it converges, pauses on quota, or hits the running-time cap. The
   page shows it on return; a notification is a later addition.
5. Pause on idle, as above. Paused time is free; snapshots count against storage.
6. Delete: the VM, the snapshots, and the secrets go.

## The product

- Artifacts: what the build `produces` is downloadable from the Product view, and a
  zip of the deliverable is always available.
- Previews: a web product's dev server port is exposed through the preview proxy at
  `<project>-<port>.jazyk.app`, on a separate registrable domain so the app cannot
  touch jazyk.dev's storage. A preview is private to the owner by default, with a
  share link as an option; it lives while the sandbox runs.
- Databases: the generated app's PostgreSQL runs inside the same VM, so it pauses and
  resumes with it. The ERP example is the test case.
- Deployment to the user's own infrastructure stays as
  [product-and-deployment.md](../product-and-deployment.md) proposes: jazyk generates
  the deploy command and records what ran, it never holds cloud credentials.

## Abuse

From strongest to weakest, as CI providers learned with miners:

1. Default-deny egress: a miner that cannot reach a pool is useless; spam and scanning
   die with it.
2. A verified card for any compute, the trial included. Never free compute without
   one.
3. Hard caps: one running project on Pro, a monthly compute cap, a running-time cap,
   cgroup CPU limits.
4. Signals: sustained full CPU with no gateway calls (a jazyk build waits on the model
   most of the time), long-lived outbound connections, known pool hosts.
5. Full internet only after account age and a paid invoice.

## Changes to jazyk

- `jazyk gui --cloud`: token from the environment, relay as the admitted origin, no
  browser. Pages: [gui.md](../../docs/frontends/gui.md), [cli.md](../../docs/frontends/cli.md).
- Command limits: timeouts, process-group kill, output caps, scrubbed environment,
  separate user. Pages: [gen.md](../../docs/consumers/gen.md),
  [generate.md](../../docs/compiler/goals/generate.md).
- `[cloud]` in `jazyk.toml`: `setup` (run once per environment), `allow` (extra egress
  hosts), `secrets` (names only, the values live in the platform). Page:
  [project-settings.md](../../docs/compiler/project-settings.md).
- `jazyk login`, `jazyk usage`, `jazyk pull`, `jazyk push`. Page:
  [cli.md](../../docs/frontends/cli.md).
- The page's `cloud` backend and project list. Pages:
  [gui.md](../../docs/frontends/gui.md), a new `docs/frontends/cloud.md` describing a
  cloud session's behavior (not prices).
- Nothing in the reconciler, the store, or the board changes: one server, one project,
  sequential builds, goals derived from disk. That design is what makes pause, resume,
  and quota stops free.
