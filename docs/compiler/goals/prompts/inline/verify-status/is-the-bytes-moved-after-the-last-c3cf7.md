---
workflow: >-
  A saved test result counts only while it matches the current requirement, test
  definition, and implementing files. These messages explain why a requirement is not
  currently verified and which binding, generation, or verification action can establish
  fresh evidence.
used_by:
  - "bootstrap/src/verify/status.rs"
---
`{}` is {} ({}): the bytes moved after the last verdict, so it judged something else; run again (run_tests or record_verdict rebaselines the hashes)