# Inline editing: LSP, jazyk.dev, and chat edits

Status: proposal plus proof of concept on branch `poc/inline-edits` (both repositories).
Not merged.

## Review: what each surface gives today

| Capability | LSP (VS Code, IntelliJ via LSP4IJ, Zed) | jazyk.dev editor |
|---|---|---|
| Open diagnostics inline | yes, standard squiggles | yes (same LSP ranges, CodeMirror lint) |
| Broken document links | yes | yes |
| Hover cards (entity, requirement, code, test) | yes, markdown + diagram image | yes, same markdown rendered by `renderMd` |
| Definition, references, type definition | yes | Cmd-click, Cmd-Shift-click |
| Completion of entity names | yes | yes |
| Prompt options as code actions | yes (quick fixes) | inline band with buttons and a freeform box |
| Requirement code lens | yes (`req:… · status`) | lens in Debug only; margin marker otherwise |
| Entity mentions | document links (underline) | underline computed client side, Cmd-click opens |
| Requirement margin marker | no | `jz-gutter-req`: blue extracted, green verified, red failing, amber stale, amber ring asked |
| Coverage strip | no | `jz-gutter-cov`: accent covered, grey non-normative, amber unprocessed |
| Build activity strip | no (log channel only) | `jz-gutter-bld`: grey queued, pulsing running, green read, amber not read |
| Change marks | no (the IDE's git gutter covers HEAD) | `jz-gutter-chg` against the last-built text only |
| Peek diff of a change | IDE's VCS | none; a whole-document side by side toggle |
| External change flash | the IDE reloads the buffer | yes, fading line tint |
| Caret context to inspector | no | yes |
| Outline / document symbols | no | yes (app outline) |

### UX problems found

- Four unlabeled strips sit left of the line numbers, each 3 to 8 px wide. Colors
  collide: amber means "unprocessed" (coverage), "not read yet" (build), "stale"
  (requirement), and "question open" (requirement). Green means "covered", "read",
  and "verified". There is no legend; the only explanation is a `title` tooltip on
  a 4 px target.
- Change marks compare against the last-built text (`GET /api/docs/baseline`) and
  nothing else. A document that never built shows none, so typing in it shows
  nothing. A built document does show marks, but they mean "differs from the last
  build", not "you changed this", and they look like the coverage strip beside them.
- Marks are not interactive. VS Code and IntelliJ both open an inline peek on a
  click of the change bar: the old lines, a Revert, next/previous change. The
  jazyk.dev diff is a whole-document side by side toggle.
- IDE convention puts the change bar immediately left of the text (after line
  numbers in VS Code, inside the line-number gutter in IntelliJ), with blue
  modified, green added, a red wedge for deleted. jazyk.dev puts it second of four
  strips, left of the line numbers.
- Chat: a dual write is one collapsed row ("Revise requirement …") with raw JSON in
  Technical details. `editor.md` promises "a diff card with a jump to the sentence";
  it is not implemented. `acp.md` promises "a diff on the tool call" through the
  proxy; `delegate_edit` only issues `fs/write_text_file` and attaches no diff.
  Graph changes (a new requirement, a changed edge, a new link) are invisible
  except as JSON.

## Proposal A: shared by LSP and jazyk.dev

Everything here is standard LSP, so VS Code, IntelliJ (LSP4IJ), and Zed get it, and
jazyk.dev consumes the same server replies.

1. Inlay hints (`textDocument/inlayHint`) at the end of each located requirement
   quote: a short status word (`verified`, `failing`, `stale`, `question`). Replaces
   the colored margin marker as the portable form; the lens stays for the id.
2. Semantic tokens (`textDocument/semanticTokens/full`) for entity mentions
   (`type` token with a `jazyk.entity` modifier) instead of document links over
   every mention, so themes color them and links stay for real links.
3. Document highlight (`textDocument/documentHighlight`): the caret inside a quote
   highlights the whole quote and the other quotes of the same requirement
   (jazyk.dev's caret highlight, made portable).
4. Document symbols (`textDocument/documentSymbol`): the section tree, each section
   with its requirement count and coverage as `detail`. IDE outline and breadcrumbs.
5. Workspace symbols (`workspace/symbol`): entities and requirements by name.
6. "Changed since last build" as a code lens on each dirty section heading
   (`Changed since build · 3 lines`), its command opening a diff of the section
   against the last-built text (`jazyk.diffSinceBuild`, answered with
   `window/showDocument` on a virtual `jazyk-baseline:` URI the extensions register
   as a read-only content provider; base clients still get the lens). The IDE's
   own git gutter stays the "changed vs HEAD" signal.
7. One vocabulary for statuses in every surface, with one color each: `verified`
   green, `failing` red, `stale` amber, `question` violet (moved off amber),
   `extracted` neutral blue. Coverage and build activity stop reusing them.

## Proposal B: jazyk.dev only (no LSP equivalent)

LSP has no gutter API, no inline widgets with inputs, no peek views of arbitrary
content, and no client-side animation. These stay jazyk.dev's.

1. Gutter, reordered to IDE convention and reduced to two lanes:
   - Right of the line numbers, against the text: the change bar. Blue modified,
     green added, red wedge deleted. Baseline: the last-built text; a document
     that never built uses the text it opened with (or its last save), so typing
     always marks. Hover says which baseline. Click opens an inline peek: the old
     lines, word-level highlights, `Revert`, previous/next change, Esc closes.
   - Left of the line numbers: one compiler lane. Requirement marker per sentence
     (status color as above); coverage collapses into it: unprocessed lines get a
     hollow amber outline, non-normative lines nothing, covered lines nothing. A
     running build pulses the lane over the batch's sections.
   - A `?` at the top of the gutter (and the status bar) opens a legend.
2. Question and proposal bands with option buttons and freeform input (exists).
3. External change flash (exists), now with "show change" opening the same peek.
4. Live build bars, the hovering agent, follow-me.
5. Chat edit cards (Proposal C's custom rendering).

## Proposal C: agent edits in chat, ACP first

One contract, rendered at two fidelities.

- Engine: every mutating chat tool reply carries `effects`:
  - `files`: `{path, abs, kind (doc|config|source|test), line, oldText, newText}`
    per changed hunk (only changed lines, so the model's context stays small).
  - `graph`: `{op (added|updated|removed), kind, id, title, node?, changes?}`.
    `changes` is per field `{field, before, after}`. Derived relationships and
    state machines are included, so a changed edge also shows the link it moved.
    Derived default views are left out (noise).
- ACP (any client): a tool call update whose output carries `effects` is
  decorated with standard fields, `kind: edit`, `content` with one `diff` block
  per file hunk and one markdown text block listing graph changes, `locations` with
  each file and line (clients use them for "follow the agent" and click-through),
  and `_meta.jazyk.effects` holding the structured record. The embedded agent, the
  native GUI chat, the browser tab, and the IDE proxy (for external agents' MCP
  calls) apply the same decoration, so Zed, JetBrains, and VS Code ACP clients show
  native diffs and a readable graph summary.
- jazyk.dev reads `_meta.jazyk.effects` and renders:
  - file hunks as inline diff cards (word-level highlights), the path clickable to
    open the document at the line, a `Show in editor` that reveals the sentence;
  - a new requirement as a requirement bubble (statement, entities as chips, the
    quote with its document link, facets);
  - an updated node as field rows `before → after` (statement word diff, entity
    chips added/removed, edges as arrows);
  - relationship changes as link chips `Order → Payment · dependency` (added green,
    removed struck);
  - a removed node struck through with its reason;
  - every node title clickable to open it in the inspector.
  A client that ignores `_meta` (any IDE) still gets the diff blocks and summary.

## Proof of concept scope

- Engine: `effects` on dual writes, `edit_fact` decrees, and `write_document`;
  `bootstrap/src/effects.rs` (graph diff, hunks, ACP decoration); decoration in the
  embedded agent, native GUI chat, and browser tab. Unit tests.
- jazyk.dev: change bar against the last build or the opened text, click-to-peek
  with Revert, gutter legend; chat effect cards (file diff, requirement bubble,
  field changes, link chips) with click-through.
- Not in the PoC: Proposal A's new LSP methods, the proxy decoration of external
  agents' calls, the lane merge of coverage into the requirement lane.
