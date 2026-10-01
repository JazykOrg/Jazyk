# Research: sandboxes and how others isolate cloud agents

Researched 2026-10-01. Marks as in [market.md](./market.md): **[V]** vendor page read
today, **[S]** search results or third parties (several 2026 comparisons are written
by competing vendors), **[M]** memory, **[E]** estimate.

## How others isolate

| Product | Isolation | Egress | Credentials | Persistence and idle |
|---|---|---|---|---|
| Claude Code cloud | A VM per session, 4 vCPU, 16 GB, 30 GB [V] | A proxy for all traffic; levels None, Trusted (default: registries, GitHub, cloud SDKs), Full, Custom [V] | GitHub proxy swaps a scoped credential for the real token after checking the operation; an agent proxy adds API keys for listed hosts [V] | Setup cached as a filesystem snapshot for about 7 days; an idle VM pauses with its files after minutes and is later reclaimed, then rebuilt from the conversation [V] |
| OpenAI Codex cloud | A container per task (`codex-universal`) [V]; OpenAI is acquiring Ona as the future runtime [S] | Internet during setup; none during the agent phase by default, or a domain allowlist [V] | Secrets exist only during setup and are removed before the agent runs [V] | Containers cached up to 12 hours [V] |
| Cursor cloud agents | A Firecracker microVM per agent, in a separate AWS account [V] | On by default; allowlist modes [V] | Encrypted, injected as env vars, stripped from transcripts [V] | Encrypted VM snapshots, deleted after 90 days idle [V] |
| Google Jules | A short-lived GCP VM per task [S] | n/a | n/a | Snapshot after the setup script [S] |
| GitHub Copilot agent | An ephemeral Actions runner VM [S] | Firewall: registries plus allowlist [S] | Actions secrets [M] | The branch and PR [S] |
| Devin | A VM per session; self-hosted "Outposts" [S] | n/a | n/a | Machine snapshots [M] |
| Replit | Own fleet; copy-on-write block storage, git commit per checkpoint, forkable Postgres [S] | n/a | n/a | Constant-time snapshots [S] |
| Lovable | Modal sandboxes (gVisor), 20k concurrent at peak [S] | n/a | n/a | n/a |
| Bolt | WebContainers in the browser [S]: no native toolchains, so not an option for jazyk | n/a | n/a | n/a |
| Vercel v0 | Vercel Sandbox, Firecracker [S] | n/a | n/a | n/a |
| Manus | An E2B sandbox per task [V] | n/a | n/a | n/a |

What they share: a VM boundary per tenant; egress through a proxy outside the
sandbox, registries allowed by default; long-lived credentials never inside; setup
snapshotted once; idle VMs paused, then reclaimed, with git and the conversation
enough to rebuild.

