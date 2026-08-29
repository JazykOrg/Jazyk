# The reconcile-section goal

`reconcile-section` brings the graph in line with one section whose text changed, whose
anchors went stale, or that was never processed. The session reads the section body
with its diff marked, extracts the statements the section makes and the entities they
need, updates what drifted, addresses every stale anchor, and marks the section
`covered` or `non-normative`. It records the facts as stated, even when sections
disagree: judging is the job of the [`rejudge-pair`](./rejudge-pair.md) and
[`review-entity`](./review-entity.md) goals that follow, and `report_diagnostic` is not
in this toolset. A conflict noticed here is not lost; the judgment goals see the
statements side by side.

- Class: compile. Mandatory. Readiness tier 1.
- Unit: one section. Goal id `g:reconcile-section:<doc>#/<ref>`, e.g.
  `g:reconcile-section:docs/orders.md#/orders/holds`.
- Skill: [`extraction`](../skills/extraction.md).

## Created when

The goal derives from two [change record](../graph.md#change-records) kinds on the
section, and from one coverage state:

- `section-dirty`: the `edit` journal entry of a build writes it for every section the
  [dirty set](../reconciler.md#dirty-set) marks: added, or changed in title or body. An
  exact move is not dirty; references are rewritten mechanically
  ([section diffing](../parsing.md#section-diffing)). `detail` carries the diff summary
  (lines added, removed, changed) and the previous hash, so the loaded body can mark
  the diff.
- `anchor-stale`: written by the `align` entry for a requirement `source` or an entity
  mention whose stored quote stops locating in the section, and by a `place_anchor`
  with `reevaluate: true` or with a quote that does not locate in its new section
  ([place-anchors](./place-anchors.md#gate)). An orphaned anchor lands on the nearest
  surviving ancestor of its old section. `detail` lists the anchors: the id, the
  `statement` or the entity name, the dead quote.
- Unprocessed coverage: a section with a body of its own whose coverage is
  `unprocessed` derives the goal without a record
  ([coverage](../compilation.md#coverage)); its `change` is `{unprocessed: true}` and
  its `cause` is the generation that parsed the section. This is what makes a fresh
  store the same case as any other build, and what resumes a section an earlier session
  left unmarked.

Several records on one section fold into one goal whose `change` merges their details
and whose `cause` is the earliest ([change records](../reconciler.md#change-records)).
A section whose own body did not change is not dirty when its parent heading changed:
a heading rename that moves children is an exact move. A section removed from the
document derives nothing here: the sweep kills the quotes anchored in it
([garbage collection](../graph.md#garbage-collection)), and what those facts held up
surfaces as [`retrace`](./retrace.md) goals.

E.g.:

```yaml
- id: c412-0
  generation: 412
  mutation: 0
  kind: section-dirty
  subject: docs/orders.md#/orders/holds
  via: section
  detail: {added: 1, removed: 1, changed: 0, previous: 8f3a1c}
```

## Readiness

- Tier 1: ready when no tier 0 goal is open or parked, the document carries no
  `alignment-pending` record, and the document's link level is reached
  ([readiness](../reconciler.md#readiness)). Link levels run breadth-first over the
  document link graph from the [roots](../project-settings.md#roots): a root document's
  sections run before the documents it links to, so the entity a parent introduced for a
  part exists before the part's sections ask what "the system" means
  ([incoming links](#incoming-links)). Documents unreachable by links run
  last, in path order.
- Locality is the document ([batching](../reconciler.md#batching)): one document's
  goals batch in document order, adjacent sections together, filling until the context
  budget says stop. A section whose body alone exceeds its share loads truncated with
  `read_section` pointers. The session budget grants at least eight rounds per section
  in the batch ([budgets](../sessions.md#budgets)).
- A goal a commit opens on the same document (an `anchor-stale` written by a
  `place_anchor` on a section the batch did not hold) joins the running session when it
  fits the locality and the budget, otherwise a later batch
  ([bubbling](../reconciler.md#bubbling)).

## Incoming links

A document set describes one subject by splitting it across files. A parent lists its
parts and links to a file per part; the file details the part. The link is what says
which entity the file is about, and the parent's session has already recorded it: the
list item yielded a requirement, and that requirement introduced the part's entity
([enumerations](../concepts/statements.md#enumerations)).

So the loaded set for a section names every incoming link the graph already resolved:

```
## Linked from
- docs/slides.md#/slides "[Introduction](./slide-intro.md)" introduced ent:introduction (Introduction)

primarySubject: ent:introduction (Introduction)
```

The block always resolves the subject question, so the session never guesses what "the
system" means:

- One introduced entity: `primarySubject: ent:introduction`. The session reads "the
  system", "this", and "it" as that entity, and its requirements reference it instead
  of minting a second one under the document's own heading.
- Several introduced entities: `candidateSubjects` lists them, and the statement's own
  section decides which one it constrains. "The system" in a detail document still
  means the part being detailed, never the containing application.

Without the block, a file linked as a part yields requirements tied to nothing the
parent knows, the part's entity keeps only the parent's one-line mention, and
[generation](../../consumers/gen.md) sees an entity with a name and no content.

A link is resolved by locating the target document in the verbatim quote of an existing
entity mention or requirement `source`, so the binding is deterministic and needs no
model judgment. Links the graph has not resolved are not listed: the link levels run a
parent's sections before the documents it links to
([readiness](../reconciler.md#readiness)), so by the time the part's session runs, the
parent's requirement exists. The block is capped at twelve links.

## Gate

The gate holds when all of the following are true for the section, over the store plus
what the session has staged:

- A coverage mark is staged or recorded: `covered`, or `non-normative` with a note
  ([coverage](../compilation.md#coverage)).
- A `covered` claim has a requirement sourced from the section behind it, staged in this
  session or already recorded.
- A `non-normative` note is not one of the three rejected reasons ("it states a fact,
  not a requirement", "it describes content or appearance, not behavior", "it is not a
  requirement on the system"). The `suspicious-non-normative` check judges the rest
  after the build.
- Every stale anchor listed in the goal's `change` is addressed: re-recorded through
  `upsert_requirement` with a fresh verbatim quote (the natural key resolves to the
  anchor and updates it in place, [identity](../model/requirement.md#identity)), revised
  through `update_requirement` carrying the new `statement` and `quote`, or deleted
  with `delete_requirement`. A stale mention is refreshed by an `upsert_entity` carrying
  a fresh `mention` in the section; a mention left dead is pruned by the sweep at
  commit.
- Every staged quote locates whitespace-insensitively in the section it names, and
  that section belongs to the document the quote claims (`wrong-document`).

`mark_goal_done({goal, justification})` is validated against these and rejected naming
the failing rule and the section. `done` runs the same gate over every goal in the
batch ([validation gates](../graph.md#validation-gates)) and gives the model repair
rounds; a batch that cannot be repaired finishes implicitly, and a goal left open after
its one fresh session parks
([resolving, failing, parking](../sessions.md#resolving-failing-parking)). The paths
differ in what one bad claim can sink ([commit](../sessions.md#commit)):

- An explicit `done` with a dishonest `covered` claim is sent back to repair it.
- An implicit `done` (the session ended with mutations staged) drops the offending
  coverage marks and commits the rest: the extracted requirements land, the miscovered
  section stays `unprocessed`, and the goal derives again next build.
- An untouched stale anchor keeps the goal open and commits nothing for its section: the
  mutations staged against the section are dropped, the batch's other goals commit as
  their own gates allow, and the goal gets one fresh session in the same build before it
  parks. Stale anchors are a contract only the model can honor, and the harness never
  commits around one.
- `mark_goal_failed({goal, reason})` is for a section that cannot honestly be
  reconciled: its stale anchors match nothing it says and nothing it deletes, or its
  text contradicts itself sentence by sentence. A failed goal keeps its record and
  surfaces on the section. It blocks convergence and does not hold tier 2
  ([readiness](../reconciler.md#readiness)).

Extraction order inside the goal is deliberate: requirements first, entities only as
requirements need them. An entity that no statement needs is noise
([what is an entity](../model/entity.md#what-is-an-entity)).

## Hints

Computed by the harness and rendered under the goal block:

- The diff summary from the record: lines added, removed, changed; or `unprocessed`.
- The count of requirements already sourced from the section, with the note that an
  unchanged statement is a no-op, not a re-extraction.
- Each stale anchor: the id, its `statement` or the entity's name, and the dead quote.
- The subject: `primarySubject: ent:<slug>` when the loaded set resolves exactly one
  entity another document introduced for this document, `candidateSubjects` when
  several, so the session never guesses what "the system" means
  ([incoming links](#incoming-links)).
- The count of entities mentioned in the document, with `search` before creating.
- `load docs/<doc>#/<ref>` when the body is not loaded in full.
- For a code-block section, its line count: coverage needs a requirement per behavioral
  step ([code blocks](../concepts/statements.md#code-blocks-state-obligations)).
- `skill extraction`, and the tools that resolve the kind: `upsert_requirement`, then
  `set_coverage` exactly once.

## What the model sees

The session prompt is [assembled](../sessions.md#the-prompt) like every session's: the
agent contract, the active skills, the project block, the goals block, the loaded set.
The goal block carries the contract paragraph from
[`prompts/reconcile-section.md`](./prompts/reconcile-section.md): apply the extraction
skill to every sentence, code block step, test case, and list item; record each
obligation as one atomic `statement` in clear wording with its entities, edges,
transition, facets, and a verbatim quote; leave an unchanged statement alone,
re-record a changed one, delete one the section stopped making; honor every stale
anchor; search before creating an entity and read "the system" as the entity another
document introduced; record the facts as stated; set coverage exactly once, with the
three rejected reasons named; stage nothing when the section already yielded everything
it states. Then the change in one line, the gate in one line, and the hints.

The `extraction` skill is active from the first round: the kind names it, and loading a
section brings it anyway ([skills](../sessions.md#skills)). The skill carries the
sentence test, granularity, enumerations, code blocks, test cases, quotes, entities,
edges, transitions, attributes, facets, stability, and coverage honesty; the doctrine
behind it is [statements](../concepts/statements.md).

The initially loaded set for the batch holds, per section:

- The section body, whole, with the diff marked (the lines removed and added since the
  previous hash), or truncated with `read_section` pointers when it exceeds its share
  of the budget; its parent chain as titles, for orientation.
- Under the body, the requirements already sourced from the section: id, `statement`,
  quote, entities, edges, transition, facets. An unchanged statement is a no-op, and a
  coverage claim sees what the section already yielded.
- The stale anchors, each with its statement or entity name, its dead quote, and its
  former section.
- The linked-from block: mentions and requirement quotes in other documents whose links
  resolve to this document, capped at twelve, and the `primarySubject` or
  `candidateSubjects` line ([incoming links](#incoming-links)).
- The known entities: those mentioned in the document first, then the rest of the graph
  as a count with a `search` pointer; each a stub (name, one definition line,
  stereotype) ([policy](../context.md#policy)).

E.g.:

```
## Goals
- [g:reconcile-section:docs/orders.md#/orders/holds] mandatory
  [contract paragraph]
  Change: 1 line added, 1 removed (edit g412); 1 stale anchor.
  Gate: coverage mark staged or recorded; stale anchors addressed; every quote locates.
  Hints: 3 requirements already sourced; stale req:orders-6 "held orders expire after
  21 days"; primarySubject: ent:order (Order); 14 known entities, search before create;
  skill extraction

## Loaded (9.8k/24k chars)
- docs/orders.md#/orders/holds   section body, with the diff marked; 3 requirements sourced
- linked from docs/main.md#/systems   "[Orders](./orders.md)" introduced ent:order
- ent:order    stub (definition only)   [7 requirements loadable: h:ent:order:requirements]
- ent:hold, ent:payment   stubs
skills: extraction (active); judgment, flow-views, structural-views, abstraction, conformance (load_skill)
```

`jazyk preview <goal>` renders the prompt before it is spent
([preview](../sessions.md#preview)).

## Tools

The `reconcile-section` toolset ([toolsets](../tools.md#toolsets)):

- The [read tools](../tools.md#read-tools): `load`, `expand`, `unload`, `graph_status`,
  `search`, `read_section`, `get_entity`, `get_view`, `diagnostics`.
- The [goal tools](../tools.md#goal-tools): `mark_goal_done`, `mark_goal_failed`,
  `load_skill`, `done`.
- Entity tools: `upsert_entity` (with `mention`, `stereotype`, `parent`, `attributes`
  where the section states them), `update_entity` (`add_aliases` for a wording found
  under another name), `delete_entity`.
- Requirement tools: `upsert_requirement` (statement, entities, section, quote, edges,
  transition, facets), `update_requirement`, `delete_requirement`.
- `set_coverage({section, state, note?})`, once per section, after its extraction.
- [`report_feedback`](../tools.md#feedback-tool).

Deliberately absent: `report_diagnostic` (extraction records, judgment judges),
`merge_entities` (a lookalike met here becomes an alias on the existing entity or a
finding for `review-entity`), the view tools (default views derive from what is
written; curation is [GC](../graph.md#garbage-collection)), `place_anchor` and
`orphan_anchor` (alignment's, decided before this goal is ready).

The stale anchor contract has three outcomes, and the gate accepts nothing else: the
fact still stands and the session re-records it with a fresh verbatim quote; the fact
changed and the session revises it with the new `statement` and `quote`; the fact is
gone and the session deletes it. A revised statement opens `rejudge-pair` goals at
commit, and a deleted one opens `retrace` goals on what pointed at it; the tool reply
previews both ([bubbling](../reconciler.md#bubbling)).
