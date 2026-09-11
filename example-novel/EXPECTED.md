# Novel fixture expected outcomes (hand-labeled, not part of the docs glob)

Counts are indicative, not graded (owner ruling 2026-08-14): a richer graph is acceptable
when every extra requirement is independently testable and every extra entity has
statements directly about it. The graded parts are the traps, the derived data (edges,
state machines, instances, flow views), and what the diagrams show.

The medium is a novel bible. Requirements are narrative obligations on the manuscript
and its parts; entities are characters, settings, the family, and the manuscript's own
structure. Nothing here is software and no entity is a system.

## Core entities (must exist)

| entity | stereotype | parent | attributes the prose gives |
| --- | --- | --- | --- |
| Manuscript | manuscript (or deliverable) | none | none |
| Chapter | chapter | Manuscript | none required (`title` acceptable) |
| Scene | scene | Chapter | `point of view`, `setting`, `goal`, `outcome` (typed, no value) |
| Ines Marlow | character (`actor` acceptable) | Marlow family | `age: 31`, `occupation: bookseller`; alias `Nessa` |
| Theo Brandt | character | none | `age` (see trap 2), `occupation: marine surveyor` |
| Callum Pryce | character | none | `age: 33`, `occupation: project lead` (at Pryce Maritime) |
| Dot Feeney | character | none | `age: 58`, `occupation: ferry office manager` |
| Marlow family | family | none | none |
| Port Alder | town (setting) | none | `population` acceptable, not required |
| The Lantern Room | bookshop (setting) | Port Alder | none required |
| Marlow House | house (setting) | Port Alder | none required |
| The notebook scene | scene (instance) | none required | `point of view: Ines`, `setting: the upstairs room of the Lantern Room`, `goal`, `outcome` (valued) |
| Harbor ledger | prop (or ledger) | none | none |
| Harbor wall | structure (setting detail) | Port Alder acceptable | none |

Stereotype labels are judgment and are not graded beyond being sensible for the medium.
Labeling the four characters `actor` is acceptable and makes the use-case and sequence
views draw them as actors; `character` is the expected judgment, and the flow clustering
then keys on the first listed entity, so every plot and arc requirement lists the acting
character first.

Borderline (acceptable, not required): Pryce Maritime, harbor council, ferry office,
the ferry, the Marlow lease, Theo's field notebook, the harbor crew, the takings ledger,
the eight chapters as individual entities under Manuscript (`Chapter 1: Arrival` ...).

Must NOT exist: `Nessa` (trap 1); the chapter file names and the `manuscript/`
directory (trap 6); `Sunday`, `close third person`, `past tense`, word counts, years,
the lantern (requirement detail on the Lantern Room); a `System` or `Novel` container
invented to hold everything (Manuscript is the deliverable and the only whole the docs
state above the chapters).

## Requirements

At least 60 statements across the six documents. Every one of the 23 plot scene lines is
its own requirement quoting its own list item; every one of the 9 arc transitions is its
own requirement (8 table rows plus the midpoint sentence). The scene lines carry a
`behavior` facet; the plan failure in chapter 5 carries `failure-mode`; the chapter
length bound carries `constraint` (or `quality` with measure `2,000 to 3,500 words`).

## Edges (the prose states these; types as listed)

- Manuscript → Chapter `composition` (cardinality `1..*` or `8`); Chapter → Scene
  `composition` `1..*`.
