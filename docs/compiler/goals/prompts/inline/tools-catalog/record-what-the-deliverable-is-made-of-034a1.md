---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Record what the deliverable is made of when no session has decided it yet (the package says medium: null with a mediumNote): form in your words (a Rust crate, a PowerPoint deck), produced written (the files are the deliverable) or built (they are the source a tool turns into artifact), toolchain, and artifact when built. An existing deliverable tree and recorded run commands pin the decision. Replaces an earlier decision until an entity is generated; refused after.