Sources: [Claude Code on the web](https://code.claude.com/docs/en/claude-code-on-the-web),
[cloud environments](https://code.claude.com/docs/en/cloud-environments),
[Claude Code sandboxing](https://anthropic.com/engineering/claude-code-sandboxing),
[Codex cloud environment](https://learn.chatgpt.com/docs/environments/cloud-environment),
[Cursor security](https://prod.cursor.com/docs/cloud-agent/security),
[Jules](https://jules.google/docs/environment/),
[Copilot firewall](https://docs.github.com/en/copilot/how-tos/use-copilot-agents/coding-agent/customize-the-agent-firewall),
[Devin Outposts](https://docs.devin.ai/cloud/outposts/overview),
[Lovable on Modal](https://modal.com/blog/lovable-case-study),
[Manus on E2B](https://e2b.dev/customers/how-manus-uses-e2b-to-provide-agents-with-virtual-computers).

## Sandbox providers

| Provider | Isolation | vCPU-hour | GB-hour | Pause and snapshot | Max lifetime | EU | Self-host |
|---|---|---|---|---|---|---|---|
| E2B | Firecracker | $0.0504 [V] | $0.0162 [V] | Memory and filesystem pause, kept indefinitely, resume about 1 s [V] | 1 h Hobby, 24 h Pro ($150/month), reset on resume [V] | reported [S]; US company | open-source infra (Terraform, Nomad, Firecracker); BYOC on Enterprise [V] |
| Northflank | Kata or gVisor [S] | $0.0167 [V] | $0.0083 [V] | Scale to zero with volumes kept; no memory snapshot [S] | none [S] | EU West [V] | BYOC on AWS, GCP, Azure at no extra cost [V] |
| Fly Machines | Firecracker | about $0.043/h for 2 vCPU 4 GB [V] | included | Suspend with memory up to 4 GB, not durable [V] | none | yes | no |
| Daytona | Sysbox, Kata optional [S] | $0.0504 [V] | $0.0162 [V] | Stop and start keep files [S] | n/a | n/a | core closed since June 2026 [V] |
| Modal | gVisor | about $0.071 [V] | about $0.024 [V] | Filesystem and memory snapshots [S] | n/a | regional multipliers [V] | no |
| Vercel Sandbox | Firecracker [S] | $0.128, active CPU only [S] | $0.0212 [S] | Filesystem snapshots, 30 days [S] | 24 h Pro [S] | yes | no |
| Cloudflare Containers | VM-based [M] | $0.072, active only [V] | $0.009 [V] | Sleep; disk is ephemeral [M] | n/a | global | no |
| Blaxel | microVM | about $0.041 per GB-hour, CPU included [V] | included | Resume in about 25 ms with memory; standby free [V] | indefinite standby [S] | US, EU [S] | no |
| Morph Cloud | VM snapshots | $0.05 per 1 vCPU + 4 GB hour [S] | included | Branch a running VM in under 250 ms [S] | n/a | n/a | no |
| Runloop, CodeSandbox, Beam | microVM or gVisor | $0.07 to $0.11 [S] | | Snapshots [S] | | | |

Most offer TypeScript and Python SDKs plus a REST API; the orchestrator calls the REST
API from Rust.

Sources: [E2B pricing](https://e2b.dev/pricing),
[E2B persistence](https://docs.e2b.dev/sandbox/persistence),
[E2B internet access](https://e2b.dev/docs/sandbox/internet-access),
[Northflank pricing](https://northflank.com/pricing),
[Fly pricing](https://docs.fly.io/about/pricing/),
[Fly suspend](https://fly.io/docs/reference/suspend-resume/),
[Daytona](https://github.com/daytonaio/daytona),
[Modal pricing](https://modal.com/pricing),
[Vercel Sandbox](https://vercel.com/docs/sandbox/pricing),
[Cloudflare Containers](https://developers.cloudflare.com/containers/pricing/),
[Blaxel](https://blaxel.ai/pricing).

## Self-hosting

- Firecracker: about 125 ms boot, about 5 MB per VM, the strongest isolation per unit
  of cost. Needs KVM, so bare metal: Hetzner dedicated servers have it, Hetzner Cloud
  VMs do not [M].
- gVisor: runs without KVM, larger attack surface than a VM.
- Kata: OCI containers in a VM, the natural fit for Kubernetes.
- Building blocks: `kubernetes-sigs/agent-sandbox` (CRDs over gVisor or Kata, v1.0,
  GKE GA in May 2026) [S]; `e2b-dev/infra` (Apache-2.0, GCP first-class, bare metal
  planned) [S]; `microsandbox` (Rust, libkrun, single server) [S].
- Hetzner raised prices in April and June 2026 [V]: AX42 €97.30 a month (8 cores,
  64 GB), AX102 €257.30 (16 cores, 128 GB).
- The burden: kernel and Firecracker patching, images, snapshot storage and GC,
  capacity and failover, the egress proxy, abuse complaints that Hetzner forwards and
  can answer by null-routing. One to two weeks to build, a few hours a week after.

Source: [Hetzner price adjustment](https://docs.hetzner.com/general/infrastructure-and-availability/price-adjustment/).

## Cost math

Assumption [E]: each active user runs 20 hours a month of a 2 vCPU, 4 GB sandbox:
40 vCPU-hours and 80 GB-hours.

| Option | Per user | 100 users | 1,000 users |
|---|---|---|---|
| E2B, plus $150 Pro | $3.31 | $481 | $3,462 |
| Northflank, plus 10 GB volume | $2.83 | $283 | $2,833 |
| Fly, EU, plus 10 GB volume | $2.45 | $245 | $2,450 |
| Daytona | $3.70 | $370 | $3,700 |
| Modal, EU | $5.95 | $595 | $5,950 |
| Hetzner self-host (AX102, 2x CPU overcommit) | n/a | 2 servers for redundancy, about €520 | 6 to 7 servers for about 150 at peak, about €1,750, plus storage and ops time |

- At 100 users, managed wins: self-hosting costs the same before counting the owner's
  time.
- At 1,000 users, self-hosting saves $1k to $2k a month at real operational risk.
  Moving from E2B to `e2b-infra` on bare metal keeps the same API.
- Model tokens dominate: a frontier model at 20 agent-hours a month is $50 to $300 per
  user, the sandbox $1 to $6. Build the gateway and the quotas before optimizing the
  sandbox provider.

## Recommendation

- E2B for the beta: Firecracker, memory pause kept indefinitely, domain-level egress
  rules, a REST API, and an open-source exit to self-hosting. Confirm the EU region
  and the data processing terms before signing up.
- Northflank as the fallback: 2.5 times cheaper per hour, EU West, free BYOC; the
  egress proxy and the pause are jazyk's to build.
- Avoid WebContainers (no native toolchains), Daytona for self-hosting (closed), and
  Modal (highest price, gVisor only).
- Behind a `Sandbox` interface in the orchestrator, so the switch to Northflank or to
  self-hosting is one module.
