# Criterion

A criterion is the observable evidence that one [requirement](./requirement.md) is met:
a concrete input and its outcome, a boundary, the failure path, a measured threshold.
The requirement states the intent; its criteria say how anyone would know. Tests are the
executable form of criteria ([tests tie criteria to the deliverable](../../consumers/gen.md#tests-tie-criteria-to-the-deliverable)).

E.g., for "An Order must be paid within 21 days of placement, and Orderly cancels it
otherwise":

```
An Order placed on day 0 and unpaid on day 21 is still placed.
An Order placed on day 0 and unpaid on day 22 is cancelled.
An Order paid on day 5 is never cancelled for non-payment.
```

## Fields

- `requirement`: the id of the one requirement this criterion belongs to. A criterion
  has exactly one owner; an example two requirements share splits into one criterion
  per requirement.
- `statement`: one condition and its observable outcome, concrete values kept. Never a
  restatement of the requirement and never an implementation detail.
- `given`, `when`, `then`: optional. The same condition split the way acceptance tests
  are written; `statement` stays the one line that carries it.
- `kind`: `positive` (the requirement holds on the stated input), `negative` (the
  stated handling of the failure path), `boundary` (a value at the edge of a bound),
  `quality` (a measured threshold, with `measure`).
- `measure`: for `quality`, the bound as stated (`under 200 ms`, `2,000 to 3,500 words`).
- `entities`: the entity ids the criterion is about, when they differ from the
  requirement's. Absent means the requirement's.
- `source`: `{doc, section, quote}` for a criterion the documents state: a test case, an
  example with values, a bullet under the rule, or descriptive prose that already
  states the criterion's observable condition and outcome. The criterion may phrase
  that observation as a test while its quote keeps the original wording. Reusing the
  owning requirement's quote is valid when it supports the whole criterion without
  adding an obligation or choosing an unstated value. The quote is verbatim, located
  whitespace-insensitively, and may sit in another section or document than the
  requirement's.
- `provenance`: `{derived: {from, reasoning}}` for a criterion a session wrote because
  the documents state none, `from` naming the requirement, with a ratification proposal
  toward prose like every derived fact ([provenance](../model.md#provenance)); or
  `{decree: {...}}` for a human write. Exactly one of `source` or `provenance`.
- `stale`: set by the store when the owning requirement's statement changed after this
  derived criterion was written, cleared by the next update or re-upsert; what
  [`derive-criteria`](../goals/derive-criteria.md) reads as `revised`.
- `confidence` and `reasoning`; `created` and `updated`.

## Identity

`crit:<stem>-<hash>`, the stem of the owning requirement whatever section the
criterion's own quote sits in, so a requirement's criteria read as one family (a session
once found `crit:expenses-24` between `crit:policies-10` and `-11`); stem `x` only when
the requirement has no document either. The hash is of the requirement id and the
statement ([identifiers](../model.md#identifiers)). The natural key is the owning requirement plus
the punctuation-insensitive `statement`, so a retried upsert lands on the same node and a
reworded sentence refreshes it in place ([mutations](../graph.md#mutations)).

## What a good criterion is

- One condition, one outcome, written from outside the system. "When a submitted Order
  holds an out-of-stock Product, the Shop sends no confirmation email."
- Concrete: the values, the state, the actor. A criterion without a value the documents
  give is a question for the author, filed as an `invented-choice` prompt, never a
  number the session picked.
- Covers what the requirement leaves implicit: the boundary of every bound, the negative
  case of every conditional, the measure of every quality.
- Never restates the requirement or any other requirement of the graph
  (`criterion-restates`): a criterion that reads as another requirement's statement is that
  requirement, not evidence for this one. A local model once gave `req:catalog-3` a
  negative that was `req:catalog-4` word for word. It never names a function, a table,
  or a file: the deliverable's shape is generation's business.
- One per requirement is the norm, the shape of the statement adding the rest (a
  bound's edge, a conditional's negative, a quality's measure); one is the floor
  [bind](../../consumers/bind.md) waits for; none is a `criterion-missing` finding.

The [criteria skill](../skills/criteria.md) carries the method.

## Lifecycle

- Extraction records stated criteria beside the requirements they exercise
  ([reconcile-section](../goals/reconcile-section.md)): a test case's lead-in with its
  values, an example sentence, a threshold bullet. A test-case section is covered when
  its case is a criterion of the requirement under test.
- [`derive-criteria`](../goals/derive-criteria.md) writes the rest for the requirements
  generation will bind, as derived facts with proposals.
- A requirement's statement change marks its derived criteria revised (their evidence
  may no longer fit) and leaves quoted ones to their own quotes; deleting a requirement
  deletes its criteria in [the sweep](../graph.md#the-sweep).
- A `criterion-created` or `criterion-revised` record invalidates the test keyed to it
  ([the ledger](../../consumers/gen.md#the-ledger)).

## Not a criterion

- A requirement restated with a different verb.
- A test artifact: tests are what criteria become, keyed by criterion id.
- The llm test setup file under `gen/criteria/` is a test artifact of that kind, not a
  criterion ([criteria files for llm tests](../../consumers/gen.md#criteria-files-for-llm-tests)).
