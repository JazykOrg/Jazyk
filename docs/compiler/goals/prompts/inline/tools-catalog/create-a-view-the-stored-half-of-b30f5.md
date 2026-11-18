---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Create a view (an authored diagram definition: intent, membership and presentation hints), or refresh it when a view of that kind and title exists. members are ordered node ids: entities for structural kinds (class, object, package, component, deployment; composite and state take exactly one), requirements for flow kinds (use-case, activity, sequence, communication, overview; order is the flow order), one entity then requirements for timing. query {scope?, parent?, stereotype?, depth?} adds membership by rule at every commit. collapse lists members (or their ancestors) shown as one node despite their children. excluded is [{id, note}]. purpose states the reader's question; area identifies the entity or scope and whether the diagram covers its area or a selection. reasoning records the evidence behind the authored choice. Optional relationship selection, labels, notes, presentation groups, direction, sourced sequence fragments, and diagram links make that intent explicit. Every referenced graph ID must exist.