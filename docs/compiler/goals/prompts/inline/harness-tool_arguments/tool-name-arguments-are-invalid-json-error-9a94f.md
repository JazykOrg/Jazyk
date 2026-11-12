---
workflow: >-
  A model proposes tool arguments as JSON text. Jazyk must parse that text before it can
  call the tool. This feedback is returned when parsing fails, so the model can correct
  its request without assuming any tool action occurred.
used_by:
  - "bootstrap/src/harness/tool_arguments.rs"
---
Tool `{name}` arguments are invalid JSON: {error}. This call did not execute. Raw argument length: {characters} characters ({} bytes). Error excerpt (at most 128 characters, {omitted} omitted): {excerpt:?}. Provider finish_reason: {reason}. {limit} Correct the JSON escaping and propose the call again.