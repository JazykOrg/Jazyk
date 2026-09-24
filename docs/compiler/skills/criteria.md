# Criteria

How to write acceptance criteria that a test can encode and an author can ratify. The
[derive-criteria](../goals/derive-criteria.md) contract says what to do; this skill says
how. Extraction sessions apply the stated-criteria part.

Stated criteria first. The documents often carry the evidence beside the rule: a test case with input and expected output, an example sentence with values ("An order of three items priced in EUR shows a total in EUR"), a bullet giving a threshold under a rule, a table row. Each is a criterion of the requirement it exercises, recorded with `upsert_criterion` carrying `section` and `quote` (the case's lead-in line, the example sentence, the bullet). It is never a requirement of its own: the rule is the requirement, the case is its evidence. A test-case section is covered when its case is a criterion of the requirement under test.

One condition, one outcome. A criterion states a single situation and what is observed: "An Order placed on day 0 and unpaid on day 22 is cancelled." Two conditions are two criteria. The outcome is what an observer sees (a state, an output, a refusal, a message), never how the system gets there. The observer uses the product the way its user does: a client sends a request and reads the response, an operator runs the command and reads its output, a reader opens the page. Inspecting a specification, a contract, a configuration, or source code is not an observation: "a client examining the integration contract finds REST operations" tests a document, and "a client sending GET /api/products receives a JSON list" tests the product.

Cover the shape of the requirement.
- A bound (`within 21 days`, `at most 3 items`, `under 200 ms`) gets the value at the edge and the value past it: on day 21 still placed, on day 22 cancelled.
- A conditional gets its negative: the handling when the condition fails, or the absence of the effect ("A Product in stock sends the confirmation email; a Product out of stock sends none").
- A `quality` facet gets a `quality` criterion carrying the `measure` as stated.
- A `transition` gets the state before, the trigger, and the state after; a guard gets both sides of the guard.
- A `behavior` gets the trigger and the observable response for one concrete actor and input.

Concrete, from the documents. Values come from the statement, the quote, or a section the requirement's document holds. A value the documents do not give is not yours to pick: file `report_diagnostic` rule `invented-choice` with a prompt naming the requirement and the options, mark the requirement `open-question`, and write no criterion that needs the value.

Never restate. A criterion whose normalized text equals the statement, or the statement of any other requirement, is refused (`criterion-restates`): another requirement's sentence is that requirement, not evidence for this one. "The Shop sends a confirmation email when an Order is submitted" is the requirement; "A Customer submitting an Order for one in-stock Product receives one confirmation email" is a criterion.

Verdicts. `complete`: the criteria cover the requirement's shape, derived or stated. `stated-only`: the documents' own criteria cover it and you added none. `open-question`: a value nobody stated keeps a criterion from being written, and the question stands as a prompt.

Density. One per requirement is the norm: the situation the statement describes, with concrete values. The shape adds the rest and nothing else: a bound adds the edge and the value past it, a conditional adds its negative, a quality adds its measure, a transition adds its guard's other side. A plain fact or a structural statement ("The company has four departments", "Backend is a REST API") takes one criterion and no negative: a contrived negative ("The Boulder Warehouse Crew is not a department") is a test that repeats the positive. Do not pad: a criterion that adds no situation the others lack is noise a test will repeat. An org handbook once carried 251 criteria for 126 requirements, most of them such pairs.

Wording. Present tense, the actor first when there is one, the values in the sentence, backticks around literal identifiers and values that must match verbatim. Optional `given`, `when`, `then` split the same sentence for a test writer; `statement` stays the one line.

Derived criteria are proposals. Each carries `provenance: {derived: {from: [the requirement id], reasoning}}` and reads as the sentence the documents should gain under the requirement's section, so the ratification proposal can be accepted as prose. Say in `reasoning` which part of the statement the criterion covers (the bound, the negative, the measure).
