# Execution providers and cost research

Status: research checked 2026-10-14 against official sources. Prices are indicative
USD supplier costs, not Jazyk prices. No provider has been provisioned or tested.
The owner favors managed isolation and low operational effort.

## Recommendation

Run a small adapter experiment with Modal and E2B. Use AWS Fargate as the cost and
infrastructure-control baseline. Prefer a managed agent sandbox for the initial
interactive loop if its time limits, isolation, and cost controls pass the experiment.
Do not choose solely on the lowest advertised CPU rate.

Start the experiment with Modal because it offers a no-fixed-fee entry plan and
custom-image sandbox APIs. Treat E2B as an equally serious candidate, particularly
for explicit renewable timeouts. This is a test order, not a final provider choice.
If strict funded lease behavior cannot be established cheaply on Modal, E2B may
be the better initial fit despite its production plan fee.

## Shortlist

| Option | Fit for Jazyk | Main issue to prove |
| --- | --- | --- |
| Modal Sandboxes | Exec/file APIs, configurable resources, registry images, managed isolation. | Strict resource bounds, renewal/orphan exposure, image compatibility, actual sandbox billing. |
| E2B | Agent-oriented Linux microVMs, lifecycle/timeout APIs, commands and files. | Template/image preparation, fixed plan cost, storage and transfer costs. |
| AWS ECS Fargate | OCI task execution, per-task isolation, AWS storage/network integration. | Interactive command transport, startup latency, enforceable lifetime and extra infrastructure. |
| GitHub Actions | Existing adapter and mature batch execution. | Repository/workflow dependence and poor fit for an interactive repair loop. |
| Dedicated worker VM | Predictable fixed bill, local adapter reuse. | Patching, isolation, capacity, cleanup, and operations become our responsibility. |
| Existing API server | Lowest immediate extra infrastructure. | Shared blast radius and competition with the API; not recommended for customer execution. |

## Managed isolation and capabilities

