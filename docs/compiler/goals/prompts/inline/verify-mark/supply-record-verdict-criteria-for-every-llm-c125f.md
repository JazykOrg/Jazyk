---
workflow: >-
  For a requirement checked by AI inspection, the judging agent records its verdict
  against the exact acceptance conditions it inspected. These messages validate that
  report and keep it separate from results produced by running executable tests.
used_by:
  - "bootstrap/src/verify/mark.rs"
---
Supply record_verdict.criteria for every LLM criterion: {}. Copy ids and factHash values from begin_verification. Programmatic evidence is not a model verdict.