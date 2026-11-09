# Inline editing: LSP, jazyk.dev, and chat edits

Status: decided and merged. This records what was decided and what remains.

## Decided and merged

- Chat edits carry [edit effects](../docs/frontends/acp.md#edit-effects): every
  mutating chat tool reply names the file hunks and graph nodes it changed. Every ACP
  surface (embedded agent, IDE proxy, GUI chat, browser tab) decorates the tool call
  with standard `diff` content, `locations`, and `_meta.jazyk.effects`. IDEs show
  native diffs of whole files; jazyk.dev shows one clickable line per change.
- The effects diff compares the graph before the commit with the store's
  in-memory graph after it, so a commit does not reload the store.
- jazyk.dev's gutter is line numbers and the change bar only. The coverage, build,
  and requirement lanes are removed, and no document view (Markdown,
  reStructuredText, AsciiDoc, PDF, DOCX) draws requirement marks beside the text:
  the hovering agent shows what a build reads in the text, and findings show as
  diagnostics.
- The change bar sits against the text, IDE style. Its baseline is the last build,
  or for a never-built document the text it opened with, moved to the saved text on
  each save. A click opens the change peek with `Revert`.
- Findings anchor where they are about
  ([LSP capabilities](../docs/frontends/lsp.md#capabilities)):
  - a section finding on the section's heading, or its first sentence without one;
  - an entity finding on the entity's defining mention only;
  - a requirement finding on its sentence.
- jazyk.dev underlines errors and warnings only; info findings are listed.
- One status vocabulary for requirements in every frontend:
  [status words](../docs/consumers/gen.md#status-words).
- The language server adds inlay hints (status words, none before a project ever
  generated), semantic tokens (entity mentions), document highlight, document and
  workspace symbols, and a "Changed since last build" lens. jazyk.dev draws only the
  requirement lens (in Debug); its change bar covers the rest.

## Not done

- A live chat turn with a tool-capable agent was never run end to end: local models
  looped instead of calling tools. The paid run (Claude Code over ACP) waits for
  the owner's approval.
