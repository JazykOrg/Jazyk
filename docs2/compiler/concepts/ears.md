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

## Shape check

The [validation gate](../graph.md#validation-gates) applies a lenient shape check when a
requirement is staged:

- The statement shall be a single testable sentence, not a paragraph.
- The statement should follow one of the patterns above, but rigid template matching is
  not required. A clear "shall" sentence with a recognizable trigger, state, or condition
  passes.
- A statement that bundles several independent obligations is rejected with a repair
  message asking for one requirement per obligation.

## What EARS does not express

EARS expresses obligations, not the concepts themselves. Entities are separate nodes that
requirements reference, and [relationships](../model/relationship.md) are derived from the
`edges` a requirement declares. See [entity](../model/entity.md).
