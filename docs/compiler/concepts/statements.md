# Statements

A statement is the text of a [requirement](../model/requirement.md): one obligation the
documents place on the subject, written as a free-form sentence. The requirement stores it
in its `statement` field and keeps the source sentence verbatim in its `quote` (see
[fields](../model/requirement.md#fields)). The wording is the model's: it writes whatever
sentence carries the obligation most clearly, in the documents' own terms. No template and
no keyword is prescribed; [wording](#wording) says what a clear statement has. The
`extraction` skill ([`skills/extraction.md`](../skills/extraction.md)) carries this page
into every [`reconcile-section`](../goals/reconcile-section.md) session.

This page is the extraction doctrine: what counts as an obligation, which subject it binds,
how finely to split, and what a statement leaves to other nodes.

## The subject is whatever the documents describe

Jazyk does not assume the documents describe software. The subject of a statement is the
thing the documents are about and its parts: a service, a slide deck, a book, a course, a
schematic, a contract, a department. A statement names that subject by its entity's name.
E.g.:

```
quote: This slide shows a headline title `Jazyk` as well as a link to the site.
statement: The Introduction slide shows a headline title `Jazyk`.
statement: The Introduction slide shows a link to the site.
```

"The system" is a placeholder, never an entity. When the documents themselves say "the
system", the statement keeps the phrase or names the real subject, and the requirement
references the entity the documents mean. Minting `System`, `Order System`, or any name
built from the placeholder invents a component the documents do not describe.

The consequence is that a document describing an artifact's content, structure, or
appearance is stating requirements on that artifact, not background. Three kinds are
missed most often:

- Content. What an artifact says, shows, or contains is an obligation on it. "This slide
  defines what Jazyk is about in a couple of sentences" yields "The About slide defines
  what Jazyk is about in a couple of sentences."
- Appearance and material facts. A stated value, color, font, size, measurement, or
  wording is an obligation on the thing it describes. "The primary color is #248555"
  yields "The slides use #248555 as the primary color." A stated fact is never "just a
  fact"; the document states it because the artifact must match it.
- Format and medium. "All slides are under the Microsoft PowerPoint file format" yields
  "The slides are delivered in the Microsoft PowerPoint file format."

The test never asks whether a sentence describes behavior. Artifacts that do nothing
still have obligations: they must contain, show, and look like what the documents say.

The counterweight is navigation. A sentence whose only content is where something is
written constrains the documentation, not the result:

```
The slides themselves are defined under [Slides](./slides.md).
This document describes how the gateway works.
```

Neither states anything the result must satisfy, so neither is a requirement. The
difference from a list item is what the sentence names: an item under "the sub-systems
are:" names a part of the subject and is a fact about it, while a navigation sentence
names a file. See [enumerations](#enumerations).

History is the other counterweight. A sentence reporting a completed past event states
no obligation on the subject today: only the standing rules such prose hides are
extracted, and nothing mentioned only in history is minted.

## Declarative prose states obligations

Documentation rarely says "shall". A declarative sentence about the subject states an
obligation all the same. The test per sentence: does it say what the subject or one of
its parts is, does, contains, shows, uses, allows, requires, or limits? If yes, it is a
requirement. The extractor writes the obligation as the `statement` and keeps the source
sentence verbatim in the `quote`. E.g.:

```
quote: The store mints every id at node creation. Ids are immutable.
statement: The store mints every id at node creation and never changes it.
```

Statements of composition and technology choice are obligations too, and one sentence
often carries several that can fail independently. Record one requirement per
obligation, all sharing the same verbatim quote (see [granularity](#granularity)).
E.g.:

```
quote: The gateway is a REST service built with Go.
statement: The gateway is a REST service.
statement: The gateway is built with Go.
```

Access and permission rules are obligations, the most commonly missed kind. A sentence
saying who may do something limits the subject. E.g.:

```
quote: All management operations can be performed by Admins only.
statement: The user management system allows only Admins to perform management operations.
```

The statement's subject is the sentence's own subject, and the requirement references
that entity. A pronoun subject ("This", "It") resolves to the subject the document
already introduced: "This is a script written in javascript" is an obligation on that
subject, referencing its existing entity, never a new `Script` entity minted from the
predicate noun. Never substitute a broader system for a named part: "The inventory
system manages products" is a requirement on the inventory system, not on the
application that contains it. The requirement also references every other entity the
statement names, which is what ties those concepts into the graph. The subject is
always an entity: mint it when search finds nothing that means the same concept. One
exception: when the grammatical subject is barred from being an entity (a flag, a path,
a command, a value), the requirement attaches to the owning entity in scope instead
("The flags `--verbose` and `--quiet` control logging" is a requirement on the CLI).
Anything else a statement merely mentions (a field, threshold, or value) stays
requirement detail, carried in the statement text, until statements are about it
directly. See [what is an entity](../model/entity.md#what-is-an-entity).

Two kinds of sentence state a fact the requirement alone does not hold:

- Structure. "An order carries a total and a currency" is a requirement on the order,
  and the order entity also gains the `attributes` the sentence states (`total`,
  `currency`). A worked example ("Ana keeps 3 items in her cart") is an instance: an
  entity tied to its type by an `instantiation` edge, with the values on its own
  `attributes`. A worked example under an illustration heading whose values nothing
  else uses is illustration, not an instance: extract only a rule no other section
  states; its incidental values (a day of week, a quantity) are illustration detail,
  never a statement, and a sentence in an example never records a transition.
  See [entity fields](../model/entity.md#fields).
- Whole and part. "The shop contains an order service" is a requirement on the shop
  with a `composition` edge from the shop to the order service, and the order service's
  `parent` follows the edge. See [containment](../model/entity.md#containment) and
  [edges](../model/requirement.md#edges).

Non-normative is the exception, not the default. A section is non-normative only when
no sentence in it passes the test above: navigation pages that only link elsewhere,
glossaries defining outside-world terms, changelogs, roadmap wish lists. A glossary
entry that states what a part does is a requirement wearing a glossary's clothes;
extract it. The counterweight: an entry that only fixes vocabulary ("X is the term
for Y") states no obligation and mints no entity, and a section that declares itself
non-authoritative and defers to the pages that own its facts is non-normative;
definitions the owning pages already state are not extracted twice. Lists of
operations, properties, or rules are never non-normative; see
[enumerations](#enumerations). Before marking a section non-normative, re-read it
sentence by sentence; if any sentence is about the subject, the section is not
non-normative. Coverage states are defined in
[coverage](../compilation.md#coverage).

Three reasons for marking a section non-normative are always wrong:

- "It states a fact, not a requirement." A stated fact about the subject is an
  obligation; the document states it because the result must match it.
- "It describes content or appearance, not behavior." Content and appearance are
  obligations. See [the subject](#the-subject-is-whatever-the-documents-describe).
- "It is not a requirement on the system." The subject is whatever the documents
  describe, and a part of it is as valid a subject as the whole.

## Granularity

Granularity is a judgment call, not arithmetic. The graph exists to build and verify the
deliverable: record a requirement where a builder needs it to build the right thing and
a tester can check it on its own. Two tests decide where to split:

- Split what can fail independently. "The gateway is a REST service built with Go" can
  fail as not-REST or as not-Go: two requirements, both quoting the same sentence.
- Keep together what one check verifies. "Uploads accept PNG, JPEG, and GIF" is one
  obligation with three values, not three requirements. Splitting it adds nodes, not
  information.

Neither direction is a goal in itself. Never shred one behavior into fragments to raise
the count, and never fuse independent obligations to lower it. When unsure, ask what
the failing test would be called: one honest test name per requirement is the right
density. No count is graded; usefulness is.

The same judgment governs entities: naming a concept does not mint it. See
[what is an entity](../model/entity.md#what-is-an-entity).

## Enumerations

A sentence ending in a colon followed by a list is a claim about the items. The lead-in
sentence alone states nothing the items do not, so requirements anchor on the items:

- An item that states its own testable obligation gets its own requirement, quoting
  that item's line verbatim. An operations list is the common case: each operation is
  separately built and separately tested. E.g.:

```
quote: - `addProduct` - adds a new product to the inventory
statement: The inventory system supports an `addProduct` operation that adds a new product to the inventory.
```

  When a statement names an operation, command, field, or value by a literal
  identifier, the statement keeps that identifier verbatim, backticks included.
  Downstream binding searches code by the statement text, so a paraphrase that drops
  the name ("adds a new product") breaks the link to the code. See
  [the bind goal](../../consumers/bind.md#the-bind-goal).

- Items share one requirement only when they are plain values with no behavior of
  their own (accepted formats, supported locales): quote the lead-in line and carry
  the values in the statement, the same way
  [test cases](#test-cases-state-obligations) carry concrete values. An item with its
  own verb or description is its own requirement.

List items are also where entities hide, under the same rule as everywhere: an item
naming an actor, a component, or a sub-system that statements are about introduces
that concept ("allows 3 roles: Admins, Warehouse Manager, Warehouse Staff" introduces
three actor entities). An item naming an operation does not: `addProduct` states what
the inventory system does, a requirement only, never an entity. A named stored field
stays requirement detail until statements are about the field itself. A sub-system
list ("the sub-systems are: User Management, Inventory Management") ties each listed
sub-system to its parent, and the lead-in's subject is that parent: an entity too,
minted if absent, with each item's requirement referencing both, declaring the pair in
`edges` as a `composition` from the parent to the part, and the part's `parent` set to
the whole. An item that is a link still counts: under "The sub-systems are:", the item
`[User Management](./user.md)` states that the parent includes the User Management
sub-system. The link is navigation; the item's text is a fact.

## Code blocks state obligations

Pseudo code, step lists, and algorithm sketches in fenced code blocks are claims about
the subject, step by step. Each step that states behavior is an obligation: extract one
requirement per step and quote that step's own line verbatim. E.g.:

```
quote: For current line, strip out whitespace before and after
statement: When reading a line of input, the sort utility strips leading and trailing whitespace from it.
```

- A branch inside the steps is its own obligation: "If stripped line is empty string,
  continue to next line" is a requirement of its own, a failure mode when the branch
  handles bad input. See [facets](../model/requirement.md#facets).
- A variable local to the steps (a loop counter, an accumulator array) is requirement
  detail, never an entity. Name it in the statement when the behavior needs it.
- A fenced block is an illustration only when it shows sample data, an outside system's
  output, or a payload format. Then it supports the surrounding sentence instead of
  stating its own obligations.
- A section whose code block states behavior is covered only when each behavioral step
  has a requirement. Marking it covered after extracting one step and skipping the rest
  is a dishonest claim.

A PlantUML block in a source document is a `diagram` section and states obligations the
same way: each element and each arrow is a claim about the subject. See
[diagrams as input](../diagrams.md#diagrams-as-input).

## Test cases state obligations

A documented test case is a behavior contract on the subject under test. Concrete input
with expected output is a triggered obligation: extract it, with the input and the
expected output in the statement. E.g.:

```
quote: Test the following input:
statement: Given the input lines `321`, `654`, `453`, the sort utility outputs `321`, `453`, `654`.
```

- The quote anchors to the case's lead-in line; the concrete values ride in the
  statement.
- The subject is the system under test, never the test file or the suite. A test
  document describes the subject's behavior; its cases reference the same entity the
  main document introduced.
- A test-case section is never non-normative.

## Wording

A statement is judged by what it lets a builder build and a tester check, never by its
grammar. The extractor writes it to be:

- Specific. The concrete values, names, and conditions the sentence states stay in the
  statement. "Handles errors well" states nothing checkable; "returns `404` when the
  order id is unknown" does.
- Testable. A reader can name the check that fails when the obligation is not met. If
  no such check exists, the sentence is background or navigation, not a statement.
- Entity-anchored. The subject appears by its entity's name, and so does every other
  entity the sentence names; the requirement references each of them. A statement whose
  subject is not an entity in the graph is not extractable until that entity exists.
- One obligation. One statement carries one obligation. A sentence carrying several
  becomes several requirements sharing the quote (see [granularity](#granularity)); a
  statement is never a paragraph.
- Literal identifiers. Operations, commands, fields, flags, and values named by an
  identifier keep it verbatim, backticks included.
- Trigger and response. When the sentence has a trigger (an event, a state, a
  condition), the statement names it and the response it obliges, in that order. When
  the response is a state change, the requirement also carries a `transition`; its
  states come from the documents, never invented, and the event that creates the
  subject names its initial state. See
  [transition](../model/requirement.md#transition). E.g.:

```
quote: When payment succeeds, the order becomes paid.
statement: When payment succeeds, the order becomes paid.
transition: {subject: ent:order, from: placed, to: paid, trigger: payment succeeds}
```

- Measured. A quality bound stays in the statement as stated, and the `quality` facet
  records it as `measure` when the bound is a number, duration, count, or rate; a bare
  adverb (promptly, quickly, soon) is the unmeasured case and `measure` stays absent.
  E.g.:

```
quote: Checkout is confirmed in under two seconds.
statement: The shop confirms checkout within 2 seconds.
facets: [{facet: quality, reasoning: a timing bound on checkout, measure: 2 seconds}]
```

- In the documents' terms. The statement uses the vocabulary of the medium it reads
  (a slide, a chapter, a department, a service), not a software vocabulary imposed on
  it.
- Stable. Re-reading an unchanged sentence yields the same statement, so the upsert
  lands on the existing requirement (the natural key is the source section plus the
  punctuation-insensitive statement, see [identity](../model/requirement.md#identity)).
  Rewording an existing requirement is `update_requirement` on its id, never a second
  upsert. A key deleted and recreated across builds is `unstable-extraction`; see
  [flip detection](../reconciler.md#flip-detection).

No syntax is prescribed. "Shall" is neither required nor forbidden; a plain declarative
sentence is the usual form. No gate checks the shape of a statement: the
[validation gates](../graph.md#validation-gates) check that the quote locates, that
the requirement references at least one entity, and that its edges run among the
entities it references. Wording quality is the extractor's judgment, carried by the
`extraction` skill and graded by benchmarking.

## What statements do not express

Statements express obligations. Everything else is a separate node or a derivation:

- Entities are separate nodes that requirements reference. A statement never stands in
  for an entity's `definition`. See [entity](../model/entity.md).
- Relationships derive from the `edges` a requirement declares, never from the
  statement text. A sentence that names two entities without declaring an edge
  contributes no arrow; when the sentence is structural, the `declare-edges` goal asks
  for the edge. See [relationship types](../model/relationship.md#types) and
  [derived data](../graph.md#derived-data).
- State machines derive from `transition`, never from the statement text. See
  [derivation](../model/state-machine.md#derivation).
- Facets are judgments recorded beside the statement, with reasoning, not read from
  its shape. See [facets](../model/requirement.md#facets).
- Which flow or diagram a requirement appears in is view membership: default views
  derive, curated views come from the `curate-view` goal. A statement carries no view.
  See [membership](../model/view.md#membership) and
  [default views](../model/view.md#default-views).
- A contradiction, a duplicate, or an ambiguity in a sentence is a
  [diagnostic](../model/diagnostic.md) filed against the requirement, never a caveat
  written into the statement.
