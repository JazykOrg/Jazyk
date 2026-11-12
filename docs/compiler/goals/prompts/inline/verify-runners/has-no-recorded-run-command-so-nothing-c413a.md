---
workflow: >-
  Jazyk needs a recorded command or saved task to execute a test. This message tells the
  agent how to supply that connection when a test has been declared but the runtime does
  not know how to run it.
used_by:
  - "bootstrap/src/verify/runners.rs"
---
`{}` has no recorded run command, so nothing can execute it; define a saved suite task and record its test declaration with record_binding, or record_generation for entity `{}`