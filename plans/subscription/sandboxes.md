# Global sandbox research

Reviewed 2026-10-01 from the primary sources linked below. Prices are USD before
tax. Published features are not measured Jazyk results. There is no EU-only filter
and no assumption that Canadian billing requires Canadian compute.

Scope update, 2026-10-02: these providers are candidates for optional execution
jobs. The shared compiler does not need a project container. Prices and vendor facts
below retain their 2026-10-01 research date and must be refreshed before procurement.
See [execution lifecycle](./cloud-sessions.md) and [shared server](./server.md).

## Workload and selection criteria

Execution jobs need a Linux environment with native toolchains, shell, optional
agent runtime, and bounded logs/preview access. A trial should materialize a virtual
workspace, build Rust/Node projects, and run the ERP fixture with a database. The
shared compiler and its ACP/LSP service stay outside the execution container.

Priorities, in order:

1. Isolation and network policy outside the untrusted runtime.
2. Correct materialization, output/evidence export, and cleanup without losing virtual source state.
3. Working toolchains, process control, streams, previews, and database behavior.
4. Total monthly cost at small scale, including idle/wait time and fixed fees.
5. Regions and measured latency to AWS storage/control and model providers.
6. Operational work, support, limits, and ability to leave the provider.

A VM is a strong candidate boundary. gVisor is also a deliberate isolation mechanism;
ordinary shared-host containers are not equivalent. Do not claim every cloud agent
uses Firecracker or that one runtime name alone proves security.

## Provider comparison

| Provider | Isolation and geography | Persistence/lifecycle | Fit and work |
|---|---|---|---|
| E2B | Firecracker-based managed sandboxes; enterprise material lists US, EU, APAC. Confirm the exact region available on the purchased plan. | Filesystem and memory pause; docs say paused state has no automatic expiry. Runtime limits depend on plan. | Strong general-purpose starting point. Custom templates and native tools. A fixed Pro fee matters in a small beta. |
| Vercel Sandbox | Firecracker; published regions include North America, Europe, Asia, Australia, South America, Africa, and Montréal. | Persistent sandboxes restore filesystem snapshots; restart application processes. Persistent drives are also offered. | Strong alternative for LLM-wait-heavy workloads. Native network rules and credential brokering reduce integration work. Verify custom image/toolchain behavior. |
| Daytona | Default container runtime and explicit Linux VM/Windows options; shared `us` and `eu` regions. | VM pause retains memory; stopped/paused disk can remain billed. Container archive differs. | Serious alternative, not excluded by an old Sysbox-only description. Test the exact runtime class, region quota, and image restrictions. |
| Modal | gVisor and explicit VM runtimes; selectable regions across clouds. | Filesystem/memory snapshot capabilities and volumes; semantics depend on runtime. | Credible, especially for full Linux/Docker needs. Old claims that Modal is gVisor-only are obsolete. Region premiums and physical-core billing matter. |
| Northflank | Managed sandbox runtimes, including Kata microVMs; US, Europe, Asia. BYOC runtime selection is explicit. | Persistent volumes and service lifecycle; verify pause behavior for the selected offering. | Low listed compute cost. More platform/network/storage integration to validate than a simple code sandbox API. |
| Fly Machines | Firecracker VMs with programmatic machine lifecycle. | Local volumes and suspend/resume; do not use suspend as the only backup. | Useful if owning the runtime lifecycle is acceptable. Compare CPU class and regional capacity, not a shared-CPU headline against dedicated vCPU prices. |
| AWS ECS/Fargate | Each task has a hardware virtualization boundary; regional AWS service availability. | Design for task restart and restore from durable storage. | Keeps procurement on AWS. More orchestration/preview/egress work; restrictions on privileged execution can limit toolchains. |
| Blaxel | Per-sandbox microVMs; confirm available region and capacity before selecting. | Standby preserves state; snapshots and persistent volumes have separate charges. | Worth revisiting for execution jobs with model waits. Verify suspend behavior and billing for that workload. |
| Cloudflare Containers | Managed container platform integrated with Workers. | Treat runtime disk/lifecycle as provider-specific; require independent checkpoints. | An alternative if the web stack moves toward Workers. Additional integration and full-toolchain validation make it a later candidate here. |

Primary references for the table:

