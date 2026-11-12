---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Create a view (the stored half of a diagram: what it includes, never how it looks), or refresh it when a view of that kind and title exists. members are ordered node ids: entities for structural kinds (class, object, package, component, deployment; composite and state take exactly one), requirements for flow kinds (use-case, activity, sequence, communication, overview; order is the flow order), one entity then requirements for timing. query {scope?, parent?, stereotype?, depth?} adds membership by rule at every commit. collapse lists members (or their ancestors) shown as one node despite their children. excluded is [{id, note}]. reasoning says why the view exists; every id must exist.