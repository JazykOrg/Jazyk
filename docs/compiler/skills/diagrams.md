---
name: diagrams
description: Choose and author diagrams that answer specific questions.
---
# Diagram selection

Choose a question before a notation. Reuse an existing diagram when it answers
that question. Match the abstraction level to its reader. Prefer a small diagram
with linked detail to a complete graph dump. Do not draw when prose is clearer,
facts are absent, or an existing diagram suffices. Use sourced semantic facts.

| Kind | When to suggest it to the model | When review becomes mandatory | What the kind-specific prompt asks |
|---|---|---|---|
| `class` | A level of domain concepts has at least 4 children; attributes, associations, inheritance, or composition make the structure worth explaining. | Structural question at 10 children, when no suitable explanation covers it. Component/package/composite may be better choices depending on the facts. | Explain the concepts, relevant attributes, and selected relationships at one abstraction level. Do not use class notation to pretend every actor or service is a data class. |
| `component` | Any stated system needs a boundary/context review; internal levels with services, interfaces, or components also qualify for structural review. | System boundary question at 8 children/interactors, or internal structural question at 10 children. Explicit context requests require context, not merely an internal component picture. | For context, show the subject system and external actors/systems. For container/component, show the stated internal units and their responsibilities/interfaces. Keep perspective explicit and link detail. |
| `package` | At least two existing containment groups have children and relationships crossing their boundaries. | No separate quota. It can answer a mandatory structural question if package boundaries are the question. | Show existing group boundaries and cross-group dependencies. Do not invent architectural layers to make packages look tidy. |
| `composite` | One stated whole has at least two parts with connectors/interactions among them or across its boundary. | No separate quota. It can answer mandatory internal-structure review when connected parts are the issue. | Draw one boundary, its parts, and sourced connectors. Distinguish presentation groups from real containment. |
| `use-case` | An actor participates in a behavior cluster of at least 4 statements, or the source explicitly asks for an actor-goal overview. | The behavior review is mandatory at 12 statements. Use-case is suitable only for actor-goal scope, not sufficient for a required process or interaction question. | Identify the externally meaningful intent and actors from statements. Never turn each sentence into a separate user goal or draw an internal subsystem as an invented actor. |
| `activity` | A behavior cluster has at least 4 statements; failure-mode facets or competing outgoing transitions particularly suggest paths and decisions. | Behavior review at 12 statements; for a process-path finding this is the preferred kind. | Show actions, sourced ordering, alternatives, and recovery. Source every guard; do not invent a branch, join, or concurrency because the picture looks better. |
| `state` | A machine has at least 3 distinct states. A smaller machine can be drawn when explicitly requested. | Lifecycle review at 8 states. A sequence diagram cannot replace it. | Explain one subject's states and allowed transitions, using its actual machine. Cite triggering requirements; expose missing information rather than invent transitions. |
| `sequence` | At least 4 behavior statements involve at least 2 message participants. Source order, causal language, and message edges suggest the candidate. | Behavior review at 12 statements. For a message-order finding, use sequence or a justified communication alternative. | Select one scenario, identify initiator/receiver per step, and source message order and fragments. Add failure paths only where stated. Do not reproduce every statement as a message. |
| `communication` | At least 3 participants interact and understanding their connections is more useful than vertical chronology. | No independent quota. It can answer mandatory interaction review when topology plus numbered order answers the question. | Show who communicates with whom and number the interactions. Prefer sequence when temporal order or nested alternatives dominate; avoid redundant twins. |
| `object` | A type has at least 2 known instances, especially instances with values or links worth comparing. | Instance-example review at 8 instances, with suppression appropriate if the examples are repetitive. | Choose a meaningful concrete snapshot, show instance/type identity and known values/links. Never invent example values or turn types into instances. |
| `deployment` | Any valued placement fact exists in an area. | Placement review at 6 placement facts. A component diagram without placements does not answer it. | Show only artifacts and placements stated by the graph. Do not assume hosts, replicas, networks, or cloud services from conventional architecture. |
| `timing` | A stateful subject also has quality measures that might express time constraints. The model must read their source and confirm temporal relevance. | No numeric rule forces timing solely from the number of quality measures. An explicit timing-diagram request or stated diagram obligation requires review. | Use actual state changes and stated durations, deadlines, or measures. If no meaningful time scale is stated, explain the missing facts and suppress rather than guessing a trace. |
| `overview` | At least 3 authored interaction diagrams explain the same area and could be related phases of one scenario. | No automatic count-based requirement. An explicit scenario-map request requires review; a mandatory readability task can require a linked coarse explanation, with overview as one option. | Verify phases and their order from sources, then link the existing interactions. Do not invent a workflow merely because three diagrams share a parent or title. |

The positive signals for package/composite/communication/timing/overview can
open optional review below the family size threshold. Fold them into the same
area task as other findings. Use stable question keys such as `time-constraints`
and `interaction-map`; put their presence/count tests in `limits.rs` alongside
the family limits. Do not introduce one model call per candidate type. False
positives are expected: a quality bound can concern throughput rather than time,
and three interactions can be unrelated. The model reads evidence and suppresses
that suggestion. Do not add undocumented keywords, regex classifications, or a
hidden model call to the static analyzer.

For behavior clusters, static evidence exposes supported questions: an actor
suggests actor-goal scope, directed edges suggest interaction, failure-mode facets
and transitions suggest process paths. Generic large clusters get a behavior
review without a forced notation. A session may produce both activity and sequence
when they explain distinct questions; it states both purposes and evidence.
The harness never requires all alternatives and the model cannot replace a
specific question with an easier unrelated one.

Explicit human requests for a kind are instructions to execute within their
scope, even below automatic thresholds. A source diagram or a quoted requirement
that explicitly requires a diagram is not waived by an ordinary heuristic
suppression. Fulfill it, or report missing prerequisites/unsupported requirements.
Human reversal of that instruction must be explicit. Extracted diagram
obligations must be represented through the normal source/requirement machinery,
not detected by scanning arbitrary prose during diagram analysis.


## Selection examples

Choose UML notation for the reader's question. C4 context, container and component
are zoom perspectives, not a requirement to create every zoom level. Kruchten's
4+1 questions concern logical structure, runtime processes, development organization,
physical placement and scenarios. They guide selection without adding new kinds.

Good: an activity diagram explains checkout alternatives and a separate sequence
explains participant order. Bad: create both merely because the catalog has both.
Good: suppress a timing suggestion when the measure concerns throughput rather
than time. Bad: invent durations. Good: link sourced interaction phases in an
overview. Bad: infer phase order from matching titles.

## References

- [OMG UML 2.5.1](https://www.omg.org/spec/UML/2.5.1/PDF) defines the structural,
  behavioral and interaction notations. The supported catalog is listed in
  [view kinds](../model/view.md#kinds).
- [C4 diagrams](https://github.com/simonbrowndotje/c4model/blob/master/diagrams/index.md)
  distinguish context, container, component and code zoom levels. Select levels
  that add value rather than treating every level as a required artifact.
- [Kruchten's 4+1 view model](https://arxiv.org/abs/2006.04975) separates logical,
  process, development and physical concerns, connected by scenarios.
