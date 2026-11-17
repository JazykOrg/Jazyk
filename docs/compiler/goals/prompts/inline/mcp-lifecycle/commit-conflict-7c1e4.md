---
workflow: >-
  Several agents may work on the same goals at once; nothing claims work, and the first
  commit wins (docs/compiler/graph.md#commit-conflicts). This message answers `done` (or
  the end-of-input finish) when the batch's changeset collided with commits that landed
  after the batch opened. The batch stays open on the current generation with the
  conflicting mutations and resolutions dropped; `conflicts` beside the message lists
  each node, section, or goal with the generation that changed it.
used_by:
  - "bootstrap/src/mcp/lifecycle.rs"
---
Other work committed while this batch was open, and part of your changeset collides with it. Nothing was committed. The batch is still open and now reads the current graph. The conflicting mutations and goal resolutions listed in `conflicts` were dropped; the rest stay staged. Reread each conflicting node or section, restage what still applies to its current version, leave goals another commit resolved alone, and call `done` again.
