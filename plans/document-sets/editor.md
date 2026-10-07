# Document sets: editor experience

Status: proposed. These flows extend the existing jazyk.dev editor. Its canonical
presentation specifications and implementation belong in `jazyk-cloud`, as detailed
in [implementation](./implementation.md). This file is a planning handoff.

## Navigation

Keep one project, the existing Tabs/Board workspace, and its chat. Add a
`Document sets` section next to `Files` in the navigator. On a narrow screen they
are two views of the same navigator drawer. They never replace the open items.

```text
shop                                      [Compile all dirty (2) | v]

Document sets                                      [+ Add]
  checkout                         Needs compile       ...
    Documents (2)
      docs/checkout.md
      docs/checkout-errors.md
    Source (18 files)
      src/checkout/
      src/shared/
      tests/checkout/
  search                           Up to date          ...
    Documents (5)                  [expand]
    Source (22 files)              [expand]
  security                         Needs compile       ...

Files
  docs/                            [3 document sets]
    checkout.md                    [Docs: checkout]
    checkout-errors.md             [Docs: checkout]
    search.md                      [Docs: search]
    security.md                    [Docs: security]
  docs-feature-search/             [Docs: search]
  src/
    shared/                        [Source: 3 sets]
```

Selecting a set opens its management page as an ordinary item. Expanding it lists
its documents and source selections. Selecting a document opens the shared physical
file; `Reveal in Files` locates it in the tree. Selecting the same file under another
set focuses that same editor buffer. Graph pages from different sets remain distinct
and show their set name, including in tabs when names collide.

The management page shows the name, compile status, documents, source selections,
matched file counts, entry points, problems, and recent run. It offers `Add documents`,
`Link source`, `Compile`, `Rename`, and `Remove set`. Patterns and exclusions expand
under `Edit selection`. Opening settings should not require editing TOML.

All sets appear initially. Optional filters narrow search, problems, history, or the
navigator without changing another panel's target. A set-specific page gives chat
a visible suggestion for scope; it does not hide other sets from the agent.

## Open a folder with no configuration

1. Follow the current folder-opening and model-choice flow. `No AI` still works.
   Show Files immediately, with no docs or source inferred into saved settings.
2. Document sets shows `No document sets yet` with `Add document set` and one short
   explanation: `Choose the documents and the source files they describe.`
3. The form asks for a name and documents. The picker supports multiple files and
   folders, repeated selections, and previews the files each choice matches.
   Offer `Select existing`, `New Markdown document`, and later `Import`.
4. `Source (optional)` uses the same multi-selection picker. Offer `Entire project`
   explicitly, showing the exclusions. Source is never assigned just because a
   folder contains code.
5. Show the resolved summary, e.g. `3 documents · 24 source files`. Existing-set
   overlaps and invalid paths appear next to the relevant selection. Name and empty
   document-pattern validation happen before save. An empty selected docs folder
   can be saved with a visible `No matching documents yet` state.
6. `Add set` saves `.jazyk/jazyk.toml` and the state ignore file atomically as an
   initialization operation. New document creation uses create-only semantics;
   collisions keep the form open. Failed initialization leaves no half-created set.
7. The new set appears in both navigators and opens its first document or management
   page. It reads `Not compiled` or `No documents`. An explicit `Add and compile`
   alternative is available when the backend can compile. Ordinary Add starts no
   model work. If watch mode is enabled, explain its automatic follow-up before save.

Use a name suggestion based on the selected filename/folder. It is editable and
not a required feature identifier. Do not ask users about graph IDs, store paths,
globs, or protocol scope in the basic flow. Advanced selections remain available.

Canceling the form leaves the folder unchanged. A draft new document is held until
Add succeeds. In a browser with unavailable write access, retain the existing held
changes/retry behavior and clearly show that setup is not yet saved to the folder.
There is no account requirement for local or browser projects.

## Add a second feature or extend the first

The distinction is always explicit:

- The project-level `+ Add` defaults to `New document set`. It starts with no
  document/source selection inherited from another set.
- `Add documents` on checkout adds to checkout. It never creates a new graph.
- A file's `Mark as documents for…` menu lists existing sets and `New document set…`.
  Creating a set preselects that file or folder and suggests its name.
- Dragging Markdown onto a set's Documents row adds to that set. Dropping onto the
  Document sets heading opens a choice of new or existing set before writing.
- Adding a new document to a set asks for its path. Suggest a selected docs folder
  when one exists; otherwise use the project folder. The user decides the layout.
  If the path falls outside current patterns, preview adding its exact path to the
  set. Never silently move it to an assumed `docs/` folder.

E.g. checkout already describes `docs/checkout.md`. Selecting `docs/search.md` and
`docs-feature-search/`, then `New document set…`, creates search. Selecting
`docs/checkout-errors.md`, then `checkout`, extends checkout. Both actions show the
target name before save and can reuse exactly the same source selection.

