# Plan: Word and PDF import

Status: proposed, not implemented. Written 2026-10-08. Extends the released
[document-set implementation](./implementation.md). Converter recommendations
below are based on upstream documentation, not a completed conversion benchmark.

## Outcome

Drop a Word document or PDF into an open project. Choose a document set and where
its Markdown should live. Review the conversion, save it, and compile that set.
Importing a second feature can create another set without changing the first.

The accepted Markdown becomes the editable compiler input. The original remains
available as a reference when retained. Import metadata connects the two. Updating
an original produces a reviewed Markdown change, never an automatic overwrite.

Import does not create graph nodes directly. Accepted Markdown follows the current
parser, alignment, incremental compilation, provenance, and document-set rules.
The graph quotes Markdown. A PDF page reference is additional origin information,
not a claim that converted text is a verbatim quote from the PDF bytes.

## User flow

Entry points:

- `Import documents…` beside `Select existing` and `New Markdown document`.
- Drop files onto a set's Documents row to preselect that set.
- Drop onto the Document sets heading to choose an existing set or a new set.
- `Import as documents…` on an in-project Word or PDF file in Files.
- Drop onto a Files folder to suggest the output folder, then choose the set.

Dropping a binary into an editor buffer must not insert its bytes as text. The
import form names the destination project and set. No drop silently initializes a
project or chooses a source-code scope.

The form has three steps:

1. **Choose destination.** Select an existing set or enter a new set name. Choose
   a Markdown path or destination folder. Source files remain optional. New-set
   creation uses the current source picker. Existing sets retain their source scope.
2. **Convert and review.** Show progress per file, then an editable Markdown draft
   and rendered preview. PDF review can show the original page alongside the draft.
   Word review shows extracted structure and an Open original action; it need not
   reproduce Word pagination. Show headings, tables, images, and located warnings.
3. **Save.** `Import` saves documents, assets, origin records, and set membership.
   `Import and compile` also requests compilation after persistence succeeds.
   Build-on-save behavior is disclosed before saving, as for ordinary document edits.

One input produces one Markdown document by default. A batch defaults to adding
all selected inputs to one chosen set. A batch mapping table can assign individual
files to different existing or new sets. Do not infer one graph per uploaded file.
Splitting a large document by headings is later work and remains a separate choice
from creating document sets.

Suggest an editable filename and set name from the original filename. Do not force
`docs/`, a feature-folder convention, or an import-specific docs folder. Preview
all created files and memberships, including incidental matches in other sets.
If an existing broad rule would claim a retained binary as a document, show the
necessary exclusion before saving. Add Markdown paths, not original binary paths.

Filename collisions offer another name, an existing import's Update action, or
explicit replacement with a diff. No silent overwrite or silent numbered duplicate.
Identical original content offers Open existing, Add its Markdown to another set,
or Import another copy. Different profiles can produce different conversions, so
the deduplication key includes original hash, converter profile, and options.

Canceling before save leaves project files and configuration unchanged. Conversion
can use isolated temporary storage. A failed file remains a failed batch row;
the user can explicitly omit it and save the successful rows. No empty placeholder
is accepted as a successful conversion.

## Conversion quality

Preserve wording, values, headings, lists, links, simple tables, captions, and
embedded images where supported. Formatting conversion must not summarize,
invent missing text, or rewrite requirements. Conversion needs no chat model.
PDF layout and OCR can use local machine-learning models without using an LLM.

Output must match the Markdown dialect the compiler and editor actually read:

- Normalize line endings and stable whitespace. Keep paragraph and heading order.
- Emit simple tables as GFM tables. Detect cells that cannot be represented
  faithfully, such as merged cells or nested content. Offer an explicit list-form
  conversion for review, or require manual repair. Never silently drop cells.
- Emit notes in readable Markdown with ordinary links. Do not depend on unsupported
  footnote syntax or raw HTML to preserve important prose.
- Keep images as files with relative links and existing alt text/captions. A diagram
  image remains an image; importing it does not extract its semantic relationships.
- Keep conversion warnings and bookkeeping outside the compiled prose. Do not insert
  warning text that the compiler might interpret as a product requirement.
- Detect comments and tracked changes in Word inputs. Default to visible final
  document content, report omitted review material, and require review for unresolved
  changes. The adapter must prove this behavior in fixtures before it is advertised.
- Detect unreadable or image-only PDF pages. Offer OCR with a language selection.
  Mixed PDFs need per-page handling so OCR does not duplicate an existing text layer.
- Report missing text, uncertain reading order, unsupported tables, and omitted
  objects with page/block locations when available. Do not invent confidence scores
  for converters that do not provide them.

