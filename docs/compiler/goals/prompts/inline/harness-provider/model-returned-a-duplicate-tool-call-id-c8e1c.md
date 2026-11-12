---
workflow: >-
  Jazyk receives model responses that may contain requests to call tools. Before acting,
  it validates that response. These messages describe a malformed model response rather
  than the outcome of a tool that actually ran.
used_by:
  - "bootstrap/src/harness/provider.rs"
---
Model returned a duplicate tool-call id in one response; no tools executed