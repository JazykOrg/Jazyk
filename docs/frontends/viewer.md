# Viewer

`jazyk viewer [--out FILE]` renders the [graph store](../compiler/graph.md) into one
self-contained HTML file, by default `<out>/graph.html`. No server, no external assets.
The file works offline and can be attached to a review or a ticket as-is.

The viewer reads the same shards every frontend reads. See
[storage layout](../compiler/graph.md#storage-layout). It renders what is on disk; it
never compiles.

## What it shows

- A header with the build stats: entity, requirement, and relationship counts, open
  [diagnostics](../compiler/model/diagnostic.md) by severity, and the coverage fraction.
- Entities: id, `name`, `scope` when not `public`, `definition`, `aliases`, mentions
  (document, section, and the located `quote`), and the requirements referencing the
  entity.
- Requirements: id, the `ears` statement, the entities it references, the `source`
  quote, and its `edges` when declared.
- Relationships: id, `type`, members, and the contributing requirement ids. Derived
  nodes, shown as stored. See [derived data](../compiler/graph.md#derived-data).
- Diagnostics: id, `rule`, a severity chip, `lifecycle`, subjects, `message`, and
  `reasoning`.
- Coverage: one row per document with covered, non-normative, and unprocessed section
  counts.

## Verification overlay

When the [ledger](../consumers/gen.md#the-ledger) exists, the viewer overlays
verification state, derived at render time exactly as
[`verify_pending`](../compiler/tools.md#verification-tools) derives it:

- The header gains a verification summary: verified, failing, stale, and unverified
  counts, plus not-generated requirements.
- Each requirement card carries a status chip (`verified`, `failing`,
  `stale-requirement`, `stale-test`, `stale-code`, `unverified`, `missing`) with the
  test kind, the recorded run command, and the last evidence line.
- Each entity card aggregates its requirements: all verified reads green, any failing
  reads red, any stale reads amber, none generated reads gray.

The overlay is read-only and deterministic. Rerunning `jazyk test` and re-rendering the
viewer is the whole refresh loop.

## Navigation

- One text filter narrows every card at once. Matching is case-insensitive over ids,
  names, and text.
- Every node id links to its card. Clicking an id anywhere jumps to it.
- Severity chips color-code diagnostics: red for `error`, amber for `warning`, blue for
  `info`, grey for `none`.
