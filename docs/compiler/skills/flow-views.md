A flow view is an ordered set of requirements: a use case, an activity, a sequence, a communication, a timing lane, an overview. Its members are requirements, and their order is the flow. Its participants are not stored: they come from the members' entities. This skill says how to read, order, branch, and populate one, in any medium: a use case in software, a presentation flow in a deck, a procedure in an organization, a scene or a plot thread in a novel.

Members and order.
- Every member is a requirement with a quote behind it. Never invent a step: a gap in the flow is a diagnostic or a docs proposal, never a fabricated member.
- The default order is document order of the members. Reorder only when the documents state the order: a "then", a numbered step, a trigger chain where one member's receiver acts as the next member's initiator, a state the next step requires. Say which sentence orders it in the justification.
- Members are `behavior` and `failure-mode` statements. A constraint, a quality bound, or a structural sentence is not a step: exclude it with a note ("constraint, not flow"). A worked example is an instance, not a step ("example, not flow").
- A flow carries one intent: what one initiator sets out to do and what the subject does in response. A statement that belongs to another intent is that flow's member. When two intents interleave in one view with no handoff between them, the view wants splitting, which is `split-view` work.
- Keep the derived members you agree with. Add a member with `update_view` `add_members` in its place in the order; drop one with `remove_members`, or exclude it with `exclude` and a note. The note is the reason the next build reads, so it names the sentence or the rule.

Placement. Every `behavior` statement belongs in some flow, and every `failure-mode` statement in some branch; the flow placement check flags the rest and opens a curation goal on the nearest view. Decide membership: add the statement in its place in the order, or exclude it with a note saying why it is not a step of this flow (a constraint in disguise, a step of the other flow the note names). The note is what settles the finding; silence brings the goal back. An exclusion note that names another view must name one that actually lists the member: add it there in the same session when none does. Batch one view's exclusions into a single `update_view`: `exclude` takes a list.

Branches.
- A `failure-mode` member gives the flow a branch. Place it immediately after the step whose failure it handles, so the activity emitter draws the condition at that step. Its condition is the member's trigger or guard ("payment declined"); the branch's outcome is the member's response.
- A failure mode with no step to branch from is either a member of another flow or a statement the documents left unattached: represent it there, or exclude it with a note saying which. An unrepresented failure mode is a check finding and comes back as a goal.
- Two branches from one step are two failure-mode members after that step, each with its own condition. A branch that rejoins the flow needs no member of its own; the next behavior member is the rejoin.

Participants.
- A member's initiator is `a` of its first edge, or its first listed entity when it has no edge. Its receiver is `b` of its first edge.
- A receiver that is interface-like (labeled `interface`, or realized by something) resolves through `realization`: the one entity realizing it is the receiver in a sequence view, and the message lands on the provider, not on the interface. Several realizers draw `provider-ambiguous`; none draws `provider-missing`. Both are check findings on the graph, not on the view: note them in the justification and never fix them by editing the view.
- The actors of a flow are the members' entities labeled `actor`; when none is, the initiators that never receive. A use-case view draws the actors and one use case; an entity that no member names is not a participant.
- A member with no edge is a step in the activity and draws no message in the sequence. When the statement clearly names a sender and a receiver, the edge is the statement's and belongs on the requirement (`declare-edges` work, or `update_requirement` passing only `id` and `edges` when the toolset has it). Membership alone never creates a message.
- The participants of a sequence view are the union of the members' initiators and receivers. Past the participant limit (8 soft, 12 hard), split by phase; never drop the members whose participants are inconvenient.

Kinds that share a flow.
- A use-case view, its activity view, and its sequence view share a title and describe one flow: order and branches decided once serve all three.
- A communication view is the same messages numbered. An overview view is an activity frame whose members reference the sequence views containing them: create one when a flow is split into phases, with one member per phase drawn from that phase's sequence view, in phase order.
- A timing view exists only where a `quality` measure with a time bound sits on a subject with a state machine; there is nothing to curate.

Titles. A derived flow view takes its title from the harness. Retitle it with `update_view` when the title reads as a label rather than an intent. The title names the intent ("Checkout", "Onboard a new hire", "The duel"), in the documents' terms, and the activity and sequence views derived from a use case follow its title.

Limits. Members per flow view: 12 soft, 20 hard. Split by phase along the documents' own breaks (a heading, a "then", a handoff to another initiator): a sequence view per phase and an overview that references them. Never satisfy a limit by omitting a step or a branch, and never split a flow whose steps the documents present as one.

Retrace. When a member died, read the surviving members as a flow before deciding: drop the member with `remove_members`, or point the view at the requirement that carries the step under a new id, in the same position. A removed step can leave a branch without its condition; move or exclude that branch so the flow stays true. A view with no flow left is deleted with `delete_view` and the reason.

Justify in one or two sentences: which sentence orders the flow, why an exclusion, why a branch sits where it does.
