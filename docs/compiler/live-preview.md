# Live compilation preview

Compilation keeps one atomic changeset per session. Accepted tool calls can expose
a provisional display of authored graph nodes before that changeset commits.
The preview is not a graph read, a persisted mutation, or a successful build.

- `graphPreview` trace events carry a session `label`, a monotonic `revision`, the
  base `generation`, and a cumulative snapshot of entities, requirements, and
  criteria. The snapshot includes accepted staged writes. Derived relationships,
  diagrams, checks, and verification remain committed projections.
- Preview construction reads the session snapshot and staged operations. It does
  not call the store commit path, change identities, run gates again, or write
  model decisions. A preview cannot grant a write capability.
- A session starts with its committed base. Accepted graph writes replace the
  cumulative preview. Unsupported compound mutations and oversized snapshots
  clear the preview rather than presenting an incomplete graph as current.
- A committed session publishes the store's actual authored nodes. Failed,
  abandoned, and cancelled attempts discard their staged display. A consumer
  clears provisional state on a project switch, stream reset, and job completion.
- A hosted adapter may persist only after the entire job. Internal session commits
  remain provisional to its clients until the adapter publishes the durable
  generation and terminal job event.
- The native ACP bridge exchanges preview snapshots through a bounded sidecar
  under the set's `derived/preview/` directory. Its filename is derived
  from the session identity. Starting an attempt removes an old sidecar; finishing
  removes it. It is neither authored source nor an exported project artifact.
  Browser and hosted runners read the same preview directly in process.
- Preview events are cumulative and ordered within the job. Reconnecting clients
  replay only the current running job and reject older revisions. A missing or
  truncated preview disables the provisional display until a fresh snapshot.
- Tool trace events carry a short `activity` derived from structured arguments
  and accepted results. E.g. `Person created` requires a successful result with
  `created: true`. Rejected calls never receive successful wording. Raw tool names,
  arguments, and results remain available for inspection.

The [session trace](./sessions.md#trace-events) transports the preview. The
[graph store](./graph.md) remains the authority for durable state.
