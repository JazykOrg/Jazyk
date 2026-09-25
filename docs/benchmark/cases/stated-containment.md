# stated-containment

Grades containment stated in passing. "The Lantern Room is a bookshop on the quay in
Port Alder" says what the Lantern Room is and where it stands: the town holds the
bookshop. That is a whole-part fact, a `composition` edge from Port Alder to the Lantern
Room with the Lantern Room's `parent` set to Port Alder (an `aggregation` passes too),
and the same holds for Marlow House on Chapel Lane. The quay and the lane are locations
named in passing, never entities: the document says everything but the three places is
a street. A novel compile once left nothing tying the Lantern Room or Marlow House to
Port Alder, so the town's level view drew three unrelated boxes (see
[containment](../../compiler/model/entity.md#containment)). The places are seeded, so
the case grades the tie and not the minting.

```yaml
name: stated-containment
description: A place stated to stand in a town is a part of the town, a composition edge with the parent following.
goal:
  kind: reconcile-section
  target: docs/settings.md
given:
  docs:
    docs/characters.md: |
      # Characters

      ## Ines Marlow

      Ines Marlow owns the Lantern Room and lives alone at Marlow House. She has never
      lived anywhere but Port Alder.
    docs/settings.md: |
      # Settings

      Three places matter, and everything else is a street the characters walk along
      between them.

      ## Port Alder

      Port Alder is a harbor town on the west coast with about four thousand residents.

      ## The Lantern Room

      The Lantern Room is a bookshop on the quay in Port Alder.

      ## Marlow House

      Marlow House stands on Chapel Lane in Port Alder, up the hill from the harbor.
  graph:
    entities:
      ent:port-alder:
        name: Port Alder
        definition: The town where Ines Marlow has always lived.
        mentions:
          - {section: docs/characters.md#/characters/ines-marlow, quote: "She has never lived anywhere but Port Alder."}
      ent:lantern-room:
        name: Lantern Room
        aliases: [The Lantern Room]
        definition: The shop Ines Marlow owns.
        mentions:
          - {section: docs/characters.md#/characters/ines-marlow, quote: "Ines Marlow owns the Lantern Room and lives alone at Marlow House."}
      ent:marlow-house:
        name: Marlow House
        definition: The house Ines Marlow lives in.
        mentions:
          - {section: docs/characters.md#/characters/ines-marlow, quote: "Ines Marlow owns the Lantern Room and lives alone at Marlow House."}
    coverage:
      docs/characters.md#/characters/ines-marlow: covered
assert:
  - relationshipExists:
      a: Port Alder
      b: Lantern Room
      types: [composition, aggregation]
  - relationshipExists:
      a: Port Alder
      b: Marlow House
      types: [composition, aggregation]
  - parentIs:
      entity: Lantern Room
      parent: Port Alder
  - parentIs:
      entity: Marlow House
      parent: Port Alder
  - entityNameCount:
      namePattern: 'lantern room'
      max: 1
  - entityAbsent:
      namePattern: 'quay|chapel lane'
  - coverageSet:
      section: 'docs/settings.md#/settings/the-lantern-room'
      state: covered
```
