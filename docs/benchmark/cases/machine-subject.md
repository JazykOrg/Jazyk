# machine-subject

Grades the subject of a transition. A character's stance toward another character is a
state of the character: "Ines's stance toward Theo changes from stranger to
acquaintance" moves Ines Marlow from `stranger` to `acquaintance`, and the derived state
machine is Ines's. The stance is the name of the lifecycle, never an entity of its own,
and the states are state names, never entities. Codex once minted `Ines's stance toward
Theo` and `Theo's stance toward Ines` as entities and hung both state machines on them,
so the characters carried no lifecycle and the state views drew two abstractions no
document names as a thing (see [transition](../../compiler/model/requirement.md#transition)
and [state machines](../../compiler/model/state-machine.md)). The characters are
seeded, so the case grades the transitions and not the minting.

```yaml
name: machine-subject
description: A character's stance changes are transitions on the character, never on a minted stance entity.
par:
  rounds: 8
goal:
  kind: reconcile-section
  target: docs/arcs.md
given:
  docs:
    docs/characters.md: |
      # Characters

      ## Ines Marlow

      Ines Marlow owns the Lantern Room, the bookshop on the quay.

      ## Theo Brandt

      Theo Brandt arrives in Port Alder to survey the harbor wall.
    docs/arcs.md: |
      # Arcs

      Each lead moves through the same four stances toward the other: stranger,
      acquaintance, rival, ally. Both leads start as strangers.

      ## Ines

      Her stance toward Theo changes three times:

      | From | To | Trigger |
      | --- | --- | --- |
      | stranger | acquaintance | Theo takes the upstairs room |
      | acquaintance | rival | Dot tells Ines who Theo works for |
      | rival | ally | Theo offers Ines the crack finding |

      ## Theo

      His stance toward Ines changes three times:

      | From | To | Trigger |
      | --- | --- | --- |
      | stranger | acquaintance | Theo finds the crack beneath the Lantern Room |
      | acquaintance | rival | Ines confronts Theo about the survey |
      | rival | ally | Ines's plan fails at the council |
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
    coverage:
      docs/characters.md#/characters/ines-marlow: covered
      docs/characters.md#/characters/theo-brandt: covered
assert:
  - transitionSubject:
      entity: Ines Marlow
      min: 3
  - transitionSubject:
      entity: Theo Brandt
      min: 3
  - entityAbsent:
      namePattern: 'stance'
  - entityAbsent:
      namePattern: '^(stranger|acquaintance|rival|ally)s?$'
  - coverageSet:
      section: 'docs/arcs.md#/arcs/ines'
      state: covered
  - coverageSet:
      section: 'docs/arcs.md#/arcs/theo'
      state: covered
```
