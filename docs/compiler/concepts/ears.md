# EARS

EARS (Easy Approach to Requirements Syntax) is the sentence syntax for
[requirements](../model/requirement.md). It is a small set of patterns that keep a
statement specific and testable while staying close to natural language. EARS covers both
behaviors and constraints, so the model does not need a separate requirement taxonomy.
The pattern itself signals the kind.

## Patterns

- Ubiquitous: "The system shall `<response>`."
  E.g. "The system shall ensure each `User` email is unique."
- Event-driven: "When `<trigger>`, the system shall `<response>`."
  E.g. "When the customer checks out, the system shall empty the `Shopping Cart`."
- State-driven: "While `<state>`, the system shall `<response>`."
- Unwanted behavior: "If `<condition>`, then the system shall `<response>`."
- Optional feature: "Where `<feature>`, the system shall `<response>`."
- Complex: a combination of the above.
  E.g. "While `<state>`, when `<trigger>`, the system shall `<response>`."

A requirement stores the statement in its `ears` field. The behavior-vs-constraint
distinction is a derived facet of the pattern, not a stored field. Ubiquitous statements
typically state constraints; triggered, stated, and conditioned patterns describe behavior.

"The system" in these patterns is a placeholder. The real subject is whatever the
documents describe; see [the subject](#the-subject-is-whatever-the-documents-describe).

## The subject is whatever the documents describe

Jazyk does not assume the documents describe software. The subject of a statement is the
thing the documents are about and its parts: a service, a slide deck, a book, a course, a
schematic, a contract. Substitute that subject for "the system" in every pattern above.
E.g.:

```
quote: This slide shows a headline title `Jazyk` as well as a link to the site.
ears:  The Introduction slide shall show a headline title `Jazyk`.
ears:  The Introduction slide shall show a link to the site.
```

The consequence is that a document describing an artifact's content, structure, or
appearance is stating requirements on that artifact, not background. Three kinds are
missed most often:

- Content. What an artifact says, shows, or contains is an obligation on it. "This slide
  defines what Jazyk is about in a couple of sentences" yields "The About slide shall
  define what Jazyk is about in a couple of sentences."
- Appearance and material facts. A stated value, color, font, size, measurement, or
  wording is an obligation on the thing it describes. "The primary color is #248555"
  yields "The slides shall use #248555 as the primary color." A stated fact is never
  "just a fact"; the document states it because the artifact must match it.
- Format and medium. "All slides are under the Microsoft PowerPoint file format" yields
  "The slides shall be delivered in the Microsoft PowerPoint file format."

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

## Declarative prose states obligations

Documentation rarely says "shall". A declarative statement about the subject states an
obligation all the same. The test per sentence: does it say what the subject or one of
its parts is, does, contains, shows, uses, allows, requires, or limits? If yes, it is a
requirement.
The turn rephrases it into EARS form for the `ears` field and keeps the source sentence
verbatim in the `quote`. E.g.:

```
quote: The store mints every id at node creation. Ids are immutable.
ears:  The store shall mint every id at node creation and never change it.
```

Statements of composition and technology choice are obligations too, and one sentence
often carries several that can fail independently. Record one requirement per
obligation, all sharing the same verbatim quote (see [granularity](#granularity)).
E.g.:

```
quote: The gateway is a REST service built with Go.
ears:  The gateway shall be a REST service.
ears:  The gateway shall be built with Go.
```

Access and permission rules are obligations, the most commonly missed kind. A sentence
saying who may do something limits the system. E.g.:

```
quote: All management operations can be performed by Admins only.
ears:  The user management system shall allow only Admins to perform management operations.
```

The `ears` subject is the sentence's own subject, and the requirement references that
entity. A pronoun subject ("This", "It") resolves to the system the document already
introduced: "This is a script written in javascript" is an obligation on that system,
referencing its existing entity, never a new `Script` entity minted from the predicate
noun. Never substitute a broader system for a named part: "The inventory system
manages products" is a requirement on the inventory system, not on the application
that contains it. The requirement also references every other entity the statement
names, which is what ties those concepts into the graph. The subject is always an
entity: mint it when search finds nothing that means the same concept. Anything else
a statement merely mentions (a field, threshold, or value) stays requirement detail,
carried in the `ears` text, until statements are about it directly. See
[what is an entity](../model/entity.md#what-is-an-entity).

Non-normative is the exception, not the default. A section is non-normative only when
no sentence in it passes the test above: navigation pages that only link elsewhere,
glossaries defining outside-world terms, changelogs, roadmap wish lists. A glossary
entry that states what a part does is a requirement wearing a glossary's
clothes; extract it. Lists of operations, properties, or rules are never non-normative;
see [enumerations](#enumerations). Before marking a section non-normative, re-read it
sentence by sentence; if any sentence is about the subject, the section is not
non-normative.

Three reasons for marking a section non-normative are always wrong:

- "It states a fact, not a requirement." A stated fact about the subject is an
  obligation; the document states it because the result must match it.
- "It describes content or appearance, not behavior." Content and appearance are
  obligations. See [the subject](#the-subject-is-whatever-the-documents-describe).
- "It is not a requirement on the system." The subject is whatever the documents
  describe, and a part of it is as valid a subject as the whole.

## Granularity

Granularity is a judgment call, not arithmetic. The graph exists to build and verify
quality software: record a requirement where a builder needs it to build the right
thing and a tester can check it on its own. Two tests decide where to split:

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
ears:  The inventory system shall support an `addProduct` operation that adds a new product to the inventory.
```

- Items share one requirement only when they are plain values with no behavior of
  their own (accepted formats, supported locales): quote the lead-in line and carry
  the values in the `ears` text, the same way
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
minted if absent, with each item's requirement referencing both and declaring the
pair in `edges`. An item that is a link still counts: under "The sub-systems are:",
the item "[User Management](./user.md)" states that the parent includes the User
Management sub-system. The link is navigation; the item's text is a fact.

## Code blocks state obligations

Pseudo code, step lists, and algorithm sketches in fenced code blocks are claims about
the system, step by step. Each step that states behavior is an obligation: extract one
requirement per step and quote that step's own line verbatim. E.g.:

```
quote: For current line, strip out whitespace before and after
ears:  When reading a line of input, the sort utility shall strip leading and trailing whitespace from it.
```

- A branch inside the steps is its own obligation: "If stripped line is empty string,
  continue to next line" is an unwanted-behavior requirement.
- A variable local to the steps (a loop counter, an accumulator array) is requirement
  detail, never an entity. Name it in the `ears` text when the behavior needs it.
- A fenced block is an illustration only when it shows sample data, an outside system's
  output, or a payload format. Then it supports the surrounding sentence instead of
  stating its own obligations.
- A section whose code block states behavior is covered only when each behavioral step
  has a requirement. Marking it covered after extracting one step and skipping the rest
  is a dishonest claim.

## Test cases state obligations

A documented test case is a behavior contract on the system under test. Concrete input
with expected output is an event-driven obligation: extract it, with the input and the
expected output in the `ears` text. E.g.:

```
quote: Test the following input:
ears:  When given the input lines `321`, `654`, `453`, the sort utility shall output `321`, `453`, `654`.
```

- The quote anchors to the case's lead-in line; the concrete values ride in the `ears`
  statement.
- The subject is the system under test, never the test file or the suite. A test
  document describes the system's behavior; its cases reference the same entity the
  main document introduced.
- A test-case section is never non-normative.

## Shape check

The [validation gate](../graph.md#validation-gates) applies a lenient shape check when a
requirement is staged:

- The statement shall be a single testable sentence, not a paragraph.
- The statement should follow one of the patterns above, but rigid template matching is
  not required. A clear "shall" sentence with a recognizable trigger, state, or condition
  passes.
- A statement that bundles several independent obligations is rejected with a repair
  message asking for one requirement per obligation. A technology list is the common
  case: "shall be built with React and TypeScript" is two requirements, one per
  technology, both quoting the same source sentence.

## What EARS does not express

EARS expresses obligations, not the concepts themselves. Entities are separate nodes that
requirements reference, and [relationships](../model/relationship.md) are derived from the
`edges` a requirement declares. See [entity](../model/entity.md).