Encrypted files get an actionable refusal in the first release. Legacy `.doc`,
macro-enabled Word documents, embedded executable objects, and automatic diagram
interpretation are outside the first supported format list. Recommend saving Word
files as `.docx` when necessary. Validate actual file signatures, not just extensions.

## Converter recommendation

Use maintained conversion libraries behind adapters. Jazyk owns preview, project
integration, origin records, and update merging. It does not implement OOXML or PDF
parsers or train an OCR/layout model.

| Input | Recommended candidate | Role |
|---|---|---|
| `.docx` in the browser | Mammoth HTML conversion, then Turndown with GFM support | Lightweight accountless import using document structure. |
| `.docx` through native/hosted workers | Docling adapter | One optional worker package can handle Word and PDF. |
| PDF with text, tables, or multiple columns | Docling standard PDF pipeline | Recover reading order and structure; export Markdown and origin data. |
| Scanned or mixed PDF | Docling with a tested OCR engine and language packs | Extract page text with explicit OCR status and review. |
| PDF preview | PDF.js | Render original pages for review. It is not the structured conversion engine. |

Mammoth supports browser execution, semantic styles, images, and conversion
messages. Its direct Markdown mode is deprecated; use its HTML API and a separate
Markdown converter. It does not sanitize its output, so preview must sanitize
intermediate HTML and restrict links. Sources: [Mammoth](https://github.com/mwilliamson/mammoth.js),
[Turndown](https://github.com/mixmark-io/turndown).

Docling supports PDF layout, reading order, tables, OCR, Word input, and Markdown
and structured exports. Its local models can be prefetched for offline use.
Remote processing is separately configurable. Recommend its standard local
pipeline first and keep optional VLM enrichment disabled. Sources:
[Docling](https://github.com/docling-project/docling),
[offline and advanced options](https://docling-project.github.io/docling/usage/advanced_options/).
[PDF.js](https://mozilla.github.io/pdf.js/) supplies browser PDF rendering.

Pin library, model, OCR-language, and normalization versions in each conversion
profile. Profiles are distinct; do not promise byte-identical output from Mammoth
and Docling. Preserve the previous profile on re-import when available. Switching
profiles or upgrading a converter requires a new preview.

The initial spike must validate fidelity, package size, cold start, warm conversion
time, peak memory, cancellation, and offline operation. These recommendations are
provisional until that corpus passes. Avoid installing model packages into the
core Rust build or making ordinary compilation depend on Python or Node.

## Where conversion runs

| Project and available capability | Default behavior |
|---|---|
| Browser project or browser-opened folder, Word input | Convert inside the browser. No account or upload. |
| Browser project with a connected local Jazyk converter | Offer local conversion, then return a draft to the same project. |
| Local API / CLI with the converter installed | Run the isolated local worker. No cloud account. |
| Hosted project | Offer the hosted conversion worker with explicit upload wording. |
| Browser-only PDF without a converter | Preview the PDF and offer Connect local converter or explicitly chosen hosted conversion. |

Browser-only structured PDF/OCR is not a first-release promise. Do not silently
fall back to incomplete text extraction. All environments share the result and
commit contract; supported conversion profiles are capability-driven.

The UI says `On this device` or `Upload to Jazyk for conversion` when that choice
matters. A local converter can perform transient conversion for a browser project
without adding the original or outputs to a different local project. Uploads for
cloud conversion require the user's chosen destination, not an automatic fallback.
For a hosted project, conversion on-device still requires uploading accepted
outputs to save the project. Make that distinction clear.

Converter installation and first model download are explicit setup actions.
Installed local conversion should work offline. Conversion jobs carry progress,
cancellation, timeouts, and resource bounds independently of compile jobs. A
separate worker keeps large PDF work off the UI thread and hosted request process.
Measure worker memory before choosing hosted capacity. The current API host is not
assumed to have enough spare memory for layout and OCR models.

## Files and identity

An import belongs to the project, not to a graph. One Markdown file may be selected
by several sets without duplicating the original or import record. Removing or
purging a set must not delete imported documents, originals, or origin records.

Example layout, with user-selected document and original locations:

```text
project/
  .jazyk/
    jazyk.toml
    ds_checkout/
    ds_search/
    imports/
      imp_a1/
        manifest.json
        base.md
        origin.json
  docs/
    checkout.md
    checkout.assets/
      image-<content-hash>.png
    originals/
      Checkout.docx
```

The document-set stores remain directly under `.jazyk`. `imports/` is shared
project metadata, not another parent for set stores. `base.md` is the latest raw
normalized conversion before review edits. `origin.json` records available
locations against that exact snapshot. The manifest contains:

- Format version and stable import ID.
- Original display filename, media type, byte size, and content hash.
- Optional project-relative original path. Never a developer-machine absolute path.
- Converter/profile version, relevant model versions, and conversion options.
- Markdown path, converted baseline hash, accepted output hash, and asset hashes.
- Conversion warnings, reviewed exceptions, and the last successful import time.

Import records and merge baselines are portable authored metadata. Commit/export
them with project files. The implementation must explicitly unignore these paths
in the current `.jazyk` ignore policy and allow them through project adapters.
Temporary uploads, conversion drafts, locks, and worker caches remain excluded.
Do not place originals in the per-set runtime store, where a reset could lose them.

Offer `Keep original in project`, enabled by default for an external file, with
its size and destination visible. If the original already lives in the project,
keep it in place and do not duplicate it. Declining retention still preserves the
filename/hash record; future re-import requires choosing the original again.
Originals, retained image assets, and import metadata are protected from generated
source writes even when a set's source selection is broad.

PDF locations may identify a page and bounding box. Word locations may identify
an extracted block or table cell, not a fabricated page number. After Markdown
edits, show origin as the conversion snapshot unless a mapping remains verified.
Never reuse stale offsets as graph provenance. Preserve asset references across
moves through the ordinary project move flow.

## Re-import

`Update from original…` appears on an imported document. Changes to an in-project
original show `Original changed`; detecting that change does not compile or import.
If the original is absent, offer Locate original. An identical content hash and
profile is a no-op unless the user explicitly requests reconversion.

Use a three-way merge with a maintained diff/merge implementation:

```text
previous raw conversion + current edited Markdown + new raw conversion
                             |
                       proposed update
                             |
                      review and save
```

Keep the raw conversion as the merge base, including when the user edited the first
preview before accepting it. Using the accepted, edited text as the conversion
base would incorrectly classify those edits as upstream content on the next import.

Apply non-conflicting upstream changes in the draft while keeping local edits.
Show conflicts as old original/current Markdown/new original, with explicit
resolution. Unresolved markers cannot be saved as an accepted import. Removing an
upstream section that has local edits requires review. A converter upgrade can
create formatting churn; show its profile change before the diff.

Keep content-addressed image names stable. Never overwrite an edited image or
delete an asset still used by another document. Asset cleanup is separate from
applying the text update. Save the new raw baseline only after the updated Markdown
and assets have persisted successfully. Changed documents dirty all matching sets
through the normal edit path. No graph reset occurs.

## Shared service and persistence

Suggested state sequence:

`selected → converting → ready for review → saving → imported`.

Failures, cancellation, and save conflicts have explicit states. A draft records
the project identity, chosen target memberships, config revision, input hash,
converter profile, output hashes, and reviewed warnings. Editing its content or
destinations invalidates any previous commit token.

The engine owns a portable import bundle validator and project commit service.
Adapters provide Markdown, asset bytes, optional original bytes, warnings, and
origin information. The shared service checks paths, sizes, hashes, membership
effects, generated-write protection, and the current project revision. The
browser adapter runs in the host; Rust/WASM does not invoke a Python subprocess.
Public converter contracts and local helpers live with the public engine, so
native import never depends on the private editor checkout.

Proposed operations, with exact route names settled in the contract change:

- List conversion capabilities and profiles.
- Prepare conversion from an authorized file or opaque upload handle.
- Read/update a preview and its proposed project changes.
- Apply the exact reviewed preview with expected revisions and an idempotency key.
- List project imports, locate an original, and prepare an update.
- Cancel and release a draft.

GUI, CLI, ACP, MCP, and LSP commands call these operations. Agents use explicit
set IDs and receive warnings and diffs before committing. Worker sessions resolving
compiler goals do not gain project-level import tools. Agent tools refer to file
paths or upload handles instead of putting large base64 originals into prompts.
Preparing a preview is read-only with respect to project files. An authorized
agent can apply it without an invented human-only approval requirement.

Conversion runs outside the build lease. Applying a preview uses existing project
write coordination and refuses stale config, changed output files, or incompatible
active runs. Flush open editor drafts before comparing hashes. Use create-only
semantics for new paths and expected hashes for replacements. The preview names
every affected set, including sets that already match the chosen destination.

Do not claim multi-file atomicity from individual filesystem renames:

- Native: stage files and journal the operation, fence compiler/watcher reads,
  publish config/manifest membership only when the bundle is complete, and recover
  interrupted writes before compilation. Recovery checks for external edits.
- Browser storage: commit the bundle transactionally before reporting success.
- Browser folder: use a retained pending-operation record and existing held-save
  behavior. Recover or retry partial filesystem writes before compiling. Do not
  report `Saved to folder` while writes or permission recovery remain pending.
- Hosted: commit source revision, blob references, and workspace state under the
  existing fenced lease. Disconnected retries use the same idempotency key.

Before confirmation, cancellation leaves no published files. During saving, do
not abandon a partially applied bundle. Finish or recover it, preserving user edits.
Watchers see a completed import as one authored change, including when broad
document globs already match the destination before config is updated.

## Storage prerequisites

The public VFS and browser/folder stores already carry bytes. The hosted service's
authored `Files` map currently holds UTF-8 strings, while compiled snapshots can
hold binary bytes. Imported originals and images are authored project files.
Putting them only into a generated snapshot would lose them from source history.

Add typed authored file entries, retaining text inline and using immutable blob
references for binary content. Cover upload/download, revisions, export/import,
Git delivery, quotas, deletion retention, and backup/restore. Reuse existing binary
snapshot and delivery mechanisms where appropriate without confusing authored
assets with disposable compiler output. Specify the durable hosted schema update
separately from any legacy project-format conversion; no automatic root-config
backfill is part of this work.

The editor's document image resolver must read authorized project assets on all
backends. Check reload, offline export, and set-sharing behavior, not only the first
conversion preview. Binary endpoints need bounded or streamed transfers; large
originals should not inflate the ordinary project JSON or chat context.

Treat inputs and converter output as untrusted files. Use maintained parsers with
archive expansion, page/pixel, memory, time, and output-size bounds. Disable
external-resource fetches and active content. Confine local workers to staged inputs
and output paths. Sanitize HTML/URLs before preview and validate the bundle again
in the engine. Cancellation terminates conversion work and releases temporary data.

## Delivery sequence

1. **Converter spike and fixtures.** Compare the recommended profiles on realistic
   Word/PDF examples. Establish support limits and record measurements. Include
   scans, mixed text layers, multi-column text, tables, images, notes, and edits.
2. **Contracts and byte storage.** Update canonical engine docs and private editor/
   runtime docs first. Specify import records, capabilities, preview/apply, recovery,
   and binary authored storage. Implement these seams and asset delivery.
3. **Complete Word flow.** Ship browser Word import and the native adapter, including
   destination selection, review, save, set membership, origin display, and re-import.
   Validate arbitrary folders, hosted persistence, and accountless browser use.
4. **PDF and OCR.** Add the isolated conversion worker, local installation, PDF page
   review, OCR setup, cancellation, and explicit hosted conversion. Measure hosted
   capacity before enabling it. Run the same import and re-import acceptance suite.
5. **Agent and release acceptance.** Exercise CLI, ACP/MCP, and LSP through shared
   operations. Verify exports, restoration, browser/folder failures, mobile review,
   and updates to shared documents. Build native/WASM and run existing suites.

Neither format is complete until re-import, persistence, and image/original access
work. Optional AI cleanup, browser-only PDF OCR, legacy Word formats, batch chapter
splitting, continuous Word synchronization, and writing back to Word/PDF are later
milestones. An eventual AI rewrite is a separate reviewed edit, with its own model
choice, rather than hidden conversion behavior.

## Acceptance cases

| Scenario | Required result |
|---|---|
| Bare folder, canceled import | No `.jazyk`, output files, or set created. |
| Word plus PDF into one set | Separate editable documents in the same graph context. |
| Two files assigned to different sets | Both visible; each compiles independently. |
| Shared imported Markdown | One physical file; every matching set becomes dirty after an accepted update. |
| Review edits, then re-import | Edits survive through the raw-conversion merge base. |
| Conflicting upstream and local edit | Conflict remains reviewable; no silent overwrite or accepted conflict markers. |
| Unchanged original/profile | No file writes, graph resets, or model calls. |
| PDF page with no extracted text | Explicit OCR/unsupported state, never a successful empty document. |
| Multi-column layout and merged table | Preserved reading order/cell content or located review warning. |
| Image and retained original | Accessible after reload, export/import, and hosted restore. |
| Existing filename or case-only collision | Explicit conflict before any replacement. |
| Source/config changes during review | Apply refuses stale hashes and retains the draft. |
| Failure after some folder writes | Recovery completes or rolls back owned changes before compilation. |
| Save acknowledged, immediate reload | Accepted files, membership, baseline, and origin record all remain. |
| Worker timeout or cancel | No published partial import; temporary resources released. |
| Switching projects during conversion | Results stay attached to the originating project and draft. |
| Removing/purging a set | Shared imported files and origin records survive. |
| Generated source writes use broad globs | Imported docs, originals, and assets remain protected. |
| Offline local/browser conversion | No remote data transfer; missing packages get a setup state. |

Implementation changes follow [the docs-first sequence](./implementation.md#1-land-the-contracts).
This plan changes no runtime behavior or production deployment.
