# Prompt guide

All authored model instructions live under this directory or `../../skills/`.
This includes short retry guidance, tool-result instructions, and framing labels.
Tool descriptions and schemas stay in the tool registry. Runtime observations,
user input, serialized graph data, and UI messages are data, not authored prompts.

## Pipeline

`bootstrap/prompt_parser.rs` is the common Markdown source parser. The build reads
all prompt and skill sources through it and embeds the prepared Markdown. It parses
YAML frontmatter, removes it from the prompt, strips HTML comments through CommonMark
events, and validates Mustache syntax. Fenced and inline code remain literal.
Markdown stays Markdown, never HTML. Runtime values are inserted once, after source
comments are removed, so quoted user data is never interpreted as template source.

A `.md` file is Markdown. A `.md.mustache` file also supports Mustache variables,
sections, comments (`{{! explanation }}`), and partials (`{{> path.md }}`). Partial
paths are relative to the including file. Includes must stay inside the payload
root, must exist, and must not form a cycle. Included files use the same parser.
Use triple braces for prose values so Markdown and JSON are not HTML-escaped.
Fragment bundles use the same Mustache rendering as whole files.
A fragment marker is structural and consumed before Markdown comments are removed.
It is never sent to the model. Do not use a bare single-word comment as rationale
in a legacy fragment bundle: its marker syntax is `<!-- name -->`.

Each payload explains its workflow and use site in frontmatter. Add a separate
purpose or rationale when it explains something beyond the message itself.
Fragment bundles carry the same context in source comments because their marker
format is parsed before Markdown metadata. Keep file-specific context in the file,
not in a separate catalog.

## Areas

- `session/`: reconciliation contract, feedback, gates, batch framing and protocols.
- `goals/`: one contract per model-executed goal kind.
- `generation/`: binding, generation, medium selection, tests, and manifests.
- `verification/`: verdict contracts and evidence framing.
- `conversation/`: conversational policy, context, answers, and history recovery.
- `runtime/`: embedded agent, continuation, and compaction.
- `decompile/`: reverse compilation and worker requests.
- `serving/`: MCP initialization and next-step guidance.
- `inline/`: extracted small instructions, grouped by owning source module.
- `../../skills/`: deferred workflow and domain knowledge, with skill metadata.

## Writing and maintenance

- Read this guide before creating or changing prompts.
- Use short paragraphs, descriptive headings, lists, and fenced examples.
- Keep one responsibility per file. Share stable instructions through partials
  when consumers have the same obligation. Keep role-specific differences explicit.
- Put the reason for a rule next to it in a Markdown or Mustache comment when
  the reason is known from the requirement, enforcement gate, or existing test.
  Cite that source in the comment. Do not invent incidents or historical motives.
- Keep comments in source, never in rendered instructions. Comments inside code
  examples are literal example content.
- Explain the surrounding workflow in plain English: who is doing what, which
  operation uses this text, and what happens next. Write for someone who has not
  read the implementation.
- Use `purpose` for the outcome or failure the text is meant to prevent. Do not
  copy or paraphrase the prompt body into that field. Omit it if you have no
  separate explanation to add.
- Explain the condition that selects corrective guidance. Distinguish a rejected
  call from an informational note returned after a successful operation.
- Define local terms and each template argument when their meaning is not obvious.
  For Rust format strings, describe positional arguments in call-site order; named
  arguments use their names. A list of code identifiers alone is not an explanation.
- Keep source paths as supporting references, not as a substitute for context.
  Keep known rationale next to the rule it explains. Trace it to code, a gate,
  documentation, or a test. Do not invent historical incidents.
- Remove comments that only repeat a heading, fragment name, or generic rendering
  mechanics. Useful context should explain this prompt, not just the template system.
- Use numeric prefixes only within an ordered workflow. Binding precedes generation;
  compaction requests precede continuation. Goal contracts and verifier alternatives
  have no universal sequence and keep descriptive names.
- Update the area guide when adding an area. Check every consumer, native and WASM.
- Exercise nested includes, metadata, comments, literal braces, and unescaped values.
  Run payload tests, engine tests, formatting, WASM checks, and a release build.
- Do not move instructions into Rust strings. Tool errors that teach a corrective
  action are prompts too. Plain factual errors and UI status text may stay in code.
