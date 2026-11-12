---
purpose: >-
  Instruction fragment: Record what YOUR run billed for the open batch, when you know it:
  tokens is the input plus output tokens your endpoint billed across every round of this
  batch, cached the input tokens among them served from its prompt cache.
used_by:
  - "bootstrap/src/tools/catalog.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Record what YOUR run billed for the open batch, when you know it: tokens is the input plus output tokens your endpoint billed across every round of this batch, cached the input tokens among them served from its prompt cache. Your own figure, never an estimate; do not call it if you do not know your usage. A second call replaces the first (the batch's whole spend so far, not an increment). At done the batch's journal entry takes the figure as tokens and cachedTokens with tokensSource agent. Stages nothing and touches no graph state. Outside an open batch it is refused; `jazyk spend <generation> <tokens>` stamps a committed entry instead.