# Current state and replacement boundary

Status: repository inspection for the [discussion draft](README.md). Engine base:
`b093014`. Cloud checkout inspected at
`05de0e08bc62c99808f75ef9f2a115a95057573c`. No runtime experiment was performed.

## Engine

- [Generation](../../docs/consumers/gen.md#the-build) already records one build per
  deliverable, including `run`, `cwd`, and `produces`. A built medium is not complete
  until its expected artifact exists. This applies to books and presentations too.
- Binding establishes tests before implementation generation. The ledger retains
  requirements, criteria, test artifacts, statement hashes, implementation hashes,
  test names, commands, and verdicts. Generation must reuse that toolchain.
- [Verification](../../docs/consumers/gen.md#runners) distinguishes `programmatic`
  and `llm`. Native verification builds once, then executes selected commands or
  asks a model to judge files. A command that could not run is unverified.
- `bootstrap/src/tools.rs` provides `run_command`, returning an output tail, plus
  `run_tests`, `begin_verification`, and `record_verdict`. Native shell execution
  reaches `bootstrap/src/sys.rs`. A working-directory restriction is not an OS
  security boundary against a shell command.
- [Hosted workspaces](../../docs/compiler/hosted-runtime.md#execution-profile)
  deliberately deny subprocess execution. Generation edits the virtual filesystem.
  Programmatic verification returns `pendingCi`. LLM verification can use existing
  files. `gen.rs` and `verify.rs` contain corresponding hosted bypasses.
- The full workspace snapshot supports binary bytes. Its delivery projection is
  UTF-8 only and excludes built products and some compiler metadata. Passing a PDF
  through the current Git delivery projection will not solve product retention.

Keep the ledger, ownership gates, criterion identities, statement freshness, build
failure feedback, and sequential compiler sessions. Replace the command transport
and add durable runtime evidence. Do not invent a separate set of tests in delivery
settings.

## Cloud and editor

The private repository's
[delivery contract](https://github.com/JazykOrg/jazyk-cloud/blob/05de0e08bc62c99808f75ef9f2a115a95057573c/docs/delivery.md)
has named `[deliveries.*]` with `kind = "git"` or `kind = "script"`. A script has
an inline command, JUnit report globs, and criterion mappings. Both kinds require a
repository binding. Script delivery publishes a managed workflow and dispatches
GitHub Actions at an immutable candidate.

The implementation already protects remote heads, preserves unowned remote files,
records idempotent attempts, reconciles uncertain dispatches, and imports
revision-bound JUnit evidence. Preserve those properties for the GitHub shortcut
and any retained Actions adapter.

The private editor contains `CloudDeliveries.tsx`, `cloud-deliveries.ts`, and
`cloud-delivery-actions.ts`. Its current form combines repository binding,
delivery definitions, reports, criterion mappings, and historical attempts. This
is the form to simplify. Extend the existing Board, Tabs, Files, chat, and account
usage views. Do not create another application.

The [hosted service](https://github.com/JazykOrg/jazyk-cloud/blob/05de0e08bc62c99808f75ef9f2a115a95057573c/docs/runtime.md)
stores versioned workspace snapshots in PostgreSQL, with project writer leases and
revision fencing. Its compiler worker slots are distinct from the future sandbox
capacity. A remote command must not consume an HTTP request or database transaction
for its entire duration.

## Billing

The [current accounting contract](https://github.com/JazykOrg/jazyk-cloud/blob/05de0e08bc62c99808f75ef9f2a115a95057573c/docs/billing.md)
already uses integer supplier cost, customer accounting units, immutable rate
snapshots, reservations, settlements, and monthly plus rolling five-hour and
seven-day limits. Paid use is cost-based. Trials additionally have token limits.

The user-facing meters show percentages, with pending usage separately. The
runtime proposal extends this system with another priced resource. It does not
replace paid accounting with token subtraction or create an unrelated allowance.

## GitHub sign-in is not repository authorization

The [GitHub contract](https://github.com/JazykOrg/jazyk-cloud/blob/05de0e08bc62c99808f75ef9f2a115a95057573c/docs/github.md)
separates sign-in from GitHub App repository access. A project needs a permitted
installation, selected repository, and branch. Existing bindings are account-scoped
and versioned. Every use rechecks access. The new shortcut must preserve this.

## Proposed changes

| Current behavior | Proposed behavior |
| --- | --- |
| Script delivery requires GitHub and a generated workflow. | Script delivery runs in the selected sandbox without a GitHub dependency. |
| Hosted commands are unavailable. | Hosted commands dispatch through a capability supplied by the service. |
| Hosted programmatic tests wait for delivery CI. | They execute directly in the same runtime used during generation. |
| Commands return a short tail. | They also expose durable command IDs, paginated logs, search, and receipts. |
| Built binaries remain inside a full snapshot or CI archive. | Products have manifests, durable binary objects, and visible workspace paths. |
| Delivery settings include criterion mappings. | Verification owns those bindings. Delivery may invoke verification. |
| One GitHub-only delivery panel. | Small delivery definitions plus an integrated Runs view. |

Old Actions attempts stay readable as historical attempts. Removing a delivery
does not delete remote workflows, repositories, retained products, or history.
Conversion of an old inline command into a script is an explicit settings change.
Do not silently execute old workflows through a new provider.
