# turn-navigation

Grades restraint on documents that state nothing about the subject. A glossary that
defines outside-world terms and a roadmap of wishes carry no obligations: the correct
turn marks both sections `non-normative` and mints nothing. The two classic failures
this case catches, both observed in real runs: the document itself becomes an entity
(`ent:glossary`), or a glossary term gets extracted as a requirement. See
[the reconcile-section goal](../../compiler/goals/reconcile-section.md).

```yaml
name: turn-navigation
description: Mark a glossary and a roadmap non-normative without minting entities or requirements.
tier: extraction
par:
  rounds: 3
task:
  type: reconcile-doc
  target: docs/glossary.md
given:
  docs:
    docs/glossary.md: |
      # Glossary

      SKU: a stock keeping unit, the industry term for one sellable variant.

      HTTP: the transfer protocol web services speak.

      ## Roadmap wishes

      Someday the team would like to explore mobile apps and voice ordering.
  graph:
    entities: {}
    requirements: {}
assert:
  - entityCount:
      max: 0
  - requirementCount:
      max: 0
  - coverageSet:
      section: 'docs/glossary.md#/glossary'
      state: non-normative
  - coverageSet:
      section: 'docs/glossary.md#/glossary/roadmap-wishes'
      state: non-normative
```
