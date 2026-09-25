# narrative-behavior

Grades the facet a narrated scene carries. A plot line naming who acts and what they do
is a `behavior` of the acting character: something the subject does, at a point in the
story. A standing fact about a character ("Theo Brandt is a marine surveyor") is a
`constraint`. Flow views draw from `behavior` statements, so a scene labeled
`constraint` never reaches a flow view. A novel compile once labeled 177 statements
`constraint` against 75 `behavior`, and the plot's scenes drew no flow (see
[facets](../../compiler/model/requirement.md#facets)). The characters are seeded, so the
case grades the facets and not the minting.

```yaml
name: narrative-behavior
description: A narrated scene carries the behavior facet of its acting character, and a standing character fact the constraint facet.
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
  - facetCount:
      facet: behavior
      statementPattern: 'works for'
      min: 1
  - facetCount:
      facet: behavior
      statementPattern: 'asks Dot|ask.*harbor ledger'
      min: 1
  - facetCount:
      facet: behavior
      statementPattern: 'lease'
      min: 1
  - facetCount:
      facet: behavior
      statementPattern: 'confront'
      min: 1
  - facetCount:
      facet: behavior
      statementPattern: 'refus'
      min: 1
  - facetCount:
      facet: constraint
      statementPattern: 'marine surveyor'
      min: 1
  - facetCount:
      facet: behavior
      statementPattern: 'marine surveyor'
      max: 0
  - coverageSet:
      section: 'docs/plot.md#/plot/chapter-3-the-ledger'
      state: covered
```
