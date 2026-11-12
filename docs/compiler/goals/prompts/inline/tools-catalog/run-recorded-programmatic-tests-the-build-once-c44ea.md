---
purpose: >-
  Instruction fragment: Run recorded programmatic tests: the build once, then each
  referenced saved task once for the selected rows, importing separate exact JUnit case
  verdicts.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Run recorded programmatic tests: the build once, then each referenced saved task once for the selected rows, importing separate exact JUnit case verdicts. Reuse ordinary project suite tasks across requirements and criteria. Each later call starts fresh task runs. requirements selects rows (requirement or entity ids); empty runs every non-verified row; all: true reruns every programmatic row, verified ones included. llm rows are skipped here; judge them and record_verdict.