# Criteria

How to write acceptance criteria that a test can encode and an author can ratify. The
[derive-criteria](../goals/derive-criteria.md) contract says what to do; this skill says
how. Extraction sessions apply the stated-criteria part.

Stated criteria first. The documents often carry the evidence beside the rule: a test case with input and expected output, an example sentence with values ("An order of three items priced in EUR shows a total in EUR"), a bullet giving a threshold under a rule, a table row. Each is a criterion of the requirement it exercises, recorded with `upsert_criterion` carrying `section` and `quote` (the case's lead-in line, the example sentence, the bullet). It is never a requirement of its own: the rule is the requirement, the case is its evidence. A test-case section is covered when its case is a criterion of the requirement under test.

Link existing prose before deriving. Read the requirement's source section and nearby
sections. Descriptive text can state a testable outcome without being labeled an
example or an acceptance test. Use `section` and `quote` when the passage supports
the whole criterion, even when the criterion uses observer wording and the quote
does not. The requirement and its criterion may share a quote. For a title described
as "Jazyk" with pronunciation in a smaller, greyed-out font, link that description
to the criterion that a visitor sees those details. Do not propose the same details
as a new paragraph. Keep the quote verbatim; do not add an unstated trigger, value,
negative case, or boundary result. For an existing pending criterion, use
`update_criterion` with its id, section, and quote to link it without duplicating it.

One condition, one outcome. A criterion states a single situation and what is observed: "An Order placed on day 0 and unpaid on day 22 is cancelled." Two conditions are two criteria. The outcome is what an observer sees (a state, an output, a refusal, a message), never how the system gets there. The observer uses the product the way its user does: a client sends a request and reads the response, an operator runs the command and reads its output, a reader opens the page. Inspecting a specification, a contract, a configuration, or source code is not an observation: "a client examining the integration contract finds REST operations" tests a document, and "a client sending GET /api/products receives a JSON list" tests the product. A technology or construction constraint ("built using Node.js and Express", "stores its data in PostgreSQL") is observed in the build, never in what the product says about itself: the criterion names the evidence the build carries (the dependency manifest lists `express` and the server module imports it; a record written survives a restart against the PostgreSQL database), since a value the product reports ("a health check answers framework Express") is text any implementation can print. The same holds for the other statements about how the product is built rather than what it does. A structural statement ("User Management is a sub-system of the backend") is observed in the build's structure (the backend's source holds a `user-management` module and its server mounts it), never in a listing the product prints. A step of a procedure that names internal state ("Initialize an array of strings called lines") is observed in the code (the implementation keeps the lines in an array before sorting), never through an outside effect invented to stand for it ("the CLI outputs `321` as a collected line value"): the step stays an obligation, and its evidence is where the step lives.

Cover the shape of the requirement. Most requirements imply a limit or a condition, and a criterion set with no failure or edge case leaves the limit untested: sixty criteria of one build carried two negatives and no edge.
- A bound (`within 21 days`, `at most 3 items`, `under 200 ms`) gets the value at the edge and the value past it: on day 21 still placed, on day 22 cancelled.
- A conditional gets its negative: the handling when the condition fails, or the absence of the effect ("A Product in stock sends the confirmation email; a Product out of stock sends none").
- A `quality` facet gets a `quality` criterion carrying the `measure` as stated.
- A `transition` gets the state before, the trigger, and the state after; a guard gets both sides of the guard.
- A `behavior` gets the trigger and the observable response for one concrete actor and input.
- Grouping and order get the complete ordered groups as the observed outcome. For three quoted paragraphs, compare the three paragraph texts in order, allowing only irrelevant whitespace differences. Three elements plus separate sentence matches would still accept the wrong grouping. Do not add grouping the documents do not prescribe.

Concrete, from the documents. Values come from the statement, the quote, or a section the requirement's document holds. Every criterion of a behavior names at least one value: an input, a count, a name, a state, a message. A criterion with no value ("A consumer of the Backend sees the Backend exposed as a REST API") is the statement reworded, and a test cannot encode it. A value the documents do not give is not yours to pick: file `report_diagnostic` rule `invented-choice` with a prompt naming the requirement and the options, mark the requirement `open-question`, and write no criterion that needs the value. A prompt option carries exactly one of `edit` (the sentence the document gains) or `answer` (the reply in words), never both: an option that edits the document already answers.

A stated example that contradicts its rule is a contradiction, never a typo to correct quietly. When a test case or an example gives an outcome the rule does not produce (the rule sorts, and the case expects `321`, `654`, `453` back unsorted), record the stated criterion as quoted, file `report_diagnostic` rule `contradiction` with both subjects (the rule's requirement and the stated criterion) and a prompt whose options edit the example or the rule, mark the requirement `open-question`, and derive no criterion that gives the corrected outcome: two criteria that answer the same input differently are the contradiction written twice, and the `criteria-conflict` check reports them.

Never restate. A criterion whose normalized text equals the statement, or the statement of any other requirement, is refused (`criterion-restates`): another requirement's sentence is that requirement, not evidence for this one. "The Shop sends a confirmation email when an Order is submitted" is the requirement; "A Customer submitting an Order for one in-stock Product receives one confirmation email" is a criterion.

Verdicts. `complete`: the criteria cover the requirement's shape, derived or stated. `stated-only`: the documents' own criteria cover it and you added none. `open-question`: a value nobody stated keeps a criterion from being written, and the question stands as a prompt.

Density. One per requirement is the norm: the situation the statement describes, with concrete values. The shape adds the rest and nothing else: a bound adds the edge and the value past it, a conditional adds its negative, a quality adds its measure, a transition adds its guard's other side. A plain fact or a structural statement ("The company has four departments", "Backend is a REST API") takes one criterion and no negative: a contrived negative ("The Boulder Warehouse Crew is not a department") is a test that repeats the positive. Do not pad: a criterion that adds no situation the others lack is noise a test will repeat. An org handbook once carried 251 criteria for 126 requirements, most of them such pairs.

Wording. Present tense, the actor first when there is one, the values in the sentence, backticks around literal identifiers and values that must match verbatim. Optional `given`, `when`, `then` split the same sentence for a test writer; `statement` stays the one line.

Derived criteria are proposals. Each carries `provenance: {derived: {from: [the requirement id], reasoning}}` and reads as the sentence the documents should gain under the requirement's section, so the ratification proposal can be accepted as prose. Say in `reasoning` which part of the statement the criterion covers (the bound, the negative, the measure).

Before adding a criterion, read the requirement's existing criteria and pending proposals.
Reuse or update a criterion covering the same condition and outcome. Combine checks
that observe one outcome into one criterion, rather than proposing each configuration
key or source reference separately. Keep independent outcomes separate. Do not derive
an operational failure from a missing configuration value unless the requirement states
that failure. Explain the proposed obligation in words an author can understand without
opening the graph. The engine groups related criteria for one ratification decision.
