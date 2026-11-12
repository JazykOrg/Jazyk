---
purpose: >-
  Instruction fragment: Tool `{name}` arguments are invalid JSON: {error}.
used_by:
  - "bootstrap/src/harness/tool_arguments.rs"
context: "Prepared before the consumer inserts runtime values. Keep format slots aligned with that consumer."
---
Tool `{name}` arguments are invalid JSON: {error}. This call did not execute. Raw argument length: {characters} characters ({} bytes). Error excerpt (at most 128 characters, {omitted} omitted): {excerpt:?}. Provider finish_reason: {reason}. {limit} Correct the JSON escaping and propose the call again.