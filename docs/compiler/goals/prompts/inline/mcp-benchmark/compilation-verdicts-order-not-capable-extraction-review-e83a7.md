---
purpose: >-
  Instruction fragment: compilation verdicts order not-capable < extraction < review.
used_by:
  - "bootstrap/src/mcp/benchmark.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
compilation verdicts order not-capable < extraction < review; review is the highest (extraction plus review judgment). generation and verification are capable or not-capable. unmeasured means the tier was never attempted.