## File and folder markings

The context menu offers:

- `Mark as documents for…`
- `Mark as source for…`
- `Exclude from…`
- `Manage document sets…`

Existing memberships are checked by set and role. Selecting an unchecked membership
adds it after the preview; removing a checked one shows the precise removal. A file
matched by a folder rule is removed with an exclusion, not by deleting its parent
rule. Clearing that exclusion restores inherited membership. Multi-select applies
one reviewed patch to all selected paths.

A badge names the set when space allows. Multiple memberships collapse to a count
that expands on click or keyboard focus. Role and set are readable text, never
color alone. Hover/focus details show `Included by docs/**/*.md` or the exclusion
that applies. A mixed folder shows contained memberships, not an exclusive folder
assignment. The Files tree contains each physical file once.

Manual TOML edits and agent edits refresh the same markings. Invalid config adds a
banner with `Open configuration`, keeps files accessible, and disables mapping-based
writes until fixed. `.jazyk/jazyk.toml` opens project settings with a raw-file option.
Generated set stores remain browsable through existing output views.

## Compile control

Default primary action: `Compile all dirty (N)`. The dropdown contains:

```text
Compile all dirty (2)                 checked
--------------------------------------------
checkout                             Needs compile
search                               Up to date
security                             Needs compile
--------------------------------------------
Manage document sets…
```

Choosing checkout changes the primary label to `Compile checkout`. Remember this
explicit choice per project, not per tab or browser globally. The current target
always appears on the button. Removing the selected set resets it to all dirty.
The management page also has its own Compile action so it needs no global switch.

Before submission, flush pending document/config saves and refresh dirty state. A
save conflict keeps the run from starting and points to the unsaved file. An empty
dirty selection shows `All up to date`; named clean sets can still be checked, with
the normal no-op behavior and zero LLM calls. `No documents`, `Needs setup`, or a
missing-model state names the required action rather than offering an ineffective
Compile. Sets waiting only for answers link to those questions.

While running, show `Compiling checkout · 1 of 2`, the child queue, and `Stop`.
Finishing reports each set, e.g. `checkout compiled · security needs an answer`.
A failed child has Retry for that set. A clean set skipped since submission is
reported as skipped. Model progress stays in the existing chat/run presentation.
Dirty counts and question counts remain separate. Generate, verify, and delivery
retain their existing actions and scopes; a compile selection never authorizes a
project-wide source generation or delivery.

## Everyday management

- Rename keeps graph and history. Warn about duplicate names before save.
- Changing selections previews matched additions/removals and affected state.
  Display `In use by a run` with Stop when a mapping edit cannot apply yet.
- Removing a set explains that docs/source remain on disk and compiled state is
  retained for recovery. `Removed sets` in management offers Restore and the
  separate explicit Purge action. Recovery is local state, not a synced trash bin.
- Deleting the last set returns to the empty navigator while retaining the project
  config and ordinary files. No automatic fallback reclaims every Markdown file.
- A file moved in the editor previews affected exact paths, patterns, and roots
  across all sets. Update config with the move; a failure retains recoverable state.
  Existing document identity/alignment rules decide the graph effect, not an
  invented cross-set graph migration. External moves show missing selections and
  offer `Locate…`; they do not guess which file replaced it.
- Missing or inaccessible source folders get a visible status. Keep existing graphs
  readable and permit unaffected sets to compile.
- Sharing a docs file shows `Shared document: changes affect checkout and search`.
  The user can keep it shared or change membership. It is still one editable file.
- Creating a linked Markdown document inherits the initiating document's set
  context only when unambiguous. If multiple sets are plausible, select the target
  memberships before adding paths outside their rules. Link navigation alone never
  merges document sets.

## Word and PDF import, later

Reserve `Import documents…` in the add-document flow once an importer is available.
Do not show an enabled control before a backend can convert the requested format.

1. Drop/select a Word or PDF file and choose an existing set or create a new one.
2. Choose the Markdown destination. Preview converted headings, tables, images,
   conversion warnings, and any OCR requirement. Keep images in a sibling assets
   directory with relative links.
3. Review and accept the Markdown. Handle filename collisions explicitly. No failed
   conversion may leave an empty document claimed as successfully imported.
4. Save Markdown/assets, record the original hash, format, converter version, and
   output paths in an import manifest, and add the Markdown selection to the set.
   Offer to keep a copy of the original alongside the docs. An in-project original
   is retained unless the user explicitly removes it.
5. Compile the accepted Markdown. Graph quotes point to that Markdown, the canonical
   editable input. Import metadata explains its origin without pretending a lossy
   conversion is a verbatim quote from the binary original.

Reimport produces a comparison with the edited Markdown. It never overwrites manual
edits silently. Conversion adapters use maintained libraries or tools behind a
capability interface. Local conversion is preferred; any service upload requires
an explicit product choice. Format/tool research belongs to that later workstream.