- Marlow family → Ines Marlow `composition` (Ines's `parent` follows it).
- Port Alder → The Lantern Room `composition`; Port Alder → Marlow House `composition`.
- Marlow family → The Lantern Room `association` or `dependency` (holds the lease).
- The notebook scene → Scene `instantiation`.
- Who knows whom (characters.md): Ines and Callum `association`; Dot and Ines
  `association`; Callum → Theo `dependency` (hires); Dot and Theo `association`.
- Plot scenes with two characters carry an edge from the actor to the other: Dot →
  Ines (tells Nessa), Ines → Dot (asks for the ledger), Ines → Theo (confronts; asks
  him to stay), Theo → Ines (refuses the report; offers the crack finding), Callum →
  Theo (leave out the crack), Callum → Ines (asks Nessa to drop the petition).
  `dependency` or `association`; the direction is the graded part.

Every edge above comes from a requirement; no arrow appears without a statement.

## Transitions and state machines

Two machines, one per lead, subjects `ent:ines-marlow` and `ent:theo-brandt`. States
(both): `stranger`, `acquaintance`, `rival`, `ally`, `beloved`. Initial (both):
`stranger`.

`sm:ines-marlow`, four transitions:
- stranger → acquaintance, trigger `Theo takes the upstairs room`
- acquaintance → rival, `Dot tells Ines who Theo works for`
- rival → ally, `Theo offers Ines the crack finding`
- ally → beloved, `Theo shores up the wall through the storm`

`sm:theo-brandt`, five transitions:
- stranger → acquaintance, `Theo finds the crack beneath the Lantern Room`
- acquaintance → rival, `Ines confronts Theo about the survey` (the table)
- acquaintance → ally, `Ines confronts Theo about the survey` (the midpoint paragraph)
- rival → ally, `Ines's plan fails at the council`
- ally → beloved, `Ines asks Theo to stay`

Checks: exactly one `nondeterministic-transition` (warning) on Theo's two
`acquaintance` transitions (trap 3). `dead-end-state` (info) on `beloved` for both
machines: the docs say beloved is where the book leaves them, so this is the
acknowledged final state, not a gap. No `unreachable-state`. `unhandled-event` (info)
fires with many (state, trigger) pairs and is not graded. The premise in README.md names
no stance, so nothing adds states such as `lovers` to either machine.

## Instances and conformance

One instance: the notebook scene (plot.md "A worked scene"), `instantiation` to Scene.
Its four attribute names (`point of view`, `setting`, `goal`, `outcome`) all exist on
Scene, so the mechanical check files no `nonconformant-instance`, and `conform-instance`
finds the values sensible (the point of view is a character, the setting is a place from
settings.md). Default `view:object/scene` with the notebook scene as its one member. The
worked scene is not merged into Scene and does not become a second Scene type.

## Flow clusters and default views

Clusters keyed (actor, document), members in document order:
- `ines-marlow-plot`: 11 scenes (ch1 s3; ch3 s2, s3, s4; ch4 s1; ch5 s2, s3; ch6 s3;
  ch7 s1, s2; ch8 s2). Under the 12 soft limit, so no `split-view`.
- `theo-brandt-plot`: 6 (ch1 s1; ch2 s1; ch4 s2; ch5 s4; ch6 s2; ch8 s1).
- `callum-pryce-plot`: 3 (ch2 s2; ch5 s1; ch7 s3).
- `dot-feeney-plot`: 3 (ch1 s2; ch3 s1; ch6 s1).
- `ines-marlow-arcs`: 4 and `theo-brandt-arcs`: 5, when the transitions carry
  `behavior` (the usual judgment).

Default views: `view:usecase/<cluster>` titled `<character>: Plot` (and `: Arcs`), and
`view:sequence/<cluster>` over the members that carry an edge (Ines, Theo, Callum, and
Dot in plot.md each have at least one). `view:state/ines-marlow`,
`view:state/theo-brandt`. `view:object/scene`. One `view:class/<scope>` per scope (one
scope expected; its name is not graded). `view:component/<root>` for each containment
root with children (Port Alder, Marlow family, Manuscript) derives mechanically and is
not graded.

## Planted traps

1. Cross-doc identity: plot.md calls Ines `Nessa` twice (ch3 "Dot tells Nessa", ch5
   "Callum asks Nessa"); characters.md explains the nickname. ONE entity
   `ent:ines-marlow` with `Nessa` in `aliases` and mentions in README.md,
   characters.md, settings.md, plot.md, and arcs.md; both Nessa scenes reference it. No
   `ent:nessa`. Secondary identity: The Lantern Room is defined in settings.md and used
   in every other document; one entity with mentions in at least 4 documents.
2. Contradiction: characters.md says Theo is 34; arcs.md says he is 36 when he arrives.
   Exactly one `contradiction` diagnostic with `ent:theo-brandt` among the subjects.
   Ines is 31 in both characters.md and arcs.md: no diagnostic.
3. Nondeterministic transition: arcs.md's Theo table sends acquaintance → rival on
   "Ines confronts Theo about the survey"; the midpoint paragraph sends acquaintance →
   ally on the same trigger. Both requirements exist with their transitions, and the
   state machine check files `nondeterministic-transition` on the pair. A session may
   also file `contradiction` on the pair; dropping either transition to silence the
   check is wrong (the fix belongs in the prose).
4. Non-normative hiding a rule: settings.md "Research notes" reads as outside-world
   background but contains "A written survey report takes three days to produce, so
   Theo's report cannot exist before chapter 3." Marking the section non-normative
   must trigger `suspicious-non-normative`; extracting the rule and marking the section
   covered is also correct.
5. Genuinely non-normative: style.md "Inspirations" states no obligation and is marked
   non-normative without a `suspicious-non-normative` finding. README.md "What is
   where" is navigation and may be marked the same way.
6. Junk bait: README.md lists the chapter files (`manuscript/01-arrival.md` ...) and
   the `manuscript/` directory. None of these become entities. Chapters as concepts
   (Chapter, or the eight numbered chapters under Manuscript) are legitimate.
7. Lookalike pairs that stay apart: `Marlow House` (a place), `the Marlow family` (a
   family), `Ines Marlow` (a person); and `the harbor ledger` (in the ferry office safe)
   versus `the takings ledger` (under the shop counter). settings.md says the two
   ledgers are never confused; merging any of these pairs is a failure.
8. Instance versus type: the worked scene in plot.md is an instance of Scene, not a
   refresh of Scene's attributes with values, and the 23 scene lines of the plot are
   requirements, never one entity per line.

## What the diagrams show

- Class (`view:class/<scope>`): the four characters with `age` and `occupation`, Scene
  with its four typed attributes, Chapter, Manuscript, the Marlow family, Port Alder,
  the Lantern Room, Marlow House, the harbor ledger, the harbor wall. Arrows:
  compositions Manuscript to Chapter to Scene, Marlow family to Ines, Port Alder to the
  Lantern Room and Marlow House; associations among the characters as stated. The
  notebook scene is absent (instances are excluded from the class query).
- Object (`view:object/scene`): one object `the notebook scene : Scene` with the four
  values.
- State (`view:state/ines-marlow`): `[*] --> stranger`, four arrows up to `beloved`.
  State (`view:state/theo-brandt`): `[*] --> stranger`, five arrows; two leave
  `acquaintance` with the same label, which is the visible nondeterminism.
- Use case (`view:usecase/ines-marlow-plot` and the others): one use case per cluster
  titled `Ines Marlow: Plot`; with characters labeled `actor` the acting character
  connects to it.
- Sequence (`view:sequence/ines-marlow-plot` and the others): messages between
  characters in chapter order, e.g. Ines → Dot "asks for the harbor ledger", Ines →
  Theo "confronts about the survey", Ines → Theo "asks Theo to stay".
- Activity: no default; a curated activity twin of the Ines plot view shows the
  chapter 5 failure as the branch (the lease page cut out, the council votes to
  proceed).