Modal documents gVisor and VM sandbox runtimes. Its network controls include
outbound restrictions, with a caveat that DNS is unrestricted except under a full
network block. Specify the chosen runtime explicitly and test egress controls.
[Modal networking and security](https://modal.com/docs/guide/sandbox-networking).
The September 28 SDK release introduced the explicit VM option; validate maturity
and required plan features rather than assuming every example is supported.
[SDK release notes](https://modal.com/docs/sdk/js/releases).

Modal supports registry-based images and recommends preparing reusable images
separately. Its documented default sandbox lifetime is five minutes, configurable
up to 24 hours. Verify runtime extension semantics during the experiment; creation
timeout support alone does not establish a renewable funded lease.
[Sandbox guide](https://modal.com/docs/guide/sandboxes).
CPU and memory requests can be exceeded unless limits are set. Configure both
requests and caps and compare actual billed usage.
[Resource controls](https://modal.com/docs/guide/resources).

E2B states that each sandbox uses a Firecracker microVM with its own kernel.
[E2B isolation](https://e2b.dev/security).
Its lifecycle API supports changing the timeout of a running sandbox, with the
new duration measured from the update. Hobby and Pro have different maximum
continuous lifetimes. Explicitly select kill-on-expiry for disposable execution;
automatic pause/resume is a different retention and billing decision.
[Lifecycle](https://docs.e2b.dev/sandbox).
Its template preparation is a separate build step that must be included in startup,
cost, and arbitrary-image compatibility tests.
[Templates](https://docs.e2b.dev/template/quickstart).

AWS states that Fargate tasks have hardware-virtualized isolation and do not share
their kernel or task resources with other tasks. Network and credential policy
remain our responsibility.
[AWS responsibility model](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/security-shared-model.html).
ECS Exec provides shell access, but has agent/filesystem requirements and a fixed
20-minute idle session timeout. It is not the complete Jazyk command protocol.
[ECS Exec](https://docs.aws.amazon.com/AmazonECS/latest/developerguide/ecs-exec.html).
An AWS adapter would need a reliable command/log transport and separate cleanup
control. Use SDKs and maintained process/transport libraries, not a new remote-shell
protocol implemented from scratch.

## Comparable indicative compute costs

Assume Linux, approximately 2 vCPU and 4 GiB RAM, allocated/used continuously for
the stated duration. Modal prices physical cores, with one physical core described
as two vCPU-equivalent. This is a billing comparison, not a performance benchmark.

| Provider | CPU and memory rate used | One hour | Ten minutes | 1,000 ten-minute runs |
| --- | --- | ---: | ---: | ---: |
| Fargate x86, US East N. Virginia | 2 × $0.000011244/vCPU-s + 4 × $0.000001235/GiB-s | $0.09874 | $0.01646 | $16.46 |
| E2B | 2 × $0.000014/vCPU-s + 4 × $0.0000045/GiB-s | $0.16560 | $0.02760 | $27.60 |
| Modal Sandbox | 1 × $0.00003942/core-s + 4 × $0.00000667/GiB-s | $0.23796 | $0.03966 | $39.66 |

Fargate rates above come from its current N. Virginia worked example. Linux has a
one-minute minimum and billing begins with image download. Extra storage, network,
public IPv4, and logging can add cost. [AWS pricing](https://aws.amazon.com/fargate/pricing/).

E2B lists Hobby without a fixed fee and Pro at $150/month plus usage; custom resource
configurations and longer runs depend on plan. Promotional credits are excluded.
[E2B pricing](https://e2b.dev/pricing).

Modal lists distinct Sandbox rates, higher than its ordinary CPU Function rates.
Its entry plan has no fixed monthly fee; Team is $250/month plus compute. Region
selection and other features can change the effective bill. Promotional credits
are excluded. [Modal pricing](https://modal.com/pricing).

These totals exclude startup, idle time, image preparation, persistent storage,
transfer, subscriptions, and Jazyk margin. At low volume, E2B Pro's fixed fee could
matter more than the compute difference. At high volume, slower builds, cache
transfer, or extra idle minutes can erase a cheaper nominal rate.

Runtime is often inexpensive per short run, but cannot be assumed to be a fixed
fraction of LLM spend. A long build with a small model may cost more than inference.
Measure the actual project mix before setting customer multipliers.

## Other options

Ordinary Lambda functions have a 900-second timeout. AWS now documents a longer
limit for specific Managed Instances invocation modes, so the universal statement
that every Lambda execution is limited to 15 minutes is too broad. Neither is the
first fit for reusable arbitrary-image command sessions.
[AWS timeout documentation](https://docs.aws.amazon.com/lambda/latest/dg/configuration-timeout.html).

A dedicated disposable worker VM is a reasonable later optimization at sustained
utilization. Estimate break-even using worker cost, occupied hours, utilization,
management time, storage, and incident cost. Rootless containers and gVisor are
defenses to evaluate, not a promise of perfect isolation.
[Rootless Docker](https://docs.docker.com/engine/security/rootless/),
[gVisor design](https://gvisor.dev/docs/architecture_guide/intro/).

The existing server could support an explicitly trusted operator-only experiment.
Even trusted source can install compromised dependencies. Keep customer data and
service credentials off the worker and prefer a separate disposable host. This is
not the preferred launch path given the owner's managed-runtime choice.

GitHub Actions can remain an optional external batch adapter, especially for native
platforms or customer-owned CI. It should not be mandatory for generating a PDF or
running a single local-style unit test. Avoid nested privileged Docker merely to
support container image builds; select a provider-supported isolated builder.

## Provider selection experiment

Use identical fixtures on each candidate and the local adapter:

- Node tests with one failing case and JUnit output.
- A Java or Rust dependency/build cache with warm and cold runs.
- A PDF or PPTX generator with a binary retained product.
- A 30-minute command, timeout extension, cancellation, and background child process.
- A large but bounded log stream and a partial upload interrupted by shutdown.
- A custom public image plus a deliberately unsupported image requirement.

Measure p50/p95 time to first command for warm and cold images, image-preparation
latency, file transfer, repeat-command latency, complete billed lifetime, cache hit
savings, and shutdown confirmation. Use benign policy checks to verify forbidden
mounts/network destinations are denied. Arrange provider-approved testing for any
intrusive security work; do not attempt to prove isolation by attacking a service.

Acceptance requires tenant separation, scoped credentials, hard resource limits,
bounded orphan exposure, reliable stop/status, binary transfer, logs, and evidence
freshness. Test provider outages and rate limits. If any are unavailable, document
the compensating design and cost before choosing.

Use a maintained official SDK where possible. If Rust support is incomplete, a
small provider bridge using the official SDK is preferable to reverse-engineering
private APIs. Keep that adapter outside compiler semantics and pin its version.
SDK presence, region/data location, terms for hosted customer workloads, retention,
support, and invoice reconciliation remain provider-selection checks.
