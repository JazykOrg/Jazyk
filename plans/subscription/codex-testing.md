# Private testing with the owner's Codex subscription

Researched 2026-10-02 using OpenAI Docs and the ACP adapter maintainers' sources.
Use the owner's subscription for real model output in local private experiments.
Keep the production provider decision open. This plan does not change credentials,
install an adapter, or start model runs.

## Verified boundaries

- Codex supports ChatGPT sign-in for subscription usage and API-key sign-in for
  usage-based API access. These are distinct billing paths. Source:
  [OpenAI authentication](https://learn.chatgpt.com/docs/auth).
- OpenAI's current app-server documentation allows existing local/open-source
  integrations to continue using its authentication, but explicitly excludes that
  authentication from commercial or hosted services. Do not assume an owner-only
  AWS deployment is an exception. Source:
  [app-server authentication](https://learn.chatgpt.com/docs/app-server#auth-endpoints).
- Sign in with ChatGPT is a separate integration for identity and eligible plan-backed
  inference. Confirm registration, eligibility, permissions, and applicable terms
  before depending on it for a hosted service. A normal Codex login is not that
  integration. Sources: [overview](https://developers.openai.com/siwc) and
  [app-server integration](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server).

This is a technical integration boundary, not a claim that Jazyk has obtained access
to a particular partner program. Do not pool or export the owner's Codex credentials
to service other customers.

## ACP adapter

The repository's current example names `@zed-industries/codex-acp`. Its maintainers
now direct new installs to `@agentclientprotocol/codex-acp`, built around Codex
app-server. Pin and test a release rather than launching an unpinned package on
every build. The current adapter advertises ChatGPT authentication and client-provided
MCP servers, and bundles a compatible Codex dependency. Test that pinned pair first;
the globally installed CLI version is only a separate local observation. Sources: [previous adapter notice](https://github.com/zed-industries/codex-acp)
and [current adapter](https://github.com/agentclientprotocol/codex-acp).

Use the maintained adapter before writing a custom Codex transport. Its official
protocol libraries own framing and session transport. Jazyk owns project tool mapping,
gates, and capability checks. Codex app-server is not itself an ACP endpoint.

The installed CLI reported `codex-cli 0.160.0` during this review. `codex app-server --help` confirms a stdio transport. This proves local availability only, not adapter
compatibility, successful login, model entitlement, or virtual-workspace support.

## First experiment

1. Add an explicit owner-only Codex ACP profile in test configuration. Use Codex's
   normal local login; leave its credentials under Codex control. Do not turn its
   token into a generic OpenAI-compatible API key or put it in a shared gateway.
2. Start the shared Jazyk server locally with two independent project contexts and
   logical in-memory stores. Use only the owner's fixtures and bounded work.
3. Verify that the adapter sees Jazyk's injected graph/workspace tools and that all
   project reads/writes resolve through those tools. Probe tool errors and gates.
4. Determine whether native file/shell tools can be disabled or redirected by the
   supported adapter. A textual request to avoid the filesystem is not proof.
5. Run `jazyk benchmark`, then one goal batch on `bootstrap/example/f1`, an incremental
   edit, and a concurrent second project. Record adapter/CLI/model versions and usage.
6. Exercise local-remote transport with separate local processes and an outbound
   connector over loopback. Exercise remote-storage semantics using a test object
   adapter or development storage, while the subscription-backed agent stays local.

If the adapter requires materialized files or a shell, record that limitation and
route it through the later execution interface for appropriate tests. Do not weaken
the filesystem-independent compilation gate or mislabel such a run as filesystem-free.
Use deterministic ACP fixtures for storage/concurrency tests that need no reasoning.
Choose a supported tools-only agent/provider for the strict no-materialization proof
if the Codex adapter cannot satisfy it.

Normal Codex state/auth files on the owner machine are separate from project storage.
They need not disappear to prove that the compiler has no project filesystem.
No unattended cloud-service topology is assumed to inherit local login permission.

## Development accounts and credits

Test the future account/project/credit database using explicit development identities
and nonmonetary grants. These exercise admission and reservations without Stripe
charges. Real Codex usage remains subject to the owner's subscription limits.

Show `Owner test: Codex subscription` and distinguish simulated Jazyk credits from
actual provider usage. Unknown provider cost stays unknown; token reports do not
become real settlement receipts. Do not infer production unit economics from a
flat personal subscription.

## Gate before hosting or public access

For a deployed service, use a commercially supported API route or a separately
eligible Sign in with ChatGPT integration. Do not assume that relaying the owner's
local Codex process makes a hosted subscription backend permitted.

Keep the ACP-facing compiler and workspace contracts unchanged when switching routes.
Rerun benchmarks with the production agent/model. Codex success does not establish
another model's extraction quality, tool discipline, cost, or generation behavior.
