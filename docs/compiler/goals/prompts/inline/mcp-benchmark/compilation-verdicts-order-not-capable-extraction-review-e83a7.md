---
workflow: >-
  An external coding agent can run the same model evaluations through the Jazyk tool
  server. These messages explain which benchmark case is active and what must happen
  before the benchmark report can be closed.
used_by:
  - "bootstrap/src/mcp/benchmark.rs"
---
compilation verdicts order not-capable < extraction < review; review is the highest (extraction plus review judgment). generation and verification are capable or not-capable. unmeasured means the tier was never attempted.