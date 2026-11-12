---
workflow: >-
  After tests have passed, source or test files can change. Jazyk audits saved results
  against the current file contents and stops counting results that no longer describe
  those contents. This message explains that loss of current evidence.
used_by:
  - "bootstrap/src/verify/audit.rs"
---
these rows' test or files bytes moved since their last run, so the verdict was dropped; `jazyk test` reruns them