# narrative-behavior

A narrated scene and a standing character fact are atomic obligations. The seeded
characters keep this case focused on extraction completeness and source coverage.

```yaml
name: narrative-behavior
description: Narrated scenes and standing facts are recorded without classification.
goal:
  kind: reconcile-section
  target: docs/plot.md
given:
  docs:
    docs/characters.md: |
      # Characters

      ## Ines Marlow

      Ines Marlow owns the Lantern Room, the bookshop on the quay.

      ## Theo Brandt

      Theo Brandt arrives in Port Alder to survey the harbor wall.

      ## Dot

      Dot runs the ferry office.
    docs/plot.md: |
      # Plot

      Theo Brandt is a marine surveyor.

      ## Chapter 3: The ledger

      - Dot tells Ines who Theo works for.
      - Ines asks Dot for the harbor ledger.
      - Ines finds the Marlow lease in the harbor ledger.

      ## Chapter 4: The quay

      - Ines confronts Theo on the quay about the survey.
      - Theo refuses to show Ines the report.
  graph:
    entities:
      ent:ines:
        name: Ines Marlow
        aliases: [Ines]
        definition: The bookseller who owns the Lantern Room.
        stereotype: character
        mentions:
          - {section: docs/characters.md#/characters/ines-marlow, quote: "Ines Marlow owns the Lantern Room, the bookshop on the quay."}
      ent:theo:
        name: Theo Brandt
        aliases: [Theo]
        definition: The surveyor who comes to Port Alder to survey the harbor wall.
        stereotype: character
        mentions:
          - {section: docs/characters.md#/characters/theo-brandt, quote: "Theo Brandt arrives in Port Alder to survey the harbor wall."}
      ent:dot:
        name: Dot
        definition: The keeper of the ferry office.
        stereotype: character
        mentions:
          - {section: docs/characters.md#/characters/dot, quote: "Dot runs the ferry office."}
    coverage:
      docs/characters.md#/characters/ines-marlow: covered
      docs/characters.md#/characters/theo-brandt: covered
      docs/characters.md#/characters/dot: covered
assert:
  - requirementExists: {statementPattern: 'works for', entity: Dot}
  - requirementExists: {statementPattern: 'asks Dot|ask.*harbor ledger', entity: Ines}
  - requirementExists: {statementPattern: 'lease', entity: Ines}
  - requirementExists: {statementPattern: 'confront', entity: Ines}
  - requirementExists: {statementPattern: 'refus', entity: Theo}
  - requirementExists: {statementPattern: 'marine surveyor', entity: Theo}
  - coverageSet:
      section: 'docs/plot.md#/plot/chapter-3-the-ledger'
      state: covered
```
