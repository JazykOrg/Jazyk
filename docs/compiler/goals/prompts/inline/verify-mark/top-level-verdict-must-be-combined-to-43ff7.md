---
workflow: >-
  For a requirement checked by AI inspection, the judging agent records its verdict
  against the exact acceptance conditions it inspected. These messages validate that
  report and keep it separate from results produced by running executable tests.
used_by:
  - "bootstrap/src/verify/mark.rs"
---
Top-level verdict must be `{combined}` to match the LLM criteria.