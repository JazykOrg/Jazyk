# A repeated call whose answer changed

A `record_binding` re-sent after its refused test file was fixed landed marked
unchanged, and a third would have been refused as a repeat. A repeated call whose answer
changed is no repeat and starts the count again: here a requirement refused for an
entity that did not exist yet, re-sent after the entity is staged, lands unmarked, and
a third identical call is not refused.

```yaml
name: repeat-changed
description: An identical call re-sent after the state changed is no repeat and starts the count again
goal: {kind: reconcile-section, target: docs/plot.md}
given:
  docs:
    docs/plot.md: |
      # Plot

      ## Chapter 1

      Theo Brandt arrives in Port Alder on the afternoon ferry.
steps:
  - call: upsert_requirement
    args:
      statement: Theo Brandt arrives in Port Alder on the afternoon ferry.
      entities: [ent:theo-brandt]
      section: docs/plot.md#/plot/chapter-1
      quote: Theo Brandt arrives in Port Alder on the afternoon ferry.
      facets: [{facet: behavior, reasoning: the character acts on the page}]
    expect: {ok: false}
  - call: upsert_entity
    args:
      name: Theo Brandt
      definition: A marine surveyor who arrives by ferry.
      mention: {section: docs/plot.md#/plot/chapter-1, quote: "Theo Brandt arrives in Port Alder on the afternoon ferry."}
    expect: {contains: '"ent:theo-brandt"'}
  - call: upsert_requirement
    args:
      statement: Theo Brandt arrives in Port Alder on the afternoon ferry.
      entities: [ent:theo-brandt]
      section: docs/plot.md#/plot/chapter-1
      quote: Theo Brandt arrives in Port Alder on the afternoon ferry.
      facets: [{facet: behavior, reasoning: the character acts on the page}]
    expect: {absent: 'already made this exact call'}
  - call: upsert_requirement
    args:
      statement: Theo Brandt arrives in Port Alder on the afternoon ferry.
      entities: [ent:theo-brandt]
      section: docs/plot.md#/plot/chapter-1
      quote: Theo Brandt arrives in Port Alder on the afternoon ferry.
      facets: [{facet: behavior, reasoning: the character acts on the page}]
assert:
  - entityCount: {max: 0}
```
