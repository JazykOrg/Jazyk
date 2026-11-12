---
workflow: >-
  These are descriptions of tools available to the model while it reads and edits the
  project graph. The graph stores concepts mentioned in the docs, requirements stated
  about them, supporting evidence, diagrams, and unresolved findings. The description
  tells the model what an operation means and how to supply its arguments.
used_by:
  - "bootstrap/src/tools/catalog.rs"
---
Run recorded programmatic tests: the build once, then each referenced saved task once for the selected rows, importing separate exact JUnit case verdicts. Reuse ordinary project suite tasks across requirements and criteria. Each later call starts fresh task runs. requirements selects rows (requirement or entity ids); empty runs every non-verified row; all: true reruns every programmatic row, verified ones included. llm rows are skipped here; judge them and record_verdict.