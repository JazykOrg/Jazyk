# Alignment

Alignment is the step between [parsing](./parsing.md) and the
[dirty set](./reconciler.md#dirty-set). It matches the fresh section trees against the
stored ones, classifies every change, and carries each anchored node (a requirement's
`source`, an entity's `mentions`) to the section that now holds its text. It runs in two
passes: a deterministic pass that applies what is certain and proposes the rest, and an
[`align-doc` turn](./turns/align-doc.md) that decides the proposals. Only then does
[ingest](./compilation.md#waves) run.

Without alignment, a section that moved and changed one word is a removal plus an
addition: every statement sourced from it becomes a stale anchor, and the `reconcile-doc`
turn must re-record each one from scratch. Alignment keeps the ids, keeps the provenance,
and tells the turn what actually happened.

## The deterministic pass

Inputs: the stored `DocRecord` of every document and the fresh parse of every matched
file, all documents at once. A document whose `contentHash` is unchanged drops out whole.
A section whose reference and identity (below) are unchanged drops out. Matching is global: a
section that left one file for another is a move, not a removal.

### Fingerprints

Each remaining section is fingerprinted on demand (nothing new is stored):

- identity: the title plus the whitespace-collapsed body, for exact matching. Unlike
  the raw `hash` it survives a heading level change and trailing blank lines, which
  move no statement.
- tokens: `raw` without its heading line, lowercased, punctuation stripped, whitespace
  collapsed (the same normalization requirement statements use).
- shingles: the set of word 3-grams over the tokens. Shorter bodies use the whole token
  list as one shingle set.
- the title slug, the document, and the parent reference.

### Phases

Each phase consumes its matches. A section is matched at most once.

1. Exact. An old and a new section with the same identity. Same reference →
   `unchanged`. New reference → `moved`. When several candidates share an identity,
   the pairing prefers the same document, then the same parent, then the nearest
   sibling `order`, so two identical sections that both moved pair one to one.
2. Same reference. An old and a new section with the same document and reference but
   a different identity → `edited`, with the similarity recorded. A renamed parent
   heading is this case for the parent (its title changed) and an exact move for each
   child.
3. Fuzzy, over what remains. Splits and merges come first because they are one-sided
   containments, and a half would otherwise pair with its whole as a move:
   - `split`: one old section whose shingles are covered at or above
     `align_split_coverage` (default `0.6`) by two or more new sections (remaining or
     `edited`). Parts are taken by descending containment in the old, each at least
     `0.3` contained, and each further part must add at least `0.2` of the old's
     shingles the earlier parts did not cover (the guard that keeps a moved section
     with a loosely similar neighbor from reading as a split),
   - `merged`: the mirror, one new section covered by two or more old sections under
     the same rules,
   - `moved`: every remaining old section is scored against every remaining new
     section. The score is the Dice coefficient over shingles, plus `0.15` for the
     same title slug, `0.05` for the same document, and `0.05` for the same parent,
     capped at `1.0`. Pairs at or above `align_move_similarity` (default `0.5`) are
     taken greedily by descending score, one to one, the rule git uses for rename
     detection,
   - what remains: old → `deleted`, new → `added`.

The result is one operation per section: `unchanged`, `edited`, `moved`, `split`,
`merged`, `deleted`, or `added`, each with its similarity where one applies. Thresholds
live in [limits](./project-settings.md#limits).

### Anchor relocation

Section matching supplies candidates and order. The quote decides. An anchor is a
requirement's `source`, or an entity mention of its own: a mention that coincides with a
requirement's source was derived from it at commit and follows the requirement wherever
it is placed. For every anchor in an old section that is not `unchanged`:

- locate the quote (whitespace-insensitive, as every [gate](./graph.md#validation-gates)
  does) in the matched new section or sections; a hit is a candidate with
  `quoteLocates: true`,
- otherwise in every other new section of the same document, then in every document,
- otherwise take the best fuzzy window: the candidate section whose tokens hold the
  longest common subsequence with the quote's tokens, scored as that length over the
  quote's length, with the matched window recorded as `nearest`.

A candidate scoring under `0.3` is discarded. The outcome per anchor is a relocation
(`anchor`, `from`, `to`, `quoteLocates`, `similarity`, `nearest`) or `homeless`.

### What applies and what is proposed

- Exact moves apply mechanically: the anchor's `section` (and `doc`, for a move across
  files) is rewritten, the quote is untouched because it still locates, coverage
  carries to the new reference, and nothing is dirty. The rewrites are journaled as one
  `align` entry per build. An `unchanged` section keeps its coverage the same way, so
  a heading level change or a trailing blank line dirties nothing.
- An anchor whose quote still locates under its own reference has not moved, whatever
  happened around it (an `edited` section, or the half of a `split` that kept its
  heading): no proposal, and the dirty section's ingest sees it as an extracted
  statement as usual.
- Every other relocation is a proposal: anchors leaving an `edited`, fuzzy `moved`,
  `split`, or `merged` section, and anchors of a `deleted` section with at least one
  candidate. The section itself is dirty by its hash, as always; the proposal concerns
  where the anchor belongs and whether its statement must be re-judged.
- A homeless anchor with no candidate is a stale anchor on its old document, the same
  contract as before alignment existed.

Proposals persist in `status.yaml` under `alignment`, one block per target document,
each proposal carrying the old location, the old quote, an excerpt of the old section
around it, and its candidates. The old tree is gone from the store once it is synced, so
the excerpt is what the turn will see. Persisting them keeps the
[task queue](./reconciler.md#the-task-queue) derivable from disk.

## The align-doc turn

One `align-doc` turn per document holding proposals. The model sees each anchor's
previous location and wording, its new candidates and their wording, and decides per
anchor:

- `place_anchor` with `reevaluate: false`: the statement is made in the new place with
  the same meaning. The anchor moves and stays an ordinary extracted statement;
  `reconcile-doc` leaves it alone.
- `place_anchor` with `reevaluate: true`: the wording, scope, or surrounding section
  changed meaning. The anchor moves and is listed as a stale anchor on the target
  document's `reconcile-doc` item, which must re-record, revise, or delete it under the
  usual [contract](./turns.md#task-types). A quote that no longer locates forces this
  outcome.
- `orphan_anchor`: the statement is no longer made anywhere. The anchor stays a stale
  anchor on its old document.

The turn only places anchors and sets their state. Extraction, revision, and deletion stay
with `reconcile-doc`; judgment stays with the review turns. The page
[align-doc](./turns/align-doc.md) states exactly what the model sees.

The align turns run as the first wave of a build, all documents at once: placing
anchors needs no vocabulary from the roots, and every ingest turn benefits from seeing
the anchors in their new place. A document with pending proposals blocks its own
`reconcile-document` task until the align task commits; a parked align task parks the
document's ingest with it. A build with no proposals runs no align turn, so a no-op
rebuild still makes zero LLM calls. The store never reaps an anchor named by a pending
proposal: [garbage collection](./graph.md#garbage-collection) waits for the decision.
