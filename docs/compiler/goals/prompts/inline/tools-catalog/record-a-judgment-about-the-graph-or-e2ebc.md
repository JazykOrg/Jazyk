---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Record a judgment about the graph or documents. Severity error only when two statements cannot both hold; warning for real but repairable issues; info for observations; none for a considered judgment recorded but not surfaced. prompt is optional and usually absent: attach one only when the diagnostic asks a person a real question, omit it entirely otherwise. When present, it carries up to 4 options, each a label with exactly one of edit (a suggested prose edit, applied without a model) or answer (a prefilled reply), plus freeform for typed replies. A decision (a choice the documents leave open) requires a prompt; nonconformant-instance is an instance whose values or links its type's statements rule out.