- E2B: [enterprise infrastructure](https://e2b.dev/enterprise),
  [persistence](https://docs.e2b.dev/sandbox/persistence),
  [network controls](https://docs.e2b.dev/network/internet-access),
  [external secret injection](https://docs.e2b.dev/secrets).
- Vercel: [sandbox](https://vercel.com/sandbox),
  [regions](https://vercel.com/docs/sandbox/concepts/regions),
  [persistence](https://vercel.com/docs/sandbox/concepts/persistent-sandboxes).
- Daytona: [runtime types](https://www.daytona.io/docs/en/sandboxes/),
  [isolation](https://www.daytona.io/docs/en/isolation/),
  [regions](https://www.daytona.io/docs/en/regions/),
  [billing by lifecycle state](https://www.daytona.io/docs/en/billing/).
- Modal: [runtimes](https://modal.com/docs/guide/sandboxes),
  [networking/isolation](https://modal.com/docs/guide/sandbox-networking),
  [regions](https://modal.com/docs/guide/region-selection).
- Northflank: [managed sandbox use](https://northflank.com/docs/v1/application/use-cases/run-code-in-sandboxes),
  [BYOC isolation](https://northflank.com/docs/v1/application/sandboxes/sandboxes-in-your-own-cloud),
  [pricing and locations](https://northflank.com/pricing).
- Fly: [Machines](https://docs.fly.io/machines/),
  [suspend](https://docs.fly.io/reference/suspend-resume/),
  [volumes and pricing](https://docs.fly.io/about/pricing/).
- AWS: [Fargate security and restrictions](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/fargate-security-considerations.html).
- Blaxel: [runtime and persistence prices](https://blaxel.ai/pricing).
- Cloudflare: [container lifecycle](https://developers.cloudflare.com/containers/concepts/architecture/).

This is a shortlist for Jazyk, not an exhaustive vendor ranking. Self-hosting, other
managed devbox suppliers, and BYOC remain alternatives if these fail the workload.
No unsupported self-host licensing claims are used to eliminate a managed provider.

E2B's network documentation limits domain filtering to HTTP/TLS on ports 80/443;
other ports use IP rules. It also documents shared-endpoint caveats and beta request
transforms. Include those details in the trial rather than treating a hostname
allowlist as complete isolation. Source: [network controls](https://docs.e2b.dev/network/internet-access).

## Cost comparison

Published unit rates, then calculated examples. Match region and resource class
before using any figure for procurement. Do not include promotional free credits in
recurring economics.

| Provider | Relevant published rates | Calculated 2 vCPU / 4 GiB running hour |
|---|---|---:|
| [E2B](https://e2b.dev/pricing) | $0.0504/vCPU-hour + $0.0162/GiB-hour; Pro $150/month separately | $0.1656 |
| [Daytona](https://www.daytona.io/pricing) | Same CPU/RAM rates; disk $0.000108/GiB-hour, with a stated free allowance | $0.1656 plus disk |
| [Vercel, iad1](https://vercel.com/docs/sandbox/pricing) | $0.128/active CPU-hour + $0.0212/GB-hour provisioned memory | $0.0848 + $0.256 × CPU utilization |
| [Modal](https://modal.com/pricing) | $0.00003942/physical-core-second, one core described as 2 vCPU equivalent; $0.00000667/GiB-second | $0.23796 at one requested/used core, before regional premiums |
| [Northflank](https://northflank.com/pricing) | $0.01667/vCPU-hour + $0.00833/GB-hour; volume $0.15/GB-month | About $0.06666 plus volume |
| [Fargate, us-east-1 Linux x86](https://aws.amazon.com/fargate/pricing/) | $0.000011244/vCPU-second + $0.000001235/GB-second | $0.0987408 before storage/network additions |
| [Blaxel](https://blaxel.ai/pricing) | $0.0000115/GB RAM-second active runtime; CPU scales with memory | $0.1656 for 4 GB; CPU allocation is not a directly equivalent guarantee |

Modal bills the higher of requested/used resources. The table is a reservation-based
example, not evidence of equal build throughput across providers. Vercel's active
CPU meter is different from a provisioned vCPU meter.

Assume 20 running hours per account per month:

| Scenario | Variable compute per account | 100 accounts | 1,000 accounts |
|---|---:|---:|---:|
| E2B | $3.31 | $481.20 including $150 base | $3,462 including $150 base |
| Daytona, before disk | $3.31 | $331.20 | $3,312 |
| Vercel, 10% CPU utilization | $2.21 | $220.80 plus plan charges | $2,208 plus plan charges |
| Vercel, 100% CPU utilization | $6.82 | $681.60 plus plan charges | $6,816 plus plan charges |
| Modal, above requested resources | $4.76 | $475.92 | $4,759.20 |
| Northflank, before volume | $1.33 | $133.32 | $1,333.20 |
| Fargate, above region/architecture | $1.97 | $197.48 | $1,974.82 |

These are deliberately compute comparisons, not total invoices:

- E2B Pro currently lists 24-hour sessions and 100 concurrent sandboxes. Higher
  concurrency changes fees. Ten accounts carry $15/account of its base fee; 100
  carry $1.50. Peak concurrency is independent of monthly hours.
- Ignoring fixed fees, Vercel's example becomes cheaper than E2B below about 31.6%
  CPU utilization. Measure execution jobs' model-wait/build mix before choosing. Regional
  rates, storage, and traffic can reverse that result.
- Daytona disk can accrue while stopped/paused. At the published disk rate, 10 GiB
  reserved for 730 hours is about $0.79 before any free allowance. Do not assume
  the allowance applies separately to every project.
- Northflank/Fly 10 GB volumes at the listed $0.15/GB-month add $1.50/month even if
  compute barely runs. Fly snapshots are a separate line item.
- Add provider snapshot/image storage, S3 recovery objects, requests, model traffic,
  preview/download egress, cross-region transfer, build cache storage, and retained
  projects. Provider-supported pause is not a backup retention policy.
- A 15-minute idle tail on each of 20 visits adds 5 running hours, 25% above this
  example. Idle behavior can matter more than headline CPU price.

## Recommendation

When execution is in scope, trial E2B and Vercel against the same acceptance list. E2B is the default integration
candidate for a general Linux workspace with memory pause. Vercel is the strongest
cost/geography challenger here, including a documented Canadian location and low
active-CPU charges while waiting. Restarting dev processes after filesystem restore
may suffice for execution workflows. Fresh short-lived jobs need no memory preservation.

Bring Daytona's explicit Linux VM runtime into the trial if fixed fees, capacity,
or compatibility make the first two unattractive. Modal and Northflank remain
credible alternatives rather than being excluded on outdated isolation assumptions.
Keep the trusted control plane on AWS whichever sandbox wins.

Use a small internal adapter for create/inspect/stop/delete, exec, file transfer,
network policy, preview access, and checkpoint operations. Report capabilities such
as memory pause and durable volumes explicitly. Switching provider also requires
export/restore, image, network, and cost validation; it is not just replacing a module.
Use the provider's maintained SDK or supported API client, not custom transports.

## Selection gate

Allocate 3–5 days within the optional execution milestone for comparison, plus bounded provider
usage once funded. Record provider, plan, region, image, date, and observed cost.

- Materialize a pinned virtual workspace, start Rust/Node toolchains, a web server,
  and the ERP database. Exercise build/test/setup commands and required Docker support.
- Measure cold start, resume, build throughput, idle CPU, model-wait CPU, streaming
  latency, and SSE/WS reconnects. Compare p50 and p95, not advertised startup alone.
- Kill a job during output collection, then retry/import from its recorded base.
  The canonical graph remains outside the container. Verify evidence, conditional
  commits, duplicate receipts, and conflicts with a newer workspace revision.
- Enforce egress outside the guest. Test direct-IP/IPv6 bypass, DNS changes, metadata
  access, credential scoping, and cross-project access. Confirm package registries
  still work under the chosen rules.
- Disconnect the page, exhaust model/compute budget, lose the orchestrator, and reach
  the provider lifetime limit. Work remains recoverable and compute eventually stops.
- Check raw preview URLs cannot bypass authentication. Verify downloads cannot read
  arbitrary platform objects.
- Reconcile observed resource use against provider billing, including disk while
  paused, snapshot retention, and transfer. Confirm account limits for beta size.

Exit: documented pass/fail results, a selected region/plan, realistic invoice
estimates at 10/100/1,000 accounts, and a tested exit path. Procurement remains open
until then; the research alone does not establish a universal best provider.

## Self-hosting

Defer a custom Firecracker/Kata fleet. AWS, other global cloud/bare-metal suppliers,
and managed BYOC are all candidates; this is not a Hetzner-only decision. Hardware
virtualization support depends on the actual instance type and platform.

E2B also advertises an Apache-2.0 single-node Embed package requiring Linux x86-64
with KVM. This is a possible prototype/exit route to evaluate, not proof that a
redundant production fleet is already operated for Jazyk.
Source: [E2B deployment options](https://e2b.dev/enterprise).

The comparison must include spare capacity, failure recovery, images, guest/host
patches, observability, network enforcement, abuse handling, backups, and operator
time. Do not promise a production multi-tenant fleet in one or two weeks or trigger
migration at an arbitrary $2,000 bill. Revisit when measured savings pay for the
engineering and ongoing operations, with an acceptable recovery story.
