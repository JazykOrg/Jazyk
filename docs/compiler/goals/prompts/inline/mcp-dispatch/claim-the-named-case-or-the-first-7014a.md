---
purpose: >-
  Instruction fragment: Claim the named case or the first pending one, against a throwaway
  sandbox.
used_by:
  - "bootstrap/src/mcp/dispatch.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Claim the named case or the first pending one, against a throwaway sandbox. The reply carries the same instructions and package an in-process turn gets; do the work exactly as if it were real. One case open at a time.