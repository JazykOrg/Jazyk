---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Record LLM judgments with evidence. Copy factHash from begin_verification. For multiple LLM criteria, criteria must include every LLM criterionTests entry with its own criterion id, factHash, verdict, and evidence. The top-level verdict is pass only when every LLM entry passes. Programmatic evidence comes from run_